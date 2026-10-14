//! Hardware detection and per-device model recommendations.
//!
//! Whisper and local LLMs share the Mac's unified memory with the meeting app itself,
//! so the right model depends far more on RAM than on raw speed. The rules here are
//! deliberately simple and conservative; `miiting-cli bench` gives real numbers.

use std::path::Path;

use crate::i18n::{Lang, Text};
use crate::models::{self, CATALOG, ModelInfo, ModelKind};
use crate::tr;

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceProfile {
    /// e.g. "Apple M1 Pro".
    pub chip: String,
    pub ram_bytes: u64,
    pub performance_cores: u32,
    pub efficiency_cores: u32,
    /// Free space on the volume holding the models folder.
    pub free_disk_bytes: u64,
    pub apple_silicon: bool,
}

impl DeviceProfile {
    pub fn ram_gb(&self) -> f64 {
        self.ram_bytes as f64 / GIB
    }

    /// Base chips (M1, M2…) have far fewer GPU cores than Pro/Max/Ultra.
    pub fn chip_variant(&self) -> ChipVariant {
        let c = self.chip.to_lowercase();
        if !c.contains("apple m") {
            ChipVariant::Unknown
        } else if c.contains("ultra") {
            ChipVariant::Ultra
        } else if c.contains("max") {
            ChipVariant::Max
        } else if c.contains("pro") {
            ChipVariant::Pro
        } else {
            ChipVariant::Base
        }
    }

    pub fn tier(&self) -> Tier {
        Tier::from_ram_gb(self.ram_gb())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipVariant {
    Base,
    Pro,
    Max,
    Ultra,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// 8 GB
    Light,
    /// 16 GB
    Standard,
    /// 18–36 GB
    High,
    /// 48 GB and up
    Max,
}

impl Tier {
    pub fn from_ram_gb(gb: f64) -> Tier {
        if gb <= 8.5 {
            Tier::Light
        } else if gb <= 16.5 {
            Tier::Standard
        } else if gb <= 40.0 {
            Tier::High
        } else {
            Tier::Max
        }
    }

    pub fn label(self, lang: Lang) -> &'static str {
        match self {
            Tier::Light => Text::new("Light", "Ringan"),
            Tier::Standard => Text::new("Standard", "Standar"),
            Tier::High => Text::new("High", "Tinggi"),
            Tier::Max => Text::new("Maximum", "Maksimal"),
        }
        .get(lang)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Fit {
    /// Barely noticeable, even with a video call running.
    Light,
    Good,
    /// Works, but other apps slow down; better for the post-meeting pass.
    Heavy,
    /// Likely to swap.
    NotRecommended,
}

impl Fit {
    fn from_ratio(ratio: f64) -> Fit {
        if ratio <= 0.15 {
            Fit::Light
        } else if ratio <= 0.30 {
            Fit::Good
        } else if ratio <= 0.50 {
            Fit::Heavy
        } else {
            Fit::NotRecommended
        }
    }
}

/// How well a model fits this device's memory.
pub fn model_fit(info: &ModelInfo, device: &DeviceProfile) -> Fit {
    if info.kind == ModelKind::Vad {
        return Fit::Light;
    }
    Fit::from_ratio(info.runtime_mb as f64 / 1024.0 / device.ram_gb().max(1.0))
}

/// Ollama models suited to Indonesian meeting notes, smallest first.
#[derive(Debug, Clone)]
pub struct LocalLlm {
    pub tag: &'static str,
    pub name: &'static str,
    /// Download size of the default (Q4) build.
    pub size_bytes: u64,
    /// Memory while summarizing, including a long context window.
    pub runtime_mb: u32,
    pub note: Text,
}

pub const LOCAL_LLMS: &[LocalLlm] = &[
    LocalLlm {
        tag: "qwen3:4b",
        name: "Qwen3 4B",
        size_bytes: 2_600_000_000,
        runtime_mb: 4200,
        note: Text::new(
            "Light and fast. Good enough for short meetings.",
            "Ringan dan cepat. Notulen cukup untuk rapat singkat.",
        ),
    },
    LocalLlm {
        tag: "gemma3:4b",
        name: "Gemma 3 4B",
        size_bytes: 3_300_000_000,
        runtime_mb: 4800,
        note: Text::new(
            "The smallest model that still writes tidy Indonesian.",
            "Paling ringan yang masih rapi berbahasa Indonesia.",
        ),
    },
    LocalLlm {
        tag: "qwen3:8b",
        name: "Qwen3 8B",
        size_bytes: 5_200_000_000,
        runtime_mb: 7000,
        note: Text::new("Balanced for 16 GB Macs.", "Seimbang untuk Mac 16 GB."),
    },
    LocalLlm {
        tag: "gemma3:12b",
        name: "Gemma 3 12B",
        size_bytes: 8_100_000_000,
        runtime_mb: 10_500,
        note: Text::new(
            "Good at Indonesian. The main pick for Macs with 18 GB or more.",
            "Bagus untuk Bahasa Indonesia. Pilihan utama untuk Mac 18 GB ke atas.",
        ),
    },
    LocalLlm {
        tag: "qwen3:14b",
        name: "Qwen3 14B",
        size_bytes: 9_300_000_000,
        runtime_mb: 12_000,
        note: Text::new(
            "A 12–14B alternative, strong at extracting action items.",
            "Alternatif 12–14B, kuat untuk mengekstrak action item.",
        ),
    },
    LocalLlm {
        tag: "gemma3:27b",
        name: "Gemma 3 27B",
        size_bytes: 17_000_000_000,
        runtime_mb: 20_500,
        note: Text::new(
            "The best local notes. Slower and needs a lot of RAM.",
            "Notulen lokal paling bagus. Lebih lambat, butuh RAM besar.",
        ),
    },
    LocalLlm {
        tag: "qwen3:32b",
        name: "Qwen3 32B",
        size_bytes: 20_000_000_000,
        runtime_mb: 24_000,
        note: Text::new("For Macs with 48 GB or more.", "Untuk Mac 48 GB ke atas."),
    },
];

/// Summaries run after the meeting (Whisper is unloaded by then), so the LLM may
/// use more of the RAM than the live model can.
pub fn llm_fit(llm: &LocalLlm, device: &DeviceProfile) -> Fit {
    let ratio = llm.runtime_mb as f64 / 1024.0 / device.ram_gb().max(1.0);
    if ratio <= 0.35 {
        Fit::Good
    } else if ratio <= 0.65 {
        Fit::Heavy
    } else {
        Fit::NotRecommended
    }
}

/// Context window and per-request transcript size for local LLMs on this device.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalLlmTuning {
    pub num_ctx: u32,
    pub chunk_chars: usize,
}

pub fn local_llm_tuning(device: &DeviceProfile) -> LocalLlmTuning {
    match device.tier() {
        Tier::Light => LocalLlmTuning { num_ctx: 8192, chunk_chars: 9_000 },
        Tier::Standard => LocalLlmTuning { num_ctx: 12_288, chunk_chars: 15_000 },
        Tier::High | Tier::Max => LocalLlmTuning { num_ctx: 16_384, chunk_chars: 20_000 },
    }
}

#[derive(Debug, Clone)]
pub struct Recommendation {
    pub tier: Tier,
    pub live_model: &'static str,
    pub final_model: &'static str,
    pub beam_size: i32,
    pub llm: &'static str,
    pub headline: String,
    pub reasons: Vec<String>,
    pub warnings: Vec<String>,
}

impl Recommendation {
    /// Models to download for this setup (live, final, VAD), deduplicated.
    pub fn model_ids(&self) -> Vec<&'static str> {
        let mut ids = vec![self.live_model];
        if self.final_model != self.live_model {
            ids.push(self.final_model);
        }
        ids.push("silero-vad");
        ids
    }
}

fn size_of(id: &str) -> u64 {
    models::find(id).map(|m| m.size_bytes).unwrap_or(0)
}

pub fn recommend(device: &DeviceProfile, lang: Lang) -> Recommendation {
    let tier = device.tier();
    let ram = device.ram_gb().round() as u64;
    let mut reasons = Vec::new();
    let mut warnings = Vec::new();

    let (live, mut final_model, mut beam, llm) = match tier {
        Tier::Light => ("large-v3-turbo-q5_0", "large-v3-q5_0", 3, "gemma3:4b"),
        Tier::Standard => ("large-v3-turbo-q5_0", "large-v3", 5, "qwen3:8b"),
        Tier::High => ("large-v3-turbo-q5_0", "large-v3", 5, "gemma3:12b"),
        Tier::Max => ("large-v3", "large-v3", 5, "gemma3:27b"),
    };
    let headline = match tier {
        Tier::Light => tr!(
            lang,
            "{ram} GB RAM: Miiting uses compressed models so Zoom and Meet stay smooth.",
            "RAM {ram} GB: Miiting memakai model yang dikompres supaya Zoom dan Meet tetap lancar."
        ),
        Tier::Standard => tr!(
            lang,
            "{ram} GB RAM: enough for the most accurate model in the final transcript.",
            "RAM {ram} GB: cukup untuk model paling akurat di transkrip final."
        ),
        Tier::High => tr!(
            lang,
            "{ram} GB RAM: every transcription model runs without compromise.",
            "RAM {ram} GB: semua model transkripsi bisa dipakai tanpa kompromi."
        ),
        Tier::Max => tr!(
            lang,
            "{ram} GB RAM: the largest models for transcription and local notes.",
            "RAM {ram} GB: model terbesar untuk transkripsi dan notulen lokal."
        ),
    };

    reasons.push(match tier {
        Tier::Max => tr!(
            lang,
            "Live: Large v3 without beam search. The most accurate live draft, and one large model covers live and final.",
            "Live: Large v3 tanpa beam. Draf live paling akurat, dan cukup satu model besar untuk live dan final."
        ),
        _ => tr!(
            lang,
            "Live: Turbo Q5 uses about 1 GB of RAM, so the meeting app stays light. Q8 was no more accurate in the benchmark.",
            "Live: Turbo Q5 hanya memakai ±1 GB RAM, jadi aplikasi rapat tetap ringan. Di benchmark, Q8 tidak lebih akurat."
        ),
    });

    if device.chip_variant() == ChipVariant::Base && beam > 3 {
        beam = 3;
        let chip = &device.chip;
        reasons.push(tr!(
            lang,
            "{chip} has a smaller GPU than the Pro/Max chips, so beam search drops to 3 to keep the final transcript quick.",
            "{chip} punya GPU lebih kecil dari seri Pro/Max, jadi beam diturunkan ke 3 supaya transkrip final tidak terlalu lama."
        ));
    }

    // Leave room for macOS updates and recordings (~1 GB per hour per channel pair).
    let margin = 5 * 1024 * 1024 * 1024u64;
    let free_gb = device.free_disk_bytes as f64 / GIB;
    let needed = size_of(live) + size_of(final_model) + size_of("silero-vad");
    if device.free_disk_bytes > 0 && device.free_disk_bytes < needed + margin && final_model == "large-v3" {
        final_model = "large-v3-q5_0";
        warnings.push(tr!(
            lang,
            "Only {free_gb:.0} GB of disk left, so the final transcript uses Large v3 Q5 (1 GB) instead of Large v3 (2.9 GB).",
            "Ruang disk tinggal {free_gb:.0} GB, jadi transkrip final memakai Large v3 Q5 (1 GB) alih-alih Large v3 (2,9 GB)."
        ));
    }
    let needed = size_of(live) + size_of(final_model) + size_of("silero-vad");
    if device.free_disk_bytes > 0 && device.free_disk_bytes < needed + margin {
        warnings.push(tr!(
            lang,
            "The disk is almost full. Free up a few GB before downloading models.",
            "Ruang disk hampir penuh. Kosongkan beberapa GB sebelum mengunduh model."
        ));
    }

    reasons.push(match final_model {
        "large-v3" => tr!(
            lang,
            "Final: Large v3 with beam {beam}. The most accurate for Indonesian.",
            "Final: Large v3 dengan beam {beam}. Paling akurat untuk Bahasa Indonesia."
        ),
        _ => tr!(
            lang,
            "Final: Large v3 Q5 with beam {beam}. Nearly as accurate as Large v3 with half the RAM.",
            "Final: Large v3 Q5 dengan beam {beam}. Akurasinya hampir sama dengan Large v3, dengan RAM separuhnya."
        ),
    });

    reasons.push(match tier {
        Tier::Light => tr!(
            lang,
            "Local notes: Gemma 3 4B. For more detailed notes on 8 GB, consider a cloud provider.",
            "Notulen lokal: Gemma 3 4B. Untuk notulen yang lebih detail di RAM 8 GB, pertimbangkan penyedia cloud."
        ),
        Tier::Standard => tr!(
            lang,
            "Local notes: Qwen3 8B. Gemma 3 12B also works if other apps are closed while summarizing.",
            "Notulen lokal: Qwen3 8B. Gemma 3 12B juga bisa, asal aplikasi lain ditutup saat meringkas."
        ),
        Tier::High => tr!(
            lang,
            "Local notes: Gemma 3 12B, good at Indonesian.",
            "Notulen lokal: Gemma 3 12B, bagus untuk Bahasa Indonesia."
        ),
        Tier::Max => tr!(
            lang,
            "Local notes: Gemma 3 27B, the best quality without the cloud.",
            "Notulen lokal: Gemma 3 27B, kualitas terbaik tanpa cloud."
        ),
    });
    if tier == Tier::Light {
        warnings.push(tr!(
            lang,
            "An 8 GB Mac may feel slow while the final transcript or local notes are running. Let them finish first.",
            "Saat transkrip final atau notulen lokal berjalan, Mac 8 GB bisa terasa lambat. Sebaiknya ditunggu sampai selesai."
        ));
    }

    if !device.apple_silicon {
        warnings.push(tr!(
            lang,
            "Miiting is optimized for Apple Silicon. Local transcription is much slower on Intel Macs.",
            "Miiting dioptimalkan untuk Apple Silicon. Di Mac Intel, transkripsi lokal akan jauh lebih lambat."
        ));
    }

    Recommendation { tier, live_model: live, final_model, beam_size: beam, llm, headline, reasons, warnings }
}

/// Every catalog model with its fit for this device.
pub fn catalog_fit(device: &DeviceProfile) -> Vec<(&'static ModelInfo, Fit)> {
    CATALOG.iter().map(|m| (m, model_fit(m, device))).collect()
}

// ── Detection ──────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn sysctl_u64(name: &str) -> Option<u64> {
    let cname = std::ffi::CString::new(name).ok()?;
    let mut value: u64 = 0;
    let mut size = std::mem::size_of::<u64>();
    let rc = unsafe {
        libc::sysctlbyname(cname.as_ptr(), (&mut value as *mut u64).cast(), &mut size, std::ptr::null_mut(), 0)
    };
    if rc != 0 {
        return None;
    }
    // Some keys are 32-bit.
    Some(if size == 4 { value & 0xFFFF_FFFF } else { value })
}

#[cfg(target_os = "macos")]
fn sysctl_string(name: &str) -> Option<String> {
    let cname = std::ffi::CString::new(name).ok()?;
    let mut size = 0usize;
    unsafe {
        if libc::sysctlbyname(cname.as_ptr(), std::ptr::null_mut(), &mut size, std::ptr::null_mut(), 0) != 0 {
            return None;
        }
        let mut buf = vec![0u8; size];
        if libc::sysctlbyname(cname.as_ptr(), buf.as_mut_ptr().cast(), &mut size, std::ptr::null_mut(), 0) != 0 {
            return None;
        }
        buf.truncate(size);
        Some(String::from_utf8_lossy(&buf).trim_end_matches('\0').trim().to_string())
    }
}

fn free_disk(path: &Path) -> u64 {
    let Ok(cpath) = std::ffi::CString::new(path.to_string_lossy().as_bytes()) else { return 0 };
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(cpath.as_ptr(), &mut st) } != 0 {
        return 0;
    }
    st.f_bavail as u64 * st.f_frsize as u64
}

/// Inspect this machine. `disk_path` should be the models folder (or any path on its volume).
pub fn detect(disk_path: &Path) -> DeviceProfile {
    #[cfg(target_os = "macos")]
    {
        let ram = sysctl_u64("hw.memsize").unwrap_or(8 * 1024 * 1024 * 1024);
        let perf = sysctl_u64("hw.perflevel0.physicalcpu").unwrap_or(0) as u32;
        let eff = sysctl_u64("hw.perflevel1.physicalcpu").unwrap_or(0) as u32;
        let chip = sysctl_string("machdep.cpu.brand_string").unwrap_or_else(|| "Mac".into());
        let apple_silicon = sysctl_u64("hw.optional.arm64").unwrap_or(0) == 1;
        let cores = if perf == 0 { sysctl_u64("hw.physicalcpu").unwrap_or(0) as u32 } else { perf };
        DeviceProfile {
            chip,
            ram_bytes: ram,
            performance_cores: cores,
            efficiency_cores: eff,
            free_disk_bytes: free_disk(disk_path),
            apple_silicon,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        DeviceProfile {
            chip: std::env::consts::ARCH.to_string(),
            ram_bytes: 8 * 1024 * 1024 * 1024,
            performance_cores: std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(4),
            efficiency_cores: 0,
            free_disk_bytes: free_disk(disk_path),
            apple_silicon: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mac(chip: &str, ram_gb: u64, free_gb: u64) -> DeviceProfile {
        DeviceProfile {
            chip: chip.into(),
            ram_bytes: ram_gb * 1024 * 1024 * 1024,
            performance_cores: 4,
            efficiency_cores: 4,
            free_disk_bytes: free_gb * 1024 * 1024 * 1024,
            apple_silicon: true,
        }
    }

    #[test]
    fn tiers() {
        assert_eq!(mac("Apple M1", 8, 100).tier(), Tier::Light);
        assert_eq!(mac("Apple M2", 16, 100).tier(), Tier::Standard);
        assert_eq!(mac("Apple M3 Pro", 18, 100).tier(), Tier::High);
        assert_eq!(mac("Apple M1 Pro", 32, 100).tier(), Tier::High);
        assert_eq!(mac("Apple M2 Max", 64, 100).tier(), Tier::Max);
    }

    #[test]
    fn eight_gb_air_gets_compressed_models_and_small_beam() {
        let r = recommend(&mac("Apple M1", 8, 100), Lang::En);
        assert_eq!(r.live_model, "large-v3-turbo-q5_0");
        assert_eq!(r.final_model, "large-v3-q5_0");
        assert_eq!(r.beam_size, 3);
        assert_eq!(r.llm, "gemma3:4b");
        assert!(!r.warnings.is_empty());
    }

    #[test]
    fn m1_pro_32gb_gets_full_large_v3() {
        let r = recommend(&mac("Apple M1 Pro", 32, 136), Lang::En);
        assert_eq!(r.tier, Tier::High);
        assert_eq!(r.live_model, "large-v3-turbo-q5_0");
        assert_eq!(r.final_model, "large-v3");
        assert_eq!(r.beam_size, 5);
        assert_eq!(r.llm, "gemma3:12b");
        let max = recommend(&mac("Apple M2 Max", 64, 500), Lang::En);
        assert_eq!(max.live_model, "large-v3");
        assert_eq!(max.model_ids(), vec!["large-v3", "silero-vad"]);
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn base_chip_keeps_beam_modest() {
        let r = recommend(&mac("Apple M2", 16, 200), Lang::En);
        assert_eq!(r.final_model, "large-v3");
        assert_eq!(r.beam_size, 3);
        let pro = recommend(&mac("Apple M2 Pro", 16, 200), Lang::En);
        assert_eq!(pro.beam_size, 5);
    }

    #[test]
    fn low_disk_downgrades_final_model() {
        let r = recommend(&mac("Apple M1 Pro", 32, 7), Lang::En);
        assert_eq!(r.final_model, "large-v3-q5_0");
        assert!(r.warnings.iter().any(|w| w.contains("disk")));
    }

    #[test]
    fn fits_scale_with_ram() {
        let large = models::find("large-v3").unwrap();
        let q5 = models::find("large-v3-turbo-q5_0").unwrap();
        assert_eq!(model_fit(large, &mac("Apple M1", 8, 100)), Fit::NotRecommended);
        assert_eq!(model_fit(large, &mac("Apple M2", 16, 100)), Fit::Good);
        assert_eq!(model_fit(large, &mac("Apple M1 Pro", 32, 100)), Fit::Light);
        assert_eq!(model_fit(q5, &mac("Apple M1", 8, 100)), Fit::Light);
        let g27 = LOCAL_LLMS.iter().find(|l| l.tag == "gemma3:27b").unwrap();
        assert_eq!(llm_fit(g27, &mac("Apple M1", 8, 100)), Fit::NotRecommended);
        assert_eq!(llm_fit(g27, &mac("Apple M1 Pro", 32, 100)), Fit::Heavy);
    }

    #[test]
    fn recommended_models_exist_in_catalog() {
        for ram in [8, 16, 24, 32, 64, 128] {
            for chip in ["Apple M1", "Apple M3 Max"] {
                let r = recommend(&mac(chip, ram, 500), Lang::En);
                for id in r.model_ids() {
                    assert!(models::find(id).is_some(), "{id}");
                }
                assert!(LOCAL_LLMS.iter().any(|l| l.tag == r.llm));
            }
        }
    }

    #[test]
    fn detects_this_machine() {
        let d = detect(Path::new("/"));
        assert!(d.ram_bytes >= 4 * 1024 * 1024 * 1024);
        assert!(!d.chip.is_empty());
        assert!(d.free_disk_bytes > 0);
    }
}
