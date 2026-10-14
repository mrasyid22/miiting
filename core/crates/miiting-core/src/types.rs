use serde::{Deserialize, Serialize};

use crate::i18n::{Lang, Text};

/// Where a piece of audio came from. Kept separate end-to-end so the transcript
/// can say who spoke without running diarization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Source {
    /// The local microphone: the person using Miiting.
    Mic,
    /// System / meeting-app output: everyone else on the call.
    System,
    /// An imported recording with both sides mixed.
    File,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Mic => "mic",
            Source::System => "system",
            Source::File => "file",
        }
    }

    pub fn parse(s: &str) -> Source {
        match s {
            "mic" => Source::Mic,
            "system" => Source::System,
            _ => Source::File,
        }
    }

    /// Speaker label shown in transcripts and given to the summarizer.
    pub fn speaker_label(self, lang: Lang) -> &'static str {
        match self {
            Source::Mic => Text::new("Me", "Saya"),
            Source::System => Text::new("Them", "Peserta"),
            Source::File => Text::new("Speaker", "Pembicara"),
        }
        .get(lang)
    }

    pub fn wav_name(self) -> &'static str {
        match self {
            Source::Mic => "mic.wav",
            Source::System => "system.wav",
            Source::File => "file.wav",
        }
    }
}

/// Transcript pass. Live segments are a fast draft; the final pass replaces them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Pass {
    Live,
    Final,
}

impl Pass {
    pub fn as_str(self) -> &'static str {
        match self {
            Pass::Live => "live",
            Pass::Final => "final",
        }
    }

    pub fn parse(s: &str) -> Pass {
        if s == "final" { Pass::Final } else { Pass::Live }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    pub source: Source,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MeetingStatus {
    Recording,
    Processing,
    Ready,
    Failed,
}

impl MeetingStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            MeetingStatus::Recording => "recording",
            MeetingStatus::Processing => "processing",
            MeetingStatus::Ready => "ready",
            MeetingStatus::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> MeetingStatus {
        match s {
            "recording" => MeetingStatus::Recording,
            "processing" => MeetingStatus::Processing,
            "failed" => MeetingStatus::Failed,
            _ => MeetingStatus::Ready,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meeting {
    pub id: String,
    pub title: String,
    pub created_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub duration_ms: i64,
    pub status: MeetingStatus,
    pub summary_md: Option<String>,
    pub summary_template: Option<String>,
    pub summary_model: Option<String>,
    pub transcript_pass: Pass,
    pub stt_engine: Option<String>,
    /// Hosts that received this meeting's audio or text. Empty means it never left the Mac.
    pub egress: Vec<String>,
    pub language: String,
}

/// How to run speech-to-text.
#[derive(Debug, Clone)]
pub enum SttConfig {
    Local {
        model_path: std::path::PathBuf,
        /// Silero VAD model for whisper.cpp's built-in VAD (used on long-form final passes).
        vad_model_path: Option<std::path::PathBuf>,
        beam_size: i32,
    },
    /// Any server that speaks OpenAI's `/audio/transcriptions` (OpenAI, Groq, speaches, LocalAI, ...).
    OpenAiCompatible { base_url: String, api_key: Option<String>, model: String },
}

impl SttConfig {
    pub fn describe(&self) -> String {
        match self {
            SttConfig::Local { model_path, .. } => {
                format!("local:{}", model_path.file_stem().and_then(|s| s.to_str()).unwrap_or("whisper"))
            }
            SttConfig::OpenAiCompatible { base_url, model, .. } => {
                format!("{}:{}", host_of(base_url), model)
            }
        }
    }

    /// Host the audio is sent to, if it leaves the machine.
    pub fn egress_host(&self) -> Option<String> {
        match self {
            SttConfig::Local { .. } => None,
            SttConfig::OpenAiCompatible { base_url, .. } => {
                let h = host_of(base_url);
                if is_local_host(&h) { None } else { Some(h) }
            }
        }
    }
}

pub fn host_of(url: &str) -> String {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let host_port = rest.split('/').next().unwrap_or(rest);
    host_port.split(':').next().unwrap_or(host_port).to_lowercase()
}

pub fn is_local_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]" | "0.0.0.0") || host.ends_with(".local")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_parsing() {
        assert_eq!(host_of("https://api.openai.com/v1"), "api.openai.com");
        assert_eq!(host_of("http://localhost:11434/v1"), "localhost");
        assert!(is_local_host(&host_of("http://127.0.0.1:8000/v1")));
        assert!(!is_local_host(&host_of("https://api.groq.com/openai/v1")));
    }
}
