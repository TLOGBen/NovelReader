//! Source-switch core logic — pure functions + the shared `run()` use case
//! invoked by both the TUI `SwitchSourceScreen` and the CLI `switch-source`
//! subcommand (REQ-005 / REQ-007 / REQ-001).
//!
//! `evaluate_toc` covers REQ-005 failure classes (d) "0 章" and (e) "全 fallback
//! name"; the remaining classes (a/b/c — fetch_info / fetch_toc HTTP / timeout)
//! are surfaced via the same `AbortReason` enum but are decided in `run()`.
//!
//! Testability: the 5-step orchestration lives in `run_with_deps`, parameterised
//! over a local `SwitchSourceDeps` trait. Production wires `RealDeps` over
//! `AppContext` (calling `catalog::facade` + `library::facade`); unit tests inject
//! a fake to assert REQ-005 (a) / (c) abort-before-tx semantics without touching
//! network / DB.

use std::time::Duration;

use anyhow::{anyhow, Context, Result};

use crate::catalog;
use crate::catalog::service::scraper::fallback_chapter_name;
use crate::catalog::BookSource;
use crate::library;
use crate::library::{ChapterMeta, Novel};
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

/// Dependency boundary for `run_with_deps`. Local trait — not exported. The
/// production impl `RealDeps` wires `catalog::facade` + `library::facade`;
/// unit tests inject a fake to exercise REQ-005 abort-before-tx behaviour
/// without network / DB side effects.
#[async_trait::async_trait(?Send)]
trait SwitchSourceDeps {
    fn lookup_source(&self, url: &str) -> Result<Option<BookSource>>;
    async fn fetch_novel_info(&self, src: &BookSource, book_url: &str) -> Result<Novel>;
    async fn fetch_toc_with_timeout(
        &self,
        src: &BookSource,
        toc_url: &str,
        deadline: Duration,
    ) -> Result<Vec<ChapterMeta>>;
    fn switch_source_tx(
        &mut self,
        novel_id: i64,
        new_src_url: &str,
        new_book_url: &str,
        new_chapters: &[ChapterMeta],
    ) -> Result<i64>;
}

/// Production wiring of `SwitchSourceDeps` over `AppContext`. Holds a `&mut`
/// borrow because `switch_source_tx` requires `&mut LibraryDb`.
struct RealDeps<'a> {
    ctx: &'a mut AppContext,
}

#[async_trait::async_trait(?Send)]
impl<'a> SwitchSourceDeps for RealDeps<'a> {
    fn lookup_source(&self, url: &str) -> Result<Option<BookSource>> {
        catalog::facade::get_source(&self.ctx.db, url)
    }
    async fn fetch_novel_info(&self, src: &BookSource, book_url: &str) -> Result<Novel> {
        catalog::facade::fetch_novel_info(&self.ctx.scraper, src, book_url).await
    }
    async fn fetch_toc_with_timeout(
        &self,
        src: &BookSource,
        toc_url: &str,
        deadline: Duration,
    ) -> Result<Vec<ChapterMeta>> {
        catalog::facade::fetch_toc_with_timeout(&self.ctx.scraper, src, toc_url, deadline).await
    }
    fn switch_source_tx(
        &mut self,
        novel_id: i64,
        new_src_url: &str,
        new_book_url: &str,
        new_chapters: &[ChapterMeta],
    ) -> Result<i64> {
        library::facade::switch_source_tx(
            &mut self.ctx.db,
            novel_id,
            new_src_url,
            new_book_url,
            new_chapters,
        )
    }
}

/// Cross-context use case shared by TUI `SwitchSourceScreen` and CLI
/// `switch-source` handler. Thin wrapper around [`run_with_deps`] —
/// production wiring via [`RealDeps`]. Composes `catalog::facade::get_source`
/// → `fetch_novel_info` → `fetch_toc_with_timeout(8s)` → `evaluate_toc` →
/// `library::facade::switch_source_tx`. Any of the five REQ-005 failure
/// classes aborts *before* the library tx, so the shelf state is unchanged.
pub async fn run(
    ctx: &mut AppContext,
    novel_id: i64,
    new_src_url: &str,
    new_book_url: &str,
) -> Result<SwitchOutcome> {
    let mut deps = RealDeps { ctx };
    run_with_deps(&mut deps, novel_id, new_src_url, new_book_url).await
}

/// Inner orchestration over the `SwitchSourceDeps` boundary. Production code
/// always calls this via [`run`]; tests inject a fake `deps` to assert
/// REQ-005 (a) / (c) abort-before-tx invariants.
async fn run_with_deps<D: SwitchSourceDeps>(
    deps: &mut D,
    novel_id: i64,
    new_src_url: &str,
    new_book_url: &str,
) -> Result<SwitchOutcome> {
    // step 1: lookup new source — None → abort, no DB tx happens.
    let src = deps
        .lookup_source(new_src_url)?
        .ok_or_else(|| anyhow!("找不到書源 {}", new_src_url))?;

    // step 2 (a): fetch_novel_info — propagate as zh-TW abort message.
    let novel_info = deps
        .fetch_novel_info(&src, new_book_url)
        .await
        .with_context(|| "換源失敗：取得詳情頁失敗 (a)".to_string())?;

    // toc_url 取自詳情頁，fallback 用 book_url 自己（與既有 sync handler 同步推導）。
    let toc_url = novel_info
        .toc_url
        .clone()
        .unwrap_or_else(|| new_book_url.to_string());

    // step 3 (b/c): fetch_toc_with_timeout — Err 與 Elapsed 都 propagate。
    let toc = deps
        .fetch_toc_with_timeout(&src, &toc_url, Duration::from_secs(8))
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
    deps.switch_source_tx(novel_id, new_src_url, new_book_url, &toc)
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
    use std::sync::Mutex;

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

    // -----------------------------------------------------------------
    // REQ-005 S2 / S3 — abort-before-tx invariant under mock scraper deps.
    //
    // anyhow::Error 不 impl Clone，故 FakeDeps 用 Option<Novel> + Option<&'static str>
    // 在 method 內每次 reconstruct 一個 Result，避免 move-out 問題。
    // -----------------------------------------------------------------

    fn dummy_source() -> BookSource {
        BookSource {
            book_source_url: "src".into(),
            book_source_name: "fake".into(),
            book_source_group: None,
            enabled: true,
            book_url_pattern: None,
            header: None,
            rule_search: Default::default(),
            rule_book_info: Default::default(),
            rule_toc: Default::default(),
            rule_content: Default::default(),
        }
    }

    fn dummy_novel() -> Novel {
        Novel {
            id: None,
            source_url: "src".into(),
            book_url: "book".into(),
            name: "n".into(),
            author: None,
            intro: None,
            cover_url: None,
            toc_url: None,
        }
    }

    struct FakeDeps {
        novel_info_ok: Option<Novel>,
        novel_info_err: Option<&'static str>,
        toc_ok: Option<Vec<ChapterMeta>>,
        toc_err: Option<&'static str>,
        switch_tx_called: Mutex<bool>,
    }

    #[async_trait::async_trait(?Send)]
    impl SwitchSourceDeps for FakeDeps {
        fn lookup_source(&self, _url: &str) -> Result<Option<BookSource>> {
            Ok(Some(dummy_source()))
        }
        async fn fetch_novel_info(
            &self,
            _src: &BookSource,
            _book_url: &str,
        ) -> Result<Novel> {
            if let Some(msg) = self.novel_info_err {
                Err(anyhow!(msg))
            } else {
                Ok(self.novel_info_ok.clone().expect("test must set novel_info_ok or _err"))
            }
        }
        async fn fetch_toc_with_timeout(
            &self,
            _src: &BookSource,
            _toc_url: &str,
            _deadline: Duration,
        ) -> Result<Vec<ChapterMeta>> {
            if let Some(msg) = self.toc_err {
                Err(anyhow!(msg))
            } else {
                Ok(self.toc_ok.clone().expect("test must set toc_ok or _err"))
            }
        }
        fn switch_source_tx(
            &mut self,
            _novel_id: i64,
            _new_src_url: &str,
            _new_book_url: &str,
            _new_chapters: &[ChapterMeta],
        ) -> Result<i64> {
            *self.switch_tx_called.lock().unwrap() = true;
            Ok(0)
        }
    }

    #[tokio::test]
    async fn req005_s2_fetch_info_fail_aborts_before_tx() {
        let mut deps = FakeDeps {
            novel_info_ok: None,
            novel_info_err: Some("HTTP 503 from new source"),
            toc_ok: Some(vec![mk(0, "ignored")]),
            toc_err: None,
            switch_tx_called: Mutex::new(false),
        };
        let r = run_with_deps(&mut deps, 1, "src", "book").await;
        assert!(r.is_err(), "fetch_novel_info Err should propagate");
        let err_msg = format!("{:#}", r.unwrap_err());
        assert!(
            err_msg.contains("(a)") || err_msg.contains("取得詳情頁"),
            "expected REQ-005 (a) zh-TW context in: {}",
            err_msg
        );
        assert!(
            !*deps.switch_tx_called.lock().unwrap(),
            "REQ-005 S2: switch_source_tx MUST NOT be called when fetch_novel_info fails"
        );
    }

    #[tokio::test]
    async fn req005_s3_fetch_toc_timeout_aborts_before_tx() {
        // fetch_toc_with_timeout 內部把 tokio::time::Elapsed 包裝成
        // anyhow!("fetch_toc timeout after {:?}", deadline)（見 catalog::facade）；
        // 這裡直接餵同型訊息給 fake 模擬該包裝結果。
        let mut deps = FakeDeps {
            novel_info_ok: Some(dummy_novel()),
            novel_info_err: None,
            toc_ok: None,
            toc_err: Some("fetch_toc timeout after 8s"),
            switch_tx_called: Mutex::new(false),
        };
        let r = run_with_deps(&mut deps, 1, "src", "book").await;
        assert!(r.is_err(), "fetch_toc_with_timeout Err should propagate");
        let err_msg = format!("{:#}", r.unwrap_err());
        assert!(
            err_msg.contains("(b/c)") || err_msg.contains("目錄頁"),
            "expected REQ-005 (b/c) zh-TW context in: {}",
            err_msg
        );
        assert!(
            !*deps.switch_tx_called.lock().unwrap(),
            "REQ-005 S3: switch_source_tx MUST NOT be called when fetch_toc times out"
        );
    }
}
