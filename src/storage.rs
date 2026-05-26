use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::PathBuf;

use crate::models::{Chapter, ChapterMeta, Novel, ReadProgress};
use crate::source::BookSource;

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

pub struct Storage {
    conn: Connection,
}

impl Storage {
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

    // ---- sources ----

    pub fn save_source(&self, src: &BookSource) -> Result<()> {
        let json = serde_json::to_string(src)?;
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
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

    pub fn list_sources(&self) -> Result<Vec<BookSource>> {
        let mut stmt = self.conn.prepare("SELECT json FROM sources ORDER BY name")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for r in rows {
            let json = r?;
            out.push(serde_json::from_str(&json)?);
        }
        Ok(out)
    }

    pub fn get_source(&self, url: &str) -> Result<Option<BookSource>> {
        let json: Option<String> = self
            .conn
            .query_row("SELECT json FROM sources WHERE url=?", [url], |r| r.get(0))
            .optional()?;
        Ok(match json {
            Some(j) => Some(serde_json::from_str(&j)?),
            None => None,
        })
    }

    // ---- novels ----

    pub fn upsert_novel(&self, n: &Novel) -> Result<i64> {
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

    pub fn replace_toc(&mut self, novel_id: i64, chapters: &[ChapterMeta]) -> Result<()> {
        let tx = self.conn.transaction()?;
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

    pub fn save_chapter_content(&self, novel_id: i64, idx: i64, content: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE chapters SET content=? WHERE novel_id=? AND idx=?",
            params![content, novel_id, idx],
        )?;
        Ok(())
    }

    // ---- progress ----

    pub fn save_progress(&self, p: &ReadProgress) -> Result<()> {
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
