use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;
use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Interleaved 32-bit float audio buffer normalized to [-1.0, 1.0].
#[derive(Clone, Debug)]
pub struct AudioBuffer {
    pub sample_rate: u32,
    pub channels: usize,
    pub samples: Vec<f32>,
}

impl AudioBuffer {
    pub fn new(sample_rate: u32, channels: usize, samples: Vec<f32>) -> Self {
        let channels = channels.max(1);
        Self {
            sample_rate: sample_rate.max(8000),
            channels,
            samples,
        }
    }

    pub fn empty(sample_rate: u32, channels: usize) -> Self {
        Self::new(sample_rate, channels, Vec::new())
    }

    pub fn num_frames(&self) -> usize {
        if self.channels == 0 {
            0
        } else {
            self.samples.len() / self.channels
        }
    }

    pub fn duration_secs(&self) -> f64 {
        self.num_frames() as f64 / self.sample_rate as f64
    }

    pub fn frame_to_secs(&self, frame: usize) -> f64 {
        frame as f64 / self.sample_rate as f64
    }

    pub fn secs_to_frame(&self, secs: f64) -> usize {
        let f = (secs * self.sample_rate as f64).round();
        if f <= 0.0 {
            0
        } else {
            (f as usize).min(self.num_frames())
        }
    }

    #[inline]
    pub fn get_sample(&self, frame: usize, channel: usize) -> f32 {
        let ch = channel.min(self.channels - 1);
        let idx = frame * self.channels + ch;
        self.samples.get(idx).copied().unwrap_or(0.0)
    }

    #[inline]
    pub fn set_sample(&mut self, frame: usize, channel: usize, val: f32) {
        if channel < self.channels {
            let idx = frame * self.channels + channel;
            if let Some(slot) = self.samples.get_mut(idx) {
                *slot = val;
            }
        }
    }

    /// Finds the nearest zero-crossing frame within `max_search_frames` of `target`.
    pub fn find_nearest_zero_crossing(&self, target: usize, max_search_frames: usize) -> usize {
        let n = self.num_frames();
        if n < 2 {
            return target.min(n);
        }
        let target = target.min(n - 1);
        let mono_at = |f: usize| -> f32 {
            let mut sum = 0.0;
            for ch in 0..self.channels {
                sum += self.get_sample(f, ch);
            }
            sum / self.channels as f32
        };

        for offset in 0..=max_search_frames {
            if target + offset + 1 < n {
                let a = mono_at(target + offset);
                let b = mono_at(target + offset + 1);
                if (a <= 0.0 && b >= 0.0) || (a >= 0.0 && b <= 0.0) {
                    return if a.abs() <= b.abs() {
                        target + offset
                    } else {
                        target + offset + 1
                    };
                }
            }
            if offset > 0 && target >= offset {
                let f = target - offset;
                if f + 1 < n {
                    let a = mono_at(f);
                    let b = mono_at(f + 1);
                    if (a <= 0.0 && b >= 0.0) || (a >= 0.0 && b <= 0.0) {
                        return if a.abs() <= b.abs() { f } else { f + 1 };
                    }
                }
            }
        }
        target
    }

    /// Returns global peak amplitude across all samples.
    pub fn peak_amplitude(&self) -> f32 {
        self.samples
            .iter()
            .fold(0.0_f32, |acc, &s| acc.max(s.abs()))
    }
}

/// Multi-resolution peak cache for fast 60fps waveform rendering.
#[derive(Clone, Debug)]
pub struct WaveformLevel {
    pub block_size: usize,
    /// Per channel: Vec<(min, max)> for each block.
    pub channels: Vec<Vec<(f32, f32)>>,
}

#[derive(Clone, Debug)]
pub struct WaveformPyramid {
    pub levels: Vec<WaveformLevel>,
}

impl WaveformPyramid {
    pub fn build(buffer: &AudioBuffer) -> Self {
        let block_sizes = [64, 512, 4096];
        let num_frames = buffer.num_frames();
        let mut levels = Vec::with_capacity(block_sizes.len());

        for &block_size in &block_sizes {
            let num_blocks = if num_frames == 0 {
                0
            } else {
                (num_frames + block_size - 1) / block_size
            };
            let mut ch_vecs = vec![Vec::with_capacity(num_blocks); buffer.channels];

            for b in 0..num_blocks {
                let start = b * block_size;
                let end = (start + block_size).min(num_frames);
                for ch in 0..buffer.channels {
                    let mut min_v = f32::INFINITY;
                    let mut max_v = f32::NEG_INFINITY;
                    for f in start..end {
                        let s = buffer.get_sample(f, ch);
                        if s < min_v {
                            min_v = s;
                        }
                        if s > max_v {
                            max_v = s;
                        }
                    }
                    if !min_v.is_finite() {
                        min_v = 0.0;
                        max_v = 0.0;
                    }
                    ch_vecs[ch].push((min_v, max_v));
                }
            }

            levels.push(WaveformLevel {
                block_size,
                channels: ch_vecs,
            });
        }

        Self { levels }
    }

    /// Computes (min, max) for frame range [start, end) on `channel`.
    pub fn query_range(&self, buffer: &AudioBuffer, channel: usize, start: usize, end: usize) -> (f32, f32) {
        let n = buffer.num_frames();
        let start = start.min(n);
        let end = end.min(n);
        if start >= end {
            return (0.0, 0.0);
        }
        let span = end - start;

        // Pick finest pyramid level whose block_size fits comfortably inside span.
        let chosen = self
            .levels
            .iter()
            .rev()
            .find(|lvl| span >= lvl.block_size * 2 && channel < lvl.channels.len());

        if let Some(lvl) = chosen {
            let bs = lvl.block_size;
            let first_block = start / bs;
            let last_block = (end - 1) / bs;
            let mut min_v = f32::INFINITY;
            let mut max_v = f32::NEG_INFINITY;

            // Prefix partial block
            let first_aligned = (first_block + 1) * bs;
            if start < first_aligned.min(end) {
                for f in start..first_aligned.min(end) {
                    let s = buffer.get_sample(f, channel);
                    min_v = min_v.min(s);
                    max_v = max_v.max(s);
                }
            } else {
                let (bmin, bmax) = lvl.channels[channel][first_block];
                min_v = min_v.min(bmin);
                max_v = max_v.max(bmax);
            }

            // Full interior blocks
            let interior_start = if start == first_block * bs {
                first_block
            } else {
                first_block + 1
            };
            let interior_end = if end == (last_block + 1) * bs {
                last_block + 1
            } else {
                last_block
            };
            if interior_start < interior_end {
                for b in interior_start..interior_end {
                    let (bmin, bmax) = lvl.channels[channel][b];
                    min_v = min_v.min(bmin);
                    max_v = max_v.max(bmax);
                }
            }

            // Suffix partial block
            let last_block_start = last_block * bs;
            if last_block > first_block && end > last_block_start && end < (last_block + 1) * bs {
                for f in last_block_start.max(start)..end {
                    let s = buffer.get_sample(f, channel);
                    min_v = min_v.min(s);
                    max_v = max_v.max(s);
                }
            }

            if min_v.is_finite() {
                return (min_v, max_v);
            }
        }

        // Exact sample scan for zoomed-in views.
        let mut min_v = f32::INFINITY;
        let mut max_v = f32::NEG_INFINITY;
        for f in start..end {
            let s = buffer.get_sample(f, channel);
            min_v = min_v.min(s);
            max_v = max_v.max(s);
        }
        if !min_v.is_finite() {
            (0.0, 0.0)
        } else {
            (min_v, max_v)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    Wav16,
    Wav24,
    Wav32Float,
    Mp3_320,
    Mp3_192,
    Flac,
    OggVorbis,
    M4aAac,
}

impl ExportFormat {
    pub fn label(&self) -> &'static str {
        match self {
            ExportFormat::Wav16 => "WAV 16-bit PCM (nativo)",
            ExportFormat::Wav24 => "WAV 24-bit PCM (nativo)",
            ExportFormat::Wav32Float => "WAV 32-bit Float (nativo)",
            ExportFormat::Mp3_320 => "MP3 320 kbps (FFmpeg)",
            ExportFormat::Mp3_192 => "MP3 192 kbps (FFmpeg)",
            ExportFormat::Flac => "FLAC Lossless (FFmpeg)",
            ExportFormat::OggVorbis => "OGG Vorbis (FFmpeg)",
            ExportFormat::M4aAac => "M4A AAC 256 kbps (FFmpeg)",
        }
    }

    pub fn extension(&self) -> &'static str {
        match self {
            ExportFormat::Wav16 | ExportFormat::Wav24 | ExportFormat::Wav32Float => "wav",
            ExportFormat::Mp3_320 | ExportFormat::Mp3_192 => "mp3",
            ExportFormat::Flac => "flac",
            ExportFormat::OggVorbis => "ogg",
            ExportFormat::M4aAac => "m4a",
        }
    }

    pub fn requires_ffmpeg(&self) -> bool {
        !matches!(
            self,
            ExportFormat::Wav16 | ExportFormat::Wav24 | ExportFormat::Wav32Float
        )
    }

    pub fn all() -> &'static [ExportFormat] {
        &[
            ExportFormat::Wav16,
            ExportFormat::Wav24,
            ExportFormat::Wav32Float,
            ExportFormat::Mp3_320,
            ExportFormat::Mp3_192,
            ExportFormat::Flac,
            ExportFormat::OggVorbis,
            ExportFormat::M4aAac,
        ]
    }
}

/// Decodes an audio file using Symphonia, falling back to FFmpeg if needed.
pub fn load_audio(path: &Path) -> Result<AudioBuffer, String> {
    match load_audio_symphonia(path) {
        Ok(buf) if buf.num_frames() > 0 => Ok(buf),
        Ok(_) => Err("Arquivo de áudio vazio ou sem quadros decodificáveis.".to_string()),
        Err(sym_err) => {
            // Try FFmpeg fallback for formats Symphonia cannot open directly.
            if let Some(ffmpeg) = find_ffmpeg() {
                load_audio_ffmpeg(&ffmpeg, path).map_err(|ff_err| {
                    format!("Falha Symphonia ({sym_err}) e fallback FFmpeg ({ff_err})")
                })
            } else {
                Err(format!(
                    "Não foi possível abrir o arquivo ({sym_err}). Instale `ffmpeg` para formatos extras."
                ))
            }
        }
    }
}

fn load_audio_symphonia(path: &Path) -> Result<AudioBuffer, String> {
    let file = File::open(path).map_err(|e| format!("Erro ao abrir arquivo: {e}"))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("Formato não reconhecido: {e}"))?;

    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .ok_or_else(|| "Nenhuma faixa de áudio encontrada".to_string())?;

    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("Erro ao criar decodificador: {e}"))?;

    let mut sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
    let mut channels = track
        .codec_params
        .channels
        .map(|c| c.count())
        .unwrap_or(2)
        .clamp(1, 2);
    let mut out_samples: Vec<f32> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(SymphoniaError::IoError(ref err))
                if err.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(SymphoniaError::IoError(_)) => break,
            Err(e) => return Err(format!("Erro de leitura de pacote: {e}")),
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                sample_rate = spec.rate;
                let src_channels = spec.channels.count();
                if out_samples.is_empty() {
                    channels = src_channels.clamp(1, 2);
                }
                append_decoded_frames(&decoded, src_channels, channels, &mut out_samples);
            }
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(SymphoniaError::IoError(_)) => break,
            Err(e) => return Err(format!("Erro ao decodificar: {e}")),
        }
    }

    Ok(AudioBuffer::new(sample_rate, channels, out_samples))
}

fn append_decoded_frames(
    decoded: &AudioBufferRef<'_>,
    src_channels: usize,
    dst_channels: usize,
    out: &mut Vec<f32>,
) {
    macro_rules! push_planes {
        ($buf:expr, $conv:expr) => {{
            let frames = $buf.frames();
            out.reserve(frames * dst_channels);
            if dst_channels == 1 {
                let p0 = $buf.chan(0);
                for i in 0..frames {
                    out.push($conv(p0[i]));
                }
            } else if src_channels == 1 {
                let p0 = $buf.chan(0);
                for i in 0..frames {
                    let s = $conv(p0[i]);
                    out.push(s);
                    out.push(s);
                }
            } else {
                let p0 = $buf.chan(0);
                let p1 = $buf.chan(1);
                for i in 0..frames {
                    out.push($conv(p0[i]));
                    out.push($conv(p1[i]));
                }
            }
        }};
    }

    match decoded {
        AudioBufferRef::F32(buf) => push_planes!(buf, |x: f32| x),
        AudioBufferRef::F64(buf) => push_planes!(buf, |x: f64| x as f32),
        AudioBufferRef::S32(buf) => push_planes!(buf, |x: i32| (x as f64 / 2147483648.0) as f32),
        AudioBufferRef::S24(buf) => push_planes!(buf, |x: symphonia::core::sample::i24| {
            x.inner() as f32 / 8388608.0
        }),
        AudioBufferRef::S16(buf) => push_planes!(buf, |x: i16| x as f32 / 32768.0),
        AudioBufferRef::S8(buf) => push_planes!(buf, |x: i8| x as f32 / 128.0),
        AudioBufferRef::U8(buf) => push_planes!(buf, |x: u8| (x as f32 - 128.0) / 128.0),
        AudioBufferRef::U16(buf) => push_planes!(buf, |x: u16| (x as f32 - 32768.0) / 32768.0),
        AudioBufferRef::U24(buf) => push_planes!(buf, |x: symphonia::core::sample::u24| {
            (x.inner() as f32 - 8388608.0) / 8388608.0
        }),
        AudioBufferRef::U32(buf) => push_planes!(buf, |x: u32| {
            ((x as f64 - 2147483648.0) / 2147483648.0) as f32
        }),
    }
}

fn load_audio_ffmpeg(ffmpeg: &Path, path: &Path) -> Result<AudioBuffer, String> {
    let tmp_wav = std::env::temp_dir().join(format!(
        "sf_linux_decode_{}.wav",
        std::process::id()
    ));

    let status = Command::new(ffmpeg)
        .arg("-y")
        .arg("-v")
        .arg("error")
        .arg("-i")
        .arg(path)
        .arg("-c:a")
        .arg("pcm_f32le")
        .arg(&tmp_wav)
        .status()
        .map_err(|e| format!("Erro ao executar ffmpeg: {e}"))?;

    if !status.success() {
        let _ = std::fs::remove_file(&tmp_wav);
        return Err("FFmpeg retornou erro na decodificação".to_string());
    }

    let res = load_audio_symphonia(&tmp_wav);
    let _ = std::fs::remove_file(&tmp_wav);
    res
}

pub fn save_wav(buffer: &AudioBuffer, path: &Path, format: ExportFormat) -> Result<(), String> {
    let (bits_per_sample, sample_format) = match format {
        ExportFormat::Wav16 => (16, hound::SampleFormat::Int),
        ExportFormat::Wav24 => (24, hound::SampleFormat::Int),
        ExportFormat::Wav32Float => (32, hound::SampleFormat::Float),
        _ => (24, hound::SampleFormat::Int),
    };

    let spec = hound::WavSpec {
        channels: buffer.channels as u16,
        sample_rate: buffer.sample_rate,
        bits_per_sample,
        sample_format,
    };

    let mut writer =
        hound::WavWriter::create(path, spec).map_err(|e| format!("Erro ao criar WAV: {e}"))?;

    match format {
        ExportFormat::Wav16 => {
            for &s in &buffer.samples {
                let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
                writer
                    .write_sample(v)
                    .map_err(|e| format!("Erro ao gravar sample WAV: {e}"))?;
            }
        }
        ExportFormat::Wav24 => {
            for &s in &buffer.samples {
                let v = (s.clamp(-1.0, 1.0) * 8388607.0).round() as i32;
                writer
                    .write_sample(v)
                    .map_err(|e| format!("Erro ao gravar sample WAV: {e}"))?;
            }
        }
        _ => {
            for &s in &buffer.samples {
                writer
                    .write_sample(s)
                    .map_err(|e| format!("Erro ao gravar sample WAV: {e}"))?;
            }
        }
    }

    writer
        .finalize()
        .map_err(|e| format!("Erro ao finalizar WAV: {e}"))?;
    Ok(())
}

pub fn export_audio(
    buffer: &AudioBuffer,
    path: &Path,
    format: ExportFormat,
    target_sample_rate: Option<u32>,
) -> Result<(), String> {
    if !format.requires_ffmpeg() && target_sample_rate.is_none() {
        return save_wav(buffer, path, format);
    }

    if !format.requires_ffmpeg() && target_sample_rate == Some(buffer.sample_rate) {
        return save_wav(buffer, path, format);
    }

    let ffmpeg = find_ffmpeg().ok_or_else(|| {
        "FFmpeg não encontrado no PATH. Exporte em WAV (nativo) ou instale o pacote `ffmpeg` no Linux."
            .to_string()
    })?;

    let tmp_wav = std::env::temp_dir().join(format!(
        "sf_linux_export_{}.wav",
        std::process::id()
    ));
    save_wav(buffer, &tmp_wav, ExportFormat::Wav32Float)?;

    let mut cmd = Command::new(&ffmpeg);
    cmd.arg("-y").arg("-v").arg("error").arg("-i").arg(&tmp_wav);

    if let Some(sr) = target_sample_rate {
        cmd.arg("-ar").arg(sr.to_string());
    }

    match format {
        ExportFormat::Mp3_320 => {
            cmd.arg("-codec:a").arg("libmp3lame").arg("-b:a").arg("320k");
        }
        ExportFormat::Mp3_192 => {
            cmd.arg("-codec:a").arg("libmp3lame").arg("-b:a").arg("192k");
        }
        ExportFormat::Flac => {
            cmd.arg("-codec:a").arg("flac");
        }
        ExportFormat::OggVorbis => {
            cmd.arg("-codec:a").arg("libvorbis").arg("-q:a").arg("7");
        }
        ExportFormat::M4aAac => {
            cmd.arg("-codec:a").arg("aac").arg("-b:a").arg("256k");
        }
        ExportFormat::Wav16 => {
            cmd.arg("-codec:a").arg("pcm_s16le");
        }
        ExportFormat::Wav24 => {
            cmd.arg("-codec:a").arg("pcm_s24le");
        }
        ExportFormat::Wav32Float => {
            cmd.arg("-codec:a").arg("pcm_f32le");
        }
    }

    cmd.arg(path);
    let status = cmd
        .status()
        .map_err(|e| format!("Falha ao iniciar FFmpeg: {e}"))?;
    let _ = std::fs::remove_file(&tmp_wav);

    if !status.success() {
        return Err("FFmpeg falhou ao exportar o arquivo.".to_string());
    }
    Ok(())
}

pub fn find_ffmpeg() -> Option<PathBuf> {
    let candidates = [
        "ffmpeg",
        "/usr/bin/ffmpeg",
        "/usr/local/bin/ffmpeg",
        "C:\\ffmpeg\\bin\\ffmpeg.exe",
    ];
    for c in candidates {
        let ok = Command::new(c)
            .arg("-version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if ok {
            return Some(PathBuf::from(c));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    #[test]
    fn test_wav_roundtrip_and_pyramid() {
        let sr = 44100;
        let frames = 4410;
        let mut samples = Vec::with_capacity(frames * 2);
        for f in 0..frames {
            let v = (2.0 * PI * 440.0 * f as f32 / sr as f32).sin() * 0.6;
            samples.push(v);
            samples.push(-v);
        }
        let mut buf = AudioBuffer::new(sr, 2, samples);
        buf.set_sample(0, 0, 0.0);

        let tmp = std::env::temp_dir().join(format!("sf_test_{}.wav", std::process::id()));
        save_wav(&buf, &tmp, ExportFormat::Wav24).expect("save_wav failed");
        let loaded = load_audio(&tmp).expect("load_audio failed");
        let _ = std::fs::remove_file(&tmp);

        assert_eq!(loaded.sample_rate, sr);
        assert_eq!(loaded.channels, 2);
        assert_eq!(loaded.num_frames(), frames);

        let pyr = WaveformPyramid::build(&loaded);
        let (min_v, max_v) = pyr.query_range(&loaded, 0, 0, frames);
        assert!(min_v < -0.55 && max_v > 0.55);
    }

    #[test]
    fn test_zero_crossing_snap() {
        let mut samples = vec![0.4, 0.2, -0.1, -0.3, 0.05, 0.5];
        // duplicate into mono frames
        let buf = AudioBuffer::new(44100, 1, samples.split_off(0));
        let zc = buf.find_nearest_zero_crossing(0, 5);
        // Crossing between frame 1 (0.2) and frame 2 (-0.1) -> frame 2 is closer to 0.0
        assert_eq!(zc, 2);
    }
}
