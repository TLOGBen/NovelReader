//! Catalog DAO. NOTE: Shared Kernel — sources 表 column + chapters.{idx,name,url}
//! 由 Catalog 寫；chapters.content 由 Library DAO 寫。所有方法借用 LibraryDb 的 Connection。
//!
//! Layering: this module is the sole `rusqlite` import point for the Catalog
//! context. Service / facade layers depend on this DAO via `&LibraryDb` /
//! `&mut LibraryDb` and never touch `rusqlite` types directly.
//!
//! Borrow rules (per design.md):
//! - SELECT (唯讀)        → `&LibraryDb`
//! - INSERT/UPDATE (寫入) → `&mut LibraryDb`
//! - Transaction          → `&mut LibraryDb`

use anyhow::Result;
use rusqlite::{params, OptionalExtension};

use crate::catalog::BookSource;
use crate::library::ChapterMeta;
use crate::library::dao::LibraryDb;

/// Upsert a book source row (sources table). Shared Kernel write.
pub fn save_source(db: &mut LibraryDb, src: &BookSource) -> Result<()> {
    let json = serde_json::to_string(src)?;
    let now = chrono::Utc::now().to_rfc3339();
    db.conn_mut().execute(
        "INSERT INTO sources(url,name,group_name,enabled,json,updated_at) VALUES(?,?,?,?,?,?)
         ON CONFLICT(url) DO UPDATE SET name=excluded.name,group_name=excluded.group_name,
            enabled=excluded.enabled,json=excluded.json,updated_at=excluded.updated_at",
        params![
            src.book_source_url,
            src.book_source_name,
            src.book_source_group,
            src.enabled as i64,
            json,
            now,
        ],
    )?;
    Ok(())
}

/// List every book source, ordered by display name.
pub fn list_sources(db: &LibraryDb) -> Result<Vec<BookSource>> {
    let mut stmt = db.conn().prepare("SELECT json FROM sources ORDER BY name")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for r in rows {
        let json = r?;
        out.push(serde_json::from_str(&json)?);
    }
    Ok(out)
}

/// Fetch a single book source by its primary key (book_source_url).
pub fn get_source(db: &LibraryDb, url: &str) -> Result<Option<BookSource>> {
    let json: Option<String> = db
        .conn()
        .query_row("SELECT json FROM sources WHERE url=?", [url], |r| r.get(0))
        .optional()?;
    Ok(match json {
        Some(j) => Some(serde_json::from_str(&j)?),
        None => None,
    })
}

/// Replace TOC for a novel — wipes + reinserts chapters in a single transaction.
/// Shared Kernel: Catalog writes idx/name/url; chapters.content is left NULL and
/// will be filled later by Library DAO via `save_chapter_content`.
pub fn replace_toc(db: &mut LibraryDb, novel_id: i64, chapters: &[ChapterMeta]) -> Result<()> {
    let tx = db.conn_mut().transaction()?;
    tx.execute("DELETE FROM chapters WHERE novel_id=?", [novel_id])?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO chapters(novel_id,idx,name,url,content) VALUES(?,?,?,?,NULL)",
        )?;
        for c in chapters {
            stmt.execute(params![novel_id, c.index, c.name, c.url])?;
        }
    }
    tx.commit()?;
    Ok(())
}
