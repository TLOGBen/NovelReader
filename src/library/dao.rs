//! Library DAO + SQLite Connection owner.
//!
//! NOTE: Shared Kernel — sources 表 + chapters.{idx,name,url} columns 由 Catalog DAO
//! 寫；chapters.content 由 Library DAO 寫。修改任一方 schema 需同步檢視對方 DAO。
//!
//! Layering: this module is the sole `rusqlite` import point for the Library
//! context. Service / facade layers depend on this DAO via `&LibraryDb` /
//! `&mut LibraryDb` and never touch `rusqlite` types directly.
//!
//! Borrow rules (per design.md):
//! - SELECT (唯讀)        → `&self`
//! - INSERT/UPDATE (寫入) → `&mut self`
//! - Transaction          → `&mut self`

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::PathBuf;

use crate::library::{Chapter, ChapterMeta, Novel, ReadProgress};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS sources (
  url        TEXT PRIMARY KEY,
  name       TEXT NOT NULL,
  group_name TEXT,
  enabled    INTEGER NOT NULL DEFAULT 1,
  json       TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS novels (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  source_url  TEXT NOT NULL,
  book_url    TEXT NOT NULL UNIQUE,
  name        TEXT NOT NULL,
  author      TEXT,
  intro       TEXT,
  cover_url   TEXT,
  toc_url     TEXT,
  added_at    TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS chapters (
  novel_id   INTEGER NOT NULL,
  idx        INTEGER NOT NULL,
  name       TEXT NOT NULL,
  url        TEXT NOT NULL,
  content    TEXT,
  PRIMARY KEY (novel_id, idx),
  FOREIGN KEY (novel_id) REFERENCES novels(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS progress (
  novel_id      INTEGER PRIMARY KEY,
  chapter_index INTEGER NOT NULL,
  scroll_offset INTEGER NOT NULL DEFAULT 0,
  updated_at    TEXT NOT NULL,
  FOREIGN KEY (novel_id) REFERENCES novels(id) ON DELETE CASCADE
);
"#;

pub struct LibraryDb {
    conn: Connection,
}

impl LibraryDb {
    pub fn open() -> Result<Self> {
        let path = data_dir()?.join("novel-looker.db");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(&path)
            .with_context(|| format!("open sqlite {}", path.display()))?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Shared Connection accessor for sibling DAOs (Catalog / Backup).
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Mutable Connection accessor for sibling DAOs that need transactions.
    pub fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    // ---- novels ----

    pub fn upsert_novel(&mut self, n: &Novel) -> Result<i64> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO novels(source_url,book_url,name,author,intro,cover_url,toc_url,added_at)
             VALUES(?,?,?,?,?,?,?,?)
             ON CONFLICT(book_url) DO UPDATE SET
               name=excluded.name, author=excluded.author, intro=excluded.intro,
               cover_url=excluded.cover_url, toc_url=excluded.toc_url",
            params![
                n.source_url, n.book_url, n.name, n.author, n.intro, n.cover_url, n.toc_url, now
            ],
        )?;
        let id: i64 = self.conn.query_row(
            "SELECT id FROM novels WHERE book_url=?",
            [&n.book_url],
            |r| r.get(0),
        )?;
        Ok(id)
    }

    pub fn list_novels(&self) -> Result<Vec<Novel>> {
        let mut stmt = self.conn.prepare(
            "SELECT id,source_url,book_url,name,author,intro,cover_url,toc_url FROM novels ORDER BY added_at DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Novel {
                id: Some(r.get(0)?),
                source_url: r.get(1)?,
                book_url: r.get(2)?,
                name: r.get(3)?,
                author: r.get(4)?,
                intro: r.get(5)?,
                cover_url: r.get(6)?,
                toc_url: r.get(7)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn get_novel(&self, id: i64) -> Result<Option<Novel>> {
        let n = self
            .conn
            .query_row(
                "SELECT id,source_url,book_url,name,author,intro,cover_url,toc_url FROM novels WHERE id=?",
                [id],
                |r| {
                    Ok(Novel {
                        id: Some(r.get(0)?),
                        source_url: r.get(1)?,
                        book_url: r.get(2)?,
                        name: r.get(3)?,
                        author: r.get(4)?,
                        intro: r.get(5)?,
                        cover_url: r.get(6)?,
                        toc_url: r.get(7)?,
                    })
                },
            )
            .optional()?;
        Ok(n)
    }

    // ---- chapters ----
    // (replace_toc lives in catalog::dao — Shared Kernel: Catalog owns TOC writes.)

    pub fn list_chapters(&self, novel_id: i64) -> Result<Vec<ChapterMeta>> {
        let mut stmt = self.conn.prepare(
            "SELECT idx,name,url FROM chapters WHERE novel_id=? ORDER BY idx",
        )?;
        let rows = stmt.query_map([novel_id], |r| {
            Ok(ChapterMeta { index: r.get(0)?, name: r.get(1)?, url: r.get(2)? })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn get_chapter(&self, novel_id: i64, idx: i64) -> Result<Option<Chapter>> {
        let row = self
            .conn
            .query_row(
                "SELECT idx,name,url,content FROM chapters WHERE novel_id=? AND idx=?",
                params![novel_id, idx],
                |r| {
                    let content: Option<String> = r.get(3)?;
                    Ok((ChapterMeta { index: r.get(0)?, name: r.get(1)?, url: r.get(2)? }, content))
                },
            )
            .optional()?;
        Ok(row.and_then(|(m, c)| c.map(|content| Chapter { meta: m, content })))
    }

    pub fn save_chapter_content(&mut self, novel_id: i64, idx: i64, content: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE chapters SET content=? WHERE novel_id=? AND idx=?",
            params![content, novel_id, idx],
        )?;
        Ok(())
    }

    // ---- progress ----

    pub fn save_progress(&mut self, p: &ReadProgress) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO progress(novel_id,chapter_index,scroll_offset,updated_at) VALUES(?,?,?,?)
             ON CONFLICT(novel_id) DO UPDATE SET
               chapter_index=excluded.chapter_index,
               scroll_offset=excluded.scroll_offset,
               updated_at=excluded.updated_at",
            params![p.novel_id, p.chapter_index, p.scroll_offset, now],
        )?;
        Ok(())
    }

    pub fn get_progress(&self, novel_id: i64) -> Result<Option<ReadProgress>> {
        let p = self
            .conn
            .query_row(
                "SELECT novel_id,chapter_index,scroll_offset FROM progress WHERE novel_id=?",
                [novel_id],
                |r| {
                    Ok(ReadProgress {
                        novel_id: r.get(0)?,
                        chapter_index: r.get(1)?,
                        scroll_offset: r.get(2)?,
                    })
                },
            )
            .optional()?;
        Ok(p)
    }
}

fn data_dir() -> Result<PathBuf> {
    let base = dirs::data_dir().context("could not resolve data dir")?;
    Ok(base.join("novel-looker"))
}
