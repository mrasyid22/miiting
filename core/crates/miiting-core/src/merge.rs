//! Combine per-source transcripts into one timeline.

use std::collections::HashSet;

use crate::text_norm::normalize_indonesian;
use crate::types::{Segment, Source};

/// When the user is on speakers instead of headphones, the mic re-records the other
/// participants. Those mic segments overlap a system segment in time and say the
/// same words; drop them so each sentence appears once, attributed to the System source.
pub fn is_echo(mic: &Segment, system: &Segment) -> bool {
    let tolerance = 1500;
    let overlaps = mic.start_ms <= system.end_ms + tolerance && system.start_ms <= mic.end_ms + tolerance;
    overlaps && similarity(&mic.text, &system.text) >= 0.6
}

/// Token-set overlap relative to the shorter text (0..1).
pub fn similarity(a: &str, b: &str) -> f64 {
    let na = normalize_indonesian(a);
    let nb = normalize_indonesian(b);
    let ta: HashSet<&str> = na.split_whitespace().collect();
    let tb: HashSet<&str> = nb.split_whitespace().collect();
    if ta.is_empty() || tb.is_empty() {
        return 0.0;
    }
    let inter = ta.intersection(&tb).count() as f64;
    inter / ta.len().min(tb.len()) as f64
}

pub fn merge_sources(mut segments: Vec<Segment>) -> Vec<Segment> {
    let system: Vec<Segment> = segments.iter().filter(|s| s.source == Source::System).cloned().collect();
    segments.retain(|s| s.source != Source::Mic || !system.iter().any(|sys| is_echo(s, sys)));
    segments.sort_by(|a, b| a.start_ms.cmp(&b.start_ms).then(a.end_ms.cmp(&b.end_ms)));
    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(source: Source, start: i64, end: i64, text: &str) -> Segment {
        Segment { source, start_ms: start, end_ms: end, text: text.into() }
    }

    #[test]
    fn drops_mic_echo_keeps_real_mic() {
        let segs = vec![
            s(Source::System, 1000, 4000, "Kita rilis versi baru hari Jumat ya"),
            s(Source::Mic, 1200, 4300, "kita rilis versi baru hari jumat"),
            s(Source::Mic, 5000, 7000, "Oke, saya siapkan catatan rilisnya"),
        ];
        let merged = merge_sources(segs);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].source, Source::System);
        assert_eq!(merged[1].text, "Oke, saya siapkan catatan rilisnya");
    }

    #[test]
    fn interleaves_by_time() {
        let merged = merge_sources(vec![s(Source::Mic, 3000, 4000, "dua"), s(Source::System, 1000, 2000, "satu")]);
        assert_eq!(merged[0].text, "satu");
    }
}
