//! Source-switch core logic — pure functions + the shared `run()` use case
//! invoked by both the TUI `SwitchSourceScreen` and the CLI `switch-source`
//! subcommand (REQ-005 / REQ-007 / REQ-001).
//!
//! `evaluate_toc` covers REQ-005 failure classes (d) "0 章" and (e) "全 fallback
//! name"; the remaining classes (a/b/c — fetch_info / fetch_toc HTTP / timeout)
//! are surfaced via the same `AbortReason` enum but are decided in `run()`.

use std::time::Duration;

use anyhow::{anyhow, Context, Result};

use crate::catalog;
use crate::catalog::service::scraper::fallback_chapter_name;
use crate::library;
use crate::library::ChapterMeta;
use crate::presentation::AppContext;

/// Why a source-switch attempt aborted. Each variant maps 1:1 to a REQ-005
/// failure class; downstream UI formats a user-facing message per variant.
#[derive(Debug)]
pub enum AbortReason {
    /// (d) `fetch_toc` returned an empty list — likely a `ruleToc.chapterList`
    /// CSS miss against the new source.
    EmptyToc,
    /// (e) Every chapter name fell back to `fallback_chapter_name` — likely
    /// a `ruleToc.chapterName` bug (e.g. the broken `&@text` self-selector).
    AllFallbackNames,
    /// (a) `catalog::facade::fetch_novel_info` returned `Err`.
    #[allow(dead_code)]
    FetchInfoFailed(anyhow::Error),
    /// (b) `catalog::facade::fetch_toc` returned `Err` (non-timeout).
    #[allow(dead_code)]
    FetchTocFailed(anyhow::Error),
    /// (c) `fetch_toc` exceeded the 8s wall-clock budget.
    #[allow(dead_code)]
    FetchTocTimeout,
}

/// Pure judgement over a freshly fetched TOC. Returns `Ok(())` if it looks
/// healthy, `Err(AbortReason)` if it tripped failure class (d) or (e).
///
/// Drift-resistance: the fallback-name comparison reuses
/// [`fallback_chapter_name`] from `catalog::service::scraper`, the same fn
/// `Scraper::fetch_toc` calls when populating the placeholder. Any future
/// reformat of "Chapter N" automatically keeps producer and detector aligned.
pub fn evaluate_toc(toc: &[ChapterMeta]) -> std::result::Result<(), AbortReason> {
    if toc.is_empty() {
        return Err(AbortReason::EmptyToc);
    }
    if toc.iter().all(|c| c.name == fallback_chapter_name(c.index)) {
        return Err(AbortReason::AllFallbackNames);
    }
    Ok(())
}

/// Outcome of a successful source-switch — surfaces what the caller needs to
/// rebuild its UI (`new_progress_idx`, `chapter_count`, `new_first_chapter_name`).
#[derive(Debug)]
pub struct SwitchOutcome {
    pub new_progress_idx: i64,
    pub chapter_count: usize,
    pub new_first_chapter_name: String,
}

/// Cross-context use case shared by TUI `SwitchSourceScreen` and CLI
/// `switch-source` handler. Composes `catalog::facade::get_source` →
/// `fetch_novel_info` → `fetch_toc_with_timeout(8s)` → `evaluate_toc` →
/// `library::facade::switch_source_tx`. Any of the five REQ-005 failure
/// classes aborts *before* the library tx, so the shelf state is unchanged.
pub async fn run(
    ctx: &mut AppContext,
    novel_id: i64,
    new_src_url: &str,
    new_book_url: &str,
) -> Result<SwitchOutcome> {
    // step 1: lookup new source — None → abort, no DB tx happens.
    let src = catalog::facade::get_source(&ctx.db, new_src_url)?
        .ok_or_else(|| anyhow!("找不到書源 {}", new_src_url))?;

    // step 2 (a): fetch_novel_info — propagate as zh-TW abort message.
    let novel_info = catalog::facade::fetch_novel_info(&ctx.scraper, &src, new_book_url)
        .await
        .with_context(|| "換源失敗：取得詳情頁失敗 (a)".to_string())?;

    // toc_url 取自詳情頁，fallback 用 book_url 自己（與既有 sync handler 同步推導）。
    let toc_url = novel_info
        .toc_url
        .clone()
        .unwrap_or_else(|| new_book_url.to_string());

    // step 3 (b/c): fetch_toc_with_timeout — Err 與 Elapsed 都 propagate。
    let toc = catalog::facade::fetch_toc_with_timeout(
        &ctx.scraper,
        &src,
        &toc_url,
        Duration::from_secs(8),
    )
    .await
    .with_context(|| "換源失敗：目錄頁讀取失敗或逾時 (b/c)".to_string())?;

    // step 4 (d/e): evaluate_toc — pure judgement, zh-TW message per variant。
    evaluate_toc(&toc).map_err(|reason| match reason {
        AbortReason::EmptyToc => anyhow!("換源失敗：新源目錄為空，可能規則錯誤 (d)"),
        AbortReason::AllFallbackNames => {
            anyhow!("換源失敗：新源章節名解析全部失敗，疑為書源規則 bug (e)")
        }
        _ => anyhow!("換源失敗：未預期錯誤"),
    })?;

    let first = toc.first().expect("non-empty checked above");
    let first_idx = first.index;
    let first_name = first.name.clone();
    let chapter_count = toc.len();

    // step 5: library tx — five-class checks all passed, safe to mutate state。
    library::facade::switch_source_tx(
        &mut ctx.db,
        novel_id,
        new_src_url,
        new_book_url,
        &toc,
    )
    .with_context(|| "換源失敗：寫入 DB tx 失敗".to_string())?;

    Ok(SwitchOutcome {
        new_progress_idx: first_idx,
        chapter_count,
        new_first_chapter_name: first_name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(idx: i64, name: &str) -> ChapterMeta {
        ChapterMeta { index: idx, name: name.to_string(), url: "x".into() }
    }

    #[test]
    fn unit1_empty_toc() {
        assert!(matches!(evaluate_toc(&[]), Err(AbortReason::EmptyToc)));
    }

    #[test]
    fn unit2_all_fallback() {
        let toc = (0..3).map(|i| mk(i, &fallback_chapter_name(i))).collect::<Vec<_>>();
        assert!(matches!(evaluate_toc(&toc), Err(AbortReason::AllFallbackNames)));
    }

    #[test]
    fn unit3_partial_fallback_is_ok() {
        let toc = vec![
            mk(0, &fallback_chapter_name(0)),
            mk(1, "真章節名"),
            mk(2, &fallback_chapter_name(2)),
        ];
        assert!(evaluate_toc(&toc).is_ok());
    }
}
