use crate::audio::AudioBuffer;
use crate::dsp::SpeedMode;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::f32::consts::PI;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub struct MeterReading {
    pub peak_l: f32,
    pub peak_r: f32,
    pub rms_l: f32,
    pub rms_r: f32,
    pub clipped: bool,
}

impl Default for MeterReading {
    fn default() -> Self {
        Self {
            peak_l: 0.0,
            peak_r: 0.0,
            rms_l: 0.0,
            rms_r: 0.0,
            clipped: false,
        }
    }
}

const GRAIN_LEN: usize = 2048;

struct PlaybackShared {
    /// Original buffer being inspected in the UI (never pre-cloned/stretched on UI thread!).
    source: Arc<AudioBuffer>,
    is_playing: bool,
    loop_enabled: bool,
    speed: f64,
    speed_mode: SpeedMode,
    /// Current fractional frame index in `source` (always accurate in source frames!).
    play_pos: f64,
    /// Range in `source` frames [range_start, range_end).
    range_start: usize,
    range_end: usize,
    /// Dual-grain real-time WSOLA state for pitch-preserving time stretch
    grain_phase: f64,
    grain_a_start: f64,
    grain_b_start: f64,
    meter: MeterReading,
    master_gain: f32,
}

pub struct PlaybackEngine {
    shared: Arc<Mutex<PlaybackShared>>,
    _stream: Option<cpal::Stream>,
    pub init_error: Option<String>,
}

impl PlaybackEngine {
    pub fn new() -> Self {
        let empty = Arc::new(AudioBuffer::empty(44100, 2));
        let shared = Arc::new(Mutex::new(PlaybackShared {
            source: empty,
            is_playing: false,
            loop_enabled: false,
            speed: 1.0,
            speed_mode: SpeedMode::PreservePitch,
            play_pos: 0.0,
            range_start: 0,
            range_end: 0,
            grain_phase: 0.0,
            grain_a_start: 0.0,
            grain_b_start: 0.0,
            meter: MeterReading::default(),
            master_gain: 1.0,
        }));

        match Self::start_stream(shared.clone()) {
            Ok(stream) => Self {
                shared,
                _stream: Some(stream),
                init_error: None,
            },
            Err(err) => Self {
                shared,
                _stream: None,
                init_error: Some(err),
            },
        }
    }

    fn start_stream(shared: Arc<Mutex<PlaybackShared>>) -> Result<cpal::Stream, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "Nenhum dispositivo de saída de áudio encontrado".to_string())?;
        let default_cfg = device
            .default_output_config()
            .map_err(|e| format!("Erro ao consultar configuração de áudio: {e}"))?;

        let sample_rate = default_cfg.sample_rate().0;
        let channels = default_cfg.channels() as usize;
        let config: cpal::StreamConfig = default_cfg.into();

        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    fill_audio_callback(data, channels, sample_rate, &shared);
                },
                |err| {
                    eprintln!("Erro no stream de áudio CPAL: {err}");
                },
                None,
            )
            .map_err(|e| format!("Falha ao abrir stream de áudio: {e}"))?;

        stream
            .play()
            .map_err(|e| format!("Falha ao iniciar reprodução de áudio: {e}"))?;

        Ok(stream)
    }

    pub fn set_buffer(&self, buffer: Arc<AudioBuffer>) {
        if let Ok(mut st) = self.shared.lock() {
            st.is_playing = false;
            st.range_start = 0;
            st.range_end = buffer.num_frames();
            st.play_pos = 0.0;
            st.grain_phase = 0.0;
            st.grain_a_start = 0.0;
            st.grain_b_start = 0.0;
            st.source = buffer;
            st.meter = MeterReading::default();
        }
    }

    pub fn play_range(
        &self,
        buffer: Arc<AudioBuffer>,
        start_frame: usize,
        end_frame: usize,
        loop_enabled: bool,
        speed: f64,
        speed_mode: SpeedMode,
    ) {
        let speed = speed.clamp(0.20, 4.0);
        let src_frames = buffer.num_frames();
        let s = start_frame.min(src_frames);
        let e = end_frame.min(src_frames).max(s);
        if e <= s {
            return;
        }

        if let Ok(mut st) = self.shared.lock() {
            st.source = buffer;
            st.range_start = s;
            st.range_end = e;
            st.play_pos = s as f64;
            st.grain_phase = 0.0;
            st.grain_a_start = s as f64;
            st.grain_b_start = (s as f64 - (GRAIN_LEN as f64 * 0.5)).max(0.0);
            st.loop_enabled = loop_enabled;
            st.speed = speed;
            st.speed_mode = speed_mode;
            st.is_playing = true;
        }
    }

    /// Instantly jumps playback to `new_start_frame` without stopping or re-allocating.
    pub fn seek_to_source_frame(&self, new_start_frame: usize, new_end_frame: Option<usize>) {
        if let Ok(mut st) = self.shared.lock() {
            let total = st.source.num_frames();
            if total == 0 {
                return;
            }
            let s = new_start_frame.min(total.saturating_sub(1));
            let e = new_end_frame
                .unwrap_or(total)
                .min(total)
                .max(s + 1);
            st.range_start = s;
            st.range_end = e;
            st.play_pos = s as f64;
            st.grain_phase = 0.0;
            st.grain_a_start = s as f64;
            st.grain_b_start = (s as f64 - (GRAIN_LEN as f64 * 0.5)).max(0.0);
        }
    }

    /// Updates playback speed and mode live while playing!
    pub fn set_speed_live(&self, speed: f64, speed_mode: SpeedMode) {
        if let Ok(mut st) = self.shared.lock() {
            st.speed = speed.clamp(0.20, 4.0);
            st.speed_mode = speed_mode;
        }
    }

    pub fn pause(&self) {
        if let Ok(mut st) = self.shared.lock() {
            st.is_playing = false;
        }
    }

    pub fn stop(&self) {
        if let Ok(mut st) = self.shared.lock() {
            st.is_playing = false;
            st.play_pos = st.range_start as f64;
            st.meter = MeterReading::default();
        }
    }

    pub fn set_loop(&self, loop_enabled: bool) {
        if let Ok(mut st) = self.shared.lock() {
            st.loop_enabled = loop_enabled;
        }
    }

    pub fn is_playing(&self) -> bool {
        self.shared
            .lock()
            .map(|st| st.is_playing)
            .unwrap_or(false)
    }

    /// Returns current playhead position in original source frames.
    pub fn current_source_frame(&self) -> usize {
        if let Ok(st) = self.shared.lock() {
            (st.play_pos.round() as usize).min(st.source.num_frames())
        } else {
            0
        }
    }

    pub fn meter_reading(&self) -> MeterReading {
        if let Ok(st) = self.shared.lock() {
            st.meter.clone()
        } else {
            MeterReading::default()
        }
    }
}

#[inline]
fn sample_lerp(buf: &AudioBuffer, pos: f64, ch: usize) -> f32 {
    let n = buf.num_frames();
    if n == 0 {
        return 0.0;
    }
    let p = pos.clamp(0.0, (n - 1) as f64);
    let idx0 = p.floor() as usize;
    let idx1 = (idx0 + 1).min(n - 1);
    let frac = (p - idx0 as f64) as f32;
    let a = buf.get_sample(idx0, ch);
    let b = buf.get_sample(idx1, ch);
    a + (b - a) * frac
}

/// Finds a phase-aligned grain start near `target_pos` matching the signal at `ref_pos`.
fn align_grain_start(buf: &AudioBuffer, target_pos: f64, ref_pos: f64) -> f64 {
    let n = buf.num_frames();
    if n < GRAIN_LEN + 256 {
        return target_pos.clamp(0.0, n.saturating_sub(1) as f64);
    }
    let ref_idx = (ref_pos.round() as usize).min(n.saturating_sub(256));
    let center = (target_pos.round() as usize).min(n.saturating_sub(GRAIN_LEN));
    let search = 192_usize;
    let min_c = center.saturating_sub(search);
    let max_c = (center + search).min(n.saturating_sub(GRAIN_LEN));

    let mut best_c = center;
    let mut best_dot = f32::NEG_INFINITY;
    let mut cand = min_c;
    while cand <= max_c {
        let mut dot = 0.0_f32;
        let mut i = 0;
        while i < 128 {
            let r = buf.get_sample(ref_idx + i, 0);
            let x = buf.get_sample(cand + i, 0);
            dot += r * x;
            i += 4;
        }
        if dot > best_dot {
            best_dot = dot;
            best_c = cand;
        }
        cand += 4;
    }
    best_c as f64
}

fn fill_audio_callback(
    out: &mut [f32],
    out_channels: usize,
    out_sr: u32,
    shared: &Arc<Mutex<PlaybackShared>>,
) {
    let Ok(mut st) = shared.try_lock() else {
        out.fill(0.0);
        return;
    };

    if !st.is_playing || st.range_end <= st.range_start || st.source.num_frames() == 0 {
        out.fill(0.0);
        st.meter.peak_l *= 0.85;
        st.meter.peak_r *= 0.85;
        st.meter.rms_l *= 0.85;
        st.meter.rms_r *= 0.85;
        return;
    }

    let buf = st.source.clone();
    let src_sr = buf.sample_rate as f64;
    let src_ch = buf.channels;
    let gain = st.master_gain;
    let speed = st.speed;
    let use_wsola = (speed - 1.0).abs() > 1e-3 && st.speed_mode == SpeedMode::PreservePitch;

    // Rate at which the playhead advances through the song timeline:
    let timeline_step = (src_sr / out_sr as f64) * speed;
    // Rate at which audio grains advance (1.0x pitch):
    let grain_step = src_sr / out_sr as f64;
    let phase_step = grain_step / GRAIN_LEN as f64;

    let out_frames = out.len() / out_channels.max(1);
    let mut peak_l = 0.0_f32;
    let mut peak_r = 0.0_f32;
    let mut sq_l = 0.0_f32;
    let mut sq_r = 0.0_f32;
    let mut clipped = false;

    for f in 0..out_frames {
        if st.play_pos >= st.range_end as f64 {
            if st.loop_enabled {
                st.play_pos = st.range_start as f64;
                st.grain_phase = 0.0;
                st.grain_a_start = st.play_pos;
                st.grain_b_start = (st.play_pos - GRAIN_LEN as f64 * 0.5).max(0.0);
            } else {
                st.is_playing = false;
                st.play_pos = st.range_start as f64;
                for rem_f in f..out_frames {
                    for c in 0..out_channels {
                        out[rem_f * out_channels + c] = 0.0;
                    }
                }
                break;
            }
        }

        let (sample_l, sample_r) = if !use_wsola {
            let l = sample_lerp(&buf, st.play_pos, 0) * gain;
            let r = if src_ch > 1 {
                sample_lerp(&buf, st.play_pos, 1) * gain
            } else {
                l
            };
            (l, r)
        } else {
            let phi_a = st.grain_phase;
            let phi_b = (phi_a + 0.5).fract();

            // Complementary Hann weights: w_a + w_b == 1.0 everywhere!
            let w_a = 0.5 * (1.0 - (2.0 * PI * phi_a as f32).cos());
            let w_b = 0.5 * (1.0 - (2.0 * PI * phi_b as f32).cos());

            let pos_a = st.grain_a_start + phi_a * GRAIN_LEN as f64;
            let pos_b = st.grain_b_start + phi_b * GRAIN_LEN as f64;

            let la = sample_lerp(&buf, pos_a, 0);
            let lb = sample_lerp(&buf, pos_b, 0);
            let l = (la * w_a + lb * w_b) * gain;

            let r = if src_ch > 1 {
                let ra = sample_lerp(&buf, pos_a, 1);
                let rb = sample_lerp(&buf, pos_b, 1);
                (ra * w_a + rb * w_b) * gain
            } else {
                l
            };

            let prev_phi = st.grain_phase;
            let next_phi = prev_phi + phase_step;
            // Did Grain B cross 1.0 -> 0.0 (which happens when phi_a crosses 0.5)?
            if prev_phi < 0.5 && next_phi >= 0.5 {
                let cur_a_pos = st.grain_a_start + 0.5 * GRAIN_LEN as f64;
                st.grain_b_start = align_grain_start(&buf, st.play_pos, cur_a_pos);
            }
            if next_phi >= 1.0 {
                st.grain_phase = next_phi - 1.0;
                let cur_b_pos = st.grain_b_start + 0.5 * GRAIN_LEN as f64;
                st.grain_a_start = align_grain_start(&buf, st.play_pos, cur_b_pos);
            } else {
                st.grain_phase = next_phi;
            }

            (l, r)
        };

        if sample_l.abs() > 0.999 || sample_r.abs() > 0.999 {
            clipped = true;
        }
        peak_l = peak_l.max(sample_l.abs());
        peak_r = peak_r.max(sample_r.abs());
        sq_l += sample_l * sample_l;
        sq_r += sample_r * sample_r;

        for c in 0..out_channels {
            let v = if c == 0 {
                sample_l
            } else if c == 1 {
                sample_r
            } else {
                (sample_l + sample_r) * 0.5
            };
            out[f * out_channels + c] = v.clamp(-1.0, 1.0);
        }

        st.play_pos += timeline_step;
    }

    let denom = out_frames.max(1) as f32;
    st.meter = MeterReading {
        peak_l: peak_l.max(st.meter.peak_l * 0.8),
        peak_r: peak_r.max(st.meter.peak_r * 0.8),
        rms_l: (sq_l / denom).sqrt(),
        rms_r: (sq_r / denom).sqrt(),
        clipped: st.meter.clipped || clipped,
    };
}
