//! UniFFI bindings exposing miiting-core to Swift (and later Kotlin/C# shells).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use miiting_core::device::{self, DeviceProfile, Fit, LOCAL_LLMS, Tier};
use miiting_core::engine::{Engine, FinalPassOptions, StartOptions, SummaryOptions};
use miiting_core::llm::{LlmClient, LlmConfig};
use miiting_core::models::{self, ModelKind, ModelRole};
use miiting_core::ollama;
use miiting_core::prompts::TEMPLATES;
use miiting_core::session::{Session, SessionEvents, SessionStatus};
use miiting_core::stt::RunControl;
use miiting_core::types::{self as core, Pass, Source, SttConfig};
use miiting_core::{Lang, tr};

uniffi::setup_scaffolding!();

// ── Errors ─────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum MiitingError {
    #[error("{message}")]
    Failed { message: String },
    #[error("Model not downloaded: {message}")]
    ModelMissing { message: String },
    #[error("Cancelled")]
    Cancelled,
}

fn to_ffi_error(e: miiting_core::Error, lang: Lang) -> MiitingError {
    match e {
        miiting_core::Error::Cancelled => MiitingError::Cancelled,
        miiting_core::Error::ModelMissing(m) => MiitingError::ModelMissing { message: m },
        other => MiitingError::Failed { message: other.localized(lang) },
    }
}

/// English fallback; engine methods localize with the interface language instead.
impl From<miiting_core::Error> for MiitingError {
    fn from(e: miiting_core::Error) -> Self {
        to_ffi_error(e, Lang::En)
    }
}

type R<T> = Result<T, MiitingError>;

// ── Value types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum AudioSource {
    Mic,
    System,
    File,
}

impl From<AudioSource> for Source {
    fn from(s: AudioSource) -> Self {
        match s {
            AudioSource::Mic => Source::Mic,
            AudioSource::System => Source::System,
            AudioSource::File => Source::File,
        }
    }
}

impl From<Source> for AudioSource {
    fn from(s: Source) -> Self {
        match s {
            Source::Mic => AudioSource::Mic,
            Source::System => AudioSource::System,
            Source::File => AudioSource::File,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MeetingStatus {
    Recording,
    Processing,
    Ready,
    Failed,
}

impl From<core::MeetingStatus> for MeetingStatus {
    fn from(s: core::MeetingStatus) -> Self {
        match s {
            core::MeetingStatus::Recording => MeetingStatus::Recording,
            core::MeetingStatus::Processing => MeetingStatus::Processing,
            core::MeetingStatus::Ready => MeetingStatus::Ready,
            core::MeetingStatus::Failed => MeetingStatus::Failed,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TranscriptPass {
    Live,
    Final,
}

impl From<Pass> for TranscriptPass {
    fn from(p: Pass) -> Self {
        match p {
            Pass::Live => TranscriptPass::Live,
            Pass::Final => TranscriptPass::Final,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct TranscriptSegment {
    pub source: AudioSource,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

impl From<&core::Segment> for TranscriptSegment {
    fn from(s: &core::Segment) -> Self {
        TranscriptSegment { source: s.source.into(), start_ms: s.start_ms, end_ms: s.end_ms, text: s.text.clone() }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct MeetingItem {
    pub id: String,
    pub title: String,
    pub created_at_ms: i64,
    pub duration_ms: i64,
    pub status: MeetingStatus,
    pub has_summary: bool,
    pub transcript_pass: TranscriptPass,
    pub stt_engine: Option<String>,
    /// Hosts that received this meeting's data. Empty = never left the Mac.
    pub egress: Vec<String>,
    /// Spoken language (Whisper code, e.g. "id").
    pub language: String,
}

impl From<&core::Meeting> for MeetingItem {
    fn from(m: &core::Meeting) -> Self {
        MeetingItem {
            id: m.id.clone(),
            title: m.title.clone(),
            created_at_ms: m.created_at_ms,
            duration_ms: m.duration_ms,
            status: m.status.into(),
            has_summary: m.summary_md.as_ref().is_some_and(|s| !s.trim().is_empty()),
            transcript_pass: m.transcript_pass.into(),
            stt_engine: m.stt_engine.clone(),
            egress: m.egress.clone(),
            language: m.language.clone(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct MeetingDetail {
    pub meeting: MeetingItem,
    pub summary_markdown: Option<String>,
    pub summary_template: Option<String>,
    pub summary_model: Option<String>,
    pub language: String,
    pub shown_pass: TranscriptPass,
    pub segments: Vec<TranscriptSegment>,
    /// Absolute paths of the recorded WAV files.
    pub audio_files: Vec<String>,
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum SttSettings {
    /// A model from the catalog, by id. The Silero VAD model is used automatically if installed.
    Local {
        model_id: String,
        beam_size: i32,
    },
    OpenAiCompatible {
        base_url: String,
        api_key: Option<String>,
        model: String,
    },
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum LlmSettings {
    OpenAiCompatible { base_url: String, api_key: Option<String>, model: String },
    Anthropic { api_key: String, model: String },
}

impl From<LlmSettings> for LlmConfig {
    fn from(s: LlmSettings) -> Self {
        match s {
            LlmSettings::OpenAiCompatible { base_url, api_key, model } => {
                LlmConfig::OpenAiCompatible { base_url, api_key, model }
            }
            LlmSettings::Anthropic { api_key, model } => LlmConfig::Anthropic { api_key, model, base_url: None },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ModelFit {
    /// Barely noticeable, even during a video call.
    Light,
    Good,
    /// Works, but other apps slow down.
    Heavy,
    NotRecommended,
}

impl From<Fit> for ModelFit {
    fn from(f: Fit) -> Self {
        match f {
            Fit::Light => ModelFit::Light,
            Fit::Good => ModelFit::Good,
            Fit::Heavy => ModelFit::Heavy,
            Fit::NotRecommended => ModelFit::NotRecommended,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ModelEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub size_bytes: u64,
    /// Approximate memory while transcribing.
    pub ram_bytes: u64,
    /// 1–5 estimates for Indonesian (0 for non-STT models).
    pub accuracy: u8,
    pub speed: u8,
    pub fit: ModelFit,
    pub installed: bool,
    pub is_vad: bool,
    pub recommended_live: bool,
    pub recommended_final: bool,
    pub license: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum DeviceTier {
    Light,
    Standard,
    High,
    Max,
}

impl From<Tier> for DeviceTier {
    fn from(t: Tier) -> Self {
        match t {
            Tier::Light => DeviceTier::Light,
            Tier::Standard => DeviceTier::Standard,
            Tier::High => DeviceTier::High,
            Tier::Max => DeviceTier::Max,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct DeviceInfo {
    pub chip: String,
    pub ram_bytes: u64,
    pub performance_cores: u32,
    pub efficiency_cores: u32,
    pub free_disk_bytes: u64,
    pub apple_silicon: bool,
    pub tier: DeviceTier,
    pub tier_label: String,
}

impl DeviceInfo {
    fn new(d: &DeviceProfile, lang: Lang) -> Self {
        DeviceInfo {
            chip: d.chip.clone(),
            ram_bytes: d.ram_bytes,
            performance_cores: d.performance_cores,
            efficiency_cores: d.efficiency_cores,
            free_disk_bytes: d.free_disk_bytes,
            apple_silicon: d.apple_silicon,
            tier: d.tier().into(),
            tier_label: d.tier().label(lang).to_string(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct LocalLlmEntry {
    /// Ollama tag, e.g. "gemma3:12b".
    pub tag: String,
    pub name: String,
    pub size_bytes: u64,
    pub ram_bytes: u64,
    pub note: String,
    pub fit: ModelFit,
    pub recommended: bool,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct OllamaStatus {
    pub running: bool,
    /// Whether the model asked about is downloaded.
    pub model_installed: bool,
    /// Downloaded models, as `name:tag` (`latest` when the tag is omitted).
    pub models: Vec<String>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Recommendation {
    pub device: DeviceInfo,
    pub headline: String,
    pub reasons: Vec<String>,
    pub warnings: Vec<String>,
    pub live_model_id: String,
    pub final_model_id: String,
    pub beam_size: i32,
    /// Live, final and VAD model ids.
    pub model_ids: Vec<String>,
    /// Bytes still to download for `model_ids`.
    pub download_bytes: u64,
    pub llm_tag: String,
    pub local_llms: Vec<LocalLlmEntry>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct TemplateEntry {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum LiveStatus {
    LoadingModel,
    Listening,
    RecordingOnly,
    Paused,
    Behind { queued: u32 },
    Stopped,
}

impl From<SessionStatus> for LiveStatus {
    fn from(s: SessionStatus) -> Self {
        match s {
            SessionStatus::LoadingModel => LiveStatus::LoadingModel,
            SessionStatus::Listening => LiveStatus::Listening,
            SessionStatus::RecordingOnly => LiveStatus::RecordingOnly,
            SessionStatus::Paused => LiveStatus::Paused,
            SessionStatus::Behind { queued } => LiveStatus::Behind { queued: queued as u32 },
            SessionStatus::Stopped => LiveStatus::Stopped,
        }
    }
}

// ── Callbacks ──────────────────────────────────────────────────────────────

#[uniffi::export(foreign)]
pub trait SessionListener: Send + Sync {
    fn on_segment(&self, segment: TranscriptSegment);
    fn on_level(&self, source: AudioSource, level: f32);
    fn on_status(&self, status: LiveStatus);
    fn on_error(&self, message: String);
}

#[uniffi::export(foreign)]
pub trait ProgressListener: Send + Sync {
    fn on_progress(&self, fraction: f32, message: String);
}

struct ListenerBridge(Arc<dyn SessionListener>);

impl SessionEvents for ListenerBridge {
    fn on_segment(&self, segment: &core::Segment) {
        self.0.on_segment(segment.into());
    }
    fn on_level(&self, source: Source, level: f32) {
        self.0.on_level(source.into(), level);
    }
    fn on_status(&self, status: SessionStatus) {
        self.0.on_status(status.into());
    }
    fn on_error(&self, message: &str) {
        self.0.on_error(message.to_string());
    }
}

#[derive(uniffi::Object, Default)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
}

#[uniffi::export]
impl CancelToken {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(CancelToken::default())
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::Relaxed)
    }
}

// ── Recording session ──────────────────────────────────────────────────────

#[derive(uniffi::Object)]
pub struct RecordingSession {
    inner: Session,
}

#[uniffi::export]
impl RecordingSession {
    pub fn meeting_id(&self) -> String {
        self.inner.meeting_id().to_string()
    }

    /// Interleaved Float32 PCM at the device's native rate.
    pub fn push_audio(&self, source: AudioSource, samples: Vec<f32>, sample_rate: u32, channels: u32) {
        self.inner.push_audio(source.into(), &samples, sample_rate, channels);
    }

    pub fn pause(&self) {
        self.inner.pause();
    }

    pub fn resume(&self) {
        self.inner.resume();
    }

    pub fn is_paused(&self) -> bool {
        self.inner.is_paused()
    }

    pub fn elapsed_ms(&self) -> i64 {
        self.inner.elapsed_ms()
    }
}

// ── Engine ─────────────────────────────────────────────────────────────────

#[derive(uniffi::Object)]
pub struct MiitingEngine {
    inner: Engine,
}

impl MiitingEngine {
    fn lang(&self) -> Lang {
        self.inner.ui_lang()
    }

    /// Convert a core result, localizing the error in the interface language.
    fn ok<T>(&self, r: miiting_core::Result<T>) -> R<T> {
        r.map_err(|e| to_ffi_error(e, self.lang()))
    }

    fn unknown_model(&self, id: &str) -> MiitingError {
        MiitingError::Failed { message: tr!(self.lang(), "Unknown model: {id}", "Model tidak dikenal: {id}") }
    }

    fn stt_config(&self, s: SttSettings) -> R<SttConfig> {
        Ok(match s {
            SttSettings::Local { model_id, beam_size } => {
                let info = models::find(&model_id)
                    .filter(|m| m.kind == ModelKind::Whisper)
                    .ok_or_else(|| self.unknown_model(&model_id))?;
                if !self.inner.is_model_installed(info) {
                    return Err(MiitingError::ModelMissing { message: info.name.to_string() });
                }
                let vad = models::find("silero-vad")
                    .filter(|v| self.inner.is_model_installed(v))
                    .map(|v| self.inner.model_path(v));
                SttConfig::Local { model_path: self.inner.model_path(info), vad_model_path: vad, beam_size }
            }
            SttSettings::OpenAiCompatible { base_url, api_key, model } => {
                SttConfig::OpenAiCompatible { base_url, api_key, model }
            }
        })
    }
}

#[uniffi::export]
impl MiitingEngine {
    /// `ui_language` is a BCP 47 code such as "en" or "id-ID".
    #[uniffi::constructor]
    pub fn new(data_dir: String, ui_language: String) -> R<Arc<Self>> {
        let lang = Lang::from_code(&ui_language);
        let inner = Engine::open(std::path::Path::new(&data_dir)).map_err(|e| to_ffi_error(e, lang))?;
        inner.set_ui_lang(lang);
        Ok(Arc::new(MiitingEngine { inner }))
    }

    pub fn set_ui_language(&self, ui_language: String) {
        self.inner.set_ui_lang(Lang::from_code(&ui_language));
    }

    pub fn data_dir(&self) -> String {
        self.inner.data_dir().display().to_string()
    }

    pub fn models_dir(&self) -> String {
        self.inner.models_dir().display().to_string()
    }

    pub fn meeting_dir(&self, meeting_id: String) -> String {
        self.inner.meeting_dir(&meeting_id).display().to_string()
    }

    // Meetings

    pub fn list_meetings(&self, query: Option<String>) -> R<Vec<MeetingItem>> {
        Ok(self.ok(self.inner.list_meetings(query.as_deref()))?.iter().map(MeetingItem::from).collect())
    }

    pub fn meeting_detail(&self, meeting_id: String) -> R<MeetingDetail> {
        let m = self.ok(self.inner.meeting(&meeting_id))?;
        let (pass, segs) = self.ok(self.inner.transcript(&meeting_id))?;
        let dir = self.inner.meeting_dir(&meeting_id);
        let audio_files = [Source::Mic, Source::System, Source::File]
            .iter()
            .map(|s| dir.join(s.wav_name()))
            .filter(|p| p.exists())
            .map(|p| p.display().to_string())
            .collect();
        Ok(MeetingDetail {
            meeting: (&m).into(),
            summary_markdown: m.summary_md.clone(),
            summary_template: m.summary_template.clone(),
            summary_model: m.summary_model.clone(),
            language: m.language.clone(),
            shown_pass: pass.into(),
            segments: segs.iter().map(TranscriptSegment::from).collect(),
            audio_files,
        })
    }

    /// The language spoken in the meeting; the next transcription pass and the notes use it.
    pub fn set_meeting_language(&self, meeting_id: String, language: String) -> R<()> {
        self.ok(self.inner.store().set_language(&meeting_id, language.trim()))
    }

    pub fn rename_meeting(&self, meeting_id: String, title: String) -> R<()> {
        self.ok(self.inner.store().rename(&meeting_id, title.trim()))
    }

    pub fn update_summary(&self, meeting_id: String, markdown: String) -> R<()> {
        self.ok(self.inner.store().set_summary(&meeting_id, &markdown, None, None))
    }

    pub fn delete_meeting(&self, meeting_id: String) -> R<()> {
        self.ok(self.inner.delete_meeting(&meeting_id))
    }

    pub fn export_markdown(&self, meeting_id: String) -> R<String> {
        self.ok(self.inner.export_markdown(&meeting_id))
    }

    // Recording

    pub fn start_session(
        &self,
        title: Option<String>,
        language: String,
        glossary: Option<String>,
        live_stt: Option<SttSettings>,
        listener: Arc<dyn SessionListener>,
    ) -> R<Arc<RecordingSession>> {
        let live = match live_stt {
            Some(s) => Some(self.stt_config(s)?),
            None => None,
        };
        let session = self.ok(self.inner.start_session(
            StartOptions { title, language, glossary: glossary.filter(|g| !g.trim().is_empty()), live_stt: live },
            Arc::new(ListenerBridge(listener)),
        ))?;
        Ok(Arc::new(RecordingSession { inner: session }))
    }

    /// Stop recording. `final_pass_follows` drops the live backlog; `processing_follows`
    /// keeps the meeting in "processing" until `mark_ready`.
    pub fn finish_session(
        &self,
        session: Arc<RecordingSession>,
        final_pass_follows: bool,
        processing_follows: bool,
    ) -> R<()> {
        self.ok(self.inner.finish_session(&session.inner, final_pass_follows, processing_follows))
    }

    /// Import a PCM WAV file (any rate / channels). Returns the new meeting id.
    pub fn import_wav_file(&self, title: String, wav_path: String, language: String) -> R<String> {
        self.ok(self.inner.import_wav(&title, std::path::Path::new(&wav_path), &language))
    }

    pub fn mark_ready(&self, meeting_id: String) -> R<()> {
        self.ok(self.inner.store().set_status(&meeting_id, core::MeetingStatus::Ready))
    }

    // Processing

    pub fn run_final_pass(
        &self,
        meeting_id: String,
        stt: SttSettings,
        language: String,
        glossary: Option<String>,
        progress: Arc<dyn ProgressListener>,
        cancel: Arc<CancelToken>,
    ) -> R<u32> {
        let cfg = self.stt_config(stt)?;
        let message = tr!(self.lang(), "Transcribing", "Mentranskrip");
        let ctl = RunControl {
            progress: Some(Arc::new(move |f: f32| progress.on_progress(f, message.clone()))),
            cancel: Some(cancel.flag.clone()),
        };
        let glossary = glossary.filter(|g| !g.trim().is_empty());
        let n = self.ok(self.inner.run_final_pass(
            &meeting_id,
            &FinalPassOptions { stt: &cfg, language: &language, glossary: glossary.as_deref() },
            &ctl,
        ))?;
        Ok(n as u32)
    }

    pub fn summarize(
        &self,
        meeting_id: String,
        llm: LlmSettings,
        template_id: String,
        glossary: Option<String>,
        extra_instructions: Option<String>,
        progress: Arc<dyn ProgressListener>,
    ) -> R<String> {
        let report = move |f: f32, m: &str| progress.on_progress(f, m.to_string());
        self.ok(self.inner.summarize(
            &meeting_id,
            SummaryOptions {
                llm: llm.into(),
                template_id: &template_id,
                glossary: glossary.as_deref(),
                extra_instructions: extra_instructions.as_deref(),
            },
            &report,
        ))
    }

    /// Round-trip a tiny prompt to check URL, key, and model name.
    pub fn test_llm(&self, llm: LlmSettings) -> R<String> {
        let client = LlmClient::new(llm.into());
        self.ok(client.complete("Answer as briefly as possible.", "Reply with just the word: ready"))
    }

    /// Whether the Ollama server at `base_url` is up and has `model`. Returns quickly when it is down.
    pub fn ollama_status(&self, base_url: String, model: String) -> OllamaStatus {
        let s = ollama::status(&base_url);
        OllamaStatus { running: s.running, model_installed: s.has(&model), models: s.models }
    }

    /// Downloads an Ollama model through the running server.
    pub fn ollama_pull(
        &self,
        base_url: String,
        model: String,
        progress: Arc<dyn ProgressListener>,
        cancel: Arc<CancelToken>,
    ) -> R<()> {
        let report = |done: u64, total: u64| {
            let f = if total > 0 { done as f32 / total as f32 } else { 0.0 };
            progress.on_progress(f, format!("{} / {} MB", done / 1_048_576, total / 1_048_576));
        };
        self.ok(ollama::pull(&base_url, &model, &report, &cancel.flag))
    }

    // Models & templates

    pub fn models(&self) -> Vec<ModelEntry> {
        let lang = self.lang();
        let device = device::detect(self.inner.models_dir());
        models::CATALOG
            .iter()
            .map(|m| ModelEntry {
                id: m.id.to_string(),
                name: m.name.to_string(),
                description: m.description.get(lang).to_string(),
                size_bytes: m.size_bytes,
                ram_bytes: m.runtime_mb as u64 * 1024 * 1024,
                accuracy: m.accuracy,
                speed: m.speed,
                fit: device::model_fit(m, &device).into(),
                installed: self.inner.is_model_installed(m),
                is_vad: m.kind == ModelKind::Vad,
                recommended_live: matches!(m.role, ModelRole::Live | ModelRole::Both),
                recommended_final: matches!(m.role, ModelRole::Final | ModelRole::Both),
                license: m.license.to_string(),
            })
            .collect()
    }

    pub fn download_model(
        &self,
        model_id: String,
        progress: Arc<dyn ProgressListener>,
        cancel: Arc<CancelToken>,
    ) -> R<()> {
        let info = models::find(&model_id).ok_or_else(|| self.unknown_model(&model_id))?;
        let cb: models::DownloadProgress = Arc::new(move |done: u64, total: u64| {
            let f = if total > 0 { done as f32 / total as f32 } else { 0.0 };
            progress.on_progress(f, format!("{} / {} MB", done / 1_048_576, total / 1_048_576));
        });
        self.ok(models::download(self.inner.models_dir(), info, cb, cancel.flag.clone()))?;
        Ok(())
    }

    pub fn delete_model(&self, model_id: String) -> R<()> {
        if let Some(info) = models::find(&model_id) {
            self.ok(models::delete(self.inner.models_dir(), info))?;
        }
        Ok(())
    }

    pub fn device_info(&self) -> DeviceInfo {
        DeviceInfo::new(&device::detect(self.inner.models_dir()), self.lang())
    }

    /// Models, beam size and local LLM suited to this Mac's RAM, chip and free disk.
    pub fn recommendation(&self) -> Recommendation {
        let lang = self.lang();
        let d = device::detect(self.inner.models_dir());
        let r = device::recommend(&d, lang);
        let model_ids: Vec<String> = r.model_ids().iter().map(|s| s.to_string()).collect();
        let download_bytes = r
            .model_ids()
            .iter()
            .filter_map(|id| models::find(id))
            .filter(|m| !self.inner.is_model_installed(m))
            .map(|m| m.size_bytes)
            .sum();
        let local_llms = LOCAL_LLMS
            .iter()
            .map(|l| LocalLlmEntry {
                tag: l.tag.to_string(),
                name: l.name.to_string(),
                size_bytes: l.size_bytes,
                ram_bytes: l.runtime_mb as u64 * 1024 * 1024,
                note: l.note.get(lang).to_string(),
                fit: device::llm_fit(l, &d).into(),
                recommended: l.tag == r.llm,
            })
            .collect();
        Recommendation {
            device: DeviceInfo::new(&d, lang),
            headline: r.headline,
            reasons: r.reasons,
            warnings: r.warnings,
            live_model_id: r.live_model.to_string(),
            final_model_id: r.final_model.to_string(),
            beam_size: r.beam_size,
            model_ids,
            download_bytes,
            llm_tag: r.llm.to_string(),
            local_llms,
        }
    }

    pub fn templates(&self) -> Vec<TemplateEntry> {
        let lang = self.lang();
        TEMPLATES
            .iter()
            .map(|t| TemplateEntry {
                id: t.id.to_string(),
                name: t.name.get(lang).to_string(),
                description: t.description.get(lang).to_string(),
            })
            .collect()
    }
}
