//! Local Whisper via whisper.cpp (Metal on Apple Silicon).

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Once};

use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState, WhisperVadContext,
    WhisperVadContextParams, WhisperVadParams,
};

use crate::chunker::{Chunker, ChunkerConfig};
use crate::stt::{RunControl, SttEngine, SttRequest, SttSegment};
use crate::{Error, Result, STT_SAMPLE_RATE};

const SR: usize = STT_SAMPLE_RATE as usize;

#[derive(Debug, Clone)]
pub struct WhisperOptions {
    /// 0 or 1 = greedy; >1 = beam search (more accurate, slower).
    pub beam_size: i32,
    pub threads: i32,
    pub vad_model_path: Option<PathBuf>,
    pub no_speech_threshold: f32,
}

impl Default for WhisperOptions {
    fn default() -> Self {
        let cores = std::thread::available_parallelism().map(|n| n.get() as i32).unwrap_or(4);
        WhisperOptions { beam_size: 5, threads: cores.clamp(2, 8), vad_model_path: None, no_speech_threshold: 0.6 }
    }
}

pub struct WhisperEngine {
    _ctx: WhisperContext,
    state: WhisperState,
    vad: Option<WhisperVadContext>,
    name: String,
    opts: WhisperOptions,
}

static LOG_HOOK: Once = Once::new();

impl WhisperEngine {
    pub fn load(model_path: &Path, opts: WhisperOptions) -> Result<Self> {
        if !model_path.exists() {
            return Err(Error::ModelMissing(model_path.display().to_string()));
        }
        // Route whisper.cpp's chatty stderr logging into the (silent by default) log facade.
        LOG_HOOK.call_once(whisper_rs::install_logging_hooks);

        let mut cparams = WhisperContextParameters::default();
        cparams.use_gpu(true).flash_attn(true);
        let path = model_path.to_str().ok_or_else(|| Error::msg("model path is not valid UTF-8"))?;
        let ctx = WhisperContext::new_with_params(path, cparams)?;
        let state = ctx.create_state()?;
        let vad = opts.vad_model_path.as_ref().filter(|p| p.exists()).and_then(|p| p.to_str()).and_then(|p| {
            let mut vp = WhisperVadContextParams::new();
            vp.set_n_threads(2);
            vp.set_use_gpu(false);
            WhisperVadContext::new(p, vp).ok()
        });
        let name = model_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("whisper")
            .trim_start_matches("ggml-")
            .to_string();
        Ok(WhisperEngine { _ctx: ctx, state, vad, name, opts })
    }

    /// Run whisper.cpp on one window of at most ~30 s. Timestamps are relative to the window.
    fn run(
        &mut self,
        samples: &[f32],
        req: &SttRequest,
        ctl: &RunControl,
        progress: Option<(f32, f32)>,
    ) -> Result<Vec<SttSegment>> {
        // whisper.cpp refuses (or hallucinates on) inputs under ~1 s; pad short chunks.
        let min_len = SR + SR / 10;
        let padded;
        let samples: &[f32] = if samples.len() < min_len {
            let mut v = samples.to_vec();
            v.resize(min_len, 0.0);
            padded = v;
            &padded
        } else {
            samples
        };

        let strategy = if self.opts.beam_size > 1 {
            SamplingStrategy::BeamSearch { beam_size: self.opts.beam_size, patience: -1.0 }
        } else {
            SamplingStrategy::Greedy { best_of: 1 }
        };
        let mut params = FullParams::new(strategy);
        params.set_n_threads(self.opts.threads);
        params.set_language(Some(req.language));
        params.set_translate(false);
        // Conditioning on earlier output spreads hallucinations; each window stands alone.
        params.set_no_context(true);
        params.set_suppress_blank(true);
        params.set_suppress_nst(true);
        params.set_no_speech_thold(self.opts.no_speech_threshold);
        params.set_temperature(0.0);
        params.set_temperature_inc(0.2);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_special(false);
        params.set_print_timestamps(false);
        if let Some(prompt) = req.prompt.filter(|p| !p.trim().is_empty()) {
            params.set_initial_prompt(&prompt.replace('\0', " "));
        }
        if let (Some(cb), Some((base, weight))) = (ctl.progress.clone(), progress) {
            params.set_progress_callback_safe(move |p: i32| cb(base + weight * (p as f32 / 100.0).clamp(0.0, 1.0)));
        }
        // whisper-rs 0.16's `set_abort_callback_safe` casts its boxed closure to the wrong
        // type and reads garbage, aborting runs at random. Use the raw callback with a
        // pointer to the AtomicBool instead; `ctl.cancel` keeps it alive during `full`.
        if let Some(cancel) = ctl.cancel.as_ref() {
            unsafe {
                params.set_abort_callback(Some(abort_trampoline));
                params.set_abort_callback_user_data(Arc::as_ptr(cancel) as *mut c_void);
            }
        }

        let result = self.state.full(params, samples);
        if ctl.cancel.as_ref().is_some_and(|c| c.load(Ordering::Relaxed)) {
            return Err(Error::Cancelled);
        }
        result?;

        let mut out = Vec::new();
        for seg in self.state.as_iter() {
            let text = seg.to_str_lossy()?.trim().to_string();
            if text.is_empty() {
                continue;
            }
            out.push(SttSegment {
                start_ms: seg.start_timestamp() * 10,
                end_ms: seg.end_timestamp() * 10,
                text,
                no_speech_prob: seg.no_speech_probability(),
            });
        }
        Ok(out)
    }

    /// Where the speech is, as sample ranges on the original timeline.
    fn speech_regions(&mut self, samples: &[f32]) -> Vec<(usize, usize)> {
        if let Some(vad) = self.vad.as_mut() {
            let mut vp = WhisperVadParams::new();
            vp.set_threshold(0.5);
            vp.set_min_speech_duration(250);
            vp.set_min_silence_duration(300);
            vp.set_speech_pad(150);
            if let Ok(segs) = vad.segments_from_samples(vp, samples) {
                // Timestamps are centiseconds on the input timeline.
                let regions: Vec<(usize, usize)> = segs
                    .map(|s| {
                        ((s.start.max(0.0) * 160.0) as usize, ((s.end.max(0.0) * 160.0) as usize).min(samples.len()))
                    })
                    .filter(|(a, b)| b > a)
                    .collect();
                return regions;
            }
        }
        // Fallback: the same energy VAD the live path uses.
        let mut chunker = Chunker::new(ChunkerConfig { max_chunk_ms: 28_000, ..ChunkerConfig::default() });
        let mut regions: Vec<(usize, usize)> = Vec::new();
        for c in chunker.push(samples).into_iter().chain(chunker.finish()) {
            let start = c.start_sample as usize;
            regions.push((start, (start + c.samples.len()).min(samples.len())));
        }
        regions
    }
}

unsafe extern "C" fn abort_trampoline(user_data: *mut c_void) -> bool {
    if user_data.is_null() {
        return false;
    }
    unsafe { (*(user_data as *const AtomicBool)).load(Ordering::Relaxed) }
}

/// Group speech regions into windows Whisper can decode in one pass (≤ `max_len`),
/// merging regions separated by short pauses and padding each window slightly.
pub fn pack_windows(
    regions: &[(usize, usize)],
    total: usize,
    max_len: usize,
    merge_gap: usize,
    pad: usize,
) -> Vec<(usize, usize)> {
    let mut sorted: Vec<(usize, usize)> = regions.iter().copied().filter(|(a, b)| b > a).collect();
    sorted.sort();
    let mut windows: Vec<(usize, usize)> = Vec::new();
    for (a, b) in sorted {
        let (a, b) = (a.saturating_sub(pad), (b + pad).min(total));
        // Split regions that are themselves too long.
        let mut start = a;
        while start < b {
            let end = (start + max_len).min(b);
            match windows.last_mut() {
                Some(w) if start <= w.1 + merge_gap && end - w.0 <= max_len => w.1 = w.1.max(end),
                Some(w) if start < w.1 => {
                    // Overlap from padding but too long to merge: start after the previous window.
                    let s = w.1;
                    if end > s {
                        windows.push((s, end));
                    }
                }
                _ => windows.push((start, end)),
            }
            start = end;
        }
    }
    windows
}

impl SttEngine for WhisperEngine {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn transcribe(&mut self, req: &SttRequest, ctl: &RunControl) -> Result<Vec<SttSegment>> {
        if !req.long_form {
            if self.vad.is_none() {
                return self.run(req.samples, req, ctl, Some((0.0, 1.0)));
            }
            // Live chunks were cut by an energy VAD, which also fires on keyboard noise,
            // fans, music and faint echo. Let Silero decide whether anyone is talking,
            // then transcribe only the part with speech.
            let regions = self.speech_regions(req.samples);
            let speech: usize = regions.iter().map(|(a, b)| b - a).sum();
            if speech < SR * 3 / 10 {
                return Ok(Vec::new());
            }
            let start = regions.first().map(|r| r.0).unwrap_or(0).saturating_sub(SR / 5);
            let end = (regions.last().map(|r| r.1).unwrap_or(req.samples.len()) + SR / 5).min(req.samples.len());
            let offset_ms = (start * 1000 / SR) as i64;
            let mut segs = self.run(&req.samples[start..end], req, ctl, Some((0.0, 1.0)))?;
            for s in &mut segs {
                s.start_ms += offset_ms;
                s.end_ms += offset_ms;
            }
            return Ok(segs);
        }

        // Long-form: transcribe only where there is speech, window by window, keeping
        // every timestamp on the original timeline (whisper.cpp's built-in VAD compresses it).
        let regions = self.speech_regions(req.samples);
        let windows = pack_windows(&regions, req.samples.len(), 28 * SR, SR, SR / 5);
        let total: usize = windows.iter().map(|(a, b)| b - a).sum::<usize>().max(1);
        let mut done = 0usize;
        let mut out = Vec::new();
        for (a, b) in windows {
            if ctl.cancel.as_ref().is_some_and(|c| c.load(Ordering::Relaxed)) {
                return Err(Error::Cancelled);
            }
            let weight = (b - a) as f32 / total as f32;
            let base = done as f32 / total as f32;
            let offset_ms = (a * 1000 / SR) as i64;
            let window_ms = ((b - a) * 1000 / SR) as i64;
            for mut s in self.run(&req.samples[a..b], req, ctl, Some((base, weight)))? {
                s.start_ms = offset_ms + s.start_ms.clamp(0, window_ms);
                s.end_ms = offset_ms + s.end_ms.clamp(0, window_ms);
                out.push(s);
            }
            done += b - a;
            if let Some(p) = &ctl.progress {
                p(done as f32 / total as f32);
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: usize = 16_000;

    #[test]
    fn merges_close_regions() {
        let w = pack_windows(&[(S, 3 * S), (3 * S + S / 2, 6 * S)], 100 * S, 28 * S, S, 0);
        assert_eq!(w, vec![(S, 6 * S)]);
    }

    #[test]
    fn keeps_far_regions_apart() {
        let w = pack_windows(&[(S, 3 * S), (10 * S, 12 * S)], 100 * S, 28 * S, S, 0);
        assert_eq!(w, vec![(S, 3 * S), (10 * S, 12 * S)]);
    }

    #[test]
    fn caps_window_length() {
        let regions: Vec<(usize, usize)> = (0..10).map(|i| (i * 5 * S, i * 5 * S + 4 * S)).collect();
        let w = pack_windows(&regions, 100 * S, 28 * S, S, 0);
        assert!(w.len() >= 2);
        assert!(w.iter().all(|(a, b)| b - a <= 28 * S));
        assert_eq!(w.first().unwrap().0, 0);
        assert_eq!(w.last().unwrap().1, 49 * S);
    }

    #[test]
    fn splits_long_monologue() {
        let w = pack_windows(&[(0, 70 * S)], 100 * S, 28 * S, S, 0);
        assert_eq!(w, vec![(0, 28 * S), (28 * S, 56 * S), (56 * S, 70 * S)]);
    }

    #[test]
    fn padding_is_clamped_and_windows_never_overlap() {
        let w = pack_windows(&[(0, 2 * S), (2 * S + S / 10, 30 * S)], 31 * S, 28 * S, S, S / 2);
        assert_eq!(w.first().unwrap().0, 0);
        for pair in w.windows(2) {
            assert!(pair[0].1 <= pair[1].0);
        }
        assert!(w.iter().all(|(_, b)| *b <= 31 * S));
    }
}
