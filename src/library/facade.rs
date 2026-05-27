//! Library facade — thin pass-through over LibraryDb.
//!
//! Each function is a use-case entry point that the application service layer
//! (cli.rs handlers / reader.rs) calls. Facade may call DAO + service, but
//! must NOT directly use rusqlite (delegates to dao module).

use anyhow::Result;

use crate::library::dao::LibraryDb;
use crate::library::{Chapter, ChapterMeta, Novel, ReadProgress};

// Re-export `LibraryDb` so downstream contexts (Backup as Conformist) can
// reference the DB handle type without importing `library::dao` directly —
// preserves the "service/facade layer never names dao module" invariant.
pub use crate::library::dao::LibraryDb as LibraryDbHandle;

/// `add` subcommand — insert or update a novel in the shelf.
pub fn add_novel(db: &mut LibraryDb, novel: &Novel) -> Result<i64> {
    db.upsert_novel(novel)
}

/// `shelf` subcommand — list every novel.
pub fn list_shelf(db: &LibraryDb) -> Result<Vec<Novel>> {
    db.list_novels()
}

/// `read` / `tui` use case — fetch a specific novel by id.
pub fn get_novel(db: &LibraryDb, id: i64) -> Result<Option<Novel>> {
    db.get_novel(id)
}

/// SearchScreen — duplicate-add detection by natural key (`book_url`).
pub fn get_novel_by_book_url(db: &LibraryDb, book_url: &str) -> Result<Option<Novel>> {
    db.get_novel_by_book_url(book_url)
}

/// Shelf delete — pass-through over atomic dao transaction.
///
/// Removes the novel and its dependent chapters / progress rows in one
/// transaction. Idempotent: non-existent `novel_id` returns Ok.
pub fn delete_novel(db: &mut LibraryDb, novel_id: i64) -> Result<()> {
    db.delete_novel_tx(novel_id)
}

/// REQ-005 switch-source — thin pass-through over the dao transaction.
///
/// Caller (presentation handler) is responsible for the upstream catalog
/// pipeline (`get_source` → `fetch_novel_info` → `fetch_toc`) and for the
/// five-class pre-checks. This facade only wraps the atomic dao step —
/// never imports `catalog::*` (REQ-007 layer invariant).
///
/// Returns the new `progress.chapter_index` — equals `target_idx` if the caller
/// supplied an in-bounds value, otherwise falls back to the first idx of
/// `new_chapters`. See `LibraryDb::update_book_source_tx` for the bounds rule.
pub fn switch_source_tx(
    db: &mut LibraryDb,
    novel_id: i64,
    new_src_url: &str,
    new_book_url: &str,
    new_chapters: &[ChapterMeta],
    target_idx: Option<i64>,
) -> Result<i64> {
    db.update_book_source_tx(novel_id, new_src_url, new_book_url, new_chapters, target_idx)
}

/// TUI reader — list chapters for a novel.
pub fn list_chapters(db: &LibraryDb, novel_id: i64) -> Result<Vec<ChapterMeta>> {
    db.list_chapters(novel_id)
}

/// `read` / `tui` use case — fetch one chapter (with cached content if any).
pub fn get_chapter(db: &LibraryDb, novel_id: i64, idx: i64) -> Result<Option<Chapter>> {
    db.get_chapter(novel_id, idx)
}

/// Cache write after Catalog fetches chapter content.
pub fn save_chapter_content(db: &mut LibraryDb, novel_id: i64, idx: i64, content: &str) -> Result<()> {
    db.save_chapter_content(novel_id, idx, content)
}

/// TUI persist — store reading progress (chapter + scroll).
pub fn save_progress(db: &mut LibraryDb, progress: &ReadProgress) -> Result<()> {
    db.save_progress(progress)
}

/// TUI startup — resume from last saved progress.
pub fn get_progress(db: &LibraryDb, novel_id: i64) -> Result<Option<ReadProgress>> {
    db.get_progress(novel_id)
}
