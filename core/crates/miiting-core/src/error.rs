use std::fmt;

use crate::i18n::Lang;
use crate::tr;

/// Errors carry structured data. `Display` is English (for logs and developers);
/// [`Error::localized`] is what the interface shows.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Free-form message, already in the right language or purely technical.
    #[error("{0}")]
    Message(String),
    #[error("model not found: {0}")]
    ModelMissing(String),
    #[error("cancelled")]
    Cancelled,
    #[error("meeting not found: {0}")]
    NotFound(String),
    #[error("no audio to transcribe")]
    NothingToTranscribe,
    #[error("the audio file is empty")]
    EmptyAudio,
    #[error("the transcript is empty")]
    EmptyTranscript,
    #[error("Ollama is not running")]
    OllamaNotRunning,
    #[error("the model declined the request")]
    Refused,
    #[error("{host}: {}{detail}", status.map(|s| format!("HTTP {s}: ")).unwrap_or_default())]
    Http { host: String, status: Option<u16>, detail: String },
    #[error("download of {name} failed: {detail}")]
    Download { name: String, detail: String },
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("audio: {0}")]
    Wav(#[from] hound::Error),
    #[error("whisper: {0}")]
    Whisper(String),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn msg(m: impl fmt::Display) -> Self {
        Error::Message(m.to_string())
    }

    pub fn http(host: impl Into<String>, status: Option<u16>, detail: impl Into<String>) -> Self {
        Error::Http { host: host.into(), status, detail: detail.into() }
    }

    /// The message to show in the interface.
    pub fn localized(&self, lang: Lang) -> String {
        match self {
            Error::Message(m) => m.clone(),
            Error::ModelMissing(m) => tr!(lang, "Model not downloaded: {m}", "Model belum diunduh: {m}"),
            Error::Cancelled => tr!(lang, "Cancelled.", "Dibatalkan."),
            Error::NotFound(_) => tr!(lang, "Meeting not found.", "Rapat tidak ditemukan."),
            Error::NothingToTranscribe => {
                tr!(lang, "There is no audio to transcribe.", "Tidak ada audio yang bisa ditranskrip.")
            }
            Error::EmptyAudio => tr!(lang, "The audio file is empty.", "Berkas audio kosong."),
            Error::EmptyTranscript => tr!(
                lang,
                "The transcript is still empty, so there is nothing to summarize.",
                "Transkrip masih kosong, belum ada yang bisa diringkas."
            ),
            Error::OllamaNotRunning => tr!(
                lang,
                "Couldn't reach Ollama. Check that it's installed (ollama.com), or choose another provider in Settings › Notes.",
                "Ollama tidak bisa dihubungi. Pastikan Ollama sudah terpasang (ollama.com), atau pilih penyedia lain di Pengaturan › Notulen."
            ),
            Error::Refused => tr!(
                lang,
                "The model declined this request. Try another model.",
                "Model menolak permintaan ini. Coba model lain."
            ),
            Error::Http { host, status, detail } => {
                let hint = match status {
                    Some(401 | 403) => tr!(lang, " (check the API key)", " (periksa API key)"),
                    Some(404) => {
                        tr!(lang, " (check the model name and base URL)", " (periksa nama model dan base URL)")
                    }
                    Some(429) => tr!(lang, " (rate limited, try again later)", " (kena rate limit, coba lagi nanti)"),
                    _ => String::new(),
                };
                match status {
                    Some(s) => format!("{host} {s}{hint}: {detail}"),
                    None => format!("{host}: {detail}"),
                }
            }
            Error::Download { name, detail } => {
                tr!(lang, "Couldn't download {name}: {detail}", "Gagal mengunduh {name}: {detail}")
            }
            Error::Db(e) => tr!(lang, "Database error: {e}", "Kesalahan database: {e}"),
            Error::Io(e) => tr!(lang, "File error: {e}", "Kesalahan berkas: {e}"),
            Error::Wav(e) => tr!(lang, "Audio file error: {e}", "Kesalahan berkas audio: {e}"),
            Error::Whisper(e) => tr!(lang, "Transcription error: {e}", "Kesalahan transkripsi: {e}"),
            Error::Json(e) => tr!(lang, "Unexpected response: {e}", "Respons tidak terduga: {e}"),
        }
    }
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        let host = e.url().and_then(|u| u.host_str()).unwrap_or("network").to_string();
        Error::Http { host, status: e.status().map(|s| s.as_u16()), detail: e.to_string() }
    }
}

impl From<whisper_rs::WhisperError> for Error {
    fn from(e: whisper_rs::WhisperError) -> Self {
        Error::Whisper(e.to_string())
    }
}
