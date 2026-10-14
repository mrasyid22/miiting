//! Indonesian text normalization for fair WER scoring and hallucination matching.
//!
//! Reference transcripts and model output disagree on surface form far more often
//! than on content ("2024" vs "dua ribu dua puluh empat", "anak-anak" vs "anak anak",
//! "Rp" vs "rupiah"). Both sides go through the same function before scoring.

const UNITS: [&str; 10] = ["nol", "satu", "dua", "tiga", "empat", "lima", "enam", "tujuh", "delapan", "sembilan"];

/// Spell out a non-negative integer in Indonesian.
pub fn number_to_words(n: u64) -> String {
    if n < 10 {
        return UNITS[n as usize].to_string();
    }
    let mut parts: Vec<String> = Vec::new();
    spell(n, &mut parts);
    parts.join(" ")
}

fn spell(n: u64, out: &mut Vec<String>) {
    const SCALES: [(u64, &str); 4] =
        [(1_000_000_000_000, "triliun"), (1_000_000_000, "miliar"), (1_000_000, "juta"), (1_000, "ribu")];
    let mut n = n;
    for (value, name) in SCALES {
        if n >= value {
            let q = n / value;
            if q == 1 && value == 1_000 {
                out.push("seribu".into());
            } else {
                spell_below_thousand(q, out);
                out.push(name.into());
            }
            n %= value;
        }
    }
    if n > 0 {
        spell_below_thousand(n, out);
    }
}

fn spell_below_thousand(n: u64, out: &mut Vec<String>) {
    let mut n = n;
    if n >= 100 {
        let h = n / 100;
        if h == 1 {
            out.push("seratus".into());
        } else {
            out.push(UNITS[h as usize].into());
            out.push("ratus".into());
        }
        n %= 100;
    }
    if n >= 20 {
        out.push(UNITS[(n / 10) as usize].into());
        out.push("puluh".into());
        n %= 10;
        if n > 0 {
            out.push(UNITS[n as usize].into());
        }
    } else if n >= 12 {
        out.push(UNITS[(n - 10) as usize].into());
        out.push("belas".into());
    } else if n == 11 {
        out.push("sebelas".into());
    } else if n == 10 {
        out.push("sepuluh".into());
    } else if n > 0 {
        out.push(UNITS[n as usize].into());
    }
}

/// Lowercase, spell out numbers, expand a few symbols, strip punctuation.
pub fn normalize_indonesian(text: &str) -> String {
    let lower = text.to_lowercase();
    let mut out = String::with_capacity(lower.len() + 16);
    let chars: Vec<char> = lower.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_digit() {
            // Collect "1.250.000", "3,5", "12".
            let start = i;
            while i < chars.len()
                && (chars[i].is_ascii_digit()
                    || ((chars[i] == '.' || chars[i] == ',') && i + 1 < chars.len() && chars[i + 1].is_ascii_digit()))
            {
                i += 1;
            }
            let raw: String = chars[start..i].iter().collect();
            out.push(' ');
            out.push_str(&spell_number_token(&raw));
            out.push(' ');
            continue;
        }
        match c {
            '%' => out.push_str(" persen "),
            '&' => out.push_str(" dan "),
            '+' => out.push_str(" plus "),
            '@' => out.push_str(" at "),
            c if c.is_alphanumeric() => out.push(c),
            '\'' | '’' => {} // drop apostrophes inside words ("ga'", "di'")
            _ => out.push(' '),
        }
        i += 1;
    }
    let words: Vec<&str> = out.split_whitespace().collect();
    let mut result: Vec<String> = Vec::with_capacity(words.len());
    for w in words {
        result.push(match w {
            "rp" => "rupiah".into(),
            "dll" => "dan lain lain".into(),
            "dsb" => "dan sebagainya".into(),
            "yg" => "yang".into(),
            "tdk" => "tidak".into(),
            "utk" => "untuk".into(),
            "dgn" => "dengan".into(),
            other => other.into(),
        });
    }
    result.join(" ")
}

fn spell_number_token(raw: &str) -> String {
    // Indonesian uses '.' for thousands and ',' for decimals. A '.' followed by exactly
    // three digits is a thousands separator; otherwise treat it as a decimal point.
    let groups: Vec<&str> = raw.split('.').collect();
    let thousands = groups.len() > 1 && groups[1..].iter().all(|g| g.len() == 3 && !g.contains(','));
    let (int_part, frac_part) = if thousands {
        let joined: String = groups.concat();
        match joined.split_once(',') {
            Some((a, b)) => (a.to_string(), Some(b.to_string())),
            None => (joined, None),
        }
    } else if let Some((a, b)) = raw.split_once(',').or_else(|| raw.split_once('.')) {
        (a.to_string(), Some(b.to_string()))
    } else {
        (raw.to_string(), None)
    };

    let mut s = match int_part.parse::<u64>() {
        Ok(n) => number_to_words(n),
        Err(_) => int_part.chars().map(digit_word).collect::<Vec<_>>().join(" "),
    };
    if let Some(frac) = frac_part.filter(|f| !f.is_empty()) {
        s.push_str(" koma ");
        s.push_str(&frac.chars().filter(|c| c.is_ascii_digit()).map(digit_word).collect::<Vec<_>>().join(" "));
    }
    s
}

fn digit_word(c: char) -> &'static str {
    UNITS[c.to_digit(10).unwrap_or(0) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(number_to_words(0), "nol");
        assert_eq!(number_to_words(11), "sebelas");
        assert_eq!(number_to_words(15), "lima belas");
        assert_eq!(number_to_words(21), "dua puluh satu");
        assert_eq!(number_to_words(100), "seratus");
        assert_eq!(number_to_words(1000), "seribu");
        assert_eq!(number_to_words(2024), "dua ribu dua puluh empat");
        assert_eq!(number_to_words(1_250_000), "satu juta dua ratus lima puluh ribu");
        assert_eq!(number_to_words(3_000_000_000), "tiga miliar");
    }

    #[test]
    fn normalization() {
        assert_eq!(normalize_indonesian("Rapat tahun 2024, oke?"), "rapat tahun dua ribu dua puluh empat oke");
        assert_eq!(normalize_indonesian("Anak-anak bermain."), "anak anak bermain");
        assert_eq!(normalize_indonesian("Naik 15%"), "naik lima belas persen");
        assert_eq!(normalize_indonesian("Rp 1.500.000"), "rupiah satu juta lima ratus ribu");
        assert_eq!(normalize_indonesian("3,5 jam"), "tiga koma lima jam");
        assert_eq!(normalize_indonesian("  Halo   DUNIA  "), "halo dunia");
    }
}
