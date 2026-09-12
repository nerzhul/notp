use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const SETTINGS_FILE: &str = "settings.json";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AppSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_vault_path: Option<PathBuf>,
}

impl AppSettings {
    pub fn load() -> Result<Self> {
        let path = settings_path()?;
        if !path.is_file() {
            return Ok(Self::default());
        }
        let raw = fs::read(&path).context("Unable to read settings")?;
        let settings: Self = serde_json::from_slice(&raw).context("Settings are corrupted")?;
        Ok(settings)
    }

    pub fn save(&self) -> Result<()> {
        let path = settings_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("Unable to create the settings directory")?;
        }
        let raw = serde_json::to_vec_pretty(self).context("Unable to encode settings")?;
        atomic_write(&path, &raw).context("Unable to write settings")?;
        Ok(())
    }
}

fn settings_path() -> Result<PathBuf> {
    let dir = settings_dir().context("Unable to determine the settings directory")?;
    Ok(dir.join("notp").join(SETTINGS_FILE))
}

fn settings_dir() -> Result<PathBuf> {
    dirs::state_dir()
        .or_else(dirs::config_dir)
        .context("Unable to determine the settings directory")
}

fn atomic_write(path: &std::path::Path, content: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid settings path")
    })?;
    let temporary = parent.join(format!(
        ".settings-{}-{}.tmp",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(content)?;
        file.sync_all()?;
    }
    let result = fs::rename(&temporary, path);
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_tempdir<T>(f: impl FnOnce(&std::path::Path) -> T) -> T {
        let dir = tempfile::tempdir().unwrap();
        f(dir.path())
    }

    #[test]
    fn loads_defaults_when_missing() {
        with_tempdir(|dir| {
            std::env::set_var("XDG_STATE_HOME", dir);
            let settings = AppSettings::load().unwrap();
            assert!(settings.last_vault_path.is_none());
        });
    }

    #[test]
    fn round_trips_vault_path() {
        with_tempdir(|dir| {
            std::env::set_var("XDG_STATE_HOME", dir);
            let mut settings = AppSettings::default();
            let path = PathBuf::from("/tmp/some/vault.notp");
            settings.last_vault_path = Some(path.clone());
            settings.save().unwrap();
            let loaded = AppSettings::load().unwrap();
            assert_eq!(loaded.last_vault_path, Some(path));
        });
    }
}
