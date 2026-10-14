//! Meeting-notes templates and the map-reduce summarizer.
//!
//! Instructions to the model are written in English so every contributor can read
//! and improve them; the model is told to write the notes in the meeting's language,
//! and each template has an output skeleton per notes language.

use crate::i18n::{Lang, Text};
use crate::llm::LlmClient;
use crate::types::{Segment, Source};
use crate::{Result, tr};

#[derive(Debug, Clone)]
pub struct Template {
    pub id: &'static str,
    pub name: Text,
    pub description: Text,
    /// Output skeleton in the notes language.
    format: Text,
}

pub const TEMPLATES: &[Template] = &[
    Template {
        id: "general",
        name: Text::new("General meeting", "Rapat umum"),
        description: Text::new(
            "Summary, discussion, decisions and action items",
            "Ringkasan, pembahasan, keputusan, dan action item",
        ),
        format: Text::new(
            "# <meeting title, at most 6 words>

## Summary
<2–4 sentences with the gist of the meeting>

## Discussion
- <one point per topic; merge similar ones>

## Decisions
- <decisions that were actually agreed>

## Action items
| Task | Owner | Due |
|---|---|---|
| <task> | <name or —> | <deadline as spoken, or —> |

## Open questions
- <things still unanswered or needing follow-up>",
            "# <judul rapat, maksimal 6 kata>

## Ringkasan
<2–4 kalimat inti rapat>

## Poin Pembahasan
- <satu poin per topik; gabungkan yang sejenis>

## Keputusan
- <keputusan yang benar-benar disepakati>

## Action Items
| Tugas | PIC | Tenggat |
|---|---|---|
| <tugas> | <nama atau —> | <tenggat seperti diucapkan, atau —> |

## Pertanyaan Terbuka
- <hal yang belum terjawab atau perlu ditindaklanjuti>",
        ),
    },
    Template {
        id: "standup",
        name: Text::new("Daily standup", "Daily standup"),
        description: Text::new("Progress, plans and blockers per person", "Progres, rencana, dan hambatan per orang"),
        format: Text::new(
            "# <short title, e.g. Backend Team Standup>

## Summary
<1–2 sentences on where the team stands>

## By person
### <name, or Me/Them>
- **Yesterday:** <what got done>
- **Today:** <plan>
- **Blockers:** <blocker or —>

## Blockers that need help
- <blocker, and who can help>

## Action items
| Task | Owner | Due |
|---|---|---|",
            "# <judul singkat, mis. Standup Tim Backend>

## Ringkasan
<1–2 kalimat kondisi tim>

## Per Orang
### <nama, atau Saya/Peserta>
- **Kemarin:** <yang sudah dikerjakan>
- **Hari ini:** <rencana>
- **Hambatan:** <hambatan atau —>

## Hambatan yang Perlu Dibantu
- <hambatan, siapa yang bisa bantu>

## Action Items
| Tugas | PIC | Tenggat |
|---|---|---|",
        ),
    },
    Template {
        id: "client",
        name: Text::new("Client / sales meeting", "Rapat klien / sales"),
        description: Text::new(
            "Client needs, objections, commitments and next steps",
            "Kebutuhan klien, keberatan, komitmen, dan langkah berikutnya",
        ),
        format: Text::new(
            "# <title: client name + topic>

## Summary
<2–3 sentences>

## Client needs and problems
- <needs, pain points, business context>

## Questions and objections
- <the client's objections and the answers given>

## Commitments
- **From us:** <promises we made>
- **From the client:** <promises the client made>

## Next steps
| Task | Owner | Due |
|---|---|---|

## Key details
- <numbers, budget, dates, decision makers, competitors mentioned>",
            "# <judul: nama klien + topik>

## Ringkasan
<2–3 kalimat>

## Kebutuhan & Masalah Klien
- <kebutuhan, pain point, konteks bisnis>

## Pertanyaan & Keberatan
- <keberatan klien dan jawaban yang diberikan>

## Komitmen
- **Dari kita:** <janji yang dibuat>
- **Dari klien:** <janji klien>

## Langkah Berikutnya
| Tugas | PIC | Tenggat |
|---|---|---|

## Info Penting
- <angka, anggaran, tanggal, pengambil keputusan, kompetitor yang disebut>",
        ),
    },
    Template {
        id: "one-on-one",
        name: Text::new("1-on-1", "1-on-1"),
        description: Text::new(
            "Topics, feedback, growth and follow-ups",
            "Topik, feedback, pengembangan, dan tindak lanjut",
        ),
        format: Text::new(
            "# <short title>

## Summary
<2–3 sentences>

## Topics
- <topic>

## Feedback
- <feedback given and received>

## Growth and career
- <goals, learning plans, support needed>

## Action items
| Task | Owner | Due |
|---|---|---|",
            "# <judul singkat>

## Ringkasan
<2–3 kalimat>

## Topik yang Dibahas
- <topik>

## Feedback
- <feedback yang diberikan dan diterima>

## Pengembangan & Karier
- <tujuan, rencana belajar, dukungan yang dibutuhkan>

## Action Items
| Tugas | PIC | Tenggat |
|---|---|---|",
        ),
    },
];

/// Look up a template, accepting the ids used before they were translated.
pub fn template(id: &str) -> &'static Template {
    let id = match id {
        "umum" => "general",
        "klien" => "client",
        "satu-satu" => "one-on-one",
        other => other,
    };
    TEMPLATES.iter().find(|t| t.id == id).unwrap_or(&TEMPLATES[0])
}

fn system_prompt(lang: Lang) -> String {
    let language = lang.english_name();
    let me = Source::Mic.speaker_label(lang);
    let them = Source::System.speaker_label(lang);
    let (ex1, ex2) = match lang {
        Lang::En => ("Tuesday afternoon", "4 pm today"),
        Lang::Id => ("Selasa sore", "jam 4 sore hari ini"),
    };
    format!(
        "You are a professional meeting secretary. You write meeting minutes from a transcript produced automatically by speech-to-text.

Rules:
- Write everything in {language}. Keep technical terms and English words the participants used as they are.
- Do not invent anything. Only include names, numbers, dates, deadlines and decisions that appear in the transcript. If something is not mentioned, write \"—\".
- Copy deadlines and times exactly as spoken (for example \"{ex1}\" or \"{ex2}\"). Do not turn weekdays or relative words into calendar dates, and do not add a year, unless the full date is said in the transcript.
- The transcript may contain mishearings. Fix the spelling of names or terms only when it is clear from context or from the glossary.
- \"{me}\" is the person who recorded the meeting. \"{them}\" are the other participants, possibly several people. If a participant mentions a name, use that name.
- Skip small talk, greetings and off-topic chatter.
- Output only Markdown in the requested format, with no introduction or closing remarks."
    )
}

fn notes_prompt(lang: Lang) -> String {
    format!(
        "You help a meeting secretary. From a part of a meeting transcript, write down every important point as short bullets in {}: topics, opinions, decisions, action items (with owner and deadline if mentioned), numbers, dates, names and open questions. Do not invent anything. Output only the bullet list.",
        lang.english_name()
    )
}

pub fn format_timestamp(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

/// Render segments as "[hh:mm:ss] Speaker: text" lines.
pub fn transcript_lines(segments: &[Segment], lang: Lang) -> Vec<String> {
    segments
        .iter()
        .map(|s| format!("[{}] {}: {}", format_timestamp(s.start_ms), s.source.speaker_label(lang), s.text))
        .collect()
}

pub struct SummaryRequest<'a> {
    pub template_id: &'a str,
    pub glossary: Option<&'a str>,
    pub extra_instructions: Option<&'a str>,
    /// Already formatted in `notes_lang`.
    pub meeting_date: &'a str,
    /// Language the notes are written in (from the meeting language).
    pub notes_lang: Lang,
    /// Language of progress messages.
    pub ui_lang: Lang,
}

pub struct SummaryOutput {
    pub title: Option<String>,
    pub markdown: String,
}

/// Summarize a transcript. Long transcripts are first condensed part by part
/// (map), then the notes are summarized with the template (reduce).
pub fn summarize(
    llm: &LlmClient,
    segments: &[Segment],
    req: &SummaryRequest,
    progress: &dyn Fn(f32, &str),
) -> Result<SummaryOutput> {
    let lines = transcript_lines(segments, req.notes_lang);
    let parts = pack_lines(&lines, llm.chunk_chars());
    let ui = req.ui_lang;

    let (material_label, material) = if parts.len() <= 1 {
        ("Transcript", lines.join("\n"))
    } else {
        let notes_system = notes_prompt(req.notes_lang);
        let mut notes = Vec::with_capacity(parts.len());
        for (i, part) in parts.iter().enumerate() {
            let (n, total) = (i + 1, parts.len());
            progress(
                i as f32 / (total + 1) as f32,
                &tr!(ui, "Reading part {n} of {total}", "Membaca bagian {n} dari {total}"),
            );
            let user = format!("{}Transcript part {n} of {total}:\n\n{part}", glossary_block(req.glossary));
            notes.push(format!("### Part {n}\n{}", llm.complete(&notes_system, &user)?));
        }
        ("Notes from each part of the meeting (taken from the full transcript)", notes.join("\n\n"))
    };

    progress(
        parts.len().saturating_sub(1) as f32 / (parts.len() + 1).max(1) as f32,
        &tr!(ui, "Writing the minutes", "Menyusun notulen"),
    );
    let tpl = template(req.template_id);
    let mut user = format!("Meeting date: {}\n\n{}", req.meeting_date, glossary_block(req.glossary));
    if let Some(extra) = req.extra_instructions.filter(|e| !e.trim().is_empty()) {
        user.push_str(&format!("Additional instructions from the user:\n{}\n\n", extra.trim()));
    }
    user.push_str(&format!(
        "Write the minutes in exactly this format (replace the text in <...>; drop sections that are truly empty, except the decisions and action items):\n\n{}\n\n{material_label}:\n\n{material}",
        tpl.format.get(req.notes_lang)
    ));
    let raw = llm.complete(&system_prompt(req.notes_lang), &user)?;
    progress(1.0, &tr!(ui, "Done", "Selesai"));
    Ok(split_title(&raw))
}

fn glossary_block(glossary: Option<&str>) -> String {
    match glossary.map(str::trim).filter(|g| !g.is_empty()) {
        Some(g) => format!("Glossary (correct spelling of names and terms):\n{g}\n\n"),
        None => String::new(),
    }
}

/// Greedily pack lines into parts of at most `budget` characters.
fn pack_lines(lines: &[String], budget: usize) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    for line in lines {
        if !cur.is_empty() && cur.len() + line.len() + 1 > budget {
            parts.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push('\n');
        }
        cur.push_str(line);
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    parts
}

/// Pull a leading "# Title" line out of the generated Markdown.
fn split_title(raw: &str) -> SummaryOutput {
    let trimmed = raw.trim().trim_start_matches("```markdown").trim_start_matches("```").trim_end_matches("```").trim();
    if let Some(t) = trimmed.lines().next().and_then(|first| first.strip_prefix("# ")) {
        let title = t.trim().trim_matches(['<', '>', '*']).trim().to_string();
        let rest: Vec<&str> = trimmed.lines().skip(1).collect();
        return SummaryOutput {
            title: (!title.is_empty()).then_some(title),
            markdown: rest.join("\n").trim().to_string(),
        };
    }
    SummaryOutput { title: None, markdown: trimmed.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps() {
        assert_eq!(format_timestamp(3_723_000), "01:02:03");
    }

    #[test]
    fn title_extraction() {
        let out = split_title("# Sprint Planning Q4\n\n## Summary\nOk");
        assert_eq!(out.title.as_deref(), Some("Sprint Planning Q4"));
        assert!(out.markdown.starts_with("## Summary"));
        let none = split_title("## Summary\nOk");
        assert!(none.title.is_none());
    }

    #[test]
    fn packing_respects_budget() {
        let lines: Vec<String> = (0..100).map(|i| format!("line number {i} with some text")).collect();
        let parts = pack_lines(&lines, 200);
        assert!(parts.len() > 5);
        assert!(parts.iter().all(|p| p.len() <= 200));
        assert_eq!(parts.join("\n").lines().count(), 100);
    }

    #[test]
    fn lines_use_speaker_labels_in_the_notes_language() {
        let segs = vec![Segment { source: Source::Mic, start_ms: 61_000, end_ms: 62_000, text: "Halo".into() }];
        assert_eq!(transcript_lines(&segs, Lang::Id), vec!["[00:01:01] Saya: Halo"]);
        assert_eq!(transcript_lines(&segs, Lang::En), vec!["[00:01:01] Me: Halo"]);
    }

    #[test]
    fn legacy_template_ids_still_resolve() {
        assert_eq!(template("umum").id, "general");
        assert_eq!(template("satu-satu").id, "one-on-one");
        assert_eq!(template("unknown").id, "general");
    }

    #[test]
    fn system_prompt_names_the_notes_language() {
        assert!(system_prompt(Lang::Id).contains("Write everything in Indonesian"));
        assert!(system_prompt(Lang::Id).contains("\"Saya\""));
        assert!(system_prompt(Lang::En).contains("\"Me\""));
    }
}
