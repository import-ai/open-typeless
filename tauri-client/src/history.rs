use chrono::Local;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone)]
pub struct Stamp {
    pub id: String,
    pub started_at_ms: i64,
    pub local_date: String,
    pub utc_offset_minutes: i32,
}

impl Stamp {
    pub fn new(run_id: u64) -> Self {
        let now = Local::now();
        Self {
            id: format!(
                "{}-{}-{run_id}",
                now.timestamp_nanos_opt().unwrap_or(now.timestamp_millis()),
                std::process::id()
            ),
            started_at_ms: now.timestamp_millis(),
            local_date: now.format("%Y-%m-%d").to_string(),
            utc_offset_minutes: now.offset().local_minus_utc() / 60,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Entry {
    pub id: String,
    pub started_at_ms: i64,
    pub local_date: String,
    pub utc_offset_minutes: i32,
    pub raw_text: String,
    pub polished_text: Option<String>,
    pub character_count: i64,
    pub audio_duration_ms: i64,
    pub deleting: bool,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Cursor {
    pub local_date: String,
    pub started_at_ms: i64,
    pub id: String,
}

#[derive(Serialize)]
pub struct Page {
    pub entries: Vec<Entry>,
    pub next_cursor: Option<Cursor>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Usage {
    pub local_date: String,
    pub character_count: i64,
    pub audio_duration_ms: i64,
    pub recognition_count: i64,
}

#[derive(Serialize)]
pub struct Insights {
    pub character_count: i64,
    pub audio_duration_ms: i64,
    pub days: Vec<Usage>,
}

pub struct Store {
    db: Connection,
    recordings: PathBuf,
}

type Result<T> = std::result::Result<T, String>;

impl Store {
    pub fn open(directory: &Path) -> Result<Self> {
        fs::create_dir_all(directory).map_err(|e| e.to_string())?;
        let mut db =
            Connection::open(directory.join("history.sqlite3")).map_err(|e| e.to_string())?;
        db.busy_timeout(Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        let version: i32 = db
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|e| e.to_string())?;
        if version > 1 {
            return Err("无法读取此版本的历史记录，请更新应用".into());
        }
        if version == 0 {
            let tx = db.transaction().map_err(|e| e.to_string())?;
            tx.execute_batch("CREATE TABLE history_entries (
                id TEXT PRIMARY KEY NOT NULL,
                started_at_ms INTEGER NOT NULL,
                local_date TEXT NOT NULL,
                utc_offset_minutes INTEGER NOT NULL,
                raw_text TEXT NOT NULL,
                polished_text TEXT,
                character_count INTEGER NOT NULL CHECK(character_count >= 0),
                audio_duration_ms INTEGER NOT NULL CHECK(audio_duration_ms > 0),
                deleting INTEGER NOT NULL DEFAULT 0 CHECK(deleting IN (0, 1))
            );
            CREATE INDEX history_order ON history_entries(local_date DESC, started_at_ms DESC, id DESC);
            CREATE TABLE daily_usage (
                local_date TEXT PRIMARY KEY NOT NULL,
                character_count INTEGER NOT NULL CHECK(character_count >= 0),
                audio_duration_ms INTEGER NOT NULL CHECK(audio_duration_ms > 0),
                recognition_count INTEGER NOT NULL CHECK(recognition_count > 0)
            );
            PRAGMA user_version = 1;").map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
        }
        let recordings = directory.join("recordings");
        fs::create_dir_all(&recordings).map_err(|e| e.to_string())?;
        let mut store = Self { db, recordings };
        let pending = store
            .db
            .prepare("SELECT id FROM history_entries WHERE deleting = 1")
            .and_then(|mut stmt| {
                stmt.query_map([], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(|e| e.to_string())?;
        // Failed removals stay visible for a manual retry; never infer deletions from unreferenced files.
        for id in pending {
            let _ = store.delete(&id);
        }
        Ok(store)
    }

    fn path(&self, id: &str) -> Result<PathBuf> {
        if id.is_empty()
            || id.len() > 100
            || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err("无效的历史记录 ID".into());
        }
        Ok(self.recordings.join(format!("{id}.wav")))
    }

    pub fn save(
        &mut self,
        stamp: &Stamp,
        source: &Path,
        raw: &str,
        polished: Option<&str>,
    ) -> Result<()> {
        self.save_source(stamp, || fs::File::open(source), raw, polished)
    }

    pub fn save_bytes(&mut self, stamp: &Stamp, data: &[u8], raw: &str, polished: Option<&str>) -> Result<()> {
        self.save_source(stamp, || Ok(std::io::Cursor::new(data)), raw, polished)
    }

    fn save_source<R: std::io::Read + std::io::Seek>(
        &mut self, stamp: &Stamp, source: impl FnOnce() -> std::io::Result<R>, raw: &str, polished: Option<&str>,
    ) -> Result<()> {
        if raw.trim().is_empty() {
            return Err("未识别到文字".into());
        }
        if self.entry(&stamp.id)?.is_some() {
            return Ok(());
        }
        let mut source = source().map_err(|e| e.to_string())?;
        let audio = hound::WavReader::new(&mut source).map_err(|e| e.to_string())?;
        if audio.spec().sample_rate == 0 {
            return Err("录音采样率无效".into());
        }
        let duration = i64::from(audio.duration()) * 1000 / i64::from(audio.spec().sample_rate);
        if duration <= 0 {
            return Err("录音时长无效".into());
        }
        drop(audio);
        source.rewind().map_err(|e| e.to_string())?;
        let count = raw.chars().filter(|c| c.is_alphanumeric()).count() as i64;
        let path = self.path(&stamp.id)?;
        let mut staged =
            tempfile::NamedTempFile::new_in(&self.recordings).map_err(|e| e.to_string())?;
        std::io::copy(&mut source, &mut staged)
        .map_err(|e| e.to_string())?;
        staged.as_file().sync_all().map_err(|e| e.to_string())?;
        staged.persist_noclobber(&path).map_err(|e| e.to_string())?;
        let write = (|| -> rusqlite::Result<()> {
            let tx = self.db.transaction()?;
            tx.execute("INSERT INTO history_entries (id, started_at_ms, local_date, utc_offset_minutes, raw_text, polished_text, character_count, audio_duration_ms)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![stamp.id, stamp.started_at_ms, stamp.local_date, stamp.utc_offset_minutes, raw, polished, count, duration])?;
            tx.execute(
                "INSERT INTO daily_usage VALUES (?1, ?2, ?3, 1)
                ON CONFLICT(local_date) DO UPDATE SET
                character_count = character_count + excluded.character_count,
                audio_duration_ms = audio_duration_ms + excluded.audio_duration_ms,
                recognition_count = recognition_count + 1",
                params![stamp.local_date, count, duration],
            )?;
            tx.commit()
        })();
        if let Err(error) = write {
            let cleanup = fs::remove_file(path);
            return Err(match cleanup {
                Ok(()) => error.to_string(),
                Err(e) => format!("{error}; 清理录音失败: {e}"),
            });
        }
        Ok(())
    }

    fn row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Entry> {
        Ok(Entry {
            id: row.get(0)?,
            started_at_ms: row.get(1)?,
            local_date: row.get(2)?,
            utc_offset_minutes: row.get(3)?,
            raw_text: row.get(4)?,
            polished_text: row.get(5)?,
            character_count: row.get(6)?,
            audio_duration_ms: row.get(7)?,
            deleting: row.get(8)?,
        })
    }

    pub fn entry(&self, id: &str) -> Result<Option<Entry>> {
        self.db
            .query_row(
                "SELECT * FROM history_entries WHERE id = ?1",
                [id],
                Self::row,
            )
            .optional()
            .map_err(|e| e.to_string())
    }

    pub fn page(&self, cursor: Option<Cursor>) -> Result<Page> {
        let cursor = cursor.unwrap_or(Cursor {
            local_date: "9999-12-31".into(),
            started_at_ms: i64::MAX,
            id: "~".into(),
        });
        let mut stmt = self
            .db
            .prepare(
                "SELECT * FROM history_entries
            WHERE (local_date, started_at_ms, id) < (?1, ?2, ?3)
            ORDER BY local_date DESC, started_at_ms DESC, id DESC LIMIT 51",
            )
            .map_err(|e| e.to_string())?;
        let mut entries = stmt
            .query_map(
                params![cursor.local_date, cursor.started_at_ms, cursor.id],
                Self::row,
            )
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|e| e.to_string())?;
        let more = entries.len() > 50;
        entries.truncate(50);
        let next_cursor = entries.last().filter(|_| more).map(|e| Cursor {
            local_date: e.local_date.clone(),
            started_at_ms: e.started_at_ms,
            id: e.id.clone(),
        });
        Ok(Page {
            entries,
            next_cursor,
        })
    }

    pub fn insights(&self) -> Result<Insights> {
        let mut stmt = self.db.prepare("SELECT local_date, character_count, audio_duration_ms, recognition_count FROM daily_usage ORDER BY local_date").map_err(|e| e.to_string())?;
        let days = stmt
            .query_map([], |r| {
                Ok(Usage {
                    local_date: r.get(0)?,
                    character_count: r.get(1)?,
                    audio_duration_ms: r.get(2)?,
                    recognition_count: r.get(3)?,
                })
            })
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|e| e.to_string())?;
        Ok(Insights {
            character_count: days.iter().map(|d| d.character_count).sum(),
            audio_duration_ms: days.iter().map(|d| d.audio_duration_ms).sum(),
            days,
        })
    }

    pub fn recording(&self, id: &str) -> Result<PathBuf> {
        let entry = self.entry(id)?.ok_or("历史记录不存在")?;
        if entry.deleting {
            return Err("记录正在删除，请重试删除操作".into());
        }
        let path = self.path(id)?;
        if !fs::symlink_metadata(&path)
            .map_err(|_| "录音文件不存在或无法访问")?
            .file_type()
            .is_file()
        {
            return Err("无效的录音文件".into());
        }
        Ok(path)
    }

    pub fn delete(&mut self, id: &str) -> Result<()> {
        let path = self.path(id)?;
        let changed = self
            .db
            .execute(
                "UPDATE history_entries SET deleting = 1 WHERE id = ?1",
                [id],
            )
            .map_err(|e| e.to_string())?;
        if changed == 0 {
            return Ok(());
        }
        match fs::remove_file(path) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(format!("录音删除失败，请重试: {e}")),
        }
        self.db
            .execute("DELETE FROM history_entries WHERE id = ?1", [id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(path: &Path) {
        let mut writer = hound::WavWriter::create(
            path,
            hound::WavSpec {
                channels: 2,
                sample_rate: 8000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..16000 {
            writer.write_sample(100i16).unwrap();
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn archives_counts_once_and_deletes_audio_without_decrementing_usage() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.wav");
        wav(&source);
        let mut store = Store::open(dir.path()).unwrap();
        let stamp = Stamp::new(1);
        store
            .save(&stamp, &source, "你好，OpenAI 2026！🙂", Some("Polished"))
            .unwrap();
        store.save(&stamp, &source, "duplicate", None).unwrap();
        let entries = store.page(None).unwrap().entries;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].character_count, 12);
        assert_eq!(entries[0].audio_duration_ms, 1000);
        assert_eq!(entries[0].polished_text.as_deref(), Some("Polished"));
        let audio = store.recording(&stamp.id).unwrap();
        let usage = store.insights().unwrap().days;
        store.delete(&stamp.id).unwrap();
        assert!(!audio.exists());
        assert!(store.page(None).unwrap().entries.is_empty());
        assert_eq!(store.insights().unwrap().days, usage);
        drop(store);
        assert_eq!(
            Store::open(dir.path()).unwrap().insights().unwrap().days,
            usage
        );
    }

    #[test]
    fn pagination_and_pending_deletions_survive_restart() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.wav");
        wav(&source);
        let mut store = Store::open(dir.path()).unwrap();
        for id in 0..51 {
            store.save(&Stamp::new(id), &source, "word", None).unwrap();
        }
        let first = store.page(None).unwrap();
        assert_eq!(first.entries.len(), 50);
        let second = store.page(first.next_cursor).unwrap();
        assert_eq!(second.entries.len(), 1);
        assert!(second.next_cursor.is_none());
        let id = &first.entries[0].id;
        let path = store.recording(id).unwrap();
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(store.delete(id).is_err());
        assert!(store.entry(id).unwrap().unwrap().deleting);
        fs::remove_dir(&path).unwrap();
        drop(store);
        let store = Store::open(dir.path()).unwrap();
        assert!(store.entry(id).unwrap().is_none());
        assert_eq!(
            store
                .insights()
                .unwrap()
                .days
                .iter()
                .map(|d| d.recognition_count)
                .sum::<i64>(),
            51
        );
    }

    #[test]
    fn failures_preserve_existing_data_and_roll_back_both_tables() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.wav");
        wav(&source);
        let mut store = Store::open(dir.path()).unwrap();
        let stamp = Stamp::new(1);
        assert!(store.save(&stamp, &source, "  ", None).is_err());
        assert!(store.delete("../outside").is_err());
        store.db.execute_batch("CREATE TRIGGER reject_usage BEFORE INSERT ON daily_usage BEGIN SELECT RAISE(ABORT, 'test failure'); END;").unwrap();
        assert!(store.save(&stamp, &source, "hello", None).is_err());
        assert!(store.page(None).unwrap().entries.is_empty());
        assert!(store.insights().unwrap().days.is_empty());
        assert!(!store.path(&stamp.id).unwrap().exists());
        // A failed history transaction must not erase the recoverable capture.
        crate::cleanup_recording(&source, true).unwrap();
        assert!(source.exists());
        let data = fs::read(&source).unwrap();
        store.db.execute_batch("DROP TRIGGER reject_usage").unwrap();
        store.save_bytes(&stamp, &data, "hello", None).unwrap();
        assert_eq!(fs::read(store.recording(&stamp.id).unwrap()).unwrap(), data);
        assert_eq!(store.insights().unwrap().character_count, 5);
        crate::cleanup_recording(&source, false).unwrap();
        assert!(!source.exists());
        drop(store);
        let broken = tempfile::tempdir().unwrap();
        let path = broken.path().join("history.sqlite3");
        fs::write(&path, b"broken database").unwrap();
        assert!(Store::open(broken.path()).is_err());
        assert_eq!(fs::read(path).unwrap(), b"broken database");
    }
}
