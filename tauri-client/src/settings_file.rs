use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::Path};

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SavedSettings {
    pub shortcut: String,
    pub server_url: String,
    pub api_key: String,
    pub tray_visible: bool,
    pub dock_visible: bool,
}

impl Default for SavedSettings {
    fn default() -> Self {
        Self {
            shortcut: crate::default_shortcut_name().into(),
            server_url: String::new(),
            api_key: String::new(),
            tray_visible: true,
            dock_visible: true,
        }
    }
}

pub fn load(path: &Path) -> Result<SavedSettings, String> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| format!("读取设置失败: {e}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(SavedSettings::default()),
        Err(error) => Err(format!("读取设置失败: {error}")),
    }
}

pub fn save(path: &Path, settings: &SavedSettings) -> Result<(), String> {
    let save = || -> Result<(), Box<dyn std::error::Error>> {
        let parent = path.parent().ok_or("设置文件路径无效")?;
        fs::create_dir_all(parent)?;
        // Replace the complete file atomically on macOS and Windows. A failed
        // write must not truncate the previously saved preferences.
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(&serde_json::to_vec_pretty(settings)?)?;
        temporary.as_file().sync_all()?;
        temporary.persist(path)?;
        Ok(())
    };
    save().map_err(|error| format!("保存设置失败: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_survive_reload_and_replacement_including_clearing_url() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config/settings.json");
        assert_eq!(load(&path).unwrap(), SavedSettings::default());
        let mut settings = SavedSettings {
            shortcut: "Control+Shift+K".into(),
            server_url: "http://localhost:8080/api/v1".into(),
            api_key: "test-key".into(),
            tray_visible: true,
            dock_visible: false,
        };
        save(&path, &settings).unwrap();
        assert_eq!(load(&path).unwrap(), settings);
        settings.shortcut = "RControl".into();
        save(&path, &settings).unwrap();
        assert_eq!(load(&path).unwrap(), settings);
        settings.server_url.clear();
        settings.api_key.clear();
        save(&path, &settings).unwrap();
        assert_eq!(load(&path).unwrap(), settings);
    }

    #[test]
    fn malformed_file_is_reported_without_overwriting_it() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        fs::write(&path, b"{broken").unwrap();
        assert!(load(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"{broken");
    }

    #[test]
    fn missing_fields_keep_defaults_and_write_failures_are_reported() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        fs::write(&path, r#"{"server_url":"https://example.com/api/v1"}"#).unwrap();
        assert_eq!(
            load(&path).unwrap().shortcut,
            crate::default_shortcut_name()
        );
        assert!(load(&path).unwrap().api_key.is_empty());
        assert!(save(&path.join("child.json"), &SavedSettings::default()).is_err());
        assert_eq!(
            load(&path).unwrap().server_url,
            "https://example.com/api/v1"
        );
    }
}
