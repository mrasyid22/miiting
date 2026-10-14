//! PCM utilities: downmix, streaming resampler, levels, WAV in/out.

use std::f64::consts::PI;
use std::fs::File;
use std::io::{BufWriter, Cursor};
use std::path::Path;

use crate::{Result, STT_SAMPLE_RATE};

/// Average interleaved channels into mono.
pub fn downmix(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    interleaved.chunks_exact(channels).map(|frame| frame.iter().sum::<f32>() / channels as f32).collect()
}

pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

/// RMS mapped to 0..1 on a -60..0 dBFS scale, for level meters.
pub fn meter_level(samples: &[f32]) -> f32 {
    let db = 20.0 * rms(samples).max(1e-6).log10();
    ((db + 60.0) / 60.0).clamp(0.0, 1.0)
}

/// Streaming windowed-sinc resampler for arbitrary rate pairs (48k/44.1k → 16k in practice).
pub struct Resampler {
    in_rate: u32,
    out_rate: u32,
    step: f64,
    cutoff: f64,
    half: i64,
    buf: Vec<f32>,
    buf_start: i64,
    pos: f64,
    weights: Vec<f64>,
}

impl Resampler {
    pub fn new(in_rate: u32, out_rate: u32) -> Self {
        let ratio = out_rate as f64 / in_rate as f64;
        Resampler {
            in_rate,
            out_rate,
            step: in_rate as f64 / out_rate as f64,
            // Cutoff in cycles per input sample, slightly under the output Nyquist.
            cutoff: 0.5 * ratio.min(1.0) * 0.94,
            half: 16,
            buf: Vec::new(),
            buf_start: 0,
            pos: 0.0,
            weights: Vec::with_capacity(32),
        }
    }

    pub fn in_rate(&self) -> u32 {
        self.in_rate
    }

    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        if self.in_rate == self.out_rate {
            return input.to_vec();
        }
        self.buf.extend_from_slice(input);
        let end = self.buf_start + self.buf.len() as i64;
        let mut out = Vec::with_capacity((input.len() as f64 / self.step) as usize + 2);

        loop {
            let center = self.pos.floor() as i64;
            if center + self.half >= end {
                break;
            }
            self.weights.clear();
            let mut acc = 0.0f64;
            let mut wsum = 0.0f64;
            for k in (center - self.half + 1)..=(center + self.half) {
                let t = self.pos - k as f64;
                let w = self.kernel(t);
                wsum += w;
                let idx = k - self.buf_start;
                if idx >= 0 {
                    acc += w * self.buf[idx as usize] as f64;
                }
            }
            out.push(if wsum.abs() > 1e-9 { (acc / wsum) as f32 } else { 0.0 });
            self.pos += self.step;
        }

        // Drop input no future output can reach.
        let keep_from = self.pos.floor() as i64 - self.half + 1;
        let drop = (keep_from - self.buf_start).clamp(0, self.buf.len() as i64) as usize;
        if drop > 0 {
            self.buf.drain(..drop);
            self.buf_start += drop as i64;
        }
        out
    }

    /// Push trailing zeros so the last real samples come out.
    pub fn flush(&mut self) -> Vec<f32> {
        if self.in_rate == self.out_rate {
            return Vec::new();
        }
        let pad = vec![0.0f32; self.half as usize + 1];
        self.process(&pad)
    }

    fn kernel(&self, t: f64) -> f64 {
        let x = t / self.half as f64;
        if x.abs() >= 1.0 {
            return 0.0;
        }
        let window = 0.42 + 0.5 * (PI * x).cos() + 0.08 * (2.0 * PI * x).cos();
        let arg = 2.0 * self.cutoff * t;
        let sinc = if arg.abs() < 1e-12 { 1.0 } else { (PI * arg).sin() / (PI * arg) };
        2.0 * self.cutoff * sinc * window
    }
}

/// One-shot conversion of arbitrary interleaved PCM to 16 kHz mono.
pub fn to_stt_format(interleaved: &[f32], sample_rate: u32, channels: usize) -> Vec<f32> {
    let mono = downmix(interleaved, channels);
    let mut rs = Resampler::new(sample_rate, STT_SAMPLE_RATE);
    let mut out = rs.process(&mono);
    out.extend(rs.flush());
    out
}

fn wav_spec() -> hound::WavSpec {
    hound::WavSpec {
        channels: 1,
        sample_rate: STT_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    }
}

fn to_i16(s: f32) -> i16 {
    (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
}

/// Incrementally written 16 kHz mono WAV. The header is refreshed periodically
/// so a crash mid-meeting still leaves a playable file.
pub struct WavSink {
    writer: Option<hound::WavWriter<BufWriter<File>>>,
    samples_written: u64,
    since_flush: u64,
}

impl WavSink {
    pub fn create(path: &Path) -> Result<Self> {
        Ok(WavSink { writer: Some(hound::WavWriter::create(path, wav_spec())?), samples_written: 0, since_flush: 0 })
    }

    pub fn write(&mut self, samples: &[f32]) -> Result<()> {
        if let Some(w) = self.writer.as_mut() {
            for &s in samples {
                w.write_sample(to_i16(s))?;
            }
            self.samples_written += samples.len() as u64;
            self.since_flush += samples.len() as u64;
            if self.since_flush >= STT_SAMPLE_RATE as u64 * 10 {
                w.flush()?;
                self.since_flush = 0;
            }
        }
        Ok(())
    }

    pub fn samples_written(&self) -> u64 {
        self.samples_written
    }

    pub fn finalize(&mut self) -> Result<()> {
        if let Some(w) = self.writer.take() {
            w.finalize()?;
        }
        Ok(())
    }
}

impl Drop for WavSink {
    fn drop(&mut self) {
        let _ = self.finalize();
    }
}

/// Read any PCM WAV and return 16 kHz mono f32.
pub fn read_wav_16k_mono(path: &Path) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<std::result::Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader.samples::<i32>().map(|s| s.map(|v| v as f32 / scale)).collect::<std::result::Result<_, _>>()?
        }
    };
    if spec.sample_rate == STT_SAMPLE_RATE && spec.channels == 1 {
        return Ok(interleaved);
    }
    Ok(to_stt_format(&interleaved, spec.sample_rate, spec.channels as usize))
}

/// Encode 16 kHz mono samples as an in-memory WAV (for cloud uploads).
pub fn encode_wav(samples: &[f32]) -> Result<Vec<u8>> {
    let mut cursor = Cursor::new(Vec::with_capacity(samples.len() * 2 + 44));
    {
        let mut w = hound::WavWriter::new(&mut cursor, wav_spec())?;
        for &s in samples {
            w.write_sample(to_i16(s))?;
        }
        w.finalize()?;
    }
    Ok(cursor.into_inner())
}

pub fn write_wav(path: &Path, samples: &[f32]) -> Result<()> {
    let mut w = hound::WavWriter::create(path, wav_spec())?;
    for &s in samples {
        w.write_sample(to_i16(s))?;
    }
    w.finalize()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f64, rate: u32, secs: f64) -> Vec<f32> {
        (0..(rate as f64 * secs) as usize)
            .map(|i| (2.0 * PI * freq * i as f64 / rate as f64).sin() as f32 * 0.5)
            .collect()
    }

    #[test]
    fn resample_length_and_amplitude() {
        let input = sine(440.0, 48_000, 1.0);
        let mut rs = Resampler::new(48_000, 16_000);
        // Feed in odd-sized blocks like a real audio callback.
        let mut out = Vec::new();
        for block in input.chunks(437) {
            out.extend(rs.process(block));
        }
        out.extend(rs.flush());
        assert!((out.len() as i64 - 16_000).abs() <= 2, "len {}", out.len());
        let steady = &out[1000..15000];
        let peak = steady.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!((peak - 0.5).abs() < 0.02, "peak {peak}");
    }

    #[test]
    fn resample_attenuates_above_nyquist() {
        // 12 kHz cannot exist at 16 kHz; it must be filtered, not aliased to 4 kHz.
        let input = sine(12_000.0, 48_000, 0.5);
        let out = to_stt_format(&input, 48_000, 1);
        let r = rms(&out[500..out.len() - 500]);
        assert!(r < 0.01, "rms {r}");
    }

    #[test]
    fn resample_44k1() {
        let input = sine(1000.0, 44_100, 1.0);
        let out = to_stt_format(&input, 44_100, 1);
        assert!((out.len() as i64 - 16_000).abs() <= 2);
    }

    #[test]
    fn downmix_stereo() {
        assert_eq!(downmix(&[1.0, 0.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
    }

    #[test]
    fn wav_roundtrip() {
        let dir = std::env::temp_dir().join(format!("miiting-wav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.wav");
        let s = sine(300.0, 16_000, 0.5);
        write_wav(&p, &s).unwrap();
        let back = read_wav_16k_mono(&p).unwrap();
        assert_eq!(back.len(), s.len());
        assert!((back[100] - s[100]).abs() < 1e-3);
        std::fs::remove_dir_all(&dir).ok();
    }
}
