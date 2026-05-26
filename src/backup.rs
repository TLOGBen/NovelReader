use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::models::{Novel, ReadProgress};
use crate::storage::Storage;

/// Portable snapshot. Chapter cache (content) is intentionally NOT included —
/// it's large and trivially regenerable via `sync`. We export only what
/// the user would lose forever: which books are on the shelf and where they
/// were reading.
#[derive(Debug, Serialize, Deserialize)]
pub struct Backup {
    pub version: u32,
    pub exported_at: String,
    pub novels: Vec<BackedUpNovel>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BackedUpNovel {
    pub source_url: String,
    pub book_url: String,
    pub name: String,
    pub author: Option<String>,
    pub intro: Option<String>,
    pub cover_url: Option<String>,
    pub toc_url: Option<String>,
    pub progress: Option<ProgressDump>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProgressDump {
    pub chapter_index: i64,
    pub scroll_offset: u16,
}

const VERSION: u32 = 1;

pub fn build_backup(store: &Storage) -> Result<Backup> {
    let novels = store.list_novels()?;
    let mut out = Vec::with_capacity(novels.len());
    for n in novels {
        let id = n.id.ok_or_else(|| anyhow!("novel has no id"))?;
        let progress = store.get_progress(id)?.map(|p| ProgressDump {
            chapter_index: p.chapter_index,
            scroll_offset: p.scroll_offset,
        });
        out.push(BackedUpNovel {
            source_url: n.source_url,
            book_url: n.book_url,
            name: n.name,
            author: n.author,
            intro: n.intro,
            cover_url: n.cover_url,
            toc_url: n.toc_url,
            progress,
        });
    }
    Ok(Backup {
        version: VERSION,
        exported_at: chrono::Utc::now().to_rfc3339(),
        novels: out,
    })
}

pub fn export_to(store: &Storage, path: &Path) -> Result<usize> {
    let b = build_backup(store)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let text = serde_json::to_string_pretty(&b)?;
    std::fs::write(path, text).with_context(|| format!("write {}", path.display()))?;
    Ok(b.novels.len())
}

pub fn import_from(store: &Storage, path: &Path) -> Result<ImportSummary> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("read {}", path.display()))?;
    let b: Backup = serde_json::from_str(&text)
        .with_context(|| format!("parse {}", path.display()))?;
    if b.version != VERSION {
        anyhow::bail!("unsupported backup version: {} (expected {VERSION})", b.version);
    }
    let mut added = 0;
    let mut with_progress = 0;
    for bn in b.novels {
        let n = Novel {
            id: None,
            source_url: bn.source_url,
            book_url: bn.book_url,
            name: bn.name,
            author: bn.author,
            intro: bn.intro,
            cover_url: bn.cover_url,
            toc_url: bn.toc_url,
        };
        let id = store.upsert_novel(&n)?;
        added += 1;
        if let Some(p) = bn.progress {
            store.save_progress(&ReadProgress {
                novel_id: id,
                chapter_index: p.chapter_index,
                scroll_offset: p.scroll_offset,
            })?;
            with_progress += 1;
        }
    }
    Ok(ImportSummary { added, with_progress })
}

#[derive(Debug)]
pub struct ImportSummary {
    pub added: usize,
    pub with_progress: usize,
}

// ---------- Backup workflow ----------

pub async fn run_backup(store: &Storage, config: &Config) -> Result<BackupReceipt> {
    let filename = backup_filename();
    let mut tmp = std::env::temp_dir();
    tmp.push(&filename);
    let count = export_to(store, &tmp)?;

    let receipt = match config.backup.backend.as_str() {
        "local" => push_local(&tmp, &filename, config)?,
        "webdav" => push_webdav(&tmp, &filename, config).await?,
        other => anyhow::bail!("unknown backup backend: {other}"),
    };

    // Best-effort cleanup of tmp file.
    let _ = std::fs::remove_file(&tmp);

    Ok(BackupReceipt {
        destination: receipt,
        novels: count,
        filename,
    })
}

#[derive(Debug)]
pub struct BackupReceipt {
    pub destination: String,
    pub novels: usize,
    pub filename: String,
}

fn backup_filename() -> String {
    let ts = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    format!("novel-looker-{ts}.json")
}

fn push_local(src: &Path, filename: &str, config: &Config) -> Result<String> {
    let path_str = config
        .backup
        .local
        .path
        .as_deref()
        .ok_or_else(|| anyhow!("backup.local.path not set. Run: novel-looker config set backup.local.path <dir>"))?;
    let dest_dir = PathBuf::from(path_str);
    std::fs::create_dir_all(&dest_dir)
        .with_context(|| format!("create {}", dest_dir.display()))?;
    let dest = dest_dir.join(filename);
    std::fs::copy(src, &dest)
        .with_context(|| format!("copy to {}", dest.display()))?;
    prune_local(&dest_dir, config.backup.keep)?;
    Ok(dest.display().to_string())
}

fn prune_local(dir: &Path, keep: usize) -> Result<()> {
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

async fn push_webdav(src: &Path, filename: &str, config: &Config) -> Result<String> {
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
        anyhow::bail!("WebDAV upload failed: {status} — {}", txt.chars().take(200).collect::<String>());
    }
    // Optional: PROPFIND + DELETE for prune. Skipped for v1; user can clean manually.
    Ok(url)
}
