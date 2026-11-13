use crate::audio::AudioBuffer;
use std::f32::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FadeCurve {
    Linear,
    SmoothSCurve,
    Exponential,
}

impl FadeCurve {
    pub fn label(&self) -> &'static str {
        match self {
            FadeCurve::Linear => "Linear",
            FadeCurve::SmoothSCurve => "Suave (Cosine S-Curve)",
            FadeCurve::Exponential => "Exponencial",
        }
    }

    pub fn eval(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            FadeCurve::Linear => t,
            FadeCurve::SmoothSCurve => 0.5 - 0.5 * (PI * t).cos(),
            FadeCurve::Exponential => t * t,
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

    // Build repeated extension tail from the selected phrase.
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
    // Crossfade start of extension into the end of the selection, then append remainder of original.
    let mut out = Vec::with_capacity(buf.samples.len() + ext_buf.samples.len());
    out.extend_from_slice(&buf.samples[..e * ch]);
    out.extend_from_slice(&ext_buf.samples);
    out.extend_from_slice(&buf.samples[e * ch..]);

    // Smooth boundary around frame `e`
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
    let gain = 10.0_f32.powf(db / 20.0);
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

/// Normalizes [start, end) so peak reaches `target_db` (e.g. -1.0 dB or 0.0 dB).
pub fn normalize_peak(buf: &AudioBuffer, start: usize, end: usize, target_db: f32) -> AudioBuffer {
    let n = buf.num_frames();
    let s = start.min(n);
    let e = end.min(n).max(s);
    let ch = buf.channels;

    let mut peak = 0.0_f32;
    for f in s..e {
        for c in 0..ch {
            peak = peak.max(buf.get_sample(f, c).abs());
        }
    }
    if peak < 1e-7 {
        return buf.clone();
    }

    let target_amp = 10.0_f32.powf(target_db / 20.0).min(1.0);
    let gain = target_amp / peak;
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
        let g = curve.eval(i as f32 / (len - 1) as f32);
        let f = s + i;
        for c in 0..ch {
            out.samples[f * ch + c] *= g;
        }
    }
    out
}

pub fn fade_out(buf: &AudioBuffer, start: usize, end: usize, curve: FadeCurve) -> AudioBuffer {
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
        let g = curve.eval(1.0 - (i as f32 / (len - 1) as f32));
        let f = s + i;
        for c in 0..ch {
            out.samples[f * ch + c] *= g;
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

/// Applies speed change to [start, end) and splices back into the full buffer.
/// `speed_factor > 1.0` speeds up (shorter duration); `< 1.0` slows down (longer duration).
pub fn change_speed_range(
    buf: &AudioBuffer,
    start: usize,
    end: usize,
    speed_factor: f64,
    mode: SpeedMode,
) -> AudioBuffer {
    let n = buf.num_frames();
    let s = start.min(n);
    let e = end.min(n).max(s);
    if e - s < 32 || (speed_factor - 1.0).abs() < 1e-4 {
        return buf.clone();
    }
    let speed = speed_factor.clamp(0.25, 4.0);
    let segment = slice_frames(buf, s, e);
    let processed = match mode {
        SpeedMode::PreservePitch => time_stretch_wsola(&segment, speed),
        SpeedMode::Varispeed => varispeed_resample(&segment, speed),
    };

    let ch = buf.channels;
    let mut out = Vec::with_capacity(
        (s + processed.num_frames() + (n - e)) * ch,
    );
    out.extend_from_slice(&buf.samples[..s * ch]);
    out.extend_from_slice(&processed.samples);
    out.extend_from_slice(&buf.samples[e * ch..]);
    AudioBuffer::new(buf.sample_rate, ch, out)
}

/// Shifts pitch by `semitones` (without changing duration) on [start, end).
pub fn pitch_shift_range(
    buf: &AudioBuffer,
    start: usize,
    end: usize,
    semitones: f64,
) -> AudioBuffer {
    let n = buf.num_frames();
    let s = start.min(n);
    let e = end.min(n).max(s);
    if e - s < 64 || semitones.abs() < 1e-3 {
        return buf.clone();
    }
    let ratio = 2.0_f64.powf(semitones.clamp(-12.0, 12.0) / 12.0);
    let segment = slice_frames(buf, s, e);
    // Step 1: Time-stretch by ratio so after varispeed by ratio, duration stays original!
    let stretched = time_stretch_wsola(&segment, 1.0 / ratio);
    let shifted = varispeed_resample(&stretched, ratio);

    let ch = buf.channels;
    let mut out = Vec::with_capacity((s + shifted.num_frames() + (n - e)) * ch);
    out.extend_from_slice(&buf.samples[..s * ch]);
    out.extend_from_slice(&shifted.samples);
    out.extend_from_slice(&buf.samples[e * ch..]);
    AudioBuffer::new(buf.sample_rate, ch, out)
}

/// Pure-Rust WSOLA (Waveform Similarity Overlap-Add) time-stretcher.
/// `speed_factor > 1.0` accelerates; `< 1.0` decelerates, preserving pitch and stereo phase.
pub fn time_stretch_wsola(buf: &AudioBuffer, speed_factor: f64) -> AudioBuffer {
    let n = buf.num_frames();
    let ch = buf.channels;
    let speed = speed_factor.clamp(0.25, 4.0);
    if n < 256 || (speed - 1.0).abs() < 1e-4 {
        return buf.clone();
    }

    // Window ~60ms at 44.1kHz, rounded to even size.
    let win_len = (((buf.sample_rate as f64 * 0.060).round() as usize).clamp(512, 4096)) & !1;
    if n <= win_len {
        return varispeed_resample(buf, speed);
    }

    let syn_hop = win_len / 2; // 50% overlap Hann window sums cleanly to 1.0!
    let ana_hop_f = syn_hop as f64 * speed;
    let search_radius = (win_len / 4).max(64);

    // Precompute Hann window.
    let hann: Vec<f32> = (0..win_len)
        .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f32 / win_len as f32).cos()))
        .collect();

    // Mono mix for fast waveform similarity alignment.
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

    // Copy first window from frame 0.
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

        // Reference segment from previous synthesis overlap (the natural continuation of prev window).
        let ref_start = prev_ana_end.min(n.saturating_sub(win_len));
        let min_cand = nominal_ana.saturating_sub(search_radius);
        let max_cand = (nominal_ana + search_radius).min(n - win_len);

        let mut best_ana = nominal_ana.min(n - win_len);
        let mut best_score = f32::NEG_INFINITY;

        // Evaluate candidate offsets on the overlapping first half of the window.
        let cmp_len = syn_hop.min(n.saturating_sub(ref_start));
        // Step by 2 samples for speed while retaining sub-millisecond alignment accuracy.
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
                i += 2;
            }
            let score = dot / (e_ref * e_cand).sqrt();
            if score > best_score {
                best_score = score;
                best_ana = cand;
            }
            cand += 2;
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

/// Resamples audio with linear/cubic interpolation for varispeed (changes speed + pitch).
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

            // Catmull-Rom cubic interpolation for clean audio resampling.
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

/// Adapts channel count and sample rate of `clip` to match target.
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
}
