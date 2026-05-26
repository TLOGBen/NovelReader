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
