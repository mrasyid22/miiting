//! A local Ollama server: whether it is up, which models it has, and pulling new ones.
//! Starting the server depends on the platform, so the app does that part.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use reqwest::blocking::Client;
use serde_json::{Value, json};

use crate::{Error, Result};

/// The server root (`http://localhost:11434`) for an OpenAI-style base URL (`…/v1`).
pub fn root_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    trimmed.strip_suffix("/v1").unwrap_or(trimmed).to_string()
}

/// Ollama treats `gemma3` and `gemma3:latest` as the same model.
pub fn normalize_tag(tag: &str) -> String {
    let t = tag.trim().to_lowercase();
    if t.contains(':') { t } else { format!("{t}:latest") }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    pub running: bool,
    /// Installed models, normalized with [`normalize_tag`].
    pub models: Vec<String>,
}

impl Status {
    pub fn has(&self, model: &str) -> bool {
        self.models.contains(&normalize_tag(model))
    }
}

/// Never fails: an unreachable server is reported as not running.
pub fn status(base_url: &str) -> Status {
    let client = Client::builder().timeout(Duration::from_secs(3)).build().expect("http client");
    let Ok(resp) = client.get(format!("{}/api/tags", root_url(base_url))).send() else {
        return Status::default();
    };
    if !resp.status().is_success() {
        return Status::default();
    }
    let models = resp
        .json::<Value>()
        .ok()
        .and_then(|v| {
            v["models"].as_array().map(|list| {
                list.iter()
                    .filter_map(|m| m["name"].as_str().or_else(|| m["model"].as_str()))
                    .map(normalize_tag)
                    .collect()
            })
        })
        .unwrap_or_default();
    Status { running: true, models }
}

/// Downloads `model`, reporting bytes done and total. Ollama resumes a partial download.
pub fn pull(base_url: &str, model: &str, progress: &dyn Fn(u64, u64), cancel: &AtomicBool) -> Result<()> {
    let fail = |detail: String| Error::Download { name: model.to_string(), detail };
    // No overall timeout: a large model takes many minutes.
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(None::<Duration>)
        .build()
        .expect("http client");
    let resp = client
        .post(format!("{}/api/pull", root_url(base_url)))
        .json(&json!({"model": model, "stream": true}))
        .send()
        .map_err(|e| if e.is_connect() { Error::OllamaNotRunning } else { fail(e.to_string()) })?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        let detail = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|v| v["error"].as_str().map(str::to_string))
            .unwrap_or_else(|| format!("HTTP {status}"));
        return Err(fail(detail));
    }
    let mut tracker = PullTracker::default();
    for line in BufReader::new(resp).lines() {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let line = line.map_err(|e| fail(e.to_string()))?;
        if line.trim().is_empty() {
            continue;
        }
        match tracker.update(&serde_json::from_str(&line)?) {
            PullEvent::Progress { done, total } => progress(done, total),
            PullEvent::Done => return Ok(()),
            PullEvent::Failed(detail) => return Err(fail(detail)),
            PullEvent::Other => {}
        }
    }
    Err(fail("the download stopped before it finished".into()))
}

/// Sums progress over the layers in Ollama's streamed pull status lines.
#[derive(Default)]
struct PullTracker {
    layers: HashMap<String, (u64, u64)>,
}

#[derive(Debug, PartialEq)]
enum PullEvent {
    Progress { done: u64, total: u64 },
    Done,
    Failed(String),
    Other,
}

impl PullTracker {
    fn update(&mut self, v: &Value) -> PullEvent {
        if let Some(e) = v["error"].as_str() {
            return PullEvent::Failed(e.to_string());
        }
        if v["status"] == "success" {
            return PullEvent::Done;
        }
        let (Some(digest), Some(total)) = (v["digest"].as_str(), v["total"].as_u64()) else {
            return PullEvent::Other;
        };
        let completed = v["completed"].as_u64().unwrap_or(0).min(total);
        self.layers.insert(digest.to_string(), (completed, total));
        let (done, total) = self.layers.values().fold((0, 0), |(d, t), (c, n)| (d + c, t + n));
        PullEvent::Progress { done, total }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_and_tags() {
        assert_eq!(root_url("http://localhost:11434/v1"), "http://localhost:11434");
        assert_eq!(root_url("http://localhost:11434/v1/"), "http://localhost:11434");
        assert_eq!(root_url("http://127.0.0.1:11434"), "http://127.0.0.1:11434");
        assert_eq!(normalize_tag("gemma3"), "gemma3:latest");
        assert_eq!(normalize_tag(" Gemma3:12B "), "gemma3:12b");
        let s = Status { running: true, models: vec!["gemma3:12b".into(), "llama3:latest".into()] };
        assert!(s.has("gemma3:12b"));
        assert!(s.has("llama3"));
        assert!(!s.has("gemma3:4b"));
    }

    #[test]
    fn tracks_pull_progress_across_layers() {
        let mut t = PullTracker::default();
        let line = |s: &str| serde_json::from_str::<Value>(s).unwrap();
        assert_eq!(t.update(&line(r#"{"status":"pulling manifest"}"#)), PullEvent::Other);
        assert_eq!(
            t.update(&line(r#"{"status":"pulling a","digest":"a","total":800,"completed":200}"#)),
            PullEvent::Progress { done: 200, total: 800 }
        );
        assert_eq!(
            t.update(&line(r#"{"status":"pulling b","digest":"b","total":200,"completed":200}"#)),
            PullEvent::Progress { done: 400, total: 1000 }
        );
        assert_eq!(
            t.update(&line(r#"{"status":"pulling a","digest":"a","total":800,"completed":800}"#)),
            PullEvent::Progress { done: 1000, total: 1000 }
        );
        assert_eq!(t.update(&line(r#"{"status":"verifying sha256 digest"}"#)), PullEvent::Other);
        assert_eq!(t.update(&line(r#"{"status":"success"}"#)), PullEvent::Done);
        assert_eq!(
            t.update(&line(r#"{"error":"pull model manifest: file does not exist"}"#)),
            PullEvent::Failed("pull model manifest: file does not exist".into())
        );
    }
}
