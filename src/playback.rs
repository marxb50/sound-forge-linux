use crate::audio::AudioBuffer;
use crate::dsp::{time_stretch_wsola, SpeedMode};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
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

struct PlaybackShared {
    /// Original buffer being inspected in the UI.
    source: Arc<AudioBuffer>,
    /// Playback buffer (either `source`, or WSOLA pre-stretched when speed != 1.0 && PreservePitch).
    play_buf: Arc<AudioBuffer>,
    is_playing: bool,
    loop_enabled: bool,
    speed: f64,
    speed_mode: SpeedMode,
    /// Current fractional frame index in `play_buf`.
    play_pos: f64,
    /// Range in `play_buf` frames [range_start, range_end).
    range_start: usize,
    range_end: usize,
    /// Ratio to convert `play_buf` frame -> original `source` frame.
    pos_scale_to_source: f64,
    meter: MeterReading,
    master_gain: f32,
}

pub struct PlaybackEngine {
    shared: Arc<Mutex<PlaybackShared>>,
    _stream: Option<cpal::Stream>,
    device_sample_rate: u32,
    device_channels: usize,
    pub init_error: Option<String>,
}

impl PlaybackEngine {
    pub fn new() -> Self {
        let empty = Arc::new(AudioBuffer::empty(44100, 2));
        let shared = Arc::new(Mutex::new(PlaybackShared {
            source: empty.clone(),
            play_buf: empty,
            is_playing: false,
            loop_enabled: false,
            speed: 1.0,
            speed_mode: SpeedMode::PreservePitch,
            play_pos: 0.0,
            range_start: 0,
            range_end: 0,
            pos_scale_to_source: 1.0,
            meter: MeterReading::default(),
            master_gain: 1.0,
        }));

        match Self::start_stream(shared.clone()) {
            Ok((stream, sr, ch)) => Self {
                shared,
                _stream: Some(stream),
                device_sample_rate: sr,
                device_channels: ch,
                init_error: None,
            },
            Err(err) => Self {
                shared,
                _stream: None,
                device_sample_rate: 44100,
                device_channels: 2,
                init_error: Some(err),
            },
        }
    }

    fn start_stream(
        shared: Arc<Mutex<PlaybackShared>>,
    ) -> Result<(cpal::Stream, u32, usize), String> {
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

        Ok((stream, sample_rate, channels))
    }

    pub fn device_info(&self) -> (u32, usize) {
        (self.device_sample_rate, self.device_channels)
    }

    pub fn set_buffer(&self, buffer: Arc<AudioBuffer>) {
        if let Ok(mut st) = self.shared.lock() {
            st.is_playing = false;
            st.source = buffer.clone();
            st.play_buf = buffer;
            st.play_pos = 0.0;
            st.range_start = 0;
            st.range_end = st.play_buf.num_frames();
            st.pos_scale_to_source = 1.0;
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
        let speed = speed.clamp(0.25, 4.0);
        let src_frames = buffer.num_frames();
        let s = start_frame.min(src_frames);
        let e = end_frame.min(src_frames).max(s);
        if e <= s {
            return;
        }

        let (play_buf, range_start, range_end, pos_scale_to_source) =
            if (speed - 1.0).abs() > 1e-3 && speed_mode == SpeedMode::PreservePitch {
                // Pre-stretch the source buffer cleanly so pitch stays intact while playing.
                let stretched = Arc::new(time_stretch_wsola(&buffer, speed));
                let pf = stretched.num_frames();
                let ps = ((s as f64 / speed).round() as usize).min(pf);
                let pe = ((e as f64 / speed).round() as usize).clamp(ps, pf);
                (stretched, ps, pe, speed)
            } else {
                (buffer.clone(), s, e, 1.0)
            };

        if let Ok(mut st) = self.shared.lock() {
            st.source = buffer;
            st.play_buf = play_buf;
            st.range_start = range_start;
            st.range_end = range_end;
            st.play_pos = range_start as f64;
            st.loop_enabled = loop_enabled;
            st.speed = speed;
            st.speed_mode = speed_mode;
            st.pos_scale_to_source = pos_scale_to_source;
            st.is_playing = true;
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

    pub fn set_master_gain(&self, gain: f32) {
        if let Ok(mut st) = self.shared.lock() {
            st.master_gain = gain.clamp(0.0, 2.0);
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
            let f = (st.play_pos * st.pos_scale_to_source).round() as usize;
            f.min(st.source.num_frames())
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

    if !st.is_playing || st.range_end <= st.range_start || st.play_buf.num_frames() == 0 {
        out.fill(0.0);
        // Gentle meter decay when stopped.
        st.meter.peak_l *= 0.85;
        st.meter.peak_r *= 0.85;
        st.meter.rms_l *= 0.85;
        st.meter.rms_r *= 0.85;
        return;
    }

    let buf = st.play_buf.clone();
    let src_sr = buf.sample_rate as f64;
    let src_ch = buf.channels;
    let gain = st.master_gain;

    // When PreservePitch is active, `play_buf` is already time-stretched, so step at 1.0x!
    // When Varispeed is active, step proportionally to `st.speed`.
    let effective_speed = if st.speed_mode == SpeedMode::Varispeed {
        st.speed
    } else {
        1.0
    };
    let step = (src_sr / out_sr as f64) * effective_speed;

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

        let idx0 = st.play_pos.floor() as usize;
        let idx1 = (idx0 + 1).min(buf.num_frames().saturating_sub(1));
        let frac = (st.play_pos - idx0 as f64) as f32;

        let sample_l = {
            let a = buf.get_sample(idx0, 0);
            let b = buf.get_sample(idx1, 0);
            (a + (b - a) * frac) * gain
        };
        let sample_r = if src_ch > 1 {
            let a = buf.get_sample(idx0, 1);
            let b = buf.get_sample(idx1, 1);
            (a + (b - a) * frac) * gain
        } else {
            sample_l
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

        st.play_pos += step;
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
