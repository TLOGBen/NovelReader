//! Source-switch core logic — pure functions split out of the (forthcoming)
//! `switch_source` handler so they can be unit-tested without network / SQL.
//!
//! `evaluate_toc` covers REQ-005 failure classes (d) "0 章" and (e) "全 fallback
//! name"; the remaining classes (a/b/c — fetch_info / fetch_toc HTTP / timeout)
//! are surfaced via the same `AbortReason` enum but are decided in `run()`
//! (TASK-hc-02), not here.

use crate::catalog::service::scraper::fallback_chapter_name;
use crate::library::ChapterMeta;

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
pub fn evaluate_toc(toc: &[ChapterMeta]) -> Result<(), AbortReason> {
    if toc.is_empty() {
        return Err(AbortReason::EmptyToc);
    }
    if toc.iter().all(|c| c.name == fallback_chapter_name(c.index)) {
        return Err(AbortReason::AllFallbackNames);
    }
    Ok(())
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
