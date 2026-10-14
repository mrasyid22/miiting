//! OpenAI-compatible `/audio/transcriptions` client.
//!
//! Covers OpenAI (whisper-1, gpt-4o-transcribe), Groq (whisper-large-v3[-turbo]),
//! and self-hosted servers such as speaches / faster-whisper-server / LocalAI.

use std::time::Duration;

use reqwest::blocking::{Client, multipart};
use serde::Deserialize;

use crate::audio::encode_wav;
use crate::stt::{RunControl, SttEngine, SttRequest, SttSegment};
use crate::types::host_of;
use crate::vad::{EnergyVad, FRAME_SAMPLES, VadConfig};
use crate::{Error, Result, STT_SAMPLE_RATE};

/// Stay well under the common 25 MB upload limit (16-bit 16 kHz ≈ 1.9 MB/min).
const MAX_UPLOAD_SECS: usize = 600;

pub struct OpenAiCompatibleStt {
    client: Client,
    base_url: String,
    api_key: Option<String>,
    model: String,
}

#[derive(Deserialize)]
struct VerboseResponse {
    text: Option<String>,
    segments: Option<Vec<VerboseSegment>>,
}

#[derive(Deserialize)]
struct VerboseSegment {
    start: f64,
    end: f64,
    text: String,
    #[serde(default)]
    no_speech_prob: f32,
}

impl OpenAiCompatibleStt {
    pub fn new(base_url: &str, api_key: Option<String>, model: &str) -> Self {
        OpenAiCompatibleStt {
            client: Client::builder().timeout(Duration::from_secs(300)).build().expect("http client"),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.filter(|k| !k.trim().is_empty()),
            model: model.to_string(),
        }
    }

    fn post(&self, samples: &[f32], req: &SttRequest, verbose: bool) -> Result<reqwest::blocking::Response> {
        let wav = encode_wav(samples)?;
        let file = multipart::Part::bytes(wav).file_name("audio.wav").mime_str("audio/wav")?;
        let mut form = multipart::Form::new()
            .part("file", file)
            .text("model", self.model.clone())
            .text("language", req.language.to_string())
            .text("temperature", "0")
            .text("response_format", if verbose { "verbose_json" } else { "json" });
        if let Some(p) = req.prompt.filter(|p| !p.trim().is_empty()) {
            form = form.text("prompt", p.to_string());
        }
        let mut rb = self.client.post(format!("{}/audio/transcriptions", self.base_url)).multipart(form);
        if let Some(key) = &self.api_key {
            rb = rb.bearer_auth(key);
        }
        Ok(rb.send()?)
    }

    fn transcribe_piece(&self, samples: &[f32], req: &SttRequest) -> Result<Vec<SttSegment>> {
        let duration_ms = (samples.len() as i64 * 1000) / STT_SAMPLE_RATE as i64;
        let mut resp = self.post(samples, req, true)?;
        if resp.status().as_u16() == 400 {
            // gpt-4o-transcribe and some servers only accept plain json.
            let body = resp.text().unwrap_or_default();
            if body.contains("response_format") || body.contains("verbose_json") {
                resp = self.post(samples, req, false)?;
            } else {
                return Err(Error::http(host_of(&self.base_url), Some(400), truncate(&body)));
            }
        }
        let status = resp.status();
        let body = resp.text()?;
        if !status.is_success() {
            return Err(Error::http(host_of(&self.base_url), Some(status.as_u16()), truncate(&body)));
        }
        let parsed: VerboseResponse = serde_json::from_str(&body)?;
        if let Some(segs) = parsed.segments.filter(|s| !s.is_empty()) {
            return Ok(segs
                .into_iter()
                .map(|s| SttSegment {
                    start_ms: (s.start * 1000.0) as i64,
                    end_ms: (s.end * 1000.0) as i64,
                    text: s.text.trim().to_string(),
                    no_speech_prob: s.no_speech_prob,
                })
                .filter(|s| !s.text.is_empty())
                .collect());
        }
        let text = parsed.text.unwrap_or_default().trim().to_string();
        Ok(if text.is_empty() {
            vec![]
        } else {
            vec![SttSegment { start_ms: 0, end_ms: duration_ms, text, no_speech_prob: 0.0 }]
        })
    }
}

impl SttEngine for OpenAiCompatibleStt {
    fn name(&self) -> String {
        format!("{}:{}", host_of(&self.base_url), self.model)
    }

    fn transcribe(&mut self, req: &SttRequest, ctl: &RunControl) -> Result<Vec<SttSegment>> {
        let pieces = split_at_silence(req.samples, MAX_UPLOAD_SECS * STT_SAMPLE_RATE as usize);
        let total = pieces.len().max(1);
        let mut out = Vec::new();
        for (i, (offset, piece)) in pieces.into_iter().enumerate() {
            if ctl.cancel.as_ref().is_some_and(|c| c.load(std::sync::atomic::Ordering::Relaxed)) {
                return Err(Error::Cancelled);
            }
            let offset_ms = (offset as i64 * 1000) / STT_SAMPLE_RATE as i64;
            for mut s in self.transcribe_piece(piece, req)? {
                s.start_ms += offset_ms;
                s.end_ms += offset_ms;
                out.push(s);
            }
            if let Some(p) = &ctl.progress {
                p((i + 1) as f32 / total as f32);
            }
        }
        Ok(out)
    }
}

/// Split long audio into pieces no longer than `max_len`, cutting at the quietest
/// frame in the last 20 s of each piece so words aren't cut in half.
pub fn split_at_silence(samples: &[f32], max_len: usize) -> Vec<(usize, &[f32])> {
    if samples.len() <= max_len {
        return vec![(0, samples)];
    }
    let mut vad = EnergyVad::new(VadConfig::default());
    let frame_db: Vec<f32> = samples.chunks(FRAME_SAMPLES).map(|f| vad.classify(f).1).collect();
    let mut out = Vec::new();
    let mut start = 0usize;
    while samples.len() - start > max_len {
        let hard_end = start + max_len;
        let search_from = hard_end.saturating_sub(20 * STT_SAMPLE_RATE as usize).max(start + 1);
        let (f0, f1) = (search_from / FRAME_SAMPLES, hard_end / FRAME_SAMPLES);
        let cut_frame = (f0..f1).min_by(|&a, &b| frame_db[a].total_cmp(&frame_db[b])).unwrap_or(f1);
        let cut = (cut_frame * FRAME_SAMPLES).clamp(start + 1, hard_end);
        out.push((start, &samples[start..cut]));
        start = cut;
    }
    out.push((start, &samples[start..]));
    out
}

fn truncate(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() > 300 { format!("{}…", s.chars().take(300).collect::<String>()) } else { s.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_covers_everything() {
        let samples: Vec<f32> = (0..16_000 * 25).map(|i| if (i / 16_000) % 7 == 0 { 0.0 } else { 0.3 }).collect();
        let parts = split_at_silence(&samples, 16_000 * 10);
        assert!(parts.len() >= 3);
        let mut expected = 0;
        for (off, p) in &parts {
            assert_eq!(*off, expected);
            assert!(p.len() <= 16_000 * 10);
            expected += p.len();
        }
        assert_eq!(expected, samples.len());
    }
}
