use crate::audio::AudioBuffer;
use std::f32::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FadeCurve {
    Linear,
    SmoothSCurve,
    Exponential,
    Logarithmic,
}

impl FadeCurve {
    pub fn label(&self) -> &'static str {
        match self {
            FadeCurve::Linear => "Linear (0 dB a -Inf)",
            FadeCurve::SmoothSCurve => "Suave S-Curve (Cosine)",
            FadeCurve::Exponential => "Exponencial Rápida",
            FadeCurve::Logarithmic => "Logarítmica (Equal Power)",
        }
    }

    pub fn eval(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            FadeCurve::Linear => t,
            FadeCurve::SmoothSCurve => 0.5 - 0.5 * (PI * t).cos(),
            FadeCurve::Exponential => t * t,
            FadeCurve::Logarithmic => t.sqrt(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpeedMode {
    /// Changes tempo while preserving musical pitch (WSOLA).
    PreservePitch,
    /// Changes both speed and pitch together (tape / turntable style).
    Varispeed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeStretchAlgorithm {
    Music1MinFlange,
    Music2Balanced,
    Music3Crisp,
    Speech,
    SoloInstruments,
    VarispeedTape,
}

impl TimeStretchAlgorithm {
    pub fn all() -> &'static [TimeStretchAlgorithm] {
        &[
            TimeStretchAlgorithm::Music1MinFlange,
            TimeStretchAlgorithm::Music2Balanced,
            TimeStretchAlgorithm::Music3Crisp,
            TimeStretchAlgorithm::Speech,
            TimeStretchAlgorithm::SoloInstruments,
            TimeStretchAlgorithm::VarispeedTape,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            TimeStretchAlgorithm::Music1MinFlange => "A01. Music 1 (minimum flange, may echo)",
            TimeStretchAlgorithm::Music2Balanced => "A02. Music 2 (balanced orchestral/ballet)",
            TimeStretchAlgorithm::Music3Crisp => "A03. Music 3 (crisp transients / percussion)",
            TimeStretchAlgorithm::Speech => "A04. Speech / Voice (short window)",
            TimeStretchAlgorithm::SoloInstruments => "A05. Solo Instruments (long harmonic window)",
            TimeStretchAlgorithm::VarispeedTape => "V01. Varispeed (altera tempo + tom juntos)",
        }
    }

    pub fn window_ms(&self) -> f64 {
        match self {
            TimeStretchAlgorithm::Music1MinFlange => 75.0,
            TimeStretchAlgorithm::Music2Balanced => 55.0,
            TimeStretchAlgorithm::Music3Crisp => 32.0,
            TimeStretchAlgorithm::Speech => 24.0,
            TimeStretchAlgorithm::SoloInstruments => 90.0,
            TimeStretchAlgorithm::VarispeedTape => 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelConvertMode {
    StereoToMonoMix,
    MonoToStereo,
    SwapLeftRight,
    LeftOnlyToMono,
    RightOnlyToMono,
    VocalCutCenterCancel,
}

impl ChannelConvertMode {
    pub fn all() -> &'static [ChannelConvertMode] {
        &[
            ChannelConvertMode::StereoToMonoMix,
            ChannelConvertMode::MonoToStereo,
            ChannelConvertMode::SwapLeftRight,
            ChannelConvertMode::LeftOnlyToMono,
            ChannelConvertMode::RightOnlyToMono,
            ChannelConvertMode::VocalCutCenterCancel,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            ChannelConvertMode::StereoToMonoMix => "Stereo to Mono (50% L + 50% R)",
            ChannelConvertMode::MonoToStereo => "Mono to Stereo (Duplicar canais)",
            ChannelConvertMode::SwapLeftRight => "Invert/Swap Left ↔ Right",
            ChannelConvertMode::LeftOnlyToMono => "Left Channel Only (Mono)",
            ChannelConvertMode::RightOnlyToMono => "Right Channel Only (Mono)",
            ChannelConvertMode::VocalCutCenterCancel => "Vocal Cut (L - R Center Cancel)",
        }
    }
}

/// Extracts frames [start, end) into a new AudioBuffer.
pub fn slice_frames(buf: &AudioBuffer, start: usize, end: usize) -> AudioBuffer {
    let n = buf.num_frames();
    let s = start.min(n);
    let e = end.min(n).max(s);
    let ch = buf.channels;
    AudioBuffer::new(
        buf.sample_rate,
        ch,
        buf.samples[s * ch..e * ch].to_vec(),
    )
}

/// Deletes frames [start, end) and returns the resulting buffer.
pub fn delete_range(buf: &AudioBuffer, start: usize, end: usize) -> AudioBuffer {
    let n = buf.num_frames();
    let s = start.min(n);
    let e = end.min(n).max(s);
    let ch = buf.channels;
    let mut out = Vec::with_capacity((n - (e - s)) * ch);
    out.extend_from_slice(&buf.samples[..s * ch]);
    out.extend_from_slice(&buf.samples[e * ch..]);
    AudioBuffer::new(buf.sample_rate, ch, out)
}

/// Inserts `clip` into `buf` at `at_frame`, matching sample rate and channel count.
pub fn insert_buffer(buf: &AudioBuffer, at_frame: usize, clip: &AudioBuffer) -> AudioBuffer {
    let adapted = adapt_buffer(clip, buf.sample_rate, buf.channels);
    let n = buf.num_frames();
    let pos = at_frame.min(n);
    let ch = buf.channels;

    let mut out = Vec::with_capacity(buf.samples.len() + adapted.samples.len());
    out.extend_from_slice(&buf.samples[..pos * ch]);
    out.extend_from_slice(&adapted.samples);
    out.extend_from_slice(&buf.samples[pos * ch..]);
    AudioBuffer::new(buf.sample_rate, ch, out)
}

/// Mixes `clip` into `buf` starting at `at_frame`. Extends `buf` if `clip` goes past the end.
pub fn mix_buffer(
    buf: &AudioBuffer,
    at_frame: usize,
    clip: &AudioBuffer,
    clip_gain: f32,
) -> AudioBuffer {
    let adapted = adapt_buffer(clip, buf.sample_rate, buf.channels);
    let ch = buf.channels;
    let total_frames = buf.num_frames().max(at_frame + adapted.num_frames());
    let mut out = vec![0.0_f32; total_frames * ch];
    out[..buf.samples.len()].copy_from_slice(&buf.samples);

    let clip_frames = adapted.num_frames();
    for f in 0..clip_frames {
        let dst_f = at_frame + f;
        for c in 0..ch {
            let idx = dst_f * ch + c;
            out[idx] = (out[idx] + adapted.get_sample(f, c) * clip_gain).clamp(-1.0, 1.0);
        }
    }
    AudioBuffer::new(buf.sample_rate, ch, out)
}

/// Concatenates `buf_b` onto `buf_a` with an equal-power crossfade of `crossfade_secs` (or silence if negative).
pub fn concat_with_crossfade(
    buf_a: &AudioBuffer,
    buf_b: &AudioBuffer,
    crossfade_secs: f64,
) -> AudioBuffer {
    let target_sr = buf_a.sample_rate.max(8000);
    let target_ch = buf_a.channels.max(buf_b.channels).clamp(1, 2);
    let a = adapt_buffer(buf_a, target_sr, target_ch);
    let b = adapt_buffer(buf_b, target_sr, target_ch);

    if crossfade_secs <= 0.0 {
        let gap_frames = ((-crossfade_secs) * target_sr as f64).round() as usize;
        let mut out = Vec::with_capacity((a.num_frames() + gap_frames + b.num_frames()) * target_ch);
        out.extend_from_slice(&a.samples);
        out.resize(out.len() + gap_frames * target_ch, 0.0);
        out.extend_from_slice(&b.samples);
        return AudioBuffer::new(target_sr, target_ch, out);
    }

    let xfade_frames = ((crossfade_secs * target_sr as f64).round() as usize)
        .min(a.num_frames() / 2)
        .min(b.num_frames() / 2);

    if xfade_frames < 4 {
        let mut out = Vec::with_capacity(a.samples.len() + b.samples.len());
        out.extend_from_slice(&a.samples);
        out.extend_from_slice(&b.samples);
        return AudioBuffer::new(target_sr, target_ch, out);
    }

    let a_keep = a.num_frames() - xfade_frames;
    let total_frames = a_keep + b.num_frames();
    let mut out = Vec::with_capacity(total_frames * target_ch);
    out.extend_from_slice(&a.samples[..a_keep * target_ch]);

    for i in 0..xfade_frames {
        let t = i as f32 / xfade_frames as f32;
        let wa = (0.5 * PI * t).cos();
        let wb = (0.5 * PI * t).sin();
        for c in 0..target_ch {
            let sa = a.get_sample(a_keep + i, c);
            let sb = b.get_sample(i, c);
            out.push((sa * wa + sb * wb).clamp(-1.0, 1.0));
        }
    }
    out.extend_from_slice(&b.samples[xfade_frames * target_ch..]);
    AudioBuffer::new(target_sr, target_ch, out)
}

/// Inserts `silence_frames` of silence at `at_frame`.
pub fn insert_silence(buf: &AudioBuffer, at_frame: usize, silence_frames: usize) -> AudioBuffer {
    let ch = buf.channels;
    let n = buf.num_frames();
    let pos = at_frame.min(n);
    let mut out = Vec::with_capacity((n + silence_frames) * ch);
    out.extend_from_slice(&buf.samples[..pos * ch]);
    out.resize(out.len() + silence_frames * ch, 0.0);
    out.extend_from_slice(&buf.samples[pos * ch..]);
    AudioBuffer::new(buf.sample_rate, ch, out)
}

/// Extends a selected musical phrase [start, end) by `extra_secs` using crossfaded repetition.
pub fn extend_range_with_crossfade(
    buf: &AudioBuffer,
    start: usize,
    end: usize,
    extra_secs: f64,
    crossfade_ms: f64,
) -> AudioBuffer {
    let n = buf.num_frames();
    let s = start.min(n);
    let e = end.min(n).max(s);
    let sel_frames = e - s;
    let extra_frames = (extra_secs.max(0.0) * buf.sample_rate as f64).round() as usize;
    if sel_frames < 16 || extra_frames == 0 {
        return buf.clone();
    }

    let xfade_frames = ((crossfade_ms.max(1.0) / 1000.0) * buf.sample_rate as f64).round() as usize;
    let xfade = xfade_frames.min(sel_frames / 3);
    let ch = buf.channels;

    let mut ext = Vec::with_capacity((extra_frames + xfade) * ch);
    let mut produced = 0_usize;
    while produced < extra_frames {
        let chunk_len = (extra_frames - produced).min(sel_frames);
        for f in 0..chunk_len {
            let fade_in = if xfade > 0 && f < xfade {
                0.5 - 0.5 * (PI * (f as f32 / xfade as f32)).cos()
            } else {
                1.0
            };
            let fade_out = if xfade > 0 && produced + f + xfade >= extra_frames {
                let rem = extra_frames.saturating_sub(produced + f);
                0.5 - 0.5 * (PI * (rem as f32 / xfade as f32)).cos()
            } else {
                1.0
            };
            for c in 0..ch {
                let v = buf.get_sample(s + f, c) * fade_in * fade_out;
                ext.push(v);
            }
        }
        produced += chunk_len;
    }

    let ext_buf = AudioBuffer::new(buf.sample_rate, ch, ext);
    let mut out = Vec::with_capacity(buf.samples.len() + ext_buf.samples.len());
    out.extend_from_slice(&buf.samples[..e * ch]);
    out.extend_from_slice(&ext_buf.samples);
    out.extend_from_slice(&buf.samples[e * ch..]);

    if xfade > 4 {
        for i in 0..xfade {
            let a_idx = e.saturating_sub(xfade) + i;
            let b_idx = e + i;
            if b_idx < out.len() / ch && a_idx < e {
                let t = i as f32 / xfade as f32;
                let w_out = (0.5 * PI * t).cos();
                let w_in = (0.5 * PI * t).sin();
                for c in 0..ch {
                    let orig = out[a_idx * ch + c];
                    let next = out[b_idx * ch + c];
                    out[a_idx * ch + c] = orig * w_out + next * w_in * 0.35;
                }
            }
        }
    }

    AudioBuffer::new(buf.sample_rate, ch, out)
}

/// Applies gain in decibels to [start, end).
pub fn apply_gain_db(buf: &AudioBuffer, start: usize, end: usize, db: f32) -> AudioBuffer {
    let mut out = buf.clone();
    let gain = if db <= -90.0 {
        0.0
    } else {
        10.0_f32.powf(db / 20.0)
    };
    let n = out.num_frames();
    let s = start.min(n);
    let e = end.min(n).max(s);
    let ch = out.channels;
    for f in s..e {
        for c in 0..ch {
            let idx = f * ch + c;
            out.samples[idx] = (out.samples[idx] * gain).clamp(-1.0, 1.0);
        }
    }
    out
}

/// Normalizes [start, end) so peak reaches `target_db` (or RMS if `use_rms` is true).
pub fn normalize_peak(buf: &AudioBuffer, start: usize, end: usize, target_db: f32) -> AudioBuffer {
    normalize_audio(buf, start, end, target_db, false)
}

pub fn normalize_audio(
    buf: &AudioBuffer,
    start: usize,
    end: usize,
    target_db: f32,
    use_rms: bool,
) -> AudioBuffer {
    let n = buf.num_frames();
    let s = start.min(n);
    let e = end.min(n).max(s);
    if e <= s {
        return buf.clone();
    }
    let ch = buf.channels;

    let measured = if use_rms {
        let mut sq = 0.0_f64;
        let count = ((e - s) * ch).max(1) as f64;
        for f in s..e {
            for c in 0..ch {
                let v = buf.get_sample(f, c) as f64;
                sq += v * v;
            }
        }
        (sq / count).sqrt() as f32
    } else {
        let mut peak = 0.0_f32;
        for f in s..e {
            for c in 0..ch {
                peak = peak.max(buf.get_sample(f, c).abs());
            }
        }
        peak
    };

    if measured < 1e-7 {
        return buf.clone();
    }

    let target_amp = 10.0_f32.powf(target_db / 20.0).min(1.0);
    let gain = target_amp / measured;
    let mut out = buf.clone();
    for f in s..e {
        for c in 0..ch {
            let idx = f * ch + c;
            out.samples[idx] = (out.samples[idx] * gain).clamp(-1.0, 1.0);
        }
    }
    out
}

pub fn fade_in(buf: &AudioBuffer, start: usize, end: usize, curve: FadeCurve) -> AudioBuffer {
    graphic_fade(buf, start, end, 0.0, 1.0, curve)
}

pub fn fade_out(buf: &AudioBuffer, start: usize, end: usize, curve: FadeCurve) -> AudioBuffer {
    graphic_fade(buf, start, end, 1.0, 0.0, curve)
}

/// Graphic Fade between `start_gain` (0.0..=2.0) and `end_gain` (0.0..=2.0) using `curve`.
pub fn graphic_fade(
    buf: &AudioBuffer,
    start: usize,
    end: usize,
    start_gain: f32,
    end_gain: f32,
    curve: FadeCurve,
) -> AudioBuffer {
    let mut out = buf.clone();
    let n = out.num_frames();
    let s = start.min(n);
    let e = end.min(n).max(s);
    let len = e - s;
    if len < 2 {
        return out;
    }
    let ch = out.channels;
    for i in 0..len {
        let t = curve.eval(i as f32 / (len - 1) as f32);
        let g = start_gain + (end_gain - start_gain) * t;
        let f = s + i;
        for c in 0..ch {
            out.samples[f * ch + c] = (out.samples[f * ch + c] * g).clamp(-1.0, 1.0);
        }
    }
    out
}

pub fn reverse_range(buf: &AudioBuffer, start: usize, end: usize) -> AudioBuffer {
    let mut out = buf.clone();
    let n = out.num_frames();
    let s = start.min(n);
    let e = end.min(n).max(s);
    let ch = out.channels;
    let mut left = s;
    let mut right = e.saturating_sub(1);
    while left < right {
        for c in 0..ch {
            out.samples.swap(left * ch + c, right * ch + c);
        }
        left += 1;
        right -= 1;
    }
    out
}

pub fn silence_range(buf: &AudioBuffer, start: usize, end: usize) -> AudioBuffer {
    let mut out = buf.clone();
    let n = out.num_frames();
    let s = start.min(n);
    let e = end.min(n).max(s);
    let ch = out.channels;
    for f in s..e {
        for c in 0..ch {
            out.samples[f * ch + c] = 0.0;
        }
    }
    out
}

/// Sound Forge `Invert/Flip`: inverts waveform polarity `s = -s` in [start, end).
pub fn invert_flip_range(buf: &AudioBuffer, start: usize, end: usize) -> AudioBuffer {
    let mut out = buf.clone();
    let n = out.num_frames();
    let s = start.min(n);
    let e = end.min(n).max(s);
    let ch = out.channels;
    for f in s..e {
        for c in 0..ch {
            let idx = f * ch + c;
            out.samples[idx] = (-out.samples[idx]).clamp(-1.0, 1.0);
        }
    }
    out
}

/// Sound Forge `DC Offset`: removes mean DC offset from [start, end).
pub fn remove_dc_offset(buf: &AudioBuffer, start: usize, end: usize) -> (AudioBuffer, f32) {
    let mut out = buf.clone();
    let n = out.num_frames();
    let s = start.min(n);
    let e = end.min(n).max(s);
    if e <= s {
        return (out, 0.0);
    }
    let ch = out.channels;
    let mut avg_dc = 0.0_f32;
    for c in 0..ch {
        let mut sum = 0.0_f64;
        for f in s..e {
            sum += out.get_sample(f, c) as f64;
        }
        let dc = (sum / (e - s) as f64) as f32;
        avg_dc += dc;
        for f in s..e {
            let idx = f * ch + c;
            out.samples[idx] = (out.samples[idx] - dc).clamp(-1.0, 1.0);
        }
    }
    (out, avg_dc / ch as f32)
}

/// Sound Forge `Auto Trim/Crop`: trims silence below `threshold_db` from start and end of [start, end).
pub fn auto_trim_crop(
    buf: &AudioBuffer,
    start: usize,
    end: usize,
    threshold_db: f32,
) -> AudioBuffer {
    let n = buf.num_frames();
    let s = start.min(n);
    let e = end.min(n).max(s);
    if e <= s {
        return buf.clone();
    }
    let thresh = 10.0_f32.powf(threshold_db / 20.0);
    let ch = buf.channels;

    let mut first_loud = s;
    while first_loud < e {
        let loud = (0..ch).any(|c| buf.get_sample(first_loud, c).abs() >= thresh);
        if loud {
            break;
        }
        first_loud += 1;
    }

    let mut last_loud = e;
    while last_loud > first_loud {
        let loud = (0..ch).any(|c| buf.get_sample(last_loud - 1, c).abs() >= thresh);
        if loud {
            break;
        }
        last_loud -= 1;
    }

    if first_loud >= last_loud {
        return buf.clone();
    }

    // Keep outside [s, e) intact and replace [s, e) with trimmed [first_loud, last_loud)
    let mut out = Vec::with_capacity((s + (last_loud - first_loud) + (n - e)) * ch);
    out.extend_from_slice(&buf.samples[..s * ch]);
    out.extend_from_slice(&buf.samples[first_loud * ch..last_loud * ch]);
    out.extend_from_slice(&buf.samples[e * ch..]);
    AudioBuffer::new(buf.sample_rate, ch, out)
}

/// Sound Forge `Channel Converter`.
pub fn convert_channels(buf: &AudioBuffer, mode: ChannelConvertMode) -> AudioBuffer {
    let n = buf.num_frames();
    match mode {
        ChannelConvertMode::StereoToMonoMix => {
            let mut out = Vec::with_capacity(n);
            for f in 0..n {
                let mut sum = 0.0;
                for c in 0..buf.channels {
                    sum += buf.get_sample(f, c);
                }
                out.push((sum / buf.channels as f32).clamp(-1.0, 1.0));
            }
            AudioBuffer::new(buf.sample_rate, 1, out)
        }
        ChannelConvertMode::MonoToStereo => {
            let mut out = Vec::with_capacity(n * 2);
            for f in 0..n {
                let l = buf.get_sample(f, 0);
                let r = if buf.channels > 1 {
                    buf.get_sample(f, 1)
                } else {
                    l
                };
                out.push(l);
                out.push(r);
            }
            AudioBuffer::new(buf.sample_rate, 2, out)
        }
        ChannelConvertMode::SwapLeftRight => {
            if buf.channels < 2 {
                return buf.clone();
            }
            let mut out = buf.clone();
            for f in 0..n {
                out.samples.swap(f * 2, f * 2 + 1);
            }
            out
        }
        ChannelConvertMode::LeftOnlyToMono => {
            let mut out = Vec::with_capacity(n);
            for f in 0..n {
                out.push(buf.get_sample(f, 0));
            }
            AudioBuffer::new(buf.sample_rate, 1, out)
        }
        ChannelConvertMode::RightOnlyToMono => {
            let ch = (buf.channels - 1).min(1);
            let mut out = Vec::with_capacity(n);
            for f in 0..n {
                out.push(buf.get_sample(f, ch));
            }
            AudioBuffer::new(buf.sample_rate, 1, out)
        }
        ChannelConvertMode::VocalCutCenterCancel => {
            let mut out = Vec::with_capacity(n * 2);
            for f in 0..n {
                let l = buf.get_sample(f, 0);
                let r = if buf.channels > 1 {
                    buf.get_sample(f, 1)
                } else {
                    l
                };
                let diff = ((l - r) * 0.707).clamp(-1.0, 1.0);
                out.push(diff);
                out.push(diff);
            }
            AudioBuffer::new(buf.sample_rate, 2, out)
        }
    }
}

/// Sound Forge `Pan/Expand`: adjusts stereo pan (-1.0 Left .. +1.0 Right) and stereo width (0.0 Mono .. 2.0 Wide).
pub fn pan_expand_range(
    buf: &AudioBuffer,
    start: usize,
    end: usize,
    pan: f32,
    width: f32,
) -> AudioBuffer {
    let mut out = if buf.channels < 2 {
        convert_channels(buf, ChannelConvertMode::MonoToStereo)
    } else {
        buf.clone()
    };
    let n = out.num_frames();
    let s = start.min(n);
    let e = end.min(n).max(s);
    let pan = pan.clamp(-1.0, 1.0);
    let width = width.clamp(0.0, 2.0);
    let gain_l = (1.0 - pan.max(0.0)).clamp(0.0, 1.0);
    let gain_r = (1.0 + pan.min(0.0)).clamp(0.0, 1.0);

    for f in s..e {
        let l = out.get_sample(f, 0);
        let r = out.get_sample(f, 1);
        let mid = (l + r) * 0.5;
        let side = (l - r) * 0.5 * width;
        let nl = ((mid + side) * gain_l).clamp(-1.0, 1.0);
        let nr = ((mid - side) * gain_r).clamp(-1.0, 1.0);
        out.samples[f * 2] = nl;
        out.samples[f * 2 + 1] = nr;
    }
    out
}

/// Sound Forge `Rotate Audio`: circularly rotates frames in [start, end) by `offset_frames`.
pub fn rotate_audio_range(
    buf: &AudioBuffer,
    start: usize,
    end: usize,
    offset_frames: usize,
) -> AudioBuffer {
    let mut out = buf.clone();
    let n = out.num_frames();
    let s = start.min(n);
    let e = end.min(n).max(s);
    let len = e - s;
    if len < 2 {
        return out;
    }
    let shift = (offset_frames % len) * out.channels;
    out.samples[s * out.channels..e * out.channels].rotate_left(shift);
    out
}

/// Sound Forge `Smooth/Enhance`: `amount` in -5 (smooth high-freq clicks) to +5 (enhance clarity).
pub fn smooth_enhance_range(
    buf: &AudioBuffer,
    start: usize,
    end: usize,
    amount: i32,
) -> AudioBuffer {
    let mut out = buf.clone();
    let n = out.num_frames();
    let s = start.min(n);
    let e = end.min(n).max(s);
    if e.saturating_sub(s) < 4 || amount == 0 {
        return out;
    }
    let ch = out.channels;
    let amt = (amount.clamp(-5, 5) as f32) * 0.12;

    for c in 0..ch {
        let mut prev = buf.get_sample(s, c);
        for f in (s + 1)..(e - 1) {
            let cur = buf.get_sample(f, c);
            let next = buf.get_sample(f + 1, c);
            let avg = 0.25 * prev + 0.5 * cur + 0.25 * next;
            let detail = cur - avg;
            let val = if amt > 0.0 {
                cur + detail * (amt * 2.5)
            } else {
                cur + (avg - cur) * (-amt * 1.5)
            };
            out.samples[f * ch + c] = val.clamp(-1.0, 1.0);
            prev = cur;
        }
    }
    out
}

/// 3-Band EQ (Low < 250 Hz, Mid ~ 1 kHz, High > 4 kHz) in dB (-18..+18 dB).
pub fn apply_3band_eq(
    buf: &AudioBuffer,
    start: usize,
    end: usize,
    low_db: f32,
    mid_db: f32,
    high_db: f32,
) -> AudioBuffer {
    let mut out = buf.clone();
    let n = out.num_frames();
    let s = start.min(n);
    let e = end.min(n).max(s);
    if e <= s {
        return out;
    }
    let sr = out.sample_rate as f32;
    let ch = out.channels;

    let g_low = 10.0_f32.powf(low_db / 20.0);
    let g_mid = 10.0_f32.powf(mid_db / 20.0);
    let g_high = 10.0_f32.powf(high_db / 20.0);

    // One-pole crossover coefficients at 250 Hz and 3500 Hz
    let a_low = (-2.0 * PI * 250.0 / sr).exp();
    let a_high = (-2.0 * PI * 3500.0 / sr).exp();

    for c in 0..ch {
        let mut lp1 = 0.0_f32;
        let mut lp2 = 0.0_f32;
        for f in s..e {
            let x = out.get_sample(f, c);
            lp1 = (1.0 - a_low) * x + a_low * lp1;
            lp2 = (1.0 - a_high) * x + a_high * lp2;
            let band_low = lp1;
            let band_mid = lp2 - lp1;
            let band_high = x - lp2;
            let y = band_low * g_low + band_mid * g_mid + band_high * g_high;
            out.samples[f * ch + c] = y.clamp(-1.0, 1.0);
        }
    }
    out
}

/// Simple Echo/Delay effect.
pub fn apply_delay_echo(
    buf: &AudioBuffer,
    start: usize,
    end: usize,
    delay_ms: f32,
    feedback: f32,
    wet: f32,
) -> AudioBuffer {
    let mut out = buf.clone();
    let n = out.num_frames();
    let s = start.min(n);
    let e = end.min(n).max(s);
    let delay_frames = ((delay_ms.clamp(10.0, 2000.0) / 1000.0) * out.sample_rate as f32).round()
        as usize;
    if delay_frames == 0 || e <= s {
        return out;
    }
    let fb = feedback.clamp(0.0, 0.88);
    let wet = wet.clamp(0.0, 1.0);
    let ch = out.channels;

    for c in 0..ch {
        let mut line = vec![0.0_f32; delay_frames];
        let mut ptr = 0;
        for f in s..e {
            let dry = out.get_sample(f, c);
            let delayed = line[ptr];
            line[ptr] = (dry + delayed * fb).clamp(-1.0, 1.0);
            ptr = (ptr + 1) % delay_frames;
            out.samples[f * ch + c] = (dry * (1.0 - wet * 0.4) + delayed * wet).clamp(-1.0, 1.0);
        }
    }
    out
}

/// Studio Reverb (parallel comb filters + series allpass).
pub fn apply_reverb(
    buf: &AudioBuffer,
    start: usize,
    end: usize,
    room_size: f32,
    damping: f32,
    wet: f32,
) -> AudioBuffer {
    let mut out = buf.clone();
    let n = out.num_frames();
    let s = start.min(n);
    let e = end.min(n).max(s);
    if e <= s {
        return out;
    }
    let ch = out.channels;
    let sr_scale = out.sample_rate as f32 / 44100.0;
    let comb_lens = [
        (1116.0 * sr_scale) as usize,
        (1188.0 * sr_scale) as usize,
        (1277.0 * sr_scale) as usize,
        (1356.0 * sr_scale) as usize,
    ];
    let ap_lens = [(556.0 * sr_scale) as usize, (441.0 * sr_scale) as usize];
    let fb = 0.70 + room_size.clamp(0.0, 1.0) * 0.24;
    let damp = damping.clamp(0.05, 0.9);
    let wet = wet.clamp(0.0, 1.0);

    for c in 0..ch {
        let stereo_spread = c * 23;
        let mut combs: Vec<Vec<f32>> = comb_lens
            .iter()
            .map(|&l| vec![0.0; (l + stereo_spread).max(16)])
            .collect();
        let mut comb_ptrs = [0_usize; 4];
        let mut comb_lp = [0.0_f32; 4];

        let mut aps: Vec<Vec<f32>> = ap_lens
            .iter()
            .map(|&l| vec![0.0; (l + stereo_spread).max(16)])
            .collect();
        let mut ap_ptrs = [0_usize; 2];

        for f in s..e {
            let input = out.get_sample(f, c);
            let mut acc = 0.0_f32;
            for i in 0..4 {
                let p = comb_ptrs[i];
                let y = combs[i][p];
                comb_lp[i] = y * (1.0 - damp) + comb_lp[i] * damp;
                combs[i][p] = input + comb_lp[i] * fb;
                comb_ptrs[i] = (p + 1) % combs[i].len();
                acc += y;
            }
            acc *= 0.25;

            for i in 0..2 {
                let p = ap_ptrs[i];
                let buf_out = aps[i][p];
                let new_val = acc + buf_out * 0.5;
                aps[i][p] = new_val;
                acc = buf_out - new_val * 0.5;
                ap_ptrs[i] = (p + 1) % aps[i].len();
            }

            out.samples[f * ch + c] =
                (input * (1.0 - wet * 0.5) + acc * wet).clamp(-1.0, 1.0);
        }
    }
    out
}

/// Resamples the entire buffer to a new sample rate (Sound Forge `Process -> Resample`).
pub fn resample_to_rate(buf: &AudioBuffer, target_sr: u32) -> AudioBuffer {
    let target_sr = target_sr.clamp(8000, 192000);
    if buf.sample_rate == target_sr {
        return buf.clone();
    }
    let ratio = buf.sample_rate as f64 / target_sr as f64;
    let mut resampled = varispeed_resample(buf, ratio);
    resampled.sample_rate = target_sr;
    resampled
}

/// Applies speed change to [start, end) and splices back into the full buffer.
pub fn change_speed_range(
    buf: &AudioBuffer,
    start: usize,
    end: usize,
    speed_factor: f64,
    mode: SpeedMode,
) -> AudioBuffer {
    let algo = match mode {
        SpeedMode::PreservePitch => TimeStretchAlgorithm::Music2Balanced,
        SpeedMode::Varispeed => TimeStretchAlgorithm::VarispeedTape,
    };
    change_speed_range_with_algo(buf, start, end, speed_factor, algo)
}

pub fn change_speed_range_with_algo(
    buf: &AudioBuffer,
    start: usize,
    end: usize,
    speed_factor: f64,
    algo: TimeStretchAlgorithm,
) -> AudioBuffer {
    let n = buf.num_frames();
    let s = start.min(n);
    let e = end.min(n).max(s);
    if e - s < 32 || (speed_factor - 1.0).abs() < 1e-4 {
        return buf.clone();
    }
    let speed = speed_factor.clamp(0.20, 4.0);
    let segment = slice_frames(buf, s, e);
    let processed = if algo == TimeStretchAlgorithm::VarispeedTape {
        varispeed_resample(&segment, speed)
    } else {
        time_stretch_wsola_window(&segment, speed, algo.window_ms())
    };

    let ch = buf.channels;
    let mut out = Vec::with_capacity((s + processed.num_frames() + (n - e)) * ch);
    out.extend_from_slice(&buf.samples[..s * ch]);
    out.extend_from_slice(&processed.samples);
    out.extend_from_slice(&buf.samples[e * ch..]);
    AudioBuffer::new(buf.sample_rate, ch, out)
}

/// Shifts pitch by `semitones` (without changing duration if `preserve_duration` is true) on [start, end).
pub fn pitch_shift_range(
    buf: &AudioBuffer,
    start: usize,
    end: usize,
    semitones: f64,
) -> AudioBuffer {
    pitch_shift_range_ex(buf, start, end, semitones, true)
}

pub fn pitch_shift_range_ex(
    buf: &AudioBuffer,
    start: usize,
    end: usize,
    semitones: f64,
    preserve_duration: bool,
) -> AudioBuffer {
    let n = buf.num_frames();
    let s = start.min(n);
    let e = end.min(n).max(s);
    if e - s < 64 || semitones.abs() < 1e-3 {
        return buf.clone();
    }
    let ratio = 2.0_f64.powf(semitones.clamp(-24.0, 24.0) / 12.0);
    let segment = slice_frames(buf, s, e);
    let shifted = if preserve_duration {
        let stretched = time_stretch_wsola(&segment, 1.0 / ratio);
        varispeed_resample(&stretched, ratio)
    } else {
        varispeed_resample(&segment, ratio)
    };

    let ch = buf.channels;
    let mut out = Vec::with_capacity((s + shifted.num_frames() + (n - e)) * ch);
    out.extend_from_slice(&buf.samples[..s * ch]);
    out.extend_from_slice(&shifted.samples);
    out.extend_from_slice(&buf.samples[e * ch..]);
    AudioBuffer::new(buf.sample_rate, ch, out)
}

pub fn time_stretch_wsola(buf: &AudioBuffer, speed_factor: f64) -> AudioBuffer {
    time_stretch_wsola_window(buf, speed_factor, 60.0)
}

/// Pure-Rust WSOLA (Waveform Similarity Overlap-Add) time-stretcher with configurable window_ms.
pub fn time_stretch_wsola_window(
    buf: &AudioBuffer,
    speed_factor: f64,
    window_ms: f64,
) -> AudioBuffer {
    let n = buf.num_frames();
    let ch = buf.channels;
    let speed = speed_factor.clamp(0.20, 4.0);
    if n < 256 || (speed - 1.0).abs() < 1e-4 {
        return buf.clone();
    }

    let win_len = (((buf.sample_rate as f64 * (window_ms.clamp(15.0, 120.0) / 1000.0)).round()
        as usize)
        .clamp(256, 8192))
        & !1;
    if n <= win_len {
        return varispeed_resample(buf, speed);
    }

    let syn_hop = win_len / 2;
    let ana_hop_f = syn_hop as f64 * speed;
    let search_radius = (win_len / 4).max(64);

    let hann: Vec<f32> = (0..win_len)
        .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f32 / win_len as f32).cos()))
        .collect();

    let mut mono = Vec::with_capacity(n);
    for f in 0..n {
        let mut s = 0.0_f32;
        for c in 0..ch {
            s += buf.get_sample(f, c);
        }
        mono.push(s / ch as f32);
    }

    let est_out_frames = ((n as f64 / speed).ceil() as usize).max(win_len) + win_len;
    let mut out = vec![0.0_f32; est_out_frames * ch];
    let mut norm = vec![0.0_f32; est_out_frames];

    for i in 0..win_len.min(n) {
        let w = hann[i];
        norm[i] += w;
        for c in 0..ch {
            out[i * ch + c] += buf.get_sample(i, c) * w;
        }
    }

    let mut out_pos = syn_hop;
    let mut ana_pos_f = ana_hop_f;
    let mut prev_ana_end = syn_hop;

    while (ana_pos_f as usize) + win_len < n {
        let nominal_ana = ana_pos_f.round() as usize;
        let ref_start = prev_ana_end.min(n.saturating_sub(win_len));
        let min_cand = nominal_ana.saturating_sub(search_radius);
        let max_cand = (nominal_ana + search_radius).min(n - win_len);

        let mut best_ana = nominal_ana.min(n - win_len);
        let mut best_score = f32::NEG_INFINITY;

        let cmp_len = syn_hop.min(n.saturating_sub(ref_start)).min(1024);
        // Stage 1: Fast coarse search
        let mut coarse_best = best_ana;
        let mut cand = min_cand;
        while cand <= max_cand {
            let mut dot = 0.0_f32;
            let mut e_ref = 1e-9_f32;
            let mut e_cand = 1e-9_f32;
            let mut i = 0;
            while i < cmp_len {
                let r = mono[ref_start + i];
                let x = mono[cand + i];
                dot += r * x;
                e_ref += r * r;
                e_cand += x * x;
                i += 4;
            }
            let score = dot / (e_ref * e_cand).sqrt();
            if score > best_score {
                best_score = score;
                coarse_best = cand;
            }
            cand += 8;
        }

        // Stage 2: Fine single-sample refinement around coarse_best
        let fine_min = coarse_best.saturating_sub(8).max(min_cand);
        let fine_max = (coarse_best + 8).min(max_cand);
        best_score = f32::NEG_INFINITY;
        for f_cand in fine_min..=fine_max {
            let mut dot = 0.0_f32;
            let mut e_ref = 1e-9_f32;
            let mut e_cand = 1e-9_f32;
            let mut i = 0;
            while i < cmp_len {
                let r = mono[ref_start + i];
                let x = mono[f_cand + i];
                dot += r * x;
                e_ref += r * r;
                e_cand += x * x;
                i += 2;
            }
            let score = dot / (e_ref * e_cand).sqrt();
            if score > best_score {
                best_score = score;
                best_ana = f_cand;
            }
        }

        if out_pos + win_len >= norm.len() {
            let extra = win_len * 2;
            norm.resize(norm.len() + extra, 0.0);
            out.resize(out.len() + extra * ch, 0.0);
        }

        for i in 0..win_len {
            let w = hann[i];
            norm[out_pos + i] += w;
            for c in 0..ch {
                out[(out_pos + i) * ch + c] += buf.get_sample(best_ana + i, c) * w;
            }
        }

        prev_ana_end = best_ana + syn_hop;
        out_pos += syn_hop;
        ana_pos_f += ana_hop_f;
    }

    let final_frames = (out_pos + syn_hop).min(norm.len());
    let mut final_samples = Vec::with_capacity(final_frames * ch);
    for f in 0..final_frames {
        let denom = norm[f];
        for c in 0..ch {
            let v = if denom > 1e-4 {
                out[f * ch + c] / denom
            } else {
                out[f * ch + c]
            };
            final_samples.push(v.clamp(-1.0, 1.0));
        }
    }

    AudioBuffer::new(buf.sample_rate, ch, final_samples)
}

/// Resamples audio with Catmull-Rom cubic interpolation for varispeed.
pub fn varispeed_resample(buf: &AudioBuffer, speed_factor: f64) -> AudioBuffer {
    let n = buf.num_frames();
    let ch = buf.channels;
    let speed = speed_factor.clamp(0.1, 8.0);
    if n < 2 || (speed - 1.0).abs() < 1e-5 {
        return buf.clone();
    }

    let out_frames = ((n as f64) / speed).round().max(1.0) as usize;
    let mut out = Vec::with_capacity(out_frames * ch);

    for of in 0..out_frames {
        let src_pos = of as f64 * speed;
        let idx = src_pos.floor() as usize;
        let frac = (src_pos - idx as f64) as f32;

        let i0 = idx.saturating_sub(1).min(n - 1);
        let i1 = idx.min(n - 1);
        let i2 = (idx + 1).min(n - 1);
        let i3 = (idx + 2).min(n - 1);

        for c in 0..ch {
            let y0 = buf.get_sample(i0, c);
            let y1 = buf.get_sample(i1, c);
            let y2 = buf.get_sample(i2, c);
            let y3 = buf.get_sample(i3, c);

            let a0 = -0.5 * y0 + 1.5 * y1 - 1.5 * y2 + 0.5 * y3;
            let a1 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
            let a2 = -0.5 * y0 + 0.5 * y2;
            let a3 = y1;
            let sample = ((a0 * frac + a1) * frac + a2) * frac + a3;
            out.push(sample.clamp(-1.0, 1.0));
        }
    }

    AudioBuffer::new(buf.sample_rate, ch, out)
}

pub fn adapt_buffer(clip: &AudioBuffer, target_sr: u32, target_channels: usize) -> AudioBuffer {
    let mut tmp = if clip.channels == target_channels {
        clip.clone()
    } else {
        let n = clip.num_frames();
        let mut out = Vec::with_capacity(n * target_channels);
        for f in 0..n {
            if target_channels == 1 {
                let mut sum = 0.0;
                for c in 0..clip.channels {
                    sum += clip.get_sample(f, c);
                }
                out.push(sum / clip.channels as f32);
            } else {
                let l = clip.get_sample(f, 0);
                let r = if clip.channels > 1 {
                    clip.get_sample(f, 1)
                } else {
                    l
                };
                out.push(l);
                out.push(r);
            }
        }
        AudioBuffer::new(clip.sample_rate, target_channels, out)
    };

    if tmp.sample_rate != target_sr && target_sr > 0 {
        let ratio = tmp.sample_rate as f64 / target_sr as f64;
        tmp = varispeed_resample(&tmp, ratio);
        tmp.sample_rate = target_sr;
    }
    tmp
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine_buffer(sr: u32, secs: f64, freq: f32, channels: usize) -> AudioBuffer {
        let frames = (sr as f64 * secs) as usize;
        let mut samples = Vec::with_capacity(frames * channels);
        for f in 0..frames {
            let s = (2.0 * PI * freq * f as f32 / sr as f32).sin() * 0.5;
            for _ in 0..channels {
                samples.push(s);
            }
        }
        AudioBuffer::new(sr, channels, samples)
    }

    #[test]
    fn test_slice_and_delete() {
        let buf = sine_buffer(44100, 1.0, 440.0, 2);
        let cut = slice_frames(&buf, 1000, 5000);
        assert_eq!(cut.num_frames(), 4000);

        let deleted = delete_range(&buf, 1000, 5000);
        assert_eq!(deleted.num_frames(), 44100 - 4000);
    }

    #[test]
    fn test_wsola_speed_up_and_slow_down() {
        let buf = sine_buffer(44100, 1.0, 440.0, 2);
        let faster = time_stretch_wsola(&buf, 1.5);
        let ratio_fast = faster.duration_secs() / buf.duration_secs();
        assert!(
            (ratio_fast - (1.0 / 1.5)).abs() < 0.08,
            "unexpected faster ratio {ratio_fast}"
        );

        let slower = time_stretch_wsola(&buf, 0.75);
        let ratio_slow = slower.duration_secs() / buf.duration_secs();
        assert!(
            (ratio_slow - (1.0 / 0.75)).abs() < 0.10,
            "unexpected slower ratio {ratio_slow}"
        );
    }

    #[test]
    fn test_normalize_and_fades() {
        let buf = sine_buffer(44100, 0.5, 440.0, 1);
        let norm = normalize_peak(&buf, 0, buf.num_frames(), 0.0);
        assert!((norm.peak_amplitude() - 1.0).abs() < 0.01);

        let fin = fade_in(&norm, 0, norm.num_frames(), FadeCurve::Linear);
        assert!(fin.get_sample(0, 0).abs() < 1e-5);

        let fout = fade_out(&norm, 0, norm.num_frames(), FadeCurve::SmoothSCurve);
        assert!(fout.get_sample(fout.num_frames() - 1, 0).abs() < 1e-5);
    }

    #[test]
    fn test_extend_with_crossfade() {
        let buf = sine_buffer(44100, 2.0, 440.0, 2);
        let extended = extend_range_with_crossfade(&buf, 22050, 44100, 1.5, 25.0);
        assert!((extended.duration_secs() - 3.5).abs() < 0.05);
    }

    #[test]
    fn test_concat_and_process_effects() {
        let a = sine_buffer(44100, 1.0, 440.0, 2);
        let b = sine_buffer(44100, 1.0, 880.0, 2);
        let joined = concat_with_crossfade(&a, &b, 0.1);
        assert!((joined.duration_secs() - 1.9).abs() < 0.02);

        let (dc_clean, _) = remove_dc_offset(&joined, 0, joined.num_frames());
        assert_eq!(dc_clean.num_frames(), joined.num_frames());

        let mono = convert_channels(&dc_clean, ChannelConvertMode::StereoToMonoMix);
        assert_eq!(mono.channels, 1);

        let eq = apply_3band_eq(&dc_clean, 0, 4410, 3.0, -2.0, 2.0);
        assert_eq!(eq.num_frames(), dc_clean.num_frames());
    }
}
