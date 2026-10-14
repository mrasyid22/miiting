//! Downloadable model catalog and downloader.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::i18n::Text;
use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelKind {
    Whisper,
    Vad,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelRole {
    /// Fast enough for live transcription on Apple Silicon.
    Live,
    /// Best accuracy, for the post-meeting pass.
    Final,
    Both,
    Support,
}

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub description: Text,
    pub file_name: &'static str,
    pub url: &'static str,
    pub size_bytes: u64,
    /// Approximate unified memory used while transcribing (weights + Metal buffers + beam KV).
    pub runtime_mb: u32,
    /// 1–5 for Indonesian, calibrated against the FLEURS benchmark (docs/benchmarks.md).
    pub accuracy: u8,
    pub speed: u8,
    pub kind: ModelKind,
    pub role: ModelRole,
    pub license: &'static str,
}

macro_rules! whisper {
    ($id:literal, $name:literal, $en:literal, $id_text:literal, $size:literal, $mb:literal, $acc:literal, $spd:literal, $role:expr) => {
        ModelInfo {
            id: $id,
            name: $name,
            description: Text::new($en, $id_text),
            file_name: concat!("ggml-", $id, ".bin"),
            url: concat!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-", $id, ".bin"),
            size_bytes: $size,
            runtime_mb: $mb,
            accuracy: $acc,
            speed: $spd,
            kind: ModelKind::Whisper,
            role: $role,
            license: "MIT",
        }
    };
}

pub const CATALOG: &[ModelInfo] = &[
    whisper!(
        "large-v3",
        "Whisper Large v3",
        "Most accurate for Indonesian (5.5% WER on FLEURS with beam 5). The best choice for the final transcript.",
        "Paling akurat untuk Bahasa Indonesia (WER FLEURS 5,5% dengan beam 5). Pilihan terbaik untuk transkrip final.",
        3_095_033_483,
        4100,
        5,
        3,
        ModelRole::Final
    ),
    whisper!(
        "large-v3-q5_0",
        "Whisper Large v3 (Q5)",
        "Nearly as accurate as Large v3 at a third of the size. Final transcript on 8–16 GB Macs.",
        "Hampir seakurat Large v3 dengan sepertiga ukuran. Transkrip final untuk Mac 8–16 GB.",
        1_081_140_203,
        1900,
        4,
        3,
        ModelRole::Final
    ),
    whisper!(
        "large-v2-q5_0",
        "Whisper Large v2 (Q5)",
        "Previous generation. Sometimes hallucinates less on noisy audio; compare with the benchmark.",
        "Generasi sebelumnya. Kadang lebih jarang berhalusinasi di audio bising. Bandingkan lewat benchmark.",
        1_080_732_091,
        1900,
        4,
        3,
        ModelRole::Final
    ),
    whisper!(
        "large-v3-turbo",
        "Whisper Large v3 Turbo",
        "Full-precision Turbo. A more accurate live draft, or a fast final transcript.",
        "Turbo presisi penuh. Draf live yang lebih akurat, atau transkrip final yang cepat.",
        1_624_555_275,
        2100,
        4,
        4,
        ModelRole::Both
    ),
    whisper!(
        "large-v3-turbo-q8_0",
        "Whisper Large v3 Turbo (Q8)",
        "As accurate as Turbo Q5 on FLEURS, so usually not worth the extra size.",
        "Di benchmark FLEURS akurasinya setara Turbo Q5, jadi biasanya tidak sepadan dengan ukurannya.",
        874_188_075,
        1400,
        4,
        5,
        ModelRole::Live
    ),
    whisper!(
        "large-v3-turbo-q5_0",
        "Whisper Large v3 Turbo (Q5)",
        "Default for live transcription (7.1% WER on FLEURS, 0.08 RTF on an M1 Pro). Light even on 8 GB Macs.",
        "Default untuk transkrip live (WER FLEURS 7,1%, RTF 0,08 di M1 Pro). Ringan bahkan di Mac 8 GB.",
        574_041_195,
        1100,
        4,
        5,
        ModelRole::Live
    ),
    whisper!(
        "small",
        "Whisper Small",
        "Very small, but much less accurate for Indonesian. Only for tight disk space.",
        "Sangat kecil, tapi akurasi Bahasa Indonesianya jauh lebih rendah. Hanya untuk disk yang sempit.",
        487_601_967,
        900,
        2,
        5,
        ModelRole::Live
    ),
    ModelInfo {
        id: "silero-vad",
        name: "Silero VAD v5",
        description: Text::new(
            "Voice activity detection. Skips silence and noise, which prevents hallucinated text.",
            "Deteksi suara. Melewati hening dan bising sehingga mencegah teks halusinasi.",
        ),
        file_name: "ggml-silero-v5.1.2.bin",
        url: "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v5.1.2.bin",
        size_bytes: 885_098,
        runtime_mb: 20,
        accuracy: 0,
        speed: 0,
        kind: ModelKind::Vad,
        role: ModelRole::Support,
        license: "MIT",
    },
];

pub fn find(id: &str) -> Option<&'static ModelInfo> {
    CATALOG.iter().find(|m| m.id == id)
}

pub fn path_for(models_dir: &Path, info: &ModelInfo) -> PathBuf {
    models_dir.join(info.file_name)
}

pub fn is_installed(models_dir: &Path, info: &ModelInfo) -> bool {
    fs::metadata(path_for(models_dir, info)).map(|m| m.len() == info.size_bytes).unwrap_or(false)
}

pub type DownloadProgress = Arc<dyn Fn(u64, u64) + Send + Sync>;

/// Download into `<file>.part`, then rename. A retry starts over (no resume), but a
/// truncated file never ends up under the final name.
pub fn download(
    models_dir: &Path,
    info: &ModelInfo,
    progress: DownloadProgress,
    cancel: Arc<AtomicBool>,
) -> Result<PathBuf> {
    fs::create_dir_all(models_dir)?;
    let dest = path_for(models_dir, info);
    let part = dest.with_extension("bin.part");
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .timeout(None)
        .user_agent("Miiting/0.1")
        .build()?;
    let mut resp = client.get(info.url).send()?;
    if !resp.status().is_success() {
        return Err(Error::Download { name: info.name.into(), detail: format!("HTTP {}", resp.status().as_u16()) });
    }
    let total = resp.content_length().unwrap_or(info.size_bytes);
    let mut file = File::create(&part)?;
    let mut buf = vec![0u8; 1 << 20];
    let mut done: u64 = 0;
    let mut last_report = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            drop(file);
            let _ = fs::remove_file(&part);
            return Err(Error::Cancelled);
        }
        let n = resp.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        done += n as u64;
        if last_report.elapsed() >= Duration::from_millis(150) {
            progress(done, total);
            last_report = Instant::now();
        }
    }
    file.flush()?;
    drop(file);
    if done != info.size_bytes {
        let _ = fs::remove_file(&part);
        return Err(Error::Download {
            name: info.name.into(),
            detail: format!("incomplete ({done} of {} bytes)", info.size_bytes),
        });
    }
    fs::rename(&part, &dest)?;
    progress(done, total);
    Ok(dest)
}

pub fn delete(models_dir: &Path, info: &ModelInfo) -> Result<()> {
    let p = path_for(models_dir, info);
    if p.exists() {
        fs::remove_file(p)?;
    }
    Ok(())
}
