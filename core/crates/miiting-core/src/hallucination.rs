//! Filters for Whisper's well-known failure modes on Indonesian audio:
//! subtitle-credit phrases on silence, and repetition loops.

use crate::text_norm::normalize_indonesian;

/// Phrases that are essentially never said in a meeting but that Whisper emits on
/// silence or noise because they are everywhere in its YouTube-subtitle training data.
const ALWAYS_DROP: &[&str] = &[
    "terima kasih telah menonton",
    "terima kasih sudah menonton",
    "terima kasih karena telah menonton",
    "terimakasih telah menonton",
    "jangan lupa like dan subscribe",
    "jangan lupa subscribe",
    "jangan lupa like share dan subscribe",
    "sampai jumpa di video berikutnya",
    "sampai jumpa di video selanjutnya",
    "sampai bertemu di video berikutnya",
    "subtitle oleh",
    "subtitle by",
    "subtitles by",
    "subtitles by the amara org community",
    "diterjemahkan oleh",
    "terjemahan oleh",
    "disarikan oleh",
    "sub indo by",
    "teks oleh",
    "thanks for watching",
    "thank you for watching",
    "please subscribe",
    "musik",
    "music",
    "tepuk tangan",
    "applause",
];

/// Short phrases that are real speech most of the time, but hallucinations when the
/// model itself thinks the chunk was probably not speech.
const DROP_IF_UNSURE: &[&str] = &["terima kasih", "terimakasih", "ya", "oke", "baik", "hmm", "you", "thank you"];

/// Returns the cleaned text, or `None` when the segment should be discarded.
pub fn clean_segment(text: &str, no_speech_prob: f32) -> Option<String> {
    let stripped = strip_annotations(text);
    let collapsed = collapse_repetitions(&stripped);
    let trimmed = collapsed.trim();
    if trimmed.is_empty() {
        return None;
    }
    let norm = normalize_indonesian(trimmed);
    if norm.is_empty() {
        return None;
    }
    if ALWAYS_DROP.iter().any(|p| norm == *p || norm.starts_with(&format!("{p} "))) {
        return None;
    }
    if no_speech_prob > 0.45 && DROP_IF_UNSURE.contains(&norm.as_str()) {
        return None;
    }
    Some(trimmed.to_string())
}

/// Remove "[MUSIK]", "(tertawa)", "♪" and similar non-speech annotations.
fn strip_annotations(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth_sq = 0;
    let mut depth_par = 0;
    for c in text.chars() {
        match c {
            '[' => depth_sq += 1,
            ']' => depth_sq = (depth_sq - 1).max(0),
            '(' => depth_par += 1,
            ')' => depth_par = (depth_par - 1).max(0),
            '♪' | '♫' | '*' => {}
            _ if depth_sq == 0 && depth_par == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Collapse an n-gram (n = 1..=6) repeated 3+ times in a row down to one copy.
/// Whisper loops ("dan dan dan dan ...", "oke, oke, oke, oke") are common on noisy audio.
pub fn collapse_repetitions(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() < 3 {
        return words.join(" ");
    }
    let key = |w: &str| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
    let mut out: Vec<&str> = Vec::with_capacity(words.len());
    let mut i = 0;
    'outer: while i < words.len() {
        for n in (1..=6).rev() {
            if i + n * 3 > words.len() {
                continue;
            }
            let gram: Vec<String> = words[i..i + n].iter().map(|w| key(w)).collect();
            if gram.iter().all(|g| g.is_empty()) {
                continue;
            }
            let mut reps = 1;
            while i + (reps + 1) * n <= words.len()
                && words[i + reps * n..i + (reps + 1) * n].iter().map(|w| key(w)).eq(gram.iter().cloned())
            {
                reps += 1;
            }
            if reps >= 3 {
                out.extend_from_slice(&words[i..i + n]);
                i += reps * n;
                continue 'outer;
            }
        }
        out.push(words[i]);
        i += 1;
    }
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_subtitle_credits() {
        assert_eq!(clean_segment("Terima kasih telah menonton!", 0.1), None);
        assert_eq!(clean_segment("[MUSIK]", 0.1), None);
        assert_eq!(clean_segment("Subtitle oleh: Fulan", 0.1), None);
    }

    #[test]
    fn keeps_real_thanks_when_confident() {
        assert_eq!(clean_segment("Terima kasih.", 0.05).as_deref(), Some("Terima kasih."));
        assert_eq!(clean_segment("Terima kasih.", 0.8), None);
    }

    #[test]
    fn collapses_loops() {
        assert_eq!(collapse_repetitions("dan dan dan dan kita lanjut"), "dan kita lanjut");
        assert_eq!(collapse_repetitions("oke, oke, oke, oke, oke."), "oke,");
        assert_eq!(
            collapse_repetitions("saya rasa begitu saya rasa begitu saya rasa begitu ya"),
            "saya rasa begitu ya"
        );
        // Two repeats are normal speech ("iya iya").
        assert_eq!(collapse_repetitions("iya iya betul"), "iya iya betul");
    }

    #[test]
    fn strips_annotations_inside_text() {
        assert_eq!(clean_segment("Oke (tertawa) kita mulai", 0.1).as_deref(), Some("Oke kita mulai"));
    }
}
