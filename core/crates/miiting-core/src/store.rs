//! SQLite storage for meetings, transcript segments, and full-text search.

use std::path::Path;

use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, params};

use crate::types::{Meeting, MeetingStatus, Pass, Segment, Source};
use crate::{Error, Result};

pub struct Store {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS meetings (
    id               TEXT PRIMARY KEY,
    title            TEXT NOT NULL,
    title_auto       INTEGER NOT NULL DEFAULT 1,
    created_at       INTEGER NOT NULL,
    ended_at         INTEGER,
    duration_ms      INTEGER NOT NULL DEFAULT 0,
    status           TEXT NOT NULL,
    summary_md       TEXT,
    summary_template TEXT,
    summary_model    TEXT,
    transcript_pass  TEXT NOT NULL DEFAULT 'live',
    stt_engine       TEXT,
    egress           TEXT NOT NULL DEFAULT '[]',
    language         TEXT NOT NULL DEFAULT 'id'
);

CREATE TABLE IF NOT EXISTS segments (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    pass       TEXT NOT NULL,
    source     TEXT NOT NULL,
    start_ms   INTEGER NOT NULL,
    end_ms     INTEGER NOT NULL,
    text       TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS segments_by_meeting ON segments(meeting_id, pass, start_ms);

CREATE VIRTUAL TABLE IF NOT EXISTS segments_fts USING fts5(
    text, content='segments', content_rowid='id', tokenize='unicode61 remove_diacritics 2'
);
CREATE TRIGGER IF NOT EXISTS segments_ai AFTER INSERT ON segments BEGIN
    INSERT INTO segments_fts(rowid, text) VALUES (new.id, new.text);
END;
CREATE TRIGGER IF NOT EXISTS segments_ad AFTER DELETE ON segments BEGIN
    INSERT INTO segments_fts(segments_fts, rowid, text) VALUES ('delete', old.id, old.text);
END;
CREATE TRIGGER IF NOT EXISTS segments_au AFTER UPDATE ON segments BEGIN
    INSERT INTO segments_fts(segments_fts, rowid, text) VALUES ('delete', old.id, old.text);
    INSERT INTO segments_fts(rowid, text) VALUES (new.id, new.text);
END;

CREATE TABLE IF NOT EXISTS kv (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

const MEETING_COLS: &str = "id, title, created_at, ended_at, duration_ms, status, summary_md, summary_template, \
     summary_model, transcript_pass, stt_engine, egress, language";

fn row_to_meeting(r: &rusqlite::Row) -> rusqlite::Result<Meeting> {
    let egress: String = r.get(11)?;
    Ok(Meeting {
        id: r.get(0)?,
        title: r.get(1)?,
        created_at_ms: r.get(2)?,
        ended_at_ms: r.get(3)?,
        duration_ms: r.get(4)?,
        status: MeetingStatus::parse(&r.get::<_, String>(5)?),
        summary_md: r.get(6)?,
        summary_template: r.get(7)?,
        summary_model: r.get(8)?,
        transcript_pass: Pass::parse(&r.get::<_, String>(9)?),
        stt_engine: r.get(10)?,
        egress: serde_json::from_str(&egress).unwrap_or_default(),
        language: r.get(12)?,
    })
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Store { conn: Mutex::new(conn) })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Store { conn: Mutex::new(conn) })
    }

    pub fn create_meeting(
        &self,
        id: &str,
        title: &str,
        title_auto: bool,
        created_at_ms: i64,
        language: &str,
    ) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO meetings (id, title, title_auto, created_at, status, language) VALUES (?1, ?2, ?3, ?4, 'recording', ?5)",
            params![id, title, title_auto as i64, created_at_ms, language],
        )?;
        Ok(())
    }

    pub fn meeting(&self, id: &str) -> Result<Meeting> {
        self.conn
            .lock()
            .query_row(&format!("SELECT {MEETING_COLS} FROM meetings WHERE id = ?1"), [id], row_to_meeting)
            .optional()?
            .ok_or_else(|| Error::NotFound(id.to_string()))
    }

    /// Meetings newest first. A non-empty query matches title, summary, and transcript text.
    pub fn list_meetings(&self, query: Option<&str>) -> Result<Vec<Meeting>> {
        let conn = self.conn.lock();
        let q = query.map(str::trim).filter(|q| !q.is_empty());
        let mut out = Vec::new();
        match q {
            None => {
                let mut st = conn.prepare(&format!("SELECT {MEETING_COLS} FROM meetings ORDER BY created_at DESC"))?;
                for m in st.query_map([], row_to_meeting)? {
                    out.push(m?);
                }
            }
            Some(q) => {
                let like = format!("%{}%", q.replace('%', "\\%").replace('_', "\\_"));
                let fts = fts_query(q);
                let base = format!(
                    "SELECT {MEETING_COLS} FROM meetings WHERE title LIKE ?1 ESCAPE '\\' OR summary_md LIKE ?1 ESCAPE '\\'"
                );
                let rows: Vec<Meeting> = if fts.is_empty() {
                    let mut st = conn.prepare(&format!("{base} ORDER BY created_at DESC"))?;
                    st.query_map(params![like], row_to_meeting)?.collect::<rusqlite::Result<_>>()?
                } else {
                    let mut st = conn.prepare(&format!(
                        "{base} OR id IN (SELECT s.meeting_id FROM segments s JOIN segments_fts f ON f.rowid = s.id \
                         WHERE segments_fts MATCH ?2) ORDER BY created_at DESC"
                    ))?;
                    st.query_map(params![like, fts], row_to_meeting)?.collect::<rusqlite::Result<_>>()?
                };
                out.extend(rows);
            }
        }
        Ok(out)
    }

    pub fn rename(&self, id: &str, title: &str) -> Result<()> {
        self.conn.lock().execute("UPDATE meetings SET title = ?2, title_auto = 0 WHERE id = ?1", params![id, title])?;
        Ok(())
    }

    /// Set a model-suggested title, but only if the user hasn't named the meeting.
    pub fn set_auto_title(&self, id: &str, title: &str) -> Result<()> {
        self.conn
            .lock()
            .execute("UPDATE meetings SET title = ?2 WHERE id = ?1 AND title_auto = 1", params![id, title])?;
        Ok(())
    }

    pub fn set_status(&self, id: &str, status: MeetingStatus) -> Result<()> {
        self.conn.lock().execute("UPDATE meetings SET status = ?2 WHERE id = ?1", params![id, status.as_str()])?;
        Ok(())
    }

    pub fn finish_recording(&self, id: &str, ended_at_ms: i64, duration_ms: i64, status: MeetingStatus) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE meetings SET ended_at = ?2, duration_ms = ?3, status = ?4 WHERE id = ?1",
            params![id, ended_at_ms, duration_ms, status.as_str()],
        )?;
        Ok(())
    }

    pub fn set_stt_engine(&self, id: &str, engine: &str) -> Result<()> {
        self.conn.lock().execute("UPDATE meetings SET stt_engine = ?2 WHERE id = ?1", params![id, engine])?;
        Ok(())
    }

    pub fn add_egress(&self, id: &str, host: &str) -> Result<()> {
        let mut m = self.meeting(id)?;
        if !m.egress.iter().any(|h| h == host) {
            m.egress.push(host.to_string());
            self.conn.lock().execute(
                "UPDATE meetings SET egress = ?2 WHERE id = ?1",
                params![id, serde_json::to_string(&m.egress)?],
            )?;
        }
        Ok(())
    }

    pub fn set_summary(&self, id: &str, markdown: &str, template: Option<&str>, model: Option<&str>) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE meetings SET summary_md = ?2, summary_template = COALESCE(?3, summary_template), \
             summary_model = COALESCE(?4, summary_model) WHERE id = ?1",
            params![id, markdown, template, model],
        )?;
        Ok(())
    }

    pub fn delete_meeting(&self, id: &str) -> Result<()> {
        self.conn.lock().execute("DELETE FROM meetings WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn insert_segment(&self, meeting_id: &str, pass: Pass, seg: &Segment) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO segments (meeting_id, pass, source, start_ms, end_ms, text) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![meeting_id, pass.as_str(), seg.source.as_str(), seg.start_ms, seg.end_ms, seg.text],
        )?;
        Ok(())
    }

    /// Atomically replace the final transcript and mark it as the one to show.
    pub fn replace_final_segments(&self, meeting_id: &str, segs: &[Segment], engine: &str) -> Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM segments WHERE meeting_id = ?1 AND pass = 'final'", [meeting_id])?;
        {
            let mut st = tx.prepare(
                "INSERT INTO segments (meeting_id, pass, source, start_ms, end_ms, text) VALUES (?1, 'final', ?2, ?3, ?4, ?5)",
            )?;
            for s in segs {
                st.execute(params![meeting_id, s.source.as_str(), s.start_ms, s.end_ms, s.text])?;
            }
        }
        tx.execute(
            "UPDATE meetings SET transcript_pass = 'final', stt_engine = ?2 WHERE id = ?1",
            params![meeting_id, engine],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn segments(&self, meeting_id: &str, pass: Pass) -> Result<Vec<Segment>> {
        let conn = self.conn.lock();
        let mut st = conn.prepare(
            "SELECT source, start_ms, end_ms, text FROM segments WHERE meeting_id = ?1 AND pass = ?2 ORDER BY start_ms, id",
        )?;
        let rows = st.query_map(params![meeting_id, pass.as_str()], |r| {
            Ok(Segment {
                source: Source::parse(&r.get::<_, String>(0)?),
                start_ms: r.get(1)?,
                end_ms: r.get(2)?,
                text: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The transcript to display: final pass if it exists, otherwise the live draft.
    pub fn best_segments(&self, meeting_id: &str) -> Result<(Pass, Vec<Segment>)> {
        let m = self.meeting(meeting_id)?;
        if m.transcript_pass == Pass::Final {
            return Ok((Pass::Final, self.segments(meeting_id, Pass::Final)?));
        }
        Ok((Pass::Live, self.segments(meeting_id, Pass::Live)?))
    }

    /// Meetings left in "recording" or "processing" by a crash or force-quit.
    pub fn interrupted_meetings(&self) -> Result<Vec<Meeting>> {
        let conn = self.conn.lock();
        let mut st = conn.prepare(&format!(
            "SELECT {MEETING_COLS} FROM meetings WHERE status IN ('recording', 'processing') ORDER BY created_at"
        ))?;
        Ok(st.query_map([], row_to_meeting)?.collect::<rusqlite::Result<_>>()?)
    }

    pub fn kv_get(&self, key: &str) -> Result<Option<String>> {
        Ok(self.conn.lock().query_row("SELECT value FROM kv WHERE key = ?1", [key], |r| r.get(0)).optional()?)
    }

    pub fn kv_set(&self, key: &str, value: &str) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO kv (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}

/// Turn free text into a safe FTS5 query: every term quoted and prefix-matched.
fn fts_query(q: &str) -> String {
    q.split_whitespace()
        .map(|t| t.chars().filter(|c| c.is_alphanumeric()).collect::<String>())
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{t}\"*"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(source: Source, start: i64, text: &str) -> Segment {
        Segment { source, start_ms: start, end_ms: start + 1000, text: text.into() }
    }

    #[test]
    fn lifecycle_and_search() {
        let s = Store::open_in_memory().unwrap();
        s.create_meeting("m1", "Rapat 14 Okt", true, 1000, "id").unwrap();
        s.create_meeting("m2", "Sync klien", false, 2000, "id").unwrap();
        s.insert_segment("m1", Pass::Live, &seg(Source::Mic, 0, "Kita bahas anggaran pemasaran")).unwrap();
        s.insert_segment("m2", Pass::Live, &seg(Source::System, 0, "Deploy minggu depan")).unwrap();

        let all = s.list_meetings(None).unwrap();
        assert_eq!(all.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), vec!["m2", "m1"]);

        // Transcript text, prefix match, case-insensitive.
        let hits = s.list_meetings(Some("ANGGAR")).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "m1");
        // Title match.
        assert_eq!(s.list_meetings(Some("klien")).unwrap()[0].id, "m2");
        // Odd characters don't break FTS syntax.
        assert!(s.list_meetings(Some("\"(*")).unwrap().is_empty());

        s.set_auto_title("m1", "Anggaran Pemasaran").unwrap();
        s.set_auto_title("m2", "Should not apply").unwrap();
        assert_eq!(s.meeting("m1").unwrap().title, "Anggaran Pemasaran");
        assert_eq!(s.meeting("m2").unwrap().title, "Sync klien");
    }

    #[test]
    fn final_pass_replaces_display() {
        let s = Store::open_in_memory().unwrap();
        s.create_meeting("m", "t", true, 0, "id").unwrap();
        s.insert_segment("m", Pass::Live, &seg(Source::Mic, 0, "draf")).unwrap();
        assert_eq!(s.best_segments("m").unwrap().0, Pass::Live);
        s.replace_final_segments("m", &[seg(Source::Mic, 0, "final"), seg(Source::System, 500, "dua")], "large-v3")
            .unwrap();
        let (pass, segs) = s.best_segments("m").unwrap();
        assert_eq!(pass, Pass::Final);
        assert_eq!(segs.len(), 2);
        assert_eq!(s.meeting("m").unwrap().stt_engine.as_deref(), Some("large-v3"));
        // Re-running replaces rather than appends.
        s.replace_final_segments("m", &[seg(Source::Mic, 0, "lagi")], "x").unwrap();
        assert_eq!(s.segments("m", Pass::Final).unwrap().len(), 1);
    }

    #[test]
    fn egress_and_delete_cascade() {
        let s = Store::open_in_memory().unwrap();
        s.create_meeting("m", "t", true, 0, "id").unwrap();
        s.add_egress("m", "api.groq.com").unwrap();
        s.add_egress("m", "api.groq.com").unwrap();
        assert_eq!(s.meeting("m").unwrap().egress, vec!["api.groq.com"]);
        s.insert_segment("m", Pass::Live, &seg(Source::Mic, 0, "x")).unwrap();
        s.delete_meeting("m").unwrap();
        assert!(s.segments("m", Pass::Live).unwrap().is_empty());
    }
}
