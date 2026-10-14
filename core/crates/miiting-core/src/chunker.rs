//! Cuts a continuous 16 kHz stream into utterance-sized chunks for live STT.

use std::collections::VecDeque;

use crate::vad::{EnergyVad, FRAME_MS, FRAME_SAMPLES, VadConfig};

#[derive(Debug, Clone)]
pub struct ChunkerConfig {
    /// Audio kept from before speech onset so the first syllable isn't clipped.
    pub pre_roll_ms: usize,
    /// Consecutive speech needed to open a chunk.
    pub onset_ms: usize,
    /// Silence that closes a chunk.
    pub end_silence_ms: usize,
    /// Hard cap; long monologues are split at the quietest recent frame.
    pub max_chunk_ms: usize,
    /// Chunks with less total speech than this are dropped (coughs, clicks).
    pub min_speech_ms: usize,
}

impl Default for ChunkerConfig {
    fn default() -> Self {
        ChunkerConfig { pre_roll_ms: 300, onset_ms: 120, end_silence_ms: 700, max_chunk_ms: 15_000, min_speech_ms: 300 }
    }
}

#[derive(Debug, Clone)]
pub struct Chunk {
    /// Position of the first sample, counted in 16 kHz samples since the stream began.
    pub start_sample: u64,
    pub samples: Vec<f32>,
}

impl Chunk {
    pub fn start_ms(&self) -> i64 {
        (self.start_sample * 1000 / 16_000) as i64
    }
}

struct Active {
    start_sample: u64,
    samples: Vec<f32>,
    frame_db: Vec<f32>,
    silence_frames: usize,
    speech_frames: usize,
}

pub struct Chunker {
    cfg: ChunkerConfig,
    vad: EnergyVad,
    pending: Vec<f32>,
    history: VecDeque<f32>,
    history_cap: usize,
    speech_run: usize,
    consumed: u64,
    active: Option<Active>,
}

impl Chunker {
    pub fn new(cfg: ChunkerConfig) -> Self {
        let history_cap = (cfg.pre_roll_ms + cfg.onset_ms) * 16;
        Chunker {
            vad: EnergyVad::new(VadConfig::default()),
            pending: Vec::new(),
            history: VecDeque::with_capacity(history_cap + FRAME_SAMPLES),
            history_cap,
            speech_run: 0,
            consumed: 0,
            active: None,
            cfg,
        }
    }

    pub fn is_in_speech(&self) -> bool {
        self.active.is_some()
    }

    /// Account for stream time that was never fed in (a source that started late).
    pub fn advance(&mut self, samples: u64) {
        if self.active.is_none() && self.pending.is_empty() {
            self.consumed += samples;
            self.history.clear();
            self.speech_run = 0;
        }
    }

    pub fn push(&mut self, samples: &[f32]) -> Vec<Chunk> {
        self.pending.extend_from_slice(samples);
        let mut out = Vec::new();
        let mut offset = 0;
        while self.pending.len() - offset >= FRAME_SAMPLES {
            let frame: Vec<f32> = self.pending[offset..offset + FRAME_SAMPLES].to_vec();
            offset += FRAME_SAMPLES;
            if let Some(c) = self.on_frame(&frame) {
                out.push(c);
            }
        }
        self.pending.drain(..offset);
        out
    }

    /// Close any open chunk (end of recording).
    pub fn finish(&mut self) -> Option<Chunk> {
        let rest = std::mem::take(&mut self.pending);
        if let Some(a) = self.active.as_mut() {
            a.samples.extend_from_slice(&rest);
        }
        self.active.take().and_then(|a| self.emit(a))
    }

    fn on_frame(&mut self, frame: &[f32]) -> Option<Chunk> {
        let (speech, db) = self.vad.classify(frame);
        let frame_start = self.consumed;
        self.consumed += frame.len() as u64;

        if let Some(a) = self.active.as_mut() {
            a.samples.extend_from_slice(frame);
            a.frame_db.push(db);
            if speech {
                a.silence_frames = 0;
                a.speech_frames += 1;
            } else {
                a.silence_frames += 1;
            }

            if a.silence_frames * FRAME_MS >= self.cfg.end_silence_ms {
                let mut a = self.active.take().unwrap();
                // Keep ~200 ms of the trailing silence; drop the rest.
                let keep_silence = (200 / FRAME_MS).min(a.silence_frames);
                let trim = (a.silence_frames - keep_silence) * FRAME_SAMPLES;
                a.samples.truncate(a.samples.len().saturating_sub(trim));
                self.speech_run = 0;
                return self.emit(a);
            }

            if a.samples.len() >= self.cfg.max_chunk_ms * 16 {
                return self.split_long();
            }
            return None;
        }

        // Idle: remember recent audio for pre-roll.
        self.history.extend(frame.iter().copied());
        while self.history.len() > self.history_cap {
            self.history.pop_front();
        }
        self.speech_run = if speech { self.speech_run + 1 } else { 0 };
        if self.speech_run * FRAME_MS >= self.cfg.onset_ms {
            let samples: Vec<f32> = self.history.drain(..).collect();
            let start = (frame_start + frame.len() as u64).saturating_sub(samples.len() as u64);
            let frames = samples.len() / FRAME_SAMPLES;
            self.active = Some(Active {
                start_sample: start,
                samples,
                frame_db: vec![db; frames],
                silence_frames: 0,
                speech_frames: self.speech_run,
            });
            self.speech_run = 0;
        }
        None
    }

    /// Split an over-long chunk at the quietest frame of its last 3 seconds.
    fn split_long(&mut self) -> Option<Chunk> {
        let mut a = self.active.take()?;
        let n = a.frame_db.len();
        let window = (3000 / FRAME_MS).min(n.saturating_sub(1));
        let search_from = n - window;
        let cut_frame = (search_from..n).min_by(|&i, &j| a.frame_db[i].total_cmp(&a.frame_db[j])).unwrap_or(n);
        let cut = (cut_frame * FRAME_SAMPLES).min(a.samples.len());

        let tail = a.samples.split_off(cut);
        let tail_db = a.frame_db.split_off(cut_frame.min(a.frame_db.len()));
        let tail_start = a.start_sample + cut as u64;
        let tail_frames = tail_db.len();
        let head_speech = a.speech_frames.saturating_sub(tail_frames);

        let head = Active { speech_frames: head_speech.max(1), ..a };
        self.active = Some(Active {
            start_sample: tail_start,
            samples: tail,
            frame_db: tail_db,
            silence_frames: 0,
            speech_frames: tail_frames,
        });
        self.emit(head)
    }

    fn emit(&self, a: Active) -> Option<Chunk> {
        if a.speech_frames * FRAME_MS < self.cfg.min_speech_ms || a.samples.is_empty() {
            return None;
        }
        Some(Chunk { start_sample: a.start_sample, samples: a.samples })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(ms: usize) -> Vec<f32> {
        (0..ms * 16).map(|i| (i as f32 * 0.15).sin() * 0.3).collect()
    }

    fn silence(ms: usize) -> Vec<f32> {
        vec![0.0; ms * 16]
    }

    #[test]
    fn two_utterances_two_chunks() {
        let mut c = Chunker::new(ChunkerConfig::default());
        let mut audio = silence(1000);
        audio.extend(tone(1200));
        audio.extend(silence(1500));
        audio.extend(tone(800));
        audio.extend(silence(1500));
        let mut chunks = Vec::new();
        for block in audio.chunks(1601) {
            chunks.extend(c.push(block));
        }
        chunks.extend(c.finish());
        assert_eq!(chunks.len(), 2, "{:?}", chunks.iter().map(|c| c.start_ms()).collect::<Vec<_>>());
        // First chunk starts within pre-roll of the 1000 ms onset.
        let s = chunks[0].start_ms();
        assert!((600..=1000).contains(&s), "start {s}");
        let s2 = chunks[1].start_ms();
        assert!((3300..=3700).contains(&s2), "start2 {s2}");
    }

    #[test]
    fn long_speech_is_split() {
        let mut c = Chunker::new(ChunkerConfig::default());
        let mut audio = silence(500);
        audio.extend(tone(40_000));
        audio.extend(silence(1000));
        let mut chunks = Vec::new();
        for block in audio.chunks(4800) {
            chunks.extend(c.push(block));
        }
        chunks.extend(c.finish());
        assert!(chunks.len() >= 3);
        for ch in &chunks {
            assert!(ch.samples.len() <= 15_000 * 16 + FRAME_SAMPLES);
        }
        // Chunks are contiguous: no audio lost at split points.
        for w in chunks.windows(2) {
            let end = w[0].start_sample + w[0].samples.len() as u64;
            assert_eq!(end, w[1].start_sample);
        }
    }

    #[test]
    fn click_is_dropped() {
        let mut c = Chunker::new(ChunkerConfig::default());
        let mut audio = silence(500);
        audio.extend(tone(150));
        audio.extend(silence(1500));
        let mut chunks = c.push(&audio);
        chunks.extend(c.finish());
        assert!(chunks.is_empty());
    }
}
