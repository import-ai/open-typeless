use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    io::Write,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

pub const MAX_HOTWORDS_BYTES: usize = 1000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub text: String,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct File {
    version: u32,
    entries: Vec<Entry>,
}

pub fn hotwords(entries: &[Entry]) -> String {
    entries
        .iter()
        .map(|entry| entry.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn normalized(text: &str) -> Result<String, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("词汇不能为空".into());
    }
    if text
        .chars()
        .any(|c| c.is_control() || c == '\u{2028}' || c == '\u{2029}')
    {
        return Err("请填写单个词汇，不要包含换行或控制字符".into());
    }
    Ok(text.into())
}

fn validate(entries: &[Entry]) -> Result<(), String> {
    let mut words = HashSet::new();
    let mut ids = HashSet::new();
    for entry in entries {
        if normalized(&entry.text)? != entry.text {
            return Err("词汇含有首尾空白".into());
        }
        if !words.insert(entry.text.to_ascii_lowercase()) {
            return Err("该词汇已存在".into());
        }
        if entry.id.is_empty() || !ids.insert(&entry.id) {
            return Err("词条 ID 无效".into());
        }
    }
    if hotwords(entries).len() > MAX_HOTWORDS_BYTES {
        return Err("词典容量已满，请缩短词汇或删除不再使用的词条".into());
    }
    Ok(())
}

pub fn load(path: &Path) -> Result<Vec<Entry>, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("读取词典失败: {error}")),
    };
    let file: File = serde_json::from_slice(&bytes).map_err(|e| format!("读取词典失败: {e}"))?;
    if file.version != 1 {
        return Err("无法读取此版本的词典，请更新应用".into());
    }
    validate(&file.entries).map_err(|e| format!("词典文件无效: {e}"))?;
    Ok(file.entries)
}

fn save(path: &Path, entries: Vec<Entry>) -> Result<Vec<Entry>, String> {
    validate(&entries)?;
    let file = File {
        version: 1,
        entries,
    };
    let write = || -> Result<(), Box<dyn std::error::Error>> {
        let parent = path.parent().ok_or("词典文件路径无效")?;
        fs::create_dir_all(parent)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(&serde_json::to_vec_pretty(&file)?)?;
        temporary.as_file().sync_all()?;
        temporary.persist(path)?;
        Ok(())
    };
    write().map_err(|e| format!("保存词典失败: {e}"))?;
    Ok(file.entries)
}

pub fn upsert(path: &Path, id: Option<&str>, text: &str) -> Result<Vec<Entry>, String> {
    let mut entries = load(path)?;
    let text = normalized(text)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?;
    let timestamp = now.as_millis() as u64;
    if let Some(id) = id {
        let entry = entries
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or("词条不存在，请重新加载词典")?;
        entry.text = text;
        entry.updated_at = timestamp;
    } else {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        entries.insert(
            0,
            Entry {
                id: format!(
                    "{}-{}-{}",
                    now.as_nanos(),
                    std::process::id(),
                    NEXT_ID.fetch_add(1, Ordering::Relaxed)
                ),
                text,
                created_at: timestamp,
                updated_at: timestamp,
            },
        );
    }
    save(path, entries)
}

pub fn delete(path: &Path, ids: &[String]) -> Result<Vec<Entry>, String> {
    let mut entries = load(path)?;
    entries.retain(|entry| !ids.contains(&entry.id));
    save(path, entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_add_edit_delete_and_keeps_order_and_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config/dictionary.json");
        assert!(load(&path).unwrap().is_empty());
        upsert(&path, None, "  OAuth  ").unwrap();
        let entries = upsert(&path, None, "语音").unwrap();
        let snapshot = hotwords(&entries);
        let id = &entries[1].id;
        let updated = upsert(&path, Some(id), "OAUTH").unwrap();
        assert_eq!(updated[1].created_at, entries[1].created_at);
        assert_eq!(updated[1].id, *id);
        assert_eq!(load(&path).unwrap(), updated);
        assert_eq!(snapshot, "语音\nOAuth");
        assert_eq!(hotwords(&updated), "语音\nOAUTH");
        delete(&path, &[id.clone()]).unwrap();
        assert_eq!(load(&path).unwrap().len(), 1);
        delete(&path, &[entries[0].id.clone()]).unwrap();
        assert!(load(&path).unwrap().is_empty());
    }

    #[test]
    fn rejects_duplicates_invalid_words_and_utf8_overflow_without_changing_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("dictionary.json");
        upsert(&path, None, "OAuth").unwrap();
        let before = fs::read(&path).unwrap();
        for text in ["oauth", " OAUTH ", " ", "foo\nbar", "a\tb"] {
            assert!(upsert(&path, None, text).is_err());
            assert_eq!(fs::read(&path).unwrap(), before);
        }
        // 5 bytes + separator + 993 UTF-8 bytes = 999, then one separator + 1 = 1001.
        upsert(&path, None, &"词".repeat(331)).unwrap();
        assert!(upsert(&path, None, "a").unwrap_err().contains("容量已满"));
        let entries = load(&path).unwrap();
        assert!(upsert(&path, Some(&entries[0].id), "oauth").is_err());
        assert!(upsert(&path, Some("missing"), "term").is_err());
    }

    #[test]
    fn accepts_exact_capacity_and_preserves_unreadable_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("dictionary.json");
        upsert(&path, None, &"a".repeat(1000)).unwrap();
        assert_eq!(hotwords(&load(&path).unwrap()).len(), 1000);
        fs::write(&path, b"{broken").unwrap();
        assert!(upsert(&path, None, "OAuth").is_err());
        assert!(delete(&path, &[]).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"{broken");
        assert!(upsert(&path.join("child.json"), None, "OAuth").is_err());
    }
}
