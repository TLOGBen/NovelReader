//! Catalog facade — use case orchestrators for Search / FetchInfo / SyncToc / FetchContent.
//!
//! Each fn corresponds to a CLI subcommand or handler step. Facade may call its
//! own service (Scraper) + own dao (sources/TOC writes); MUST NOT call other
//! context's facade per design.md "facade 不互呼" constraint (the sole exception
//! is backup→library Conformist relationship, which is not this context).
//!
//! Cross-context handoff happens in the Presentation handler (cli.rs / reader.rs),
//! which composes catalog::facade + library::facade for use cases like `add`
//! (fetch_novel_info + add_novel) and `read` cache-miss (fetch_chapter_content +
//! save_chapter_content).

use std::time::Duration;

use anyhow::{anyhow, Result};

use crate::catalog::dao;
use crate::catalog::service::scraper::Scraper;
use crate::catalog::BookSource;
use crate::library::dao::LibraryDb;
use crate::library::{ChapterMeta, Novel};

// Note: `search` is intentionally not exposed as a facade fn — the `search`
// handler iterates multiple BookSources (one or all enabled), so it composes
// `facade::list_sources` / `facade::get_source` + scraper.search directly.

/// `source import` subcommand — upsert a book source row.
pub fn save_source(db: &mut LibraryDb, src: &BookSource) -> Result<()> {
    dao::save_source(db, src)
}

/// `source list` / `search` subcommand — enumerate every installed book source.
pub fn list_sources(db: &LibraryDb) -> Result<Vec<BookSource>> {
    dao::list_sources(db)
}

/// Lookup a book source by its primary key URL — needed by `search` / `add` /
/// `sync` / `read` handlers (which compose catalog::facade + library::facade).
pub fn get_source(db: &LibraryDb, url: &str) -> Result<Option<BookSource>> {
    dao::get_source(db, url)
}

/// `add` subcommand — fetch novel info from book detail URL (no DB writes here;
/// handler subsequently calls `library::facade::add_novel` to persist).
pub async fn fetch_novel_info(
    scraper: &Scraper,
    source: &BookSource,
    book_url: &str,
) -> Result<Novel> {
    scraper.fetch_info(source, book_url).await
}

/// `sync` subcommand — fetch TOC + write chapters (Shared Kernel).
/// Returns chapter count for the handler to display.
pub async fn sync_toc(
    db: &mut LibraryDb,
    scraper: &Scraper,
    source: &BookSource,
    novel_id: i64,
    toc_url: &str,
) -> Result<usize> {
    let chapters: Vec<ChapterMeta> = scraper.fetch_toc(source, toc_url).await?;
    let n = chapters.len();
    dao::replace_toc(db, novel_id, &chapters)?;
    Ok(n)
}

/// Switch-source TOC fetch with a hard overall deadline. Wraps `Scraper::fetch_toc`
/// in `tokio::time::timeout`. Used by the switch-source handler so a hung TOC page
/// can't stall the whole transaction; the existing `sync_toc` path is unchanged.
///
/// - `Ok(Ok(chapters))` → `Ok(chapters)`
/// - `Ok(Err(e))`       → propagate the scraper error
/// - `Err(_elapsed)`    → `Err(anyhow!("fetch_toc timeout after {:?}", deadline))`
pub async fn fetch_toc_with_timeout(
    scraper: &Scraper,
    src: &BookSource,
    toc_url: &str,
    deadline: Duration,
) -> Result<Vec<ChapterMeta>> {
    match tokio::time::timeout(deadline, scraper.fetch_toc(src, toc_url)).await {
        Ok(Ok(chapters)) => Ok(chapters),
        Ok(Err(e)) => Err(e),
        Err(_elapsed) => Err(anyhow!("fetch_toc timeout after {:?}", deadline)),
    }
}

/// `read` / `tui` use case (cache miss path) — fetch chapter content. Handler
/// must subsequently call `library::facade::save_chapter_content` to cache.
pub async fn fetch_chapter_content(
    scraper: &Scraper,
    source: &BookSource,
    chapter_url: &str,
) -> Result<String> {
    scraper.fetch_content(source, chapter_url).await
}
