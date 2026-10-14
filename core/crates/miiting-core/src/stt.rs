//! Speech-to-text engine abstraction.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::Result;
use crate::cloud_stt::OpenAiCompatibleStt;
use crate::types::SttConfig;
use crate::whisper_local::{WhisperEngine, WhisperOptions};

#[derive(Debug, Clone)]
pub struct SttSegment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    pub no_speech_prob: f32,
}

pub struct SttRequest<'a> {
    /// 16 kHz mono.
    pub samples: &'a [f32],
    /// ISO-639-1, e.g. "id". Forcing it avoids Whisper drifting into Malay or English.
    pub language: &'a str,
    /// Vocabulary hint: names, products, jargon.
    pub prompt: Option<&'a str>,
    /// Whether this is long-form audio (final pass) rather than a short live chunk.
    pub long_form: bool,
}

pub type ProgressFn = Arc<dyn Fn(f32) + Send + Sync>;

pub struct RunControl {
    pub progress: Option<ProgressFn>,
    pub cancel: Option<Arc<AtomicBool>>,
}

impl RunControl {
    pub fn none() -> Self {
        RunControl { progress: None, cancel: None }
    }
}

pub trait SttEngine: Send {
    fn name(&self) -> String;
    fn transcribe(&mut self, req: &SttRequest, ctl: &RunControl) -> Result<Vec<SttSegment>>;
}

pub fn build_engine(cfg: &SttConfig) -> Result<Box<dyn SttEngine>> {
    Ok(match cfg {
        SttConfig::Local { model_path, vad_model_path, beam_size } => Box::new(WhisperEngine::load(
            model_path,
            WhisperOptions {
                beam_size: *beam_size,
                vad_model_path: vad_model_path.clone(),
                ..WhisperOptions::default()
            },
        )?),
        SttConfig::OpenAiCompatible { base_url, api_key, model } => {
            Box::new(OpenAiCompatibleStt::new(base_url, api_key.clone(), model))
        }
    })
}
