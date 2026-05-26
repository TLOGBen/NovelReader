//! Snapshot serialization + ACL (BackedUpNovel ↔ Novel mapping).
//!
//! Conformist of Library: 所有 storage 操作走 [`crate::library::facade`]，
//! 禁 import `rusqlite` 與 `crate::library::dao::*`。
//! `LibraryDb` 型別本身只作為 facade 呼叫參數的型別容器出現。

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::library::facade;
use crate::library::facade::LibraryDbHandle as LibraryDb;
use crate::library::{Novel, ReadProgress};

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

#[derive(Debug)]
pub struct ImportSummary {
    pub added: usize,
    pub with_progress: usize,
}

const VERSION: u32 = 1;

pub fn build_backup(db: &LibraryDb) -> Result<Backup> {
    let novels = facade::list_shelf(db)?;
    let mut out = Vec::with_capacity(novels.len());
    for n in novels {
        let id = n.id.ok_or_else(|| anyhow!("novel has no id"))?;
        let progress = facade::get_progress(db, id)?.map(|p| ProgressDump {
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

pub fn export_to(db: &LibraryDb, path: &Path) -> Result<usize> {
    let b = build_backup(db)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let text = serde_json::to_string_pretty(&b)?;
    std::fs::write(path, text).with_context(|| format!("write {}", path.display()))?;
    Ok(b.novels.len())
}

pub fn import_from(db: &mut LibraryDb, path: &Path) -> Result<ImportSummary> {
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
        let id = facade::add_novel(db, &n)?;
        added += 1;
        if let Some(p) = bn.progress {
            facade::save_progress(
                db,
                &ReadProgress {
                    novel_id: id,
                    chapter_index: p.chapter_index,
                    scroll_offset: p.scroll_offset,
                },
            )?;
            with_progress += 1;
        }
    }
    Ok(ImportSummary { added, with_progress })
}
