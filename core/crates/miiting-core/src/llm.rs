//! Chat-completion clients for summaries.
//!
//! Two wire formats cover every provider Miiting offers:
//! - OpenAI-compatible `/chat/completions`: OpenAI, OpenRouter, Groq, Gemini (OpenAI endpoint),
//!   LM Studio, llama.cpp server, vLLM. Ollama is detected and called through its native
//!   `/api/chat` so we can raise its small default context window.
//! - Anthropic Messages API.

use std::time::Duration;

use reqwest::blocking::Client;
use serde_json::{Value, json};

use crate::device::LocalLlmTuning;
use crate::types::{host_of, is_local_host};
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub enum LlmConfig {
    OpenAiCompatible { base_url: String, api_key: Option<String>, model: String },
    Anthropic { api_key: String, model: String, base_url: Option<String> },
}

const ANTHROPIC_URL: &str = "https://api.anthropic.com";
/// Cloud models have huge context windows; one request covers a long meeting.
const CLOUD_CHUNK_CHARS: usize = 120_000;

impl LlmConfig {
    pub fn base_url(&self) -> String {
        match self {
            LlmConfig::OpenAiCompatible { base_url, .. } => base_url.trim_end_matches('/').to_string(),
            LlmConfig::Anthropic { base_url, .. } => {
                base_url.clone().unwrap_or_else(|| ANTHROPIC_URL.to_string()).trim_end_matches('/').to_string()
            }
        }
    }

    pub fn model(&self) -> &str {
        match self {
            LlmConfig::OpenAiCompatible { model, .. } | LlmConfig::Anthropic { model, .. } => model,
        }
    }

    pub fn is_local(&self) -> bool {
        is_local_host(&host_of(&self.base_url()))
    }

    pub fn egress_host(&self) -> Option<String> {
        if self.is_local() { None } else { Some(host_of(&self.base_url())) }
    }

    pub fn describe(&self) -> String {
        format!("{}:{}", host_of(&self.base_url()), self.model())
    }

    fn is_ollama(&self) -> bool {
        match self {
            LlmConfig::OpenAiCompatible { base_url, .. } => {
                base_url.contains(":11434") || base_url.to_lowercase().contains("ollama")
            }
            _ => false,
        }
    }
}

pub struct LlmClient {
    http: Client,
    cfg: LlmConfig,
    /// Context and chunk sizes for models running on this machine.
    local: LocalLlmTuning,
}

impl LlmClient {
    pub fn new(cfg: LlmConfig) -> Self {
        Self::with_tuning(cfg, LocalLlmTuning { num_ctx: 16_384, chunk_chars: 20_000 })
    }

    pub fn with_tuning(cfg: LlmConfig, local: LocalLlmTuning) -> Self {
        LlmClient {
            // Local models on a laptop can take minutes for a long meeting.
            http: Client::builder().timeout(Duration::from_secs(900)).build().expect("http client"),
            cfg,
            local,
        }
    }

    pub fn config(&self) -> &LlmConfig {
        &self.cfg
    }

    /// How much transcript text to send per request before switching to map-reduce.
    pub fn chunk_chars(&self) -> usize {
        if self.cfg.is_local() { self.local.chunk_chars } else { CLOUD_CHUNK_CHARS }
    }

    pub fn complete(&self, system: &str, user: &str) -> Result<String> {
        let text = match &self.cfg {
            LlmConfig::Anthropic { api_key, model, .. } => self.anthropic(api_key, model, system, user)?,
            LlmConfig::OpenAiCompatible { api_key, model, .. } if self.cfg.is_ollama() => {
                self.ollama(api_key.as_deref(), model, system, user)?
            }
            LlmConfig::OpenAiCompatible { api_key, model, .. } => {
                self.openai(api_key.as_deref(), model, system, user)?
            }
        };
        Ok(strip_think(&text))
    }

    fn openai(&self, key: Option<&str>, model: &str, system: &str, user: &str) -> Result<String> {
        let base = self.cfg.base_url();
        let mut body = json!({
            "model": model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
        });
        // OpenAI's reasoning models reject custom temperatures; everyone else benefits from a low one.
        if host_of(&base) != "api.openai.com" {
            body["temperature"] = json!(0.2);
        }
        let mut rb = self.http.post(format!("{base}/chat/completions")).json(&body);
        if let Some(k) = key.filter(|k| !k.trim().is_empty()) {
            rb = rb.bearer_auth(k);
        }
        if host_of(&base) == "openrouter.ai" {
            rb = rb.header("X-Title", "Miiting");
        }
        let v = read_json(rb.send()?, &base)?;
        v["choices"][0]["message"]["content"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| Error::http(host_of(&base), None, "the response has no text"))
    }

    fn ollama(&self, key: Option<&str>, model: &str, system: &str, user: &str) -> Result<String> {
        let base = self.cfg.base_url();
        let root = base.trim_end_matches("/v1");
        let body = json!({
            "model": model,
            "stream": false,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
            // Ollama's default context (2–4k tokens) silently truncates meeting transcripts.
            "options": {"num_ctx": self.local.num_ctx, "temperature": 0.2},
        });
        let mut rb = self.http.post(format!("{root}/api/chat")).json(&body);
        if let Some(k) = key.filter(|k| !k.trim().is_empty()) {
            rb = rb.bearer_auth(k);
        }
        let resp = rb.send().map_err(|e| if e.is_connect() { Error::OllamaNotRunning } else { e.into() })?;
        let v = read_json(resp, &base)?;
        v["message"]["content"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| Error::http(host_of(&base), None, "the response has no text"))
    }

    fn anthropic(&self, key: &str, model: &str, system: &str, user: &str) -> Result<String> {
        let base = self.cfg.base_url();
        let mut body = json!({
            "model": model,
            "max_tokens": 16000,
            "system": system,
            "messages": [{"role": "user", "content": user}],
        });
        // Current-generation models take effort; summarizing a meeting doesn't need deep thinking.
        let current_gen = ["claude-opus-5", "claude-sonnet-5", "claude-fable-5", "claude-haiku-5"]
            .iter()
            .any(|p| model.starts_with(p));
        if current_gen {
            body["output_config"] = json!({"effort": "medium"});
        }
        // Server-side fallback re-runs a safety-classifier decline on another model.
        // Supported on the first-party API for these models (not Haiku).
        let fallback = base == ANTHROPIC_URL
            && ["claude-opus-5", "claude-sonnet-5-5", "claude-fable-5-1"].iter().any(|p| model.starts_with(p));
        if fallback {
            body["fallbacks"] = json!("default");
        }
        let mut rb = self
            .http
            .post(format!("{base}/v1/messages"))
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01");
        if fallback {
            rb = rb.header("anthropic-beta", "server-side-fallback-2026-07-01");
        }
        let v = read_json(rb.json(&body).send()?, &base)?;
        if v["stop_reason"].as_str() == Some("refusal") {
            return Err(Error::Refused);
        }
        let text: String = v["content"]
            .as_array()
            .map(|blocks| {
                blocks
                    .iter()
                    .filter(|b| b["type"] == "text")
                    .filter_map(|b| b["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();
        if text.trim().is_empty() {
            return Err(Error::http(host_of(&base), None, "the response has no text"));
        }
        Ok(text)
    }
}

fn read_json(resp: reqwest::blocking::Response, base: &str) -> Result<Value> {
    let status = resp.status();
    let body = resp.text()?;
    if !status.is_success() {
        let detail = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|v| {
                v["error"]["message"]
                    .as_str()
                    .or_else(|| v["error"].as_str())
                    .or_else(|| v["message"].as_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| body.chars().take(300).collect());
        // Status-specific hints (API key, model name, rate limit) are added by Error::localized.
        return Err(Error::http(host_of(base), Some(status.as_u16()), detail));
    }
    Ok(serde_json::from_str(&body)?)
}

/// Remove `<think>…</think>` blocks that reasoning models (Qwen3, DeepSeek-R1) put in content.
pub fn strip_think(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<think>") {
        out.push_str(&rest[..start]);
        match rest[start..].find("</think>") {
            Some(end) => rest = &rest[start + end + "</think>".len()..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_reasoning() {
        assert_eq!(strip_think("<think>hmm</think>\n## Ringkasan\nOke"), "## Ringkasan\nOke");
        assert_eq!(strip_think("Tanpa think"), "Tanpa think");
    }

    #[test]
    fn locality() {
        let local = LlmConfig::OpenAiCompatible {
            base_url: "http://localhost:11434/v1".into(),
            api_key: None,
            model: "qwen3:14b".into(),
        };
        assert!(local.is_local());
        assert!(local.is_ollama());
        assert_eq!(local.egress_host(), None);
        let cloud = LlmConfig::Anthropic { api_key: "k".into(), model: "claude-opus-5-5".into(), base_url: None };
        assert_eq!(cloud.egress_host().as_deref(), Some("api.anthropic.com"));
    }
}
