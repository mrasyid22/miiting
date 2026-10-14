//! Word and character error rates.

use serde::Serialize;

use crate::text_norm::normalize_indonesian;

#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq)]
pub struct ErrorCounts {
    pub substitutions: usize,
    pub deletions: usize,
    pub insertions: usize,
    /// Reference length (words or characters).
    pub reference_len: usize,
}

impl ErrorCounts {
    pub fn errors(&self) -> usize {
        self.substitutions + self.deletions + self.insertions
    }

    pub fn rate(&self) -> f64 {
        if self.reference_len == 0 {
            return if self.insertions > 0 { 1.0 } else { 0.0 };
        }
        self.errors() as f64 / self.reference_len as f64
    }

    pub fn add(&mut self, o: &ErrorCounts) {
        self.substitutions += o.substitutions;
        self.deletions += o.deletions;
        self.insertions += o.insertions;
        self.reference_len += o.reference_len;
    }
}

/// Levenshtein alignment over arbitrary tokens, returning S/D/I counts.
pub fn align<T: PartialEq>(reference: &[T], hypothesis: &[T]) -> ErrorCounts {
    let (n, m) = (reference.len(), hypothesis.len());
    // Each cell: (cost, subs, dels, ins)
    let mut prev: Vec<(usize, usize, usize, usize)> = (0..=m).map(|j| (j, 0, 0, j)).collect();
    let mut cur = vec![(0, 0, 0, 0); m + 1];
    for i in 1..=n {
        cur[0] = (i, 0, i, 0);
        for j in 1..=m {
            let (dc, ds, dd, di) = prev[j - 1];
            let diag = if reference[i - 1] == hypothesis[j - 1] { (dc, ds, dd, di) } else { (dc + 1, ds + 1, dd, di) };
            let (uc, us, ud, ui) = prev[j];
            let del = (uc + 1, us, ud + 1, ui);
            let (lc, ls, ld, li) = cur[j - 1];
            let ins = (lc + 1, ls, ld, li + 1);
            cur[j] = [diag, del, ins].into_iter().min_by_key(|c| c.0).unwrap();
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    let (_, s, d, ins) = prev[m];
    ErrorCounts { substitutions: s, deletions: d, insertions: ins, reference_len: n }
}

/// WER after Indonesian normalization.
pub fn wer(reference: &str, hypothesis: &str) -> ErrorCounts {
    let r = normalize_indonesian(reference);
    let h = normalize_indonesian(hypothesis);
    let rw: Vec<&str> = r.split_whitespace().collect();
    let hw: Vec<&str> = h.split_whitespace().collect();
    align(&rw, &hw)
}

/// CER after normalization, ignoring spaces.
pub fn cer(reference: &str, hypothesis: &str) -> ErrorCounts {
    let r: Vec<char> = normalize_indonesian(reference).chars().filter(|c| !c.is_whitespace()).collect();
    let h: Vec<char> = normalize_indonesian(hypothesis).chars().filter(|c| !c.is_whitespace()).collect();
    align(&r, &h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_after_normalization() {
        assert_eq!(wer("Tahun 2024.", "tahun dua ribu dua puluh empat").rate(), 0.0);
    }

    #[test]
    fn counts_each_error_kind() {
        let c = wer("saya pergi ke pasar hari ini", "saya pergi pasar besar hari itu");
        // "ke" deleted, "besar" inserted, "ini"->"itu" substituted (or an equivalent-cost alignment).
        assert_eq!(c.errors(), 3);
        assert_eq!(c.reference_len, 6);
        assert!((c.rate() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn empty_cases() {
        assert_eq!(wer("", "").rate(), 0.0);
        assert_eq!(wer("halo", "").deletions, 1);
        assert_eq!(wer("", "halo").rate(), 1.0);
    }

    #[test]
    fn cer_ignores_spaces() {
        assert_eq!(cer("anak anak", "anakanak").rate(), 0.0);
    }
}
