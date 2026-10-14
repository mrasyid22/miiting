//! miiting-cli: benchmark, transcribe, and dev utilities for the Miiting core.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use serde::Serialize;

use miiting_core::audio::read_wav_16k_mono;
use miiting_core::engine::{Engine, now_ms};
use miiting_core::hallucination::clean_segment;
use miiting_core::stt::{RunControl, SttEngine, SttRequest};
use miiting_core::types::{MeetingStatus, Segment, Source, SttConfig};
use miiting_core::wer::{ErrorCounts, cer, wer};
use miiting_core::{Lang, STT_SAMPLE_RATE, stt};

#[derive(Parser)]
#[command(name = "miiting-cli", version, about = "Miiting core tools")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Measure WER / CER / speed of STT engines on a labeled dataset.
    Bench(BenchArgs),
    /// Transcribe one audio file (WAV) and print the text.
    Transcribe(TranscribeArgs),
    /// Stream recorded channels through a live session exactly like the app does,
    /// then run the final pass and (optionally) a summary.
    Simulate(SimulateArgs),
    /// (Re)write the notes for an existing meeting in a data folder.
    Summarize {
        #[arg(long)]
        data_dir: PathBuf,
        #[arg(long)]
        meeting: String,
        #[arg(long, default_value = "gemma3:12b")]
        ollama_model: String,
        #[arg(long, default_value = "http://localhost:11434/v1")]
        ollama_url: String,
        #[arg(long, default_value = "umum")]
        template: String,
    },
    /// Show this Mac's specs and the recommended models.
    Doctor {
        /// Folder whose volume is checked for free space.
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Fill a data directory with example meetings (for UI work and screenshots).
    SeedDemo {
        #[arg(long)]
        data_dir: PathBuf,
    },
}

#[derive(Args)]
struct EngineArgs {
    /// ggml Whisper model file. Repeat to compare several models.
    #[arg(long = "model")]
    models: Vec<PathBuf>,
    /// Beam sizes to try (1 = greedy). Comma-separated.
    #[arg(long, value_delimiter = ',', default_value = "5")]
    beam: Vec<i32>,
    /// Silero VAD model (ggml) for long-form audio.
    #[arg(long)]
    vad: Option<PathBuf>,
    /// Also benchmark an OpenAI-compatible endpoint (API key from MIITING_API_KEY).
    #[arg(long)]
    openai_base_url: Option<String>,
    #[arg(long)]
    openai_model: Option<String>,
}

#[derive(Args)]
struct BenchArgs {
    /// FLEURS language directory, e.g. data/fleurs/id_id (contains test.tsv and audio/test/).
    #[arg(long, conflicts_with = "manifest")]
    fleurs: Option<PathBuf>,
    #[arg(long, default_value = "test")]
    split: String,
    /// TSV with `audio_path<TAB>reference transcript` per line (your own recordings).
    #[arg(long)]
    manifest: Option<PathBuf>,
    #[command(flatten)]
    engines: EngineArgs,
    #[arg(long, default_value = "id")]
    language: String,
    /// Vocabulary prompt passed to the engine.
    #[arg(long)]
    prompt: Option<String>,
    /// Only use the first N utterances.
    #[arg(long)]
    limit: Option<usize>,
    /// Directory for the JSON report and per-utterance results.
    #[arg(long, default_value = "reports")]
    out: PathBuf,
}

#[derive(Args)]
struct TranscribeArgs {
    audio: PathBuf,
    #[arg(long)]
    model: PathBuf,
    #[arg(long, default_value_t = 5)]
    beam: i32,
    #[arg(long)]
    vad: Option<PathBuf>,
    #[arg(long, default_value = "id")]
    language: String,
    #[arg(long)]
    prompt: Option<String>,
}

#[derive(Args)]
struct SimulateArgs {
    /// Microphone channel ("Saya"), any WAV.
    #[arg(long)]
    mic: Option<PathBuf>,
    /// System-audio channel ("Peserta"), any WAV.
    #[arg(long)]
    system: Option<PathBuf>,
    /// Data folder for the engine (use a scratch folder, not the app's).
    #[arg(long)]
    data_dir: PathBuf,
    #[arg(long)]
    live_model: Option<PathBuf>,
    #[arg(long)]
    final_model: Option<PathBuf>,
    #[arg(long)]
    vad: Option<PathBuf>,
    #[arg(long, default_value_t = 5)]
    beam: i32,
    /// 1.0 = real time, 2.0 = twice as fast.
    #[arg(long, default_value_t = 1.0)]
    speed: f64,
    #[arg(long)]
    glossary: Option<String>,
    /// Reference transcript for WER.
    #[arg(long)]
    reference: Option<PathBuf>,
    /// Ollama model for the summary, e.g. gemma3:12b.
    #[arg(long)]
    ollama_model: Option<String>,
    #[arg(long, default_value = "http://localhost:11434/v1")]
    ollama_url: String,
    #[arg(long, default_value = "umum")]
    template: String,
}

struct Item {
    id: String,
    audio: PathBuf,
    reference: String,
}

#[derive(Serialize)]
struct UtteranceResult<'a> {
    id: &'a str,
    reference: &'a str,
    hypothesis: String,
    wer: f64,
    cer: f64,
    audio_secs: f64,
    proc_secs: f64,
}

#[derive(Serialize)]
struct EngineReport {
    engine: String,
    beam: Option<i32>,
    utterances: usize,
    wer: f64,
    cer: f64,
    word_errors: ErrorCounts,
    char_errors: ErrorCounts,
    audio_secs: f64,
    proc_secs: f64,
    /// Processing time / audio time. Below 1.0 is faster than real time.
    rtf: f64,
    load_secs: f64,
}

fn main() -> Result<()> {
    env_logger::init();
    match Cli::parse().cmd {
        Cmd::Bench(a) => bench(a),
        Cmd::Transcribe(a) => transcribe(a),
        Cmd::Simulate(a) => simulate(a),
        Cmd::Doctor { path } => doctor(&path),
        Cmd::Summarize { data_dir, meeting, ollama_model, ollama_url, template } => {
            let engine = Engine::open(&data_dir)?;
            let t = Instant::now();
            let md = engine.summarize(
                &meeting,
                miiting_core::engine::SummaryOptions {
                    llm: miiting_core::llm::LlmConfig::OpenAiCompatible {
                        base_url: ollama_url,
                        api_key: None,
                        model: ollama_model,
                    },
                    template_id: &template,
                    glossary: None,
                    extra_instructions: None,
                },
                &|f, m| eprintln!("  {:>3.0}% {m}", f * 100.0),
            )?;
            println!("Title: {}\n\n{md}\n\n({:.1}s)", engine.meeting(&meeting)?.title, t.elapsed().as_secs_f64());
            Ok(())
        }
        Cmd::SeedDemo { data_dir } => seed_demo(&data_dir),
    }
}

fn load_fleurs(dir: &Path, split: &str) -> Result<Vec<Item>> {
    let tsv = dir.join(format!("{split}.tsv"));
    let text = fs::read_to_string(&tsv).with_context(|| format!("reading {}", tsv.display()))?;
    let audio_dir = dir.join("audio").join(split);
    let mut items = Vec::new();
    for line in text.lines() {
        // id, file_name, raw_transcription, transcription, phonemes, num_samples, gender
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 4 {
            continue;
        }
        let audio = audio_dir.join(cols[1]);
        if audio.exists() {
            items.push(Item { id: cols[0].to_string(), audio, reference: cols[2].to_string() });
        }
    }
    if items.is_empty() {
        bail!("no FLEURS audio in {} — run scripts/fetch-fleurs.sh first", audio_dir.display());
    }
    // FLEURS repeats each sentence with several speakers; keep them all (different voices).
    Ok(items)
}

fn load_manifest(path: &Path) -> Result<Vec<Item>> {
    let base = path.parent().unwrap_or(Path::new("."));
    let text = fs::read_to_string(path)?;
    Ok(text
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .enumerate()
        .filter_map(|(i, l)| {
            let (a, r) = l.split_once('\t')?;
            let audio = if Path::new(a).is_absolute() { PathBuf::from(a) } else { base.join(a) };
            Some(Item { id: format!("{i}"), audio, reference: r.to_string() })
        })
        .collect())
}

fn engine_configs(e: &EngineArgs) -> Vec<(SttConfig, Option<i32>)> {
    let mut out = Vec::new();
    for m in &e.models {
        for &b in &e.beam {
            out.push((
                SttConfig::Local { model_path: m.clone(), vad_model_path: e.vad.clone(), beam_size: b },
                Some(b),
            ));
        }
    }
    if let (Some(url), Some(model)) = (&e.openai_base_url, &e.openai_model) {
        out.push((
            SttConfig::OpenAiCompatible {
                base_url: url.clone(),
                api_key: std::env::var("MIITING_API_KEY").ok(),
                model: model.clone(),
            },
            None,
        ));
    }
    out
}

fn run_one(engine: &mut dyn SttEngine, pcm: &[f32], language: &str, prompt: Option<&str>) -> Result<String> {
    let long_form = pcm.len() > 30 * STT_SAMPLE_RATE as usize;
    let segs = engine.transcribe(&SttRequest { samples: pcm, language, prompt, long_form }, &RunControl::none())?;
    Ok(segs.into_iter().filter_map(|s| clean_segment(&s.text, s.no_speech_prob)).collect::<Vec<_>>().join(" "))
}

fn bench(a: BenchArgs) -> Result<()> {
    let mut items = match (&a.fleurs, &a.manifest) {
        (Some(d), _) => load_fleurs(d, &a.split)?,
        (None, Some(m)) => load_manifest(m)?,
        _ => bail!("pass --fleurs <dir> or --manifest <file.tsv>"),
    };
    if let Some(n) = a.limit {
        items.truncate(n);
    }
    let configs = engine_configs(&a.engines);
    if configs.is_empty() {
        bail!("pass at least one --model, or --openai-base-url with --openai-model");
    }
    fs::create_dir_all(&a.out)?;
    println!("{} utterances, {} engine configurations\n", items.len(), configs.len());

    // Decode audio once.
    let audio: Vec<Vec<f32>> =
        items.iter().map(|i| read_wav_16k_mono(&i.audio)).collect::<std::result::Result<_, _>>()?;
    let mut reports = Vec::new();

    for (cfg, beam) in configs {
        let t0 = Instant::now();
        let mut engine = stt::build_engine(&cfg)?;
        let load_secs = t0.elapsed().as_secs_f64();
        let name = engine.name();
        let label = match beam {
            Some(b) => format!("{name} (beam {b})"),
            None => name.clone(),
        };
        let slug: String = label.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
        let mut jsonl = fs::File::create(a.out.join(format!("{slug}.jsonl")))?;

        let (mut we, mut ce) = (ErrorCounts::default(), ErrorCounts::default());
        let (mut audio_secs, mut proc_secs) = (0.0, 0.0);
        for (i, (item, pcm)) in items.iter().zip(&audio).enumerate() {
            let t = Instant::now();
            let hyp = run_one(engine.as_mut(), pcm, &a.language, a.prompt.as_deref())?;
            let dt = t.elapsed().as_secs_f64();
            let secs = pcm.len() as f64 / STT_SAMPLE_RATE as f64;
            let w = wer(&item.reference, &hyp);
            let c = cer(&item.reference, &hyp);
            we.add(&w);
            ce.add(&c);
            audio_secs += secs;
            proc_secs += dt;
            serde_json::to_writer(
                &mut jsonl,
                &UtteranceResult {
                    id: &item.id,
                    reference: &item.reference,
                    hypothesis: hyp,
                    wer: w.rate(),
                    cer: c.rate(),
                    audio_secs: secs,
                    proc_secs: dt,
                },
            )?;
            writeln!(jsonl)?;
            if (i + 1) % 25 == 0 || i + 1 == items.len() {
                eprint!("\r{label}: {}/{}  WER {:.2}%", i + 1, items.len(), we.rate() * 100.0);
            }
        }
        eprintln!();
        reports.push(EngineReport {
            engine: name,
            beam,
            utterances: items.len(),
            wer: we.rate(),
            cer: ce.rate(),
            word_errors: we,
            char_errors: ce,
            audio_secs,
            proc_secs,
            rtf: if audio_secs > 0.0 { proc_secs / audio_secs } else { 0.0 },
            load_secs,
        });
    }

    println!("\n| Engine | Beam | N | WER | CER | RTF | Load |");
    println!("|---|---|---|---|---|---|---|");
    for r in &reports {
        println!(
            "| {} | {} | {} | {:.2}% | {:.2}% | {:.3} | {:.1}s |",
            r.engine,
            r.beam.map(|b| b.to_string()).unwrap_or_else(|| "–".into()),
            r.utterances,
            r.wer * 100.0,
            r.cer * 100.0,
            r.rtf,
            r.load_secs
        );
    }
    let report_path = a.out.join("report.json");
    fs::write(&report_path, serde_json::to_string_pretty(&reports)?)?;
    println!("\nLaporan: {}", report_path.display());
    Ok(())
}

fn transcribe(a: TranscribeArgs) -> Result<()> {
    let pcm = read_wav_16k_mono(&a.audio)?;
    let mut engine =
        stt::build_engine(&SttConfig::Local { model_path: a.model, vad_model_path: a.vad, beam_size: a.beam })?;
    let t = Instant::now();
    let segs = engine.transcribe(
        &SttRequest { samples: &pcm, language: &a.language, prompt: a.prompt.as_deref(), long_form: true },
        &RunControl::none(),
    )?;
    for s in segs {
        if let Some(text) = clean_segment(&s.text, s.no_speech_prob) {
            println!("[{}] {}", miiting_core::prompts::format_timestamp(s.start_ms), text);
        }
    }
    let secs = pcm.len() as f64 / STT_SAMPLE_RATE as f64;
    eprintln!(
        "\n{:.1}s of audio in {:.1}s (RTF {:.3})",
        secs,
        t.elapsed().as_secs_f64(),
        t.elapsed().as_secs_f64() / secs
    );
    Ok(())
}

/// Interleaved f32 samples at the file's own rate and channel count.
fn read_raw(path: &Path) -> Result<(Vec<f32>, u32, u16)> {
    let mut r = hound::WavReader::open(path)?;
    let spec = r.spec();
    let samples = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().collect::<std::result::Result<Vec<_>, _>>()?,
        hound::SampleFormat::Int => {
            let scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>().map(|s| s.map(|v| v as f32 / scale)).collect::<std::result::Result<Vec<_>, _>>()?
        }
    };
    Ok((samples, spec.sample_rate, spec.channels))
}

struct PrintEvents {
    started: Instant,
    speed: f64,
    latencies: std::sync::Mutex<Vec<f64>>,
}

impl miiting_core::session::SessionEvents for PrintEvents {
    fn on_segment(&self, s: &Segment) {
        // How long after the words were spoken did they appear?
        let now_audio_ms = self.started.elapsed().as_secs_f64() * 1000.0 * self.speed;
        let latency = (now_audio_ms - s.end_ms as f64) / 1000.0 / self.speed;
        self.latencies.lock().unwrap().push(latency);
        println!(
            "  live [{}] {:<7} {}  (+{:.1}s)",
            miiting_core::prompts::format_timestamp(s.start_ms),
            s.source.speaker_label(Lang::En),
            s.text,
            latency
        );
    }
    fn on_level(&self, _: Source, _: f32) {}
    fn on_status(&self, status: miiting_core::session::SessionStatus) {
        println!("  status: {status:?}");
    }
    fn on_error(&self, message: &str) {
        println!("  ERROR: {message}");
    }
}

fn simulate(a: SimulateArgs) -> Result<()> {
    use miiting_core::engine::{FinalPassOptions, StartOptions, SummaryOptions};
    use miiting_core::llm::LlmConfig;
    use std::sync::Arc;

    let engine = Engine::open(&a.data_dir)?;
    let mic = a.mic.as_deref().map(read_raw).transpose()?;
    let sys = a.system.as_deref().map(read_raw).transpose()?;
    if mic.is_none() && sys.is_none() {
        bail!("pass --mic and/or --system");
    }
    let events = Arc::new(PrintEvents { started: Instant::now(), speed: a.speed, latencies: Default::default() });
    // Like the app: the VAD model (if installed) is attached to the live engine too.
    let live =
        a.live_model.clone().map(|m| SttConfig::Local { model_path: m, vad_model_path: a.vad.clone(), beam_size: 1 });
    let t0 = Instant::now();
    let session = engine.start_session(
        StartOptions { title: None, language: "id".into(), glossary: a.glossary.clone(), live_stt: live },
        events.clone(),
    )?;
    let id = session.meeting_id().to_string();
    println!("Meeting {id}\n── Live ({}x real time) ──", a.speed);

    // Push in device-sized buffers: system 512 frames per callback, mic 4096 frames.
    let step_frames = 512usize;
    let rate = sys.as_ref().map(|s| s.1).or(mic.as_ref().map(|m| m.1)).unwrap();
    let total_frames = [mic.as_ref().map(|m| m.0.len() / m.2 as usize), sys.as_ref().map(|s| s.0.len() / s.2 as usize)]
        .into_iter()
        .flatten()
        .max()
        .unwrap();
    let step = std::time::Duration::from_secs_f64(step_frames as f64 / rate as f64 / a.speed);
    let mut mic_pos = 0usize;
    let mut frame = 0usize;
    let started = Instant::now();
    while frame < total_frames {
        if let Some((s, r, ch)) = &sys {
            let ch = *ch as usize;
            let end = ((frame + step_frames) * ch).min(s.len());
            if frame * ch < end {
                session.push_audio(Source::System, &s[frame * ch..end], *r, ch as u32);
            }
        }
        if let Some((m, r, ch)) = &mic {
            let ch = *ch as usize;
            while (mic_pos + 4096) * ch <= ((frame + step_frames) * ch).min(m.len()) {
                session.push_audio(Source::Mic, &m[mic_pos * ch..(mic_pos + 4096) * ch], *r, ch as u32);
                mic_pos += 4096;
            }
        }
        frame += step_frames;
        let target = step * (frame / step_frames) as u32;
        if let Some(wait) = target.checked_sub(started.elapsed()) {
            std::thread::sleep(wait);
        }
    }
    if let Some((m, r, ch)) = &mic {
        let ch = *ch as usize;
        if mic_pos * ch < m.len() {
            session.push_audio(Source::Mic, &m[mic_pos * ch..], *r, ch as u32);
        }
    }
    let final_follows = a.final_model.is_some();
    engine.finish_session(&session, final_follows, true)?;
    let lat = events.latencies.lock().unwrap().clone();
    if !lat.is_empty() {
        let avg = lat.iter().sum::<f64>() / lat.len() as f64;
        let max = lat.iter().cloned().fold(0.0, f64::max);
        println!("Live latency: avg {avg:.1}s, max {max:.1}s ({} segments)", lat.len());
    }
    let reference = a.reference.as_deref().map(fs::read_to_string).transpose()?;
    let score = |label: &str| -> Result<()> {
        if let Some(r) = &reference {
            let (_, segs) = engine.transcript(&id)?;
            let hyp: Vec<&str> = segs.iter().map(|s| s.text.as_str()).collect();
            let w = wer(r, &hyp.join(" "));
            println!("WER {label}: {:.2}% ({} errors in {} words)", w.rate() * 100.0, w.errors(), w.reference_len);
        }
        Ok(())
    };
    score("live")?;

    if let Some(model) = a.final_model.clone() {
        println!("\n── Final pass ──");
        let t = Instant::now();
        let cfg = SttConfig::Local { model_path: model, vad_model_path: a.vad.clone(), beam_size: a.beam };
        let progress: miiting_core::stt::ProgressFn = Arc::new(|f: f32| eprint!("\r  {:>3.0}%", f * 100.0));
        let n = engine.run_final_pass(
            &id,
            &FinalPassOptions { stt: &cfg, language: "id", glossary: a.glossary.as_deref() },
            // Same as the app: a cancel token is always attached.
            &RunControl { progress: Some(progress), cancel: Some(Arc::new(std::sync::atomic::AtomicBool::new(false))) },
        )?;
        eprintln!();
        let audio_secs = total_frames as f64 / rate as f64;
        println!(
            "{n} segments in {:.1}s (RTF {:.3} of the meeting length)",
            t.elapsed().as_secs_f64(),
            t.elapsed().as_secs_f64() / audio_secs
        );
        for s in engine.transcript(&id)?.1 {
            println!(
                "  [{}] {:<7} {}",
                miiting_core::prompts::format_timestamp(s.start_ms),
                s.source.speaker_label(Lang::En),
                s.text
            );
        }
        score("final")?;
    }

    if let Some(model) = a.ollama_model.clone() {
        println!("\n── Notes ({model}) ──");
        let t = Instant::now();
        let md = engine.summarize(
            &id,
            SummaryOptions {
                llm: LlmConfig::OpenAiCompatible { base_url: a.ollama_url.clone(), api_key: None, model },
                template_id: &a.template,
                glossary: a.glossary.as_deref(),
                extra_instructions: None,
            },
            &|f, m| eprintln!("  {:>3.0}% {m}", f * 100.0),
        )?;
        let m = engine.meeting(&id)?;
        println!("Title: {}\n\n{md}\n\n({:.1}s)", m.title, t.elapsed().as_secs_f64());
    }
    engine.store().set_status(&id, MeetingStatus::Ready)?;
    println!("\nTotal {:.1}s · egress: {:?}", t0.elapsed().as_secs_f64(), engine.meeting(&id)?.egress);
    Ok(())
}

fn doctor(path: &Path) -> Result<()> {
    use miiting_core::device::{self, Fit, LOCAL_LLMS};
    let gb = |b: u64| b as f64 / 1_073_741_824.0;
    let fit = |f: Fit| match f {
        Fit::Light => "light",
        Fit::Good => "good",
        Fit::Heavy => "heavy",
        Fit::NotRecommended => "not recommended",
    };
    let d = device::detect(path);
    let r = device::recommend(&d, Lang::En);
    println!(
        "{} · {:.0} GB RAM · {}P+{}E cores · {:.0} GB free disk",
        d.chip,
        gb(d.ram_bytes),
        d.performance_cores,
        d.efficiency_cores,
        gb(d.free_disk_bytes)
    );
    println!("Tier: {}\n{}\n", r.tier.label(Lang::En), r.headline);
    println!("Live : {}\nFinal: {} (beam {})\nNotes: {} (Ollama)\n", r.live_model, r.final_model, r.beam_size, r.llm);
    for reason in &r.reasons {
        println!("  • {reason}");
    }
    for w in &r.warnings {
        println!("  ⚠ {w}");
    }
    println!("\nTranscription models:");
    for (m, f) in device::catalog_fit(&d) {
        println!(
            "  {:<22} {:>6.2} GB  ±{:.1} GB RAM  {}",
            m.id,
            gb(m.size_bytes),
            m.runtime_mb as f64 / 1024.0,
            fit(f)
        );
    }
    println!("\nLocal notes models (Ollama):");
    for l in LOCAL_LLMS {
        println!("  {:<12} ±{:>4.1} GB RAM  {}", l.tag, l.runtime_mb as f64 / 1024.0, fit(device::llm_fit(l, &d)));
    }
    Ok(())
}

fn seed_demo(dir: &Path) -> Result<()> {
    let engine = Engine::open(dir)?;
    let store = engine.store();
    let now = now_ms();
    let day = 86_400_000;

    let id = "demo-sprint";
    store.create_meeting(id, "Sprint Planning Q4 — Tim Produk", false, now - 2 * 3_600_000, "id")?;
    let lines: &[(Source, i64, &str)] = &[
        (Source::Mic, 2_000, "Oke, kita mulai ya. Hari ini agendanya sprint planning untuk dua minggu ke depan."),
        (Source::Mic, 9_000, "Pertama soal fitur ekspor laporan PDF yang diminta klien minggu lalu."),
        (
            Source::System,
            16_000,
            "Dari sisi backend, endpoint ekspornya sudah lima puluh persen. Tinggal template dan antrean job-nya.",
        ),
        (Source::System, 24_500, "Estimasi saya tiga hari lagi, asal desain template-nya sudah final hari Rabu."),
        (Source::Mic, 32_000, "Desain bisa final Rabu? Dimas, gimana?"),
        (Source::System, 37_000, "Bisa, saya kirim Figma-nya Selasa sore biar ada waktu review."),
        (Source::Mic, 44_000, "Sip. Berarti target rilis ekspor PDF Jumat tanggal 24, ya."),
        (Source::System, 50_500, "Satu lagi, bug login di Android versi 12 masih muncul di beberapa device Samsung."),
        (Source::Mic, 58_000, "Itu prioritas satu. Rina tolong pegang, minta log dari tim support."),
        (Source::System, 64_000, "Oke, saya cek hari ini dan update di channel jam empat."),
        (
            Source::Mic,
            71_000,
            "Yang belum jelas: apakah kita perlu dukung ekspor Excel juga. Nanti saya tanyakan ke klien.",
        ),
    ];
    let segs: Vec<Segment> = lines
        .iter()
        .map(|(s, t, x)| Segment { source: *s, start_ms: *t, end_ms: t + 6_000, text: x.to_string() })
        .collect();
    store.replace_final_segments(id, &segs, "large-v3")?;
    store.set_summary(
        id,
        "## Ringkasan\nTim menyepakati target rilis fitur ekspor laporan PDF pada Jumat, 24 Oktober, dengan desain template final hari Rabu. Bug login Android 12 di perangkat Samsung menjadi prioritas utama sprint ini.\n\n## Poin Pembahasan\n- Endpoint ekspor PDF sudah sekitar 50%; sisa pekerjaan di template dan antrean job.\n- Desain template dikirim Selasa sore agar ada waktu review.\n- Bug login Android 12 masih muncul di beberapa perangkat Samsung.\n\n## Keputusan\n- Rilis ekspor PDF ditargetkan Jumat, 24 Oktober.\n- Bug login Android 12 menjadi prioritas satu.\n\n## Action Items\n| Tugas | PIC | Tenggat |\n|---|---|---|\n| Kirim desain template PDF di Figma | Dimas | Selasa sore |\n| Selesaikan template dan antrean job ekspor | Tim backend | Jumat, 24 Okt |\n| Investigasi bug login Android 12, minta log ke support | Rina | Hari ini, 16.00 |\n| Tanyakan kebutuhan ekspor Excel ke klien | Saya | — |\n\n## Pertanyaan Terbuka\n- Apakah klien juga membutuhkan ekspor Excel?",
        Some("umum"),
        Some("localhost:gemma3:12b"),
    )?;
    store.finish_recording(id, now - 2 * 3_600_000 + 1_920_000, 1_920_000, MeetingStatus::Ready)?;

    let id2 = "demo-klien";
    store.create_meeting(id2, "Sync klien PT Maju Jaya", false, now - day - 5 * 3_600_000, "id")?;
    let segs2 = vec![
        Segment {
            source: Source::System,
            start_ms: 1_000,
            end_ms: 6_000,
            text: "Selamat siang, terima kasih sudah meluangkan waktu.".into(),
        },
        Segment {
            source: Source::Mic,
            start_ms: 7_000,
            end_ms: 13_000,
            text: "Siang, Pak Budi. Kami mau presentasikan progres integrasi pembayaran.".into(),
        },
    ];
    for s in &segs2 {
        store.insert_segment(id2, miiting_core::types::Pass::Live, s)?;
    }
    store.add_egress(id2, "api.groq.com")?;
    store.set_stt_engine(id2, "api.groq.com:whisper-large-v3")?;
    store.finish_recording(id2, now - day, 2_700_000, MeetingStatus::Ready)?;

    let id3 = "demo-standup";
    store.create_meeting(id3, "Daily standup", true, now - 9 * day, "id")?;
    store.finish_recording(id3, now - 9 * day, 600_000, MeetingStatus::Ready)?;
    println!("Demo data written to {}", dir.display());
    Ok(())
}
