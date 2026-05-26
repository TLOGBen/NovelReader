use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub backup: BackupConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupConfig {
    /// Which backend to use: "local" or "webdav". Default: "local".
    #[serde(default = "default_backend")]
    pub backend: String,
    /// How many backup files to retain (oldest pruned). 0 = unlimited.
    #[serde(default = "default_keep")]
    pub keep: usize,
    #[serde(default)]
    pub local: LocalBackend,
    #[serde(default)]
    pub webdav: WebdavBackend,
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            backend: default_backend(),
            keep: default_keep(),
            local: LocalBackend::default(),
            webdav: WebdavBackend::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LocalBackend {
    /// Directory to write backup JSON files into. Point at a folder synced by
    /// Google Drive Desktop / Dropbox / iCloud / OneDrive for cloud backup.
    pub path: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WebdavBackend {
    /// Base URL, e.g. "https://dav.example.com/remote.php/dav/files/me/novel-looker/"
    pub url: Option<String>,
    pub username: Option<String>,
    /// Password is read from env var `NOVEL_LOOKER_WEBDAV_PASS` to keep it out
    /// of the config file. If a literal password is set here, it's used as fallback.
    pub password: Option<String>,
}

fn default_backend() -> String {
    "local".to_string()
}

fn default_keep() -> usize {
    7
}

pub fn config_path() -> Result<PathBuf> {
    let base = dirs::config_dir().context("could not resolve config dir")?;
    Ok(base.join("novel-looker").join("config.toml"))
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("read {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parse {}", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let text = toml::to_string_pretty(self)?;
        std::fs::write(&path, text)
            .with_context(|| format!("write {}", path.display()))?;
        Ok(())
    }

    /// Set a dotted key. Returns the previous value (as string) if any.
    pub fn set(&mut self, key: &str, value: &str) -> Result<Option<String>> {
        let empty = value.is_empty();
        let some_or_none = |s: &str| if empty { None } else { Some(s.to_string()) };

        let prev = match key {
            "backup.backend" => {
                if !["local", "webdav"].contains(&value) {
                    anyhow::bail!("backup.backend must be 'local' or 'webdav'");
                }
                let p = Some(self.backup.backend.clone());
                self.backup.backend = value.to_string();
                p
            }
            "backup.keep" => {
                let p = Some(self.backup.keep.to_string());
                self.backup.keep = value
                    .parse()
                    .with_context(|| "backup.keep must be an integer")?;
                p
            }
            "backup.local.path" => {
                let p = self.backup.local.path.take();
                self.backup.local.path = some_or_none(value);
                p
            }
            "backup.webdav.url" => {
                let p = self.backup.webdav.url.take();
                self.backup.webdav.url = some_or_none(value);
                p
            }
            "backup.webdav.username" => {
                let p = self.backup.webdav.username.take();
                self.backup.webdav.username = some_or_none(value);
                p
            }
            "backup.webdav.password" => {
                let p = self.backup.webdav.password.take();
                self.backup.webdav.password = some_or_none(value);
                p
            }
            _ => anyhow::bail!(
                "unknown config key: {key}\n\
                 valid keys: backup.backend, backup.keep, backup.local.path, \
                 backup.webdav.url, backup.webdav.username, backup.webdav.password"
            ),
        };
        Ok(prev)
    }

    /// Resolve effective WebDAV password (env var > config file).
    pub fn webdav_password(&self) -> Option<String> {
        std::env::var("NOVEL_LOOKER_WEBDAV_PASS")
            .ok()
            .or_else(|| self.backup.webdav.password.clone())
    }
}
