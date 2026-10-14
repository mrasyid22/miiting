//! Localization for the text the core shows to people.
//!
//! Two different languages flow through the core:
//! - [`Lang`], the interface language: error messages, model descriptions,
//!   recommendations, default meeting titles.
//! - The meeting language (an ISO 639-1 code such as `"id"`), which drives
//!   transcription and the language the meeting notes are written in.
//!
//! Strings live next to the code that uses them, via [`Text`] for fixed strings
//! and [`tr!`](crate::tr) for formatted ones. To add a language, add a variant here
//! and the compiler will point at every string that needs a translation.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Lang {
    #[default]
    En,
    Id,
}

impl Lang {
    /// Accepts BCP 47 / ISO codes such as "id", "id-ID", "in", "en-GB".
    pub fn from_code(code: &str) -> Lang {
        let c = code.trim().to_ascii_lowercase();
        if c == "id" || c == "in" || c.starts_with("id-") || c.starts_with("id_") { Lang::Id } else { Lang::En }
    }

    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Id => "id",
        }
    }

    /// The language meeting notes are written in for a given meeting language.
    /// Regional languages (Javanese, Sundanese, Malay) get Indonesian notes.
    pub fn for_notes(meeting_language: &str) -> Lang {
        if meeting_language.eq_ignore_ascii_case("en") { Lang::En } else { Lang::Id }
    }

    /// The language's own name in English, for prompts.
    pub fn english_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Id => "Indonesian",
        }
    }
}

/// A fixed string in every supported language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Text {
    pub en: &'static str,
    pub id: &'static str,
}

impl Text {
    pub const fn new(en: &'static str, id: &'static str) -> Text {
        Text { en, id }
    }

    pub fn get(self, lang: Lang) -> &'static str {
        match lang {
            Lang::En => self.en,
            Lang::Id => self.id,
        }
    }
}

/// `tr!(lang, "English {x}", "Indonesia {x}")` formats the string for `lang`.
/// Both literals take the same arguments, inline or positional.
#[macro_export]
macro_rules! tr {
    ($lang:expr, $en:literal, $id:literal $(,)?) => {
        match $lang {
            $crate::i18n::Lang::En => format!($en),
            $crate::i18n::Lang::Id => format!($id),
        }
    };
    ($lang:expr, $en:literal, $id:literal, $($arg:tt)+) => {
        match $lang {
            $crate::i18n::Lang::En => format!($en, $($arg)+),
            $crate::i18n::Lang::Id => format!($id, $($arg)+),
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_codes() {
        assert_eq!(Lang::from_code("id-ID"), Lang::Id);
        assert_eq!(Lang::from_code("in"), Lang::Id);
        assert_eq!(Lang::from_code("en-GB"), Lang::En);
        assert_eq!(Lang::from_code("es"), Lang::En);
    }

    #[test]
    fn formats_with_inline_and_positional_args() {
        let n = 3;
        assert_eq!(tr!(Lang::En, "{n} files", "{n} berkas"), "3 files");
        assert_eq!(tr!(Lang::Id, "{} files", "{} berkas", n), "3 berkas");
    }

    #[test]
    fn notes_language() {
        assert_eq!(Lang::for_notes("en"), Lang::En);
        assert_eq!(Lang::for_notes("id"), Lang::Id);
        assert_eq!(Lang::for_notes("jw"), Lang::Id);
    }
}
