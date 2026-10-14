//! High-level API used by the native shells.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{Datelike, Local, TimeZone, Timelike};

use crate::audio::{read_wav_16k_mono, rms, write_wav};
use crate::device;
use crate::hallucination::clean_segment;
use crate::i18n::Lang;
use crate::llm::{LlmClient, LlmConfig};
use crate::merge::merge_sources;
use crate::models::{self, ModelInfo};
use crate::prompts::{self, SummaryRequest, format_timestamp};
use crate::session::{Session, SessionEvents, SessionOptions};
use crate::store::Store;
use crate::stt::{ProgressFn, RunControl, SttRequest, build_engine};
use crate::types::{Meeting, MeetingStatus, Pass, Segment, Source, SttConfig};
use crate::{Error, Result, tr};

pub struct Engine {
    store: Arc<Store>,
    data_dir: PathBuf,
    models_dir: PathBuf,
    meetings_dir: PathBuf,
    /// Interface language for titles, messages and exports.
    ui_lang: parking_lot::RwLock<Lang>,
}

pub struct StartOptions {
    pub title: Option<String>,
    pub language: String,
    pub glossary: Option<String>,
    pub live_stt: Option<SttConfig>,
}

pub struct FinalPassOptions<'a> {
    pub stt: &'a SttConfig,
    pub language: &'a str,
    pub glossary: Option<&'a str>,
}

pub struct SummaryOptions<'a> {
    pub llm: LlmConfig,
    pub template_id: &'a str,
    pub glossary: Option<&'a str>,
    pub extra_instructions: Option<&'a str>,
}

const MONTHS_EN: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const MONTHS_ID: [&str; 12] = [
    "Januari",
    "Februari",
    "Maret",
    "April",
    "Mei",
    "Juni",
    "Juli",
    "Agustus",
    "September",
    "Oktober",
    "November",
    "Desember",
];
const DAYS_EN: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const DAYS_ID: [&str; 7] = ["Minggu", "Senin", "Selasa", "Rabu", "Kamis", "Jumat", "Sabtu"];

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn local(ms: i64) -> chrono::DateTime<Local> {
    Local.timestamp_millis_opt(ms).single().unwrap_or_else(Local::now)
}

/// "Tuesday, 14 October 2026 at 10:30" / "Selasa, 14 Oktober 2026 pukul 10.30"
pub fn long_date(ms: i64, lang: Lang) -> String {
    let d = local(ms);
    let wd = d.weekday().num_days_from_sunday() as usize;
    let (day, month, year, h, m) = (d.day(), d.month0() as usize, d.year(), d.hour(), d.minute());
    match lang {
        Lang::En => format!("{}, {day} {} {year} at {h:02}:{m:02}", DAYS_EN[wd], MONTHS_EN[month]),
        Lang::Id => format!("{}, {day} {} {year} pukul {h:02}.{m:02}", DAYS_ID[wd], MONTHS_ID[month]),
    }
}

/// "Meeting 14 Oct, 10:30" / "Rapat 14 Okt, 10.30"
pub fn default_title(ms: i64, lang: Lang) -> String {
    let d = local(ms);
    let (day, month, h, m) = (d.day(), d.month0() as usize, d.hour(), d.minute());
    match lang {
        Lang::En => format!("Meeting {day} {}, {h:02}:{m:02}", &MONTHS_EN[month][..3]),
        Lang::Id => {
            let short = match &MONTHS_ID[month][..3] {
                "Agu" => "Agt",
                s => s,
            };
            format!("Rapat {day} {short}, {h:02}.{m:02}")
        }
    }
}

pub fn format_duration(ms: i64, lang: Lang) -> String {
    let minutes = ms / 60_000;
    if minutes >= 60 {
        let (h, m) = (minutes / 60, minutes % 60);
        tr!(lang, "{h} h {m} min", "{h} jam {m} menit")
    } else {
        let m = minutes.max(if ms > 0 { 1 } else { 0 });
        tr!(lang, "{m} min", "{m} menit")
    }
}

impl Engine {
    pub fn open(data_dir: &Path) -> Result<Engine> {
        let models_dir = data_dir.join("models");
        let meetings_dir = data_dir.join("meetings");
        std::fs::create_dir_all(&models_dir)?;
        std::fs::create_dir_all(&meetings_dir)?;
        let store = Arc::new(Store::open(&data_dir.join("miiting.db"))?);
        let engine = Engine {
            store,
            data_dir: data_dir.to_path_buf(),
            models_dir,
            meetings_dir,
            ui_lang: parking_lot::RwLock::new(Lang::En),
        };
        engine.recover_interrupted()?;
        Ok(engine)
    }

    /// A crash or force-quit leaves meetings "recording"/"processing". Their audio and
    /// live transcript are on disk, so mark them ready rather than losing them.
    fn recover_interrupted(&self) -> Result<()> {
        for m in self.store.interrupted_meetings()? {
            let dir = self.meeting_dir(&m.id);
            let mut duration = m.duration_ms;
            if duration == 0 {
                for src in [Source::Mic, Source::System, Source::File] {
                    if let Ok(r) = hound::WavReader::open(dir.join(src.wav_name())) {
                        duration = duration.max(r.duration() as i64 * 1000 / r.spec().sample_rate as i64);
                    }
                }
            }
            self.store.finish_recording(
                &m.id,
                m.ended_at_ms.unwrap_or(m.created_at_ms + duration),
                duration,
                MeetingStatus::Ready,
            )?;
        }
        Ok(())
    }

    pub fn ui_lang(&self) -> Lang {
        *self.ui_lang.read()
    }

    pub fn set_ui_lang(&self, lang: Lang) {
        *self.ui_lang.write() = lang;
    }

    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn models_dir(&self) -> &Path {
        &self.models_dir
    }

    pub fn meeting_dir(&self, id: &str) -> PathBuf {
        self.meetings_dir.join(id)
    }

    // ── Recording ────────────────────────────────────────────────────────────

    pub fn start_session(&self, opts: StartOptions, events: Arc<dyn SessionEvents>) -> Result<Session> {
        let id = uuid::Uuid::new_v4().to_string();
        let created = now_ms();
        let (title, auto) = match opts.title.filter(|t| !t.trim().is_empty()) {
            Some(t) => (t, false),
            None => (default_title(created, self.ui_lang()), true),
        };
        self.store.create_meeting(&id, &title, auto, created, &opts.language)?;
        let session = Session::start(
            SessionOptions {
                meeting_id: id.clone(),
                meeting_dir: self.meeting_dir(&id),
                language: opts.language,
                glossary: opts.glossary,
                live_stt: opts.live_stt,
                ui_lang: self.ui_lang(),
            },
            self.store.clone(),
            events,
        );
        if session.is_err() {
            let _ = self.store.delete_meeting(&id);
        }
        session
    }

    /// Stop a session.
    /// - `final_pass_follows`: drop the live backlog, the final pass re-transcribes everything.
    /// - `processing_follows`: leave the meeting in "processing" for the caller's next step.
    pub fn finish_session(&self, session: &Session, final_pass_follows: bool, processing_follows: bool) -> Result<()> {
        let duration = session.stop(!final_pass_follows)?;
        let status = if processing_follows { MeetingStatus::Processing } else { MeetingStatus::Ready };
        self.store.finish_recording(session.meeting_id(), now_ms(), duration, status)
    }

    /// Create a meeting from an imported recording (a PCM WAV at any rate / channel count;
    /// the shell converts other formats first).
    pub fn import_wav(&self, title: &str, wav_path: &Path, language: &str) -> Result<String> {
        let pcm = read_wav_16k_mono(wav_path)?;
        if pcm.is_empty() {
            return Err(Error::EmptyAudio);
        }
        let id = uuid::Uuid::new_v4().to_string();
        let created = now_ms();
        let dir = self.meeting_dir(&id);
        std::fs::create_dir_all(&dir)?;
        write_wav(&dir.join(Source::File.wav_name()), &pcm)?;
        let t = if title.trim().is_empty() { default_title(created, self.ui_lang()) } else { title.trim().to_string() };
        self.store.create_meeting(&id, &t, title.trim().is_empty(), created, language)?;
        let duration = pcm.len() as i64 * 1000 / 16_000;
        self.store.finish_recording(&id, created, duration, MeetingStatus::Processing)?;
        Ok(id)
    }

    // ── Final pass ───────────────────────────────────────────────────────────

    /// Re-transcribe the saved audio with the (usually larger) final model and replace
    /// the live draft. Each source is transcribed separately, then merged.
    pub fn run_final_pass(&self, meeting_id: &str, opts: &FinalPassOptions, ctl: &RunControl) -> Result<usize> {
        self.store.set_status(meeting_id, MeetingStatus::Processing)?;
        let result = self.final_pass_inner(meeting_id, opts, ctl);
        self.store.set_status(meeting_id, MeetingStatus::Ready)?;
        result
    }

    fn final_pass_inner(&self, meeting_id: &str, opts: &FinalPassOptions, ctl: &RunControl) -> Result<usize> {
        let dir = self.meeting_dir(meeting_id);
        let mut inputs: Vec<(Source, Vec<f32>)> = Vec::new();
        for src in [Source::Mic, Source::System, Source::File] {
            let p = dir.join(src.wav_name());
            if p.exists() {
                let pcm = read_wav_16k_mono(&p)?;
                // Skip channels that were captured but stayed silent (e.g. system audio with no call).
                if rms(&pcm) > 1e-4 {
                    inputs.push((src, pcm));
                }
            }
        }
        if inputs.is_empty() {
            return Err(Error::NothingToTranscribe);
        }

        let mut engine = build_engine(opts.stt)?;
        if let Some(host) = opts.stt.egress_host() {
            self.store.add_egress(meeting_id, &host)?;
        }
        let total: usize = inputs.iter().map(|(_, p)| p.len()).sum::<usize>().max(1);
        let mut done = 0usize;
        let mut segments = Vec::new();
        for (src, pcm) in &inputs {
            let (base, weight) = (done as f32 / total as f32, pcm.len() as f32 / total as f32);
            let progress: Option<ProgressFn> = ctl.progress.clone().map(|p| {
                let f: ProgressFn = Arc::new(move |x: f32| p(base + x * weight));
                f
            });
            let sub = RunControl { progress, cancel: ctl.cancel.clone() };
            let req = SttRequest { samples: pcm, language: opts.language, prompt: opts.glossary, long_form: true };
            for s in engine.transcribe(&req, &sub)? {
                if let Some(text) = clean_segment(&s.text, s.no_speech_prob) {
                    segments.push(Segment { source: *src, start_ms: s.start_ms, end_ms: s.end_ms, text });
                }
            }
            done += pcm.len();
        }
        let merged = merge_sources(segments);
        self.store.replace_final_segments(meeting_id, &merged, &engine.name())?;
        Ok(merged.len())
    }

    // ── Summary ──────────────────────────────────────────────────────────────

    pub fn summarize(&self, meeting_id: &str, opts: SummaryOptions, progress: &dyn Fn(f32, &str)) -> Result<String> {
        let meeting = self.store.meeting(meeting_id)?;
        let (_, segments) = self.store.best_segments(meeting_id)?;
        if segments.is_empty() {
            return Err(Error::EmptyTranscript);
        }
        let tuning = device::local_llm_tuning(&device::detect(&self.models_dir));
        let llm = LlmClient::with_tuning(opts.llm, tuning);
        if let Some(host) = llm.config().egress_host() {
            self.store.add_egress(meeting_id, &host)?;
        }
        let notes_lang = Lang::for_notes(&meeting.language);
        let date = long_date(meeting.created_at_ms, notes_lang);
        let out = prompts::summarize(
            &llm,
            &segments,
            &SummaryRequest {
                template_id: opts.template_id,
                glossary: opts.glossary,
                extra_instructions: opts.extra_instructions,
                meeting_date: &date,
                notes_lang,
                ui_lang: self.ui_lang(),
            },
            progress,
        )?;
        self.store.set_summary(meeting_id, &out.markdown, Some(opts.template_id), Some(&llm.config().describe()))?;
        if let Some(t) = out.title {
            self.store.set_auto_title(meeting_id, &t)?;
        }
        Ok(out.markdown)
    }

    // ── Meetings ─────────────────────────────────────────────────────────────

    pub fn list_meetings(&self, query: Option<&str>) -> Result<Vec<Meeting>> {
        self.store.list_meetings(query)
    }

    pub fn meeting(&self, id: &str) -> Result<Meeting> {
        self.store.meeting(id)
    }

    pub fn transcript(&self, id: &str) -> Result<(Pass, Vec<Segment>)> {
        self.store.best_segments(id)
    }

    pub fn delete_meeting(&self, id: &str) -> Result<()> {
        self.store.delete_meeting(id)?;
        let dir = self.meeting_dir(id);
        if dir.exists() {
            std::fs::remove_dir_all(dir)?;
        }
        Ok(())
    }

    /// The meeting as Markdown: notes followed by the transcript, in the interface language.
    pub fn export_markdown(&self, id: &str) -> Result<String> {
        let lang = self.ui_lang();
        let m = self.store.meeting(id)?;
        let (pass, segs) = self.store.best_segments(id)?;
        let mut md = format!("# {}\n\n", m.title);
        md.push_str(&format!("*{} · {}", long_date(m.created_at_ms, lang), format_duration(m.duration_ms, lang)));
        if let Some(engine) = &m.stt_engine {
            md.push_str(&tr!(lang, " · Transcribed with {engine}", " · Ditranskrip dengan {engine}"));
        }
        if pass == Pass::Live {
            md.push_str(&tr!(lang, " (live draft)", " (draf live)"));
        }
        md.push_str("*\n\n");
        if let Some(summary) = m.summary_md.as_ref().filter(|s| !s.trim().is_empty()) {
            md.push_str(summary.trim());
            md.push_str("\n\n---\n\n");
        }
        md.push_str(&tr!(lang, "## Transcript\n\n", "## Transkrip\n\n"));
        for s in &segs {
            md.push_str(&format!(
                "**[{}] {}:** {}\n\n",
                format_timestamp(s.start_ms),
                s.source.speaker_label(lang),
                s.text
            ));
        }
        Ok(md)
    }

    // ── Models ───────────────────────────────────────────────────────────────

    pub fn model_path(&self, info: &ModelInfo) -> PathBuf {
        models::path_for(&self.models_dir, info)
    }

    pub fn is_model_installed(&self, info: &ModelInfo) -> bool {
        models::is_installed(&self.models_dir, info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(format_duration(0, Lang::En), "0 min");
        assert_eq!(format_duration(30_000, Lang::Id), "1 menit");
        assert_eq!(format_duration(3_900_000, Lang::En), "1 h 5 min");
        assert!(default_title(now_ms(), Lang::En).starts_with("Meeting "));
        assert!(default_title(now_ms(), Lang::Id).starts_with("Rapat "));
        assert!(long_date(now_ms(), Lang::Id).contains(" pukul "));
        assert!(long_date(now_ms(), Lang::En).contains(" at "));
    }

    struct NullEvents;
    impl SessionEvents for NullEvents {
        fn on_segment(&self, _: &Segment) {}
        fn on_level(&self, _: Source, _: f32) {}
        fn on_status(&self, _: crate::session::SessionStatus) {}
        fn on_error(&self, e: &str) {
            panic!("{e}")
        }
    }

    #[test]
    fn record_only_session_writes_aligned_wavs() {
        let dir = std::env::temp_dir().join(format!("miiting-engine-{}", uuid::Uuid::new_v4()));
        let engine = Engine::open(&dir).unwrap();
        let session = engine
            .start_session(
                StartOptions { title: None, language: "id".into(), glossary: None, live_stt: None },
                Arc::new(NullEvents),
            )
            .unwrap();
        let tone: Vec<f32> = (0..48_000)
            .flat_map(|i| {
                let v = (i as f32 * 0.05).sin() * 0.2;
                [v, v]
            })
            .collect();
        session.push_audio(Source::Mic, &tone, 48_000, 2);
        std::thread::sleep(std::time::Duration::from_millis(300));
        session.push_audio(Source::System, &tone, 48_000, 2);
        engine.finish_session(&session, false, false).unwrap();

        let id = session.meeting_id().to_string();
        let m = engine.meeting(&id).unwrap();
        assert_eq!(m.status, MeetingStatus::Ready);
        assert!(m.title.starts_with("Meeting "));
        let mic = read_wav_16k_mono(&engine.meeting_dir(&id).join("mic.wav")).unwrap();
        let sys = read_wav_16k_mono(&engine.meeting_dir(&id).join("system.wav")).unwrap();
        assert!((mic.len() as i64 - 16_000).abs() < 100, "mic {}", mic.len());
        // System started ~300 ms later and was padded with that much silence.
        let pad = sys.len() as i64 - mic.len() as i64;
        assert!((4_000..7_000).contains(&pad), "pad {pad}");

        let md = engine.export_markdown(&id).unwrap();
        assert!(md.contains("## Transcript"));
        engine.delete_meeting(&id).unwrap();
        assert!(engine.list_meetings(None).unwrap().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
