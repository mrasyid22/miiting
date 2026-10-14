//! A live recording session: PCM in, WAV files + live transcript out.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};
use parking_lot::Mutex;

use crate::audio::{Resampler, WavSink, downmix, meter_level};
use crate::chunker::{Chunker, ChunkerConfig};
use crate::hallucination::clean_segment;
use crate::i18n::Lang;
use crate::merge::is_echo;
use crate::store::Store;
use crate::stt::{RunControl, SttRequest, build_engine};
use crate::types::{Pass, Segment, Source, SttConfig};
use crate::{Result, STT_SAMPLE_RATE, tr};

#[derive(Debug, Clone, PartialEq)]
pub enum SessionStatus {
    LoadingModel,
    Listening,
    /// Recording audio only; no live transcript (no engine configured or it failed to load).
    RecordingOnly,
    Paused,
    /// Live transcription is lagging behind real time.
    Behind {
        queued: usize,
    },
    Stopped,
}

pub trait SessionEvents: Send + Sync {
    fn on_segment(&self, segment: &Segment);
    fn on_level(&self, source: Source, level: f32);
    fn on_status(&self, status: SessionStatus);
    fn on_error(&self, message: &str);
}

#[derive(Clone)]
pub struct SessionOptions {
    pub meeting_id: String,
    pub meeting_dir: PathBuf,
    pub language: String,
    pub glossary: Option<String>,
    /// `None` records audio without a live transcript.
    pub live_stt: Option<SttConfig>,
    /// Language of error messages sent to `SessionEvents::on_error`.
    pub ui_lang: Lang,
}

struct SourcePipe {
    resampler: Resampler,
    chunker: Chunker,
    wav: WavSink,
    level_buf: Vec<f32>,
}

enum Job {
    Chunk { source: Source, start_ms: i64, samples: Vec<f32> },
}

struct Inner {
    opts: SessionOptions,
    events: Arc<dyn SessionEvents>,
    started: Instant,
    pipes: Mutex<HashMap<Source, SourcePipe>>,
    tx: Mutex<Option<Sender<Job>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    paused: AtomicBool,
    stopped: AtomicBool,
    /// Set on stop when a final pass will replace the live draft anyway.
    discard_backlog: Arc<AtomicBool>,
    pause_total: Mutex<Duration>,
    pause_started: Mutex<Option<Instant>>,
}

#[derive(Clone)]
pub struct Session {
    inner: Arc<Inner>,
}

const LEVEL_INTERVAL: usize = STT_SAMPLE_RATE as usize / 10;

impl Session {
    pub fn start(opts: SessionOptions, store: Arc<Store>, events: Arc<dyn SessionEvents>) -> Result<Session> {
        std::fs::create_dir_all(&opts.meeting_dir)?;
        let (tx, rx) = unbounded::<Job>();
        let discard = Arc::new(AtomicBool::new(false));
        let inner = Arc::new(Inner {
            opts: opts.clone(),
            events: events.clone(),
            started: Instant::now(),
            pipes: Mutex::new(HashMap::new()),
            tx: Mutex::new(Some(tx)),
            worker: Mutex::new(None),
            paused: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
            discard_backlog: discard.clone(),
            pause_total: Mutex::new(Duration::ZERO),
            pause_started: Mutex::new(None),
        });
        let handle = std::thread::Builder::new()
            .name("miiting-live-stt".into())
            .spawn(move || live_worker(opts, store, events, rx, discard))?;
        *inner.worker.lock() = Some(handle);
        Ok(Session { inner })
    }

    pub fn meeting_id(&self) -> &str {
        &self.inner.opts.meeting_id
    }

    /// Recorded time so far, excluding pauses.
    pub fn elapsed_ms(&self) -> i64 {
        let mut paused = *self.inner.pause_total.lock();
        if let Some(p) = *self.inner.pause_started.lock() {
            paused += p.elapsed();
        }
        self.inner.started.elapsed().saturating_sub(paused).as_millis() as i64
    }

    pub fn is_paused(&self) -> bool {
        self.inner.paused.load(Ordering::Relaxed)
    }

    /// Feed interleaved Float32 PCM at any rate / channel count. Call from a
    /// non-realtime thread (it allocates and may write to disk).
    pub fn push_audio(&self, source: Source, interleaved: &[f32], sample_rate: u32, channels: u32) {
        if self.inner.stopped.load(Ordering::Relaxed) || self.is_paused() || sample_rate == 0 {
            return;
        }
        let elapsed_samples = (self.elapsed_ms().max(0) as u64) * STT_SAMPLE_RATE as u64 / 1000;
        let mut pipes = self.inner.pipes.lock();
        let pipe = match pipes.entry(source) {
            Entry::Occupied(e) => e.into_mut(),
            Entry::Vacant(slot) => match self.open_pipe(source, sample_rate, elapsed_samples) {
                Ok(p) => slot.insert(p),
                Err(e) => {
                    let (lang, who) = (self.inner.opts.ui_lang, source.speaker_label(self.inner.opts.ui_lang));
                    let reason = e.localized(lang);
                    self.inner.events.on_error(&tr!(
                        lang,
                        "Couldn't save the {who} audio: {reason}",
                        "Gagal menyimpan audio {who}: {reason}"
                    ));
                    return;
                }
            },
        };
        if pipe.resampler.in_rate() != sample_rate {
            // Device switched mid-meeting (e.g. AirPods connected).
            pipe.resampler = Resampler::new(sample_rate, STT_SAMPLE_RATE);
        }
        let mono = downmix(interleaved, channels.max(1) as usize);
        let pcm = pipe.resampler.process(&mono);
        if let Err(e) = pipe.wav.write(&pcm) {
            let (lang, reason) = (self.inner.opts.ui_lang, e.localized(self.inner.opts.ui_lang));
            self.inner.events.on_error(&tr!(lang, "Couldn't write audio: {reason}", "Gagal menulis audio: {reason}"));
        }

        pipe.level_buf.extend_from_slice(&pcm);
        if pipe.level_buf.len() >= LEVEL_INTERVAL {
            self.inner.events.on_level(source, meter_level(&pipe.level_buf));
            pipe.level_buf.clear();
        }

        let chunks = pipe.chunker.push(&pcm);
        drop(pipes);
        if let Some(tx) = self.inner.tx.lock().as_ref() {
            for c in chunks {
                let start_ms = c.start_ms();
                let _ = tx.send(Job::Chunk { source, start_ms, samples: c.samples });
            }
        }
    }

    fn open_pipe(&self, source: Source, sample_rate: u32, offset_samples: u64) -> Result<SourcePipe> {
        let mut wav = WavSink::create(&self.inner.opts.meeting_dir.join(source.wav_name()))?;
        let mut chunker = Chunker::new(ChunkerConfig::default());
        // A source that starts late is padded so both WAVs share the session timeline.
        if offset_samples > 0 {
            let zeros = vec![0.0f32; 16_000];
            let mut left = offset_samples;
            while left > 0 {
                let n = left.min(zeros.len() as u64) as usize;
                wav.write(&zeros[..n])?;
                left -= n as u64;
            }
            chunker.advance(offset_samples);
        }
        Ok(SourcePipe {
            resampler: Resampler::new(sample_rate, STT_SAMPLE_RATE),
            chunker,
            wav,
            level_buf: Vec::with_capacity(LEVEL_INTERVAL * 2),
        })
    }

    pub fn pause(&self) {
        if !self.inner.paused.swap(true, Ordering::Relaxed) {
            *self.inner.pause_started.lock() = Some(Instant::now());
            // Close utterances in progress so the transcript doesn't stall until resume.
            let mut pipes = self.inner.pipes.lock();
            let mut pending = Vec::new();
            for (src, pipe) in pipes.iter_mut() {
                if let Some(c) = pipe.chunker.finish() {
                    pending.push((*src, c));
                }
            }
            drop(pipes);
            if let Some(tx) = self.inner.tx.lock().as_ref() {
                for (source, c) in pending {
                    let _ = tx.send(Job::Chunk { source, start_ms: c.start_ms(), samples: c.samples });
                }
            }
            self.inner.events.on_status(SessionStatus::Paused);
        }
    }

    pub fn resume(&self) {
        if self.inner.paused.swap(false, Ordering::Relaxed) {
            if let Some(p) = self.inner.pause_started.lock().take() {
                *self.inner.pause_total.lock() += p.elapsed();
            }
            self.inner.events.on_status(SessionStatus::Listening);
        }
    }

    /// Stop recording. With `drain_live = false` the live backlog is dropped because a
    /// final pass is about to re-transcribe everything. Returns the recorded duration.
    pub fn stop(&self, drain_live: bool) -> Result<i64> {
        if self.inner.stopped.swap(true, Ordering::Relaxed) {
            return Ok(self.elapsed_ms());
        }
        let duration = self.elapsed_ms();
        if !drain_live {
            self.inner.discard_backlog.store(true, Ordering::Relaxed);
        }
        let mut pipes = self.inner.pipes.lock();
        let mut tail_chunks = Vec::new();
        for (src, pipe) in pipes.iter_mut() {
            let rest = pipe.resampler.flush();
            let _ = pipe.wav.write(&rest);
            tail_chunks.extend(pipe.chunker.push(&rest).into_iter().map(|c| (*src, c)));
            if let Some(c) = pipe.chunker.finish() {
                tail_chunks.push((*src, c));
            }
            pipe.wav.finalize()?;
        }
        drop(pipes);
        if let Some(tx) = self.inner.tx.lock().take() {
            if drain_live {
                for (source, c) in tail_chunks {
                    let _ = tx.send(Job::Chunk { source, start_ms: c.start_ms(), samples: c.samples });
                }
            }
            drop(tx);
        }
        if let Some(h) = self.inner.worker.lock().take() {
            let _ = h.join();
        }
        self.inner.events.on_status(SessionStatus::Stopped);
        Ok(duration)
    }

    pub fn recorded_sources(&self) -> Vec<Source> {
        self.inner.pipes.lock().keys().copied().collect()
    }
}

fn live_worker(
    opts: SessionOptions,
    store: Arc<Store>,
    events: Arc<dyn SessionEvents>,
    rx: Receiver<Job>,
    discard: Arc<AtomicBool>,
) {
    let mut engine = match &opts.live_stt {
        None => {
            events.on_status(SessionStatus::RecordingOnly);
            None
        }
        Some(cfg) => {
            events.on_status(SessionStatus::LoadingModel);
            match build_engine(cfg) {
                Ok(e) => {
                    if let Some(host) = cfg.egress_host() {
                        let _ = store.add_egress(&opts.meeting_id, &host);
                    }
                    let _ = store.set_stt_engine(&opts.meeting_id, &e.name());
                    events.on_status(SessionStatus::Listening);
                    Some(e)
                }
                Err(e) => {
                    let (lang, reason) = (opts.ui_lang, e.localized(opts.ui_lang));
                    events.on_error(&tr!(
                        lang,
                        "Live transcript is off: {reason}",
                        "Transkrip live tidak aktif: {reason}"
                    ));
                    events.on_status(SessionStatus::RecordingOnly);
                    None
                }
            }
        }
    };

    let mut recent_system: VecDeque<Segment> = VecDeque::new();
    let mut was_behind = false;
    let ctl = RunControl::none();

    for job in rx.iter() {
        let Some(engine) = engine.as_mut() else { continue };
        if discard.load(Ordering::Relaxed) {
            continue;
        }
        let queued = rx.len();
        if queued >= 3 {
            events.on_status(SessionStatus::Behind { queued });
            was_behind = true;
        } else if was_behind && queued == 0 {
            events.on_status(SessionStatus::Listening);
            was_behind = false;
        }

        let Job::Chunk { source, start_ms, samples } = job;
        let chunk_ms = (samples.len() as i64 * 1000) / STT_SAMPLE_RATE as i64;
        let req = SttRequest {
            samples: &samples,
            language: &opts.language,
            prompt: opts.glossary.as_deref(),
            long_form: false,
        };
        let segs = match engine.transcribe(&req, &ctl) {
            Ok(s) => s,
            Err(e) => {
                let (lang, reason) = (opts.ui_lang, e.localized(opts.ui_lang));
                events.on_error(&tr!(lang, "Transcription failed: {reason}", "Transkripsi gagal: {reason}"));
                continue;
            }
        };
        for s in segs {
            let Some(text) = clean_segment(&s.text, s.no_speech_prob) else { continue };
            let seg = Segment {
                source,
                start_ms: start_ms + s.start_ms.clamp(0, chunk_ms),
                end_ms: start_ms + s.end_ms.clamp(0, chunk_ms),
                text,
            };
            if seg.source == Source::Mic && recent_system.iter().any(|sys| is_echo(&seg, sys)) {
                continue;
            }
            if let Err(e) = store.insert_segment(&opts.meeting_id, Pass::Live, &seg) {
                let (lang, reason) = (opts.ui_lang, e.localized(opts.ui_lang));
                events.on_error(&tr!(
                    lang,
                    "Couldn't save the transcript: {reason}",
                    "Gagal menyimpan transkrip: {reason}"
                ));
            }
            events.on_segment(&seg);
            if seg.source == Source::System {
                recent_system.push_back(seg);
                while recent_system.front().is_some_and(|f| start_ms - f.end_ms > 30_000) {
                    recent_system.pop_front();
                }
            }
        }
    }
}
