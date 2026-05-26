//! Transport: write JSON snapshot to local path / WebDAV; rotation.
//!
//! 與 storage 完全無關 — 只接受 `&Path`（已寫好的 snapshot tmp 檔）+ `&Config`。

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};

use crate::config::Config;

pub fn backup_filename() -> String {
    let ts = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    format!("novel-looker-{ts}.json")
}

pub fn push_local(src: &Path, filename: &str, config: &Config) -> Result<String> {
    let path_str = config
        .backup
        .local
        .path
        .as_deref()
        .ok_or_else(|| {
            anyhow!("backup.local.path not set. Run: novel-looker config set backup.local.path <dir>")
        })?;
    let dest_dir = PathBuf::from(path_str);
    std::fs::create_dir_all(&dest_dir)
        .with_context(|| format!("create {}", dest_dir.display()))?;
    let dest = dest_dir.join(filename);
    std::fs::copy(src, &dest)
        .with_context(|| format!("copy to {}", dest.display()))?;
    prune_local(&dest_dir, config.backup.keep)?;
    Ok(dest.display().to_string())
}

pub fn prune_local(dir: &Path, keep: usize) -> Result<()> {
    if keep == 0 {
        return Ok(());
    }
    let mut entries: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("novel-looker-")
                && e.file_name().to_string_lossy().ends_with(".json")
        })
        .collect();
    entries.sort_by_key(|e| e.file_name());
    while entries.len() > keep {
        let oldest = entries.remove(0);
        let _ = std::fs::remove_file(oldest.path());
    }
    Ok(())
}

pub async fn push_webdav(src: &Path, filename: &str, config: &Config) -> Result<String> {
    let base = config
        .backup
        .webdav
        .url
        .as_deref()
        .ok_or_else(|| anyhow!("backup.webdav.url not set"))?;
    let user = config
        .backup
        .webdav
        .username
        .as_deref()
        .ok_or_else(|| anyhow!("backup.webdav.username not set"))?;
    let pass = config
        .webdav_password()
        .ok_or_else(|| anyhow!("WebDAV password not set: export NOVEL_LOOKER_WEBDAV_PASS=..."))?;

    let base = if base.ends_with('/') { base.to_string() } else { format!("{base}/") };
    let url = format!("{base}{filename}");

    let body = std::fs::read(src)?;
    let client = wreq::Client::builder()
        .emulation(wreq_util::Emulation::Chrome131)
        .build()?;
    let resp = client
        .put(&url)
        .basic_auth(user, Some(&pass))
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .await
        .with_context(|| format!("PUT {url}"))?;
    let status = resp.status();
    if !status.is_success() {
        let txt = resp.text().await.unwrap_or_default();
        anyhow::bail!(
            "WebDAV upload failed: {status} — {}",
            txt.chars().take(200).collect::<String>()
        );
    }
    // Optional: PROPFIND + DELETE for prune. Skipped for v1; user can clean manually.
    Ok(url)
}
