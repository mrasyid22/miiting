//! Lightweight energy VAD with an adaptive noise floor.
//!
//! Used only to cut the live stream into utterances. Hallucination control on the
//! final pass is handled by whisper.cpp's Silero VAD, which is far more precise.

use crate::audio::rms;

pub const FRAME_MS: usize = 30;
pub const FRAME_SAMPLES: usize = 16_000 * FRAME_MS / 1000;

#[derive(Debug, Clone)]
pub struct VadConfig {
    /// dB above the tracked noise floor that counts as speech.
    pub threshold_db: f32,
    /// Absolute floor: anything quieter is never speech.
    pub min_db: f32,
}

impl Default for VadConfig {
    fn default() -> Self {
        VadConfig { threshold_db: 9.0, min_db: -50.0 }
    }
}

pub struct EnergyVad {
    cfg: VadConfig,
    noise_db: f32,
}

impl EnergyVad {
    pub fn new(cfg: VadConfig) -> Self {
        EnergyVad { cfg, noise_db: -70.0 }
    }

    pub fn noise_floor_db(&self) -> f32 {
        self.noise_db
    }

    /// Classify one frame. Returns (is_speech, frame_db).
    pub fn classify(&mut self, frame: &[f32]) -> (bool, f32) {
        let db = 20.0 * rms(frame).max(1e-7).log10();
        // Fall fast to quiet frames, creep up slowly so long speech doesn't become "noise".
        if db < self.noise_db {
            self.noise_db = 0.85 * self.noise_db + 0.15 * db;
        } else {
            self.noise_db += 0.0005 * (db - self.noise_db);
        }
        let speech = db > (self.noise_db + self.cfg.threshold_db).max(self.cfg.min_db);
        (speech, db)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_tone_over_noise() {
        let mut vad = EnergyVad::new(VadConfig::default());
        let quiet: Vec<f32> = (0..FRAME_SAMPLES).map(|i| ((i * 7919) % 13) as f32 * 1e-4).collect();
        for _ in 0..50 {
            assert!(!vad.classify(&quiet).0);
        }
        let loud: Vec<f32> = (0..FRAME_SAMPLES).map(|i| (i as f32 * 0.2).sin() * 0.3).collect();
        assert!(vad.classify(&loud).0);
    }

    #[test]
    fn digital_silence_is_not_speech() {
        let mut vad = EnergyVad::new(VadConfig::default());
        assert!(!vad.classify(&[0.0; FRAME_SAMPLES]).0);
    }
}
