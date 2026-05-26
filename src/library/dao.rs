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

use anyhow::{anyhow, Context, Result};
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

    // ---- switch-source transaction (REQ-005) ----

    /// Atomically swap a novel's source + TOC + reset progress to new TOC's first idx.
    ///
    /// Single transaction wraps:
    ///   1. UPDATE novels SET source_url=?, book_url=? WHERE id=?
    ///   2. DELETE FROM chapters WHERE novel_id=?
    ///   3. INSERT INTO chapters(novel_id,idx,name,url,content=NULL) for each new chapter
    ///   4. UPSERT progress SET chapter_index=first_idx, scroll_offset=0, updated_at=now
    ///
    /// Any step's error propagates via `?` → `Transaction` Drop without commit = rollback.
    /// Returns the first chapter's idx (the new `progress.chapter_index`).
    ///
    /// Defensive: empty `new_chapters` returns `Err` (caller should have aborted earlier).
    pub fn update_book_source_tx(
        &mut self,
        novel_id: i64,
        new_src_url: &str,
        new_book_url: &str,
        new_chapters: &[ChapterMeta],
    ) -> Result<i64> {
        self.update_book_source_tx_inner(novel_id, new_src_url, new_book_url, new_chapters, None)
    }

    /// Test-only: same as `update_book_source_tx` but injects an `Err` after the
    /// given DB step (1 = UPDATE novels, 2 = DELETE chapters, 3 = INSERT chapters,
    /// 4 = UPSERT progress). Verifies that any-step rollback works.
    #[cfg(test)]
    pub fn update_book_source_tx_with_fault(
        &mut self,
        novel_id: i64,
        new_src_url: &str,
        new_book_url: &str,
        new_chapters: &[ChapterMeta],
        fault_step: u8,
    ) -> Result<i64> {
        self.update_book_source_tx_inner(
            novel_id,
            new_src_url,
            new_book_url,
            new_chapters,
            Some(fault_step),
        )
    }

    fn update_book_source_tx_inner(
        &mut self,
        novel_id: i64,
        new_src_url: &str,
        new_book_url: &str,
        new_chapters: &[ChapterMeta],
        fault_step: Option<u8>,
    ) -> Result<i64> {
        if new_chapters.is_empty() {
            return Err(anyhow!(
                "update_book_source_tx: empty new_chapters — caller should have aborted"
            ));
        }
        let first_idx = new_chapters.first().unwrap().index;
        let now = chrono::Utc::now().to_rfc3339();

        let tx = self.conn.transaction()?;

        // Step 1: UPDATE novels
        tx.execute(
            "UPDATE novels SET source_url=?, book_url=? WHERE id=?",
            params![new_src_url, new_book_url, novel_id],
        )?;
        if fault_step == Some(1) {
            return Err(anyhow!("injected fault @ step 1 (UPDATE novels)"));
        }

        // Step 2: DELETE chapters
        tx.execute("DELETE FROM chapters WHERE novel_id=?", [novel_id])?;
        if fault_step == Some(2) {
            return Err(anyhow!("injected fault @ step 2 (DELETE chapters)"));
        }

        // Step 3: INSERT new chapters
        {
            let mut stmt = tx.prepare(
                "INSERT INTO chapters(novel_id,idx,name,url,content) VALUES(?,?,?,?,NULL)",
            )?;
            for c in new_chapters {
                stmt.execute(params![novel_id, c.index, c.name, c.url])?;
            }
        }
        if fault_step == Some(3) {
            return Err(anyhow!("injected fault @ step 3 (INSERT chapters)"));
        }

        // Step 4: UPSERT progress
        tx.execute(
            "INSERT INTO progress(novel_id,chapter_index,scroll_offset,updated_at) VALUES(?,?,?,?)
             ON CONFLICT(novel_id) DO UPDATE SET
               chapter_index=excluded.chapter_index,
               scroll_offset=excluded.scroll_offset,
               updated_at=excluded.updated_at",
            params![novel_id, first_idx, 0_i64, now],
        )?;
        if fault_step == Some(4) {
            return Err(anyhow!("injected fault @ step 4 (UPSERT progress)"));
        }

        tx.commit()?;
        Ok(first_idx)
    }
}

#[cfg(test)]
impl LibraryDb {
    /// Test-only ctor backed by an in-memory SQLite database.
    ///
    /// Diverges from production `open()` in one way: enables `PRAGMA foreign_keys = ON`
    /// so that INT-3 meaningfully verifies "UPDATE novels does NOT cascade-delete progress".
    /// Without this PRAGMA, FK constraints are silently ignored by SQLite and the test
    /// would vacuously pass. Production `open()` keeps the default (off) for backwards
    /// compatibility with existing on-disk databases.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().context("open in-memory sqlite")?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }
}

fn data_dir() -> Result<PathBuf> {
    let base = dirs::data_dir().context("could not resolve data dir")?;
    Ok(base.join("novel-looker"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{ChapterMeta, Novel, ReadProgress};

    /// Sentinel timestamp written to progress.updated_at in the fixture so that
    /// "updated_at changed after tx" assertions don't race the system clock.
    const SENTINEL_UPDATED_AT: &str = "2000-01-01T00:00:00+00:00";

    struct Fixture {
        db: LibraryDb,
        novel_id: i64,
        progress_row_id: i64,
    }

    fn make_novel(source_url: &str, book_url: &str) -> Novel {
        Novel {
            id: None,
            source_url: source_url.into(),
            book_url: book_url.into(),
            name: "Test Novel".into(),
            author: Some("Author".into()),
            intro: Some("Intro".into()),
            cover_url: None,
            toc_url: None,
        }
    }

    fn make_chapter(idx: i64, name: &str, url: &str) -> ChapterMeta {
        ChapterMeta { index: idx, name: name.into(), url: url.into() }
    }

    /// Build a fixture: in-memory db with 1 novel + 5 chapters (idx 0..5) +
    /// progress(chapter_index=10, updated_at=SENTINEL).
    fn setup_fixture() -> Fixture {
        let mut db = LibraryDb::open_in_memory().expect("open in-memory db");

        let novel_id = db
            .upsert_novel(&make_novel("https://old-source.example/", "https://old-book.example/1"))
            .expect("upsert novel");

        // Old TOC: 5 chapters, idx 0..5.
        let old_toc: Vec<ChapterMeta> = (0..5)
            .map(|i| {
                make_chapter(
                    i,
                    &format!("舊章 {}", i),
                    &format!("https://old-book.example/1/c{}", i),
                )
            })
            .collect();
        {
            let tx = db.conn.transaction().unwrap();
            {
                let mut stmt = tx
                    .prepare(
                        "INSERT INTO chapters(novel_id,idx,name,url,content) VALUES(?,?,?,?,NULL)",
                    )
                    .unwrap();
                for c in &old_toc {
                    stmt.execute(params![novel_id, c.index, c.name, c.url]).unwrap();
                }
            }
            tx.commit().unwrap();
        }

        // Progress with chapter_index=10 (legit pre-state; we're not validating the value
        // matches a real chapter row — the test only verifies that update_book_source_tx
        // overwrites it).
        db.save_progress(&ReadProgress {
            novel_id,
            chapter_index: 10,
            scroll_offset: 7,
        })
        .expect("save progress");

        // Force progress.updated_at to a sentinel so we can assert "changed" deterministically.
        db.conn
            .execute(
                "UPDATE progress SET updated_at=? WHERE novel_id=?",
                params![SENTINEL_UPDATED_AT, novel_id],
            )
            .unwrap();

        // progress uses novel_id as PK, so "row id" == novel_id.
        let progress_row_id: i64 = db
            .conn
            .query_row(
                "SELECT novel_id FROM progress WHERE novel_id=?",
                [novel_id],
                |r| r.get(0),
            )
            .unwrap();

        Fixture { db, novel_id, progress_row_id }
    }

    fn new_toc(first_idx: i64, count: i64) -> Vec<ChapterMeta> {
        (0..count)
            .map(|i| {
                let idx = first_idx + i;
                make_chapter(
                    idx,
                    &format!("新章 {}", idx),
                    &format!("https://new-book.example/1/c{}", idx),
                )
            })
            .collect()
    }

    /// Snapshot of the three tables for a novel, used by INT-2 rollback assertions.
    #[derive(Debug, PartialEq)]
    struct Snapshot {
        source_url: String,
        book_url: String,
        chapters: Vec<(i64, String, String, Option<String>)>, // (idx, name, url, content)
        progress: Option<(i64, i64, i64, String)>, // (novel_id, chapter_index, scroll_offset, updated_at)
    }

    fn snapshot(db: &LibraryDb, novel_id: i64) -> Snapshot {
        let (source_url, book_url): (String, String) = db
            .conn
            .query_row(
                "SELECT source_url, book_url FROM novels WHERE id=?",
                [novel_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let mut stmt = db
            .conn
            .prepare("SELECT idx,name,url,content FROM chapters WHERE novel_id=? ORDER BY idx")
            .unwrap();
        let chapters: Vec<(i64, String, String, Option<String>)> = stmt
            .query_map([novel_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let progress = db
            .conn
            .query_row(
                "SELECT novel_id,chapter_index,scroll_offset,updated_at FROM progress WHERE novel_id=?",
                [novel_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .unwrap();
        Snapshot { source_url, book_url, chapters, progress }
    }

    // ---- INT-1: happy path ----

    #[test]
    fn int1_update_book_source_tx_happy_path() {
        let mut f = setup_fixture();
        let new_src = "https://new-source.example/";
        let new_book = "https://new-book.example/1";
        let new_chapters = new_toc(3, 4); // first.idx = 3, count = 4

        let returned = f
            .db
            .update_book_source_tx(f.novel_id, new_src, new_book, &new_chapters)
            .expect("happy path should succeed");

        // (vi) returned value is new TOC's first idx.
        assert_eq!(returned, 3);

        // (i) novels.source_url updated.
        let novel = f.db.get_novel(f.novel_id).unwrap().unwrap();
        assert_eq!(novel.source_url, new_src);

        // (ii) novels.book_url updated.
        assert_eq!(novel.book_url, new_book);

        // (iii) chapters count == 4 (old 5 gone, new 4 present).
        let count: i64 = f
            .db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM chapters WHERE novel_id=?",
                [f.novel_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 4);

        // (iv) progress.chapter_index == 3 (new first idx).
        let progress = f.db.get_progress(f.novel_id).unwrap().unwrap();
        assert_eq!(progress.chapter_index, 3);
        assert_eq!(progress.scroll_offset, 0);

        // (v) progress row id (= novel_id) unchanged.
        let row_id: i64 = f
            .db
            .conn
            .query_row(
                "SELECT novel_id FROM progress WHERE novel_id=?",
                [f.novel_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(row_id, f.progress_row_id);

        // (vii) progress.updated_at changed (no longer SENTINEL).
        let updated_at: String = f
            .db
            .conn
            .query_row(
                "SELECT updated_at FROM progress WHERE novel_id=?",
                [f.novel_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_ne!(updated_at, SENTINEL_UPDATED_AT);
    }

    // ---- INT-2a/b/c/d: rollback at each step ----

    fn assert_rollback_for_step(fault_step: u8) {
        let mut f = setup_fixture();
        let before = snapshot(&f.db, f.novel_id);

        let new_chapters = new_toc(3, 4);
        let result = f.db.update_book_source_tx_with_fault(
            f.novel_id,
            "https://new-source.example/",
            "https://new-book.example/1",
            &new_chapters,
            fault_step,
        );
        assert!(result.is_err(), "step {} should fail", fault_step);

        let after = snapshot(&f.db, f.novel_id);
        assert_eq!(
            before, after,
            "DB state must be unchanged after rollback @ step {}",
            fault_step
        );
    }

    #[test]
    fn int2a_rollback_at_update_novels() {
        assert_rollback_for_step(1);
    }

    #[test]
    fn int2b_rollback_at_delete_chapters() {
        assert_rollback_for_step(2);
    }

    #[test]
    fn int2c_rollback_at_insert_chapters() {
        assert_rollback_for_step(3);
    }

    #[test]
    fn int2d_rollback_at_update_progress() {
        assert_rollback_for_step(4);
    }

    // ---- INT-3: no cascade — progress row survives ----

    #[test]
    fn int3_no_cascade_progress_row_survives() {
        let mut f = setup_fixture();

        // Pre-condition: progress row exists with chapter_index=10.
        let before = f.db.get_progress(f.novel_id).unwrap().unwrap();
        assert_eq!(before.chapter_index, 10);

        let new_chapters = new_toc(0, 3);
        f.db.update_book_source_tx(
            f.novel_id,
            "https://new-source.example/",
            "https://new-book.example/1",
            &new_chapters,
        )
        .expect("update should succeed");

        // Same novel_id (= progress PK) row still present.
        let after = f.db.get_progress(f.novel_id).unwrap().unwrap();
        assert_eq!(after.novel_id, before.novel_id);
        assert_eq!(after.novel_id, f.progress_row_id);

        // Only chapter_index (and updated_at) changed — row was UPDATEd, not CASCADE-dropped.
        assert_ne!(after.chapter_index, before.chapter_index);
        assert_eq!(after.chapter_index, 0);
    }

    // ---- INT-4: chapter_index matches new TOC's first idx (not hardcoded) ----

    #[test]
    fn int4_progress_chapter_index_matches_new_first_idx() {
        let mut f = setup_fixture();
        // czbooks-like: first chapter has idx=5 (e.g. due to volume headers at 0..5).
        let new_chapters = new_toc(5, 3);

        let returned = f
            .db
            .update_book_source_tx(
                f.novel_id,
                "https://new-source.example/",
                "https://new-book.example/1",
                &new_chapters,
            )
            .expect("update should succeed");

        assert_eq!(returned, 5);
        let p = f.db.get_progress(f.novel_id).unwrap().unwrap();
        assert_eq!(p.chapter_index, 5);
    }

    // ---- Defensive: empty new_chapters ----

    #[test]
    fn empty_new_chapters_returns_err_without_touching_db() {
        let mut f = setup_fixture();
        let before = snapshot(&f.db, f.novel_id);

        let res = f.db.update_book_source_tx(
            f.novel_id,
            "https://new-source.example/",
            "https://new-book.example/1",
            &[],
        );
        assert!(res.is_err());

        let after = snapshot(&f.db, f.novel_id);
        assert_eq!(before, after, "empty input must not modify DB");
    }
}
