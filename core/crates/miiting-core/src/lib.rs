//! Miiting core: platform-independent meeting transcription engine.
//!
//! The native shell (SwiftUI on macOS, later something else on Windows) captures
//! raw PCM and hands it to [`session::Session`]. Everything after that — resampling,
//! voice activity detection, chunking, speech-to-text, storage, summaries — lives here.

pub mod audio;
pub mod chunker;
pub mod cloud_stt;
pub mod device;
pub mod engine;
pub mod error;
pub mod hallucination;
pub mod i18n;
pub mod llm;
pub mod merge;
pub mod models;
pub mod prompts;
pub mod session;
pub mod store;
pub mod stt;
pub mod text_norm;
pub mod types;
pub mod vad;
pub mod wer;
pub mod whisper_local;

pub use error::{Error, Result};
pub use i18n::Lang;

/// Sample rate every STT engine in Miiting consumes.
pub const STT_SAMPLE_RATE: u32 = 16_000;
