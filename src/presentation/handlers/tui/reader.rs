//! ReaderScreen — TUI 兩 pane 閱讀器 + ChapterBuffer-based 跨章 scroll。
//!
//! REQ-006：構造時帶 `EntryMode`，`m` 鍵依入口分流：
//!   - `EntryMode::Menu`         → `Transition::To(MenuScreen::new())`（回主菜單）
//!   - `EntryMode::DirectReader` → `Transition::Quit`（exit process）
//!
//! 本 task (TASK-reader-buffer-02) 重構 state：
//!   - 以 [`ChapterBuffer`] 取代既有 single-chapter `raw_content/content` 欄位
//!   - j/k 跳章改走 [`rebuild_buffer`]（CurrFailed → set toast、不改 state）
//!   - 一次 stub 進後續 toc-* / mouse-* group 會用的欄位（toc_collapsed /
//!     toc_width_cached / mode），避免後續 group 都動 struct 引發 merge 衝突。
//!
//! 既有 v1 鍵綁定（j/k/J/K/Space/PgUp/PgDn/n/p/Tab/g/G/Up/Down）行為保留，
//! 只是作用對象從「single chapter scroll」變成「combined buffer scroll」。

use anyhow::{anyhow, Result};
use crossterm::event::{Event, KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
use std::time::Instant;

use crate::catalog;
use crate::catalog::BookSource;
use crate::library;
use crate::library::dao::LibraryDb;
use crate::library::{ChapterMeta, Novel, ReadProgress};
use crate::presentation::handlers::tui::{
    menu::MenuScreen, EntryMode, Screen, Transition, TOAST_TTL,
};
use crate::presentation::AppContext;

// ---------------------------------------------------------------------------
// REQ-006 (TASK-reader-buffer-01) — ChapterBuffer + ScraperLike injection seam
// + init_buffer / rebuild_buffer fetch APIs.
//
// Wired into ReaderScreen state by TASK-reader-buffer-02 (this task) — the
// `#[allow(dead_code)]` markers on these items have been removed now that
// ReaderScreen actually consumes them.
// ---------------------------------------------------------------------------

/// Placeholder text for the B2 boundary: a chapter whose fetched content is
/// the empty string is rendered as one row "（本章空白）" so layout / progress
/// math never has to special-case a zero-row section.
pub(crate) const EMPTY_CONTENT_PLACEHOLDER: &str = "（本章空白）";

/// Eager 3-chapter buffer: prev + curr + next concatenated with "\n\n" gaps,
/// plus an offset table (`prev_end_row`, `curr_end_row`) so the renderer can
/// figure out which chapter the viewport top is currently inside.
///
/// - Mid-book: all three chapters present, `prev_chapter_idx = Some(p)`,
///   `next_chapter_idx = Some(n)`.
/// - Chapter 0: `prev_chapter_idx = None`, `prev_end_row = 0`, buffer holds 2.
/// - Last chapter: `next_chapter_idx = None`, buffer holds 2.
#[derive(Debug, Clone)]
pub(crate) struct ChapterBuffer {
    pub(crate) combined_text: String,
    pub(crate) prev_chapter_idx: Option<i64>,
    pub(crate) curr_chapter_idx: i64,
    pub(crate) next_chapter_idx: Option<i64>,
    /// Row index (0-based, **exclusive end**) where the prev section finishes.
    /// `0` when there is no prev.
    pub(crate) prev_end_row: u16,
    /// Row index where the curr section finishes (exclusive end).
    /// `next` section starts at `curr_end_row + 1` (1 blank-line separator).
    pub(crate) curr_end_row: u16,
}

/// `rebuild_buffer` failure modes. `init_buffer` returns plain `anyhow::Error`
/// because reader can't start with a missing chapter; `rebuild_buffer` runs
/// after the reader is alive, so a prev/next fetch miss only degrades the
/// buffer (caller keeps the old one or shows a toast).
#[derive(Debug)]
pub(crate) enum RebuildError {
    /// curr chapter fetch failed → caller must surface a toast and keep the
    /// previous buffer; the new buffer is unusable.
    ///
    /// `source` is retained for the Debug impl + future diagnostics (e.g.
    /// stderr log on test failure, second-line toast detail), but the toast
    /// message itself only uses `idx` to keep UX terse — hence `allow(dead_code)`
    /// on this single field to silence the read-side warning.
    CurrFailed {
        idx: i64,
        #[allow(dead_code)]
        source: anyhow::Error,
    },
    /// prev and/or next fetch failed → buffer still usable, just smaller.
    PartialDegraded(ChapterBuffer),
}

/// Scraper injection seam for UT mocking (design.md Seam 1, recommended path B).
/// Production: `impl ScraperLike for catalog::service::scraper::Scraper`.
/// Tests: a hand-rolled mock under `#[cfg(test)] mod tests`.
#[async_trait::async_trait(?Send)]
pub(crate) trait ScraperLike {
    async fn fetch_chapter_content(
        &self,
        src: &BookSource,
        url: &str,
    ) -> Result<String>;
}

#[async_trait::async_trait(?Send)]
impl ScraperLike for catalog::service::scraper::Scraper {
    async fn fetch_chapter_content(
        &self,
        src: &BookSource,
        url: &str,
    ) -> Result<String> {
        self.fetch_content(src, url).await
    }
}

/// Reader-startup buffer init.
///
/// Returns `Err` if any of the three (or two, at boundaries) chapters fails
/// to load — reader-screen ctor propagates the error to caller (menu/shelf
/// shows toast).
///
/// `pos` is the **position in `chapters`** (== `ReaderScreen.current` usize)
/// — it is not necessarily equal to `chapters[pos].index` because the
/// catalog::dao::replace_toc impl can leave holes (see CLAUDE.md "chapters.idx
/// is not a dense 0..N-1 sequence").
pub(crate) async fn init_buffer<S: ScraperLike>(
    pos: usize,
    novel_id: i64,
    chapters: &[ChapterMeta],
    scraper: &S,
    src: &BookSource,
    db: &mut LibraryDb,
) -> Result<ChapterBuffer> {
    if chapters.is_empty() {
        return Err(anyhow!("無章節可讀"));
    }
    let last = chapters.len() - 1;
    let prev_pos = if pos > 0 { Some(pos - 1) } else { None };
    let next_pos = if pos < last { Some(pos + 1) } else { None };

    let curr = load_content_or_fetch(novel_id, chapters, pos, scraper, src, db).await?;
    let prev = match prev_pos {
        Some(p) => Some(load_content_or_fetch(novel_id, chapters, p, scraper, src, db).await?),
        None => None,
    };
    let next = match next_pos {
        Some(p) => Some(load_content_or_fetch(novel_id, chapters, p, scraper, src, db).await?),
        None => None,
    };

    Ok(assemble_buffer(
        chapters,
        pos,
        prev_pos,
        next_pos,
        prev.as_deref(),
        &curr,
        next.as_deref(),
    ))
}

/// Reader-runtime rebuild: caller already has a working buffer; on failure
/// we degrade (PartialDegraded) instead of bubbling Err and breaking the UI.
pub(crate) async fn rebuild_buffer<S: ScraperLike>(
    pos: usize,
    novel_id: i64,
    chapters: &[ChapterMeta],
    scraper: &S,
    src: &BookSource,
    db: &mut LibraryDb,
) -> std::result::Result<ChapterBuffer, RebuildError> {
    if chapters.is_empty() {
        return Err(RebuildError::CurrFailed {
            idx: 0,
            source: anyhow!("無章節可讀"),
        });
    }
    let last = chapters.len() - 1;
    let curr_idx = chapters[pos].index;
    let prev_pos = if pos > 0 { Some(pos - 1) } else { None };
    let next_pos = if pos < last { Some(pos + 1) } else { None };

    // curr fetch fail = fatal for this rebuild
    let curr = match load_content_or_fetch(novel_id, chapters, pos, scraper, src, db).await {
        Ok(s) => s,
        Err(source) => return Err(RebuildError::CurrFailed { idx: curr_idx, source }),
    };

    // prev / next fetch fail = silent degrade
    let mut prev_ok: Option<String> = None;
    let mut prev_pos_final = prev_pos;
    if let Some(p) = prev_pos {
        match load_content_or_fetch(novel_id, chapters, p, scraper, src, db).await {
            Ok(s) => prev_ok = Some(s),
            Err(_) => prev_pos_final = None,
        }
    }
    let mut next_ok: Option<String> = None;
    let mut next_pos_final = next_pos;
    if let Some(p) = next_pos {
        match load_content_or_fetch(novel_id, chapters, p, scraper, src, db).await {
            Ok(s) => next_ok = Some(s),
            Err(_) => next_pos_final = None,
        }
    }

    let degraded =
        prev_pos.is_some() && prev_pos_final.is_none()
            || next_pos.is_some() && next_pos_final.is_none();

    let buf = assemble_buffer(
        chapters,
        pos,
        prev_pos_final,
        next_pos_final,
        prev_ok.as_deref(),
        &curr,
        next_ok.as_deref(),
    );

    if degraded {
        Err(RebuildError::PartialDegraded(buf))
    } else {
        Ok(buf)
    }
}

/// Cache hit → return DB content; cache miss → fetch via `ScraperLike`,
/// write back via `library::facade::save_chapter_content`, return text.
async fn load_content_or_fetch<S: ScraperLike>(
    novel_id: i64,
    chapters: &[ChapterMeta],
    pos: usize,
    scraper: &S,
    src: &BookSource,
    db: &mut LibraryDb,
) -> Result<String> {
    let meta = &chapters[pos];
    if let Ok(Some(ch)) = library::facade::get_chapter(db, novel_id, meta.index) {
        return Ok(ch.content);
    }
    let text = scraper.fetch_chapter_content(src, &meta.url).await?;
    // Best-effort cache write; failure here shouldn't make the buffer step Err.
    let _ = library::facade::save_chapter_content(db, novel_id, meta.index, &text);
    Ok(text)
}

/// Pure assembly: stitches up to 3 sections, computes row offsets, applies
/// B2 placeholder for empty sections. Section row count == `lines().count()`
/// of the rendered section (placeholder == 1 row).
fn assemble_buffer(
    chapters: &[ChapterMeta],
    pos: usize,
    prev_pos: Option<usize>,
    next_pos: Option<usize>,
    prev: Option<&str>,
    curr: &str,
    next: Option<&str>,
) -> ChapterBuffer {
    let render = |s: &str| -> String {
        if s.is_empty() {
            EMPTY_CONTENT_PLACEHOLDER.to_string()
        } else {
            s.to_string()
        }
    };
    let curr_text = render(curr);
    let prev_text = prev.map(render);
    let next_text = next.map(render);

    let row_count = |s: &str| -> u16 { s.lines().count().max(1) as u16 };

    let prev_rows = prev_text.as_deref().map(row_count).unwrap_or(0);
    let curr_rows = row_count(&curr_text);

    // combined_text: join the present sections with "\n\n" (one blank line).
    let mut parts: Vec<String> = Vec::with_capacity(3);
    if let Some(p) = &prev_text {
        parts.push(p.clone());
    }
    parts.push(curr_text);
    if let Some(n) = &next_text {
        parts.push(n.clone());
    }
    let combined_text = parts.join("\n\n");

    // Offset semantics: row indices (0-based, exclusive ends).
    // prev_end_row == prev_rows (0 when no prev).
    // curr_end_row == prev_end_row + (1 separator row if prev present) + curr_rows.
    let prev_end_row = prev_rows;
    let sep_after_prev = if prev_text.is_some() { 1 } else { 0 };
    let curr_end_row = prev_end_row.saturating_add(sep_after_prev).saturating_add(curr_rows);

    ChapterBuffer {
        combined_text,
        prev_chapter_idx: prev_pos.map(|p| chapters[p].index),
        curr_chapter_idx: chapters[pos].index,
        next_chapter_idx: next_pos.map(|p| chapters[p].index),
        prev_end_row,
        curr_end_row,
    }
}

/// Total rendered row count of the buffer (curr_end_row plus the optional
/// next section + its 1-row separator).
fn buffer_total_rows(buf: &ChapterBuffer) -> u16 {
    // combined_text 行數 == 所有 section rows + (sections-1) 分隔空行。
    // 直接從 combined_text 算最穩。
    buf.combined_text.lines().count().max(1) as u16
}

/// Pure helper — returns the chapter index (NOT position) that owns the row
/// at `scroll`. Used by INT-viewport-01/02/03 and as the "viewport top
/// chapter" for progress display + scroll-edge rebuild detection.
pub(crate) fn viewport_top_chapter(buf: &ChapterBuffer, scroll: u16) -> i64 {
    if scroll < buf.prev_end_row {
        buf.prev_chapter_idx.unwrap_or(buf.curr_chapter_idx)
    } else if scroll < buf.curr_end_row {
        buf.curr_chapter_idx
    } else {
        buf.next_chapter_idx.unwrap_or(buf.curr_chapter_idx)
    }
}

/// Pure helper — builds the bottom status-bar progress label.
///
/// Format: `"第 X 章 / 共 N 章 (Y%)"` where
///   - `X = viewport_top_chapter(buffer, scroll) + 1`  (1-based chapter num)
///   - `Y = X * 100 / N`                                (integer percent, clamped to 100)
///
/// `total` is `chapters.len()`; when 0 the percent is 0 (and X is still
/// printed as 1, but this is a degenerate state — the reader rejects
/// empty-chapter init upstream).
///
/// Free fn (not method) so INT-progress-01 can drive it with a hand-built
/// `ChapterBuffer`, no DB / no fetch.
pub(crate) fn progress_text(buffer: &ChapterBuffer, scroll: u16, total: usize) -> String {
    // viewport_top_chapter -> i64 chapter index; +1 for 1-based display.
    let x_i64 = viewport_top_chapter(buffer, scroll) + 1;
    // Stay in u64 land for the percent math; total fits, x fits (chapter
    // indices come from DB i64 but are non-negative in practice).
    let x_u64 = x_i64.max(0) as u64;
    let total_u64 = total as u64;
    let percent = if total_u64 == 0 {
        0
    } else {
        (x_u64.saturating_mul(100) / total_u64).min(100)
    };
    format!("第 {} 章 / 共 {} 章 ({}%)", x_i64, total, percent)
}

/// REQ-003 (TASK-reader-toc-02) — fuzzy filter helper.
///
/// 對 `chapters[*].name` 跑 SkimMatcherV2 fuzzy match，命中者按 score
/// 降序排列、回傳對應 `chapters` index 列表。
///
/// `query` 為空 → 回傳 `(0..chapters.len()).collect()`（不過濾）。
///
/// Free fn（不掛 method）讓 INT-mode-02/03 可不需 ReaderScreen 直接驗。
pub(crate) fn apply_fuzzy_filter(query: &str, chapters: &[ChapterMeta]) -> Vec<usize> {
    if query.is_empty() {
        return (0..chapters.len()).collect();
    }
    let matcher = SkimMatcherV2::default();
    let mut scored: Vec<(usize, i64)> = chapters
        .iter()
        .enumerate()
        .filter_map(|(i, c)| matcher.fuzzy_match(&c.name, query).map(|s| (i, s)))
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1));
    scored.into_iter().map(|(i, _)| i).collect()
}

// ---------------------------------------------------------------------------
// TASK-reader-mouse-01 — hit-test helpers (REQ-004 / REQ-005 基礎建設)
//
// Pane / hit_test_pane / hit_test_toc_row 為純 free fn — UT 可直接驗，
// 不依賴 ReaderScreen / DB / 真實 frame。toc_width 由 Screen::draw 寫入
// ReaderScreen.toc_width_cached（同 content_area_h pattern），
// 後續 mouse-02 (wheel) / mouse-03 (click) 從 state 取用此值跑 hit-test。
//
// `#[allow(dead_code)]` 暫時掛上：本 task 僅建 hit-test 基礎，production
// caller（mouse-02 wheel handler / mouse-03 click handler）為後續 task。
// `cargo test` build 下 UT 是 caller；`cargo build` 看不到 test 模組，故
// 不掛 allow 會誤報 dead_code。mouse-02 wire 後可移除 allow。
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Pane {
    Toc,
    Content,
}

/// Hit-test 哪個 pane 接收 mouse event。
///
/// 規則（per design.md Seam 3）：
///   - `toc_width == 0` → 全畫面 content（TOC collapsed）
///   - `column >= toc_width` → content pane
///   - 否則 → TOC pane
///
/// `toc_width` 由 draw() 寫入 `ReaderScreen.toc_width_cached`，
/// 等於 `if toc_collapsed { 0 } else { area.width * 30 / 100 }`。
pub(crate) fn hit_test_pane(column: u16, toc_width: u16) -> Pane {
    if toc_width == 0 || column >= toc_width {
        Pane::Content
    } else {
        Pane::Toc
    }
}

/// Hit-test TOC list 內第幾個 item 被點到。
///
/// - `row`：滑鼠事件的螢幕 row（從 0 開始）
/// - `list_offset`：list pane 在螢幕上的起始 row（含 Block border 等
///   padding；若 list 從 row 2 開始顯示則 list_offset=2）
/// - `items_count`：當前顯示在 list 內的 item 數（Normal mode = chapters.len()；
///   Filter mode = filtered_indices.len()）
///
/// 回 `Some(idx)` 表示點到第 idx 個 item；回 `None` 表示點到
/// list pane 外（row < list_offset）或空白 row（idx >= items_count，
/// 例如 list 短於畫面）。
///
/// Production caller wire-up done by `TASK-reader-mouse-03` (click handler in
/// `try_mouse_wheel`); `allow(dead_code)` removed.
pub(crate) fn hit_test_toc_row(
    row: u16,
    list_offset: u16,
    items_count: usize,
) -> Option<usize> {
    let row = row as usize;
    let off = list_offset as usize;
    let idx = row.checked_sub(off)?;
    if idx < items_count {
        Some(idx)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Stub enums used by the new state (mouse-* / toc-* groups will wire these).
// ---------------------------------------------------------------------------

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Focus {
    Toc,
    Content,
}

/// Reader 操作模式 — Normal 與 Filter（fuzzy 過濾 TOC）。
/// REQ-003 / TASK-reader-toc-02 wire：'/' 進 Filter、Esc / Enter 退；
/// Filter mode 下所有 printable Char 一律 append 到 query（含 t / / / j / k）。
pub(crate) enum ReaderMode {
    Normal,
    Filter {
        query: String,
        filtered_indices: Vec<usize>,
        selected: usize,
    },
}

// ---------------------------------------------------------------------------
// ReaderScreen — state container.
// ---------------------------------------------------------------------------

pub struct ReaderScreen {
    pub entry_mode: EntryMode,
    pub novel_id: i64,
    pub novel: Novel,
    pub chapters: Vec<ChapterMeta>,
    pub focus: Focus,
    /// 當前 viewport top 所屬章節在 `chapters` 中的 position (NOT chapter.index)。
    pub current: usize,
    /// Combined-buffer 內 row offset。
    pub scroll: u16,
    /// content area height cached on last draw（J/K 翻頁步幅用）。
    content_area_h: u16,

    // ---- 新欄位（本 task 一次 stub 進全部）----
    pub(crate) buffer: ChapterBuffer,
    /// Catalog book source — 重複使用，避免每次 rebuild 都重查 DB。
    book_source: BookSource,
    /// TOC ListState（取代 v1 `toc_state`）。
    pub(crate) toc_list_state: ListState,
    /// 收合狀態 — false = 30% width, true = 0%（reader-toc-01 才 wire 行為）。
    #[allow(dead_code)]
    pub(crate) toc_collapsed: bool,
    /// 當下 draw 算出的 TOC pane 寬度（mouse-01 hit-test 取用）。
    pub(crate) toc_width_cached: u16,
    /// 操作模式 — Filter 時 j/k/printable 一律 append query；Esc/Enter 退回 Normal。
    pub(crate) mode: ReaderMode,
    /// 失敗 / 警告 toast；toast_active() 過期判斷。
    pub(crate) toast: Option<String>,
    pub(crate) toast_expires_at: Option<Instant>,
}

impl ReaderScreen {
    /// 構造 + 預載：抓 novel / chapters / book_source / progress、用
    /// [`init_buffer`] 初始化 3 章 buffer。任一章載入失敗 → reader 開不起來
    /// （上層帶 toast）。
    pub async fn new(
        entry_mode: EntryMode,
        ctx: &mut AppContext,
        novel_id: i64,
    ) -> Result<Self> {
        let novel = library::facade::get_novel(&ctx.db, novel_id)?
            .ok_or_else(|| anyhow!("找不到小說 #{novel_id}"))?;
        let chapters = library::facade::list_chapters(&ctx.db, novel_id)?;
        if chapters.is_empty() {
            anyhow::bail!("尚無章節，請先 `sync {novel_id}`");
        }
        let book_source = catalog::facade::get_source(&ctx.db, &novel.source_url)?
            .ok_or_else(|| anyhow!("書源不存在: {}", novel.source_url))?;

        let progress = library::facade::get_progress(&ctx.db, novel_id)?;
        let start_pos = progress
            .as_ref()
            .map(|p| p.chapter_index as usize)
            .unwrap_or(0)
            .min(chapters.len().saturating_sub(1));

        let buffer = init_buffer(
            start_pos,
            novel_id,
            &chapters,
            &ctx.scraper,
            &book_source,
            &mut ctx.db,
        )
        .await?;

        let scroll = buffer.prev_end_row;
        let mut toc_list_state = ListState::default();
        toc_list_state.select(Some(start_pos));

        Ok(Self {
            entry_mode,
            novel_id,
            novel,
            chapters,
            focus: Focus::Toc,
            current: start_pos,
            scroll,
            content_area_h: 20,
            buffer,
            book_source,
            toc_list_state,
            toc_collapsed: false,
            toc_width_cached: 0,
            mode: ReaderMode::Normal,
            toast: None,
            toast_expires_at: None,
        })
    }

    /// 回傳目前該顯示的 toast — toast 不存在 / 過期 → `None`（同 shelf pattern）。
    #[allow(dead_code)]
    pub(crate) fn toast_active(&self) -> Option<&str> {
        self.toast.as_deref().filter(|_| {
            self.toast_expires_at
                .map_or(true, |t| Instant::now() < t)
        })
    }

    /// 把 buffer rebuild 到 `target_pos`（chapters 中的 position）。
    /// 失敗 → set toast、保持原 buffer / scroll / current 不變。
    pub(crate) async fn try_rebuild_to<S: ScraperLike>(
        &mut self,
        target_pos: usize,
        scraper: &S,
        db: &mut LibraryDb,
    ) {
        if target_pos >= self.chapters.len() {
            return;
        }
        match rebuild_buffer(
            target_pos,
            self.novel_id,
            &self.chapters,
            scraper,
            &self.book_source,
            db,
        )
        .await
        {
            Ok(buf) => {
                self.scroll = buf.prev_end_row;
                self.current = target_pos;
                self.buffer = buf;
                self.toc_list_state.select(Some(target_pos));
                self.toast = None;
                self.toast_expires_at = None;
            }
            Err(RebuildError::PartialDegraded(buf)) => {
                self.scroll = buf.prev_end_row;
                self.current = target_pos;
                self.buffer = buf;
                self.toc_list_state.select(Some(target_pos));
                self.toast = Some("部分章節載入失敗、僅顯示可讀部分".into());
                self.toast_expires_at = Some(Instant::now() + TOAST_TTL);
            }
            Err(RebuildError::CurrFailed { idx, .. }) => {
                self.toast = Some(format!("載入第 {} 章失敗", idx + 1));
                self.toast_expires_at = Some(Instant::now() + TOAST_TTL);
                // buffer / scroll / current 完全不變
            }
        }
    }

    /// Convenience wrapper — used by handle_event with production scraper.
    pub(crate) async fn apply_jump_to(&mut self, target_pos: usize, ctx: &mut AppContext) {
        // Split-borrow workaround: scraper 與 db 都在 ctx 裡，但 try_rebuild_to
        // 同時需要 &S 與 &mut LibraryDb；ctx.scraper 與 ctx.db 互不衝突，
        // 但 Rust 借用檢查無法看穿 method receiver，所以 inline。
        let AppContext { scraper, db, .. } = ctx;
        self.try_rebuild_to(target_pos, scraper, db).await;
    }

    /// 對 scroll 套用 `delta`（正向下、負向上），執行 B3 邊界檢查 + 若越界
    /// 但 prev/next exists 則 trigger rebuild。
    pub(crate) async fn try_scroll<S: ScraperLike>(
        &mut self,
        delta: i32,
        scraper: &S,
        db: &mut LibraryDb,
    ) {
        let total = buffer_total_rows(&self.buffer);
        // Max scroll = 讓 last row 可見的最大 offset；簡化為 total（reader 之後
        // 的 viewport clamping 由 ratatui Paragraph::scroll 處理）。
        let max_scroll = total;

        if delta < 0 {
            let abs = (-delta) as u16;
            // 越 prev 開頭：scroll 已是 0 + 還想往上 → rebuild prev 章節 or B3 鎖。
            if abs > self.scroll {
                // 試圖滾過頂端。
                if self.buffer.prev_chapter_idx.is_some() && self.current > 0 {
                    let target = self.current - 1;
                    self.try_rebuild_to(target, scraper, db).await;
                } else {
                    // B3 邊界：第 0 章 + 沒 prev → 鎖在 0。
                    self.scroll = 0;
                }
            } else {
                self.scroll -= abs;
                self.refresh_current_from_viewport();
            }
        } else if delta > 0 {
            let abs = delta as u16;
            let new_scroll = self.scroll.saturating_add(abs);
            if new_scroll >= max_scroll {
                // 越 next 結尾。
                if self.buffer.next_chapter_idx.is_some()
                    && self.current + 1 < self.chapters.len()
                {
                    let target = self.current + 1;
                    self.try_rebuild_to(target, scraper, db).await;
                } else {
                    // B3 邊界：最後章 + 沒 next → 鎖在 max。
                    self.scroll = max_scroll.saturating_sub(1);
                }
            } else {
                self.scroll = new_scroll;
                self.refresh_current_from_viewport();
            }
        }
    }

    /// 同 try_scroll 但用 AppContext。
    pub(crate) async fn apply_scroll(&mut self, delta: i32, ctx: &mut AppContext) {
        let AppContext { scraper, db, .. } = ctx;
        self.try_scroll(delta, scraper, db).await;
    }

    /// REQ-004 (TASK-reader-mouse-02) — Mouse wheel handler with injectable
    /// scraper for UT mocking. 分派規則：
    /// - Filter mode + TOC pane → selected ±1 in filtered_indices（不改
    ///   reader.current、不 rebuild）
    /// - Normal mode + TOC pane → ±1 章（try_rebuild_to → buffer rebuild +
    ///   scroll = new prev_end_row）
    /// - 任何 mode + content pane → scroll ±3 行（try_scroll；越界時走
    ///   try_scroll 的 cross-chapter rebuild）
    /// - toc_collapsed → toc_width_cached=0 → hit_test_pane 必回 Content
    ///   → 走 content pane 分支（全畫面 scroll）
    /// - 其他 MouseEventKind（Down/Up/Drag/Moved 等）→ no-op（mouse-03 才處理）
    pub(crate) async fn try_mouse_wheel<S: ScraperLike>(
        &mut self,
        me: MouseEvent,
        scraper: &S,
        db: &mut LibraryDb,
    ) {
        let pane = hit_test_pane(me.column, self.toc_width_cached);
        match (me.kind, pane) {
            // Filter mode + TOC pane: move selected within filtered_indices.
            (MouseEventKind::ScrollDown, Pane::Toc)
                if matches!(self.mode, ReaderMode::Filter { .. }) =>
            {
                if let ReaderMode::Filter {
                    selected,
                    filtered_indices,
                    ..
                } = &mut self.mode
                {
                    if !filtered_indices.is_empty()
                        && *selected + 1 < filtered_indices.len()
                    {
                        *selected += 1;
                    }
                }
            }
            (MouseEventKind::ScrollUp, Pane::Toc)
                if matches!(self.mode, ReaderMode::Filter { .. }) =>
            {
                if let ReaderMode::Filter { selected, .. } = &mut self.mode {
                    if *selected > 0 {
                        *selected -= 1;
                    }
                }
            }
            // Normal mode + TOC pane: jump ±1 chapter (rebuild).
            (MouseEventKind::ScrollDown, Pane::Toc) => {
                let target = (self.current + 1).min(self.chapters.len().saturating_sub(1));
                if target != self.current {
                    self.try_rebuild_to(target, scraper, db).await;
                }
            }
            (MouseEventKind::ScrollUp, Pane::Toc) => {
                if self.current > 0 {
                    let target = self.current - 1;
                    self.try_rebuild_to(target, scraper, db).await;
                }
            }
            // Content pane: ±3 行（含 Filter mode；spec 未明禁，與 Normal 一致）.
            (MouseEventKind::ScrollDown, Pane::Content) => {
                self.try_scroll(3, scraper, db).await;
            }
            (MouseEventKind::ScrollUp, Pane::Content) => {
                self.try_scroll(-3, scraper, db).await;
            }
            // REQ-005 (TASK-reader-mouse-03) — Left-click dispatch.
            //   - Content pane → no-op (S2)
            //   - TOC pane + blank row (row < list_offset or idx >= items) → no-op (S3)
            //   - TOC pane + valid row in Normal mode → jump to that chapter (S1)
            //   - TOC pane + valid row in Filter mode → jump to chapters[
            //     filtered_indices[idx]] and exit Filter mode back to Normal (S4)
            //
            // list_offset = 1 — TOC pane uses Block::default().borders(Borders::ALL)
            // so the top border occupies row 0 within body[0]; first list item is row 1.
            (MouseEventKind::Down(MouseButton::Left), Pane::Toc) => {
                const LIST_OFFSET: u16 = 1;
                let items_count = match &self.mode {
                    ReaderMode::Normal => self.chapters.len(),
                    ReaderMode::Filter { filtered_indices, .. } => filtered_indices.len(),
                };
                let hit = match hit_test_toc_row(me.row, LIST_OFFSET, items_count) {
                    Some(i) => i,
                    None => return,
                };
                let target_pos = match &self.mode {
                    ReaderMode::Normal => hit,
                    ReaderMode::Filter { filtered_indices, .. } => filtered_indices[hit],
                };
                // S4: Filter mode click is the Enter-equivalent — exit Filter mode
                // back to Normal before (or after) the jump. Exit first so subsequent
                // state matches the Enter path observed by INT-mode-* tests.
                if matches!(self.mode, ReaderMode::Filter { .. }) {
                    self.mode = ReaderMode::Normal;
                }
                if target_pos != self.current && target_pos < self.chapters.len() {
                    self.try_rebuild_to(target_pos, scraper, db).await;
                }
            }
            _ => {}
        }
    }

    /// AppContext wrapper for `try_mouse_wheel` — used by `handle_event` with
    /// the production scraper.
    pub(crate) async fn apply_mouse_wheel(&mut self, me: MouseEvent, ctx: &mut AppContext) {
        let AppContext { scraper, db, .. } = ctx;
        self.try_mouse_wheel(me, scraper, db).await;
    }

    /// scroll 移動但未越界 → 同步 `self.current` 到 viewport top 所屬章節
    /// （根據 chapter.index 反查 position）。
    fn refresh_current_from_viewport(&mut self) {
        let idx = viewport_top_chapter(&self.buffer, self.scroll);
        if let Some(pos) = self.chapters.iter().position(|c| c.index == idx) {
            self.current = pos;
            self.toc_list_state.select(Some(pos));
        }
    }

    /// Save current progress (chapter + scroll). Best-effort: 失敗也不擋退出。
    fn save_progress(&self, ctx: &mut AppContext) {
        let _ = library::facade::save_progress(
            &mut ctx.db,
            &ReadProgress {
                novel_id: self.novel_id,
                chapter_index: self.current as i64,
                scroll_offset: self.scroll,
            },
        );
    }

    /// REQ-003 (TASK-reader-toc-02) — Filter mode 按鍵分派。
    ///
    /// 分派規則：
    /// - Esc → 退回 Normal（toc_collapsed 不還原，依 S8）
    /// - Enter → 跳到 chapters[filtered_indices[selected]] 並退回 Normal；
    ///           filtered 為空時不跳章不退出（S7）
    /// - Backspace → query.pop()、重算 filter、selected=0；query 已空時 no-op
    /// - j/k → filter 結果的 selected ± 1（不出界）
    /// - 其他 printable Char（含 t / / / 等） → append query、重算 filter
    ///
    /// borrow checker note：因 enum variant 內 `query / filtered_indices /
    /// selected` 需 mut 並同時讀 `self.chapters` 跑 fuzzy → clone query string
    /// 出來、算好新值後再 mut borrow 寫回 是 idiomatic 解。
    async fn handle_filter_key(&mut self, key: KeyEvent, ctx: &mut AppContext) {
        match key.code {
            KeyCode::Esc => {
                self.mode = ReaderMode::Normal;
            }
            KeyCode::Enter => {
                let target = if let ReaderMode::Filter {
                    filtered_indices,
                    selected,
                    ..
                } = &self.mode
                {
                    if filtered_indices.is_empty() {
                        None
                    } else {
                        Some(filtered_indices[*selected])
                    }
                } else {
                    None
                };
                if let Some(pos) = target {
                    self.save_progress(ctx);
                    self.apply_jump_to(pos, ctx).await;
                    self.mode = ReaderMode::Normal;
                }
                // filtered 空 → 不跳章、不退出（REQ-003 S7）
            }
            KeyCode::Backspace => {
                // Stage 1: pop 字元（拿到 new query string）；空 query 直接 no-op return。
                let new_query = if let ReaderMode::Filter { query, .. } = &mut self.mode {
                    if query.is_empty() {
                        return;
                    }
                    query.pop();
                    query.clone()
                } else {
                    return;
                };
                // Stage 2: 重算 filter（讀 self.chapters，無 mut borrow）。
                let new_filter = apply_fuzzy_filter(&new_query, &self.chapters);
                // Stage 3: 寫回 filtered_indices / selected。
                if let ReaderMode::Filter {
                    filtered_indices,
                    selected,
                    ..
                } = &mut self.mode
                {
                    *filtered_indices = new_filter;
                    *selected = 0;
                }
            }
            KeyCode::Char('j') => {
                if let ReaderMode::Filter {
                    filtered_indices,
                    selected,
                    ..
                } = &mut self.mode
                {
                    if *selected + 1 < filtered_indices.len() {
                        *selected += 1;
                    }
                }
            }
            KeyCode::Char('k') => {
                if let ReaderMode::Filter { selected, .. } = &mut self.mode {
                    if *selected > 0 {
                        *selected -= 1;
                    }
                }
            }
            KeyCode::Char(c) => {
                // Append c 到 query、重算 filter、selected=0（含 t / / 等）。
                let new_query = if let ReaderMode::Filter { query, .. } = &self.mode {
                    let mut new = query.clone();
                    new.push(c);
                    new
                } else {
                    return;
                };
                let new_filter = apply_fuzzy_filter(&new_query, &self.chapters);
                if let ReaderMode::Filter {
                    query,
                    filtered_indices,
                    selected,
                } = &mut self.mode
                {
                    *query = new_query;
                    *filtered_indices = new_filter;
                    *selected = 0;
                }
            }
            _ => {}
        }
    }

    /// 依 entry_mode 決定退出語意（給 m / q 共用）。
    fn exit_transition(&self) -> Transition {
        match self.entry_mode {
            EntryMode::Menu => Transition::To(Box::new(MenuScreen::new())),
            EntryMode::DirectReader => Transition::Quit,
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Screen for ReaderScreen {
    fn draw(&mut self, frame: &mut Frame, _ctx: &AppContext) {
        let area = frame.area();
        self.content_area_h = area.height.saturating_sub(3);
        // REQ-002 (TASK-reader-toc-01): TOC pane 寬度動態計算 —
        // collapsed → 0；展開 → area.width 的 30%。
        // 取代 buffer-02 review finding 提到的 hardcoded 28。
        // 同步寫入 toc_width_cached，mouse-01 hit-test 取用。
        self.toc_width_cached = if self.toc_collapsed {
            0
        } else {
            area.width.saturating_mul(30) / 100
        };
        draw(frame, area, self);
    }

    async fn handle_event(&mut self, event: Event, ctx: &mut AppContext) -> Transition {
        let key: KeyEvent = match event {
            Event::Key(k) => k,
            // REQ-004 (TASK-reader-mouse-02): wheel handler dispatched by
            // pane / mode; click handler 由 mouse-03 接走（其他 MouseEventKind
            // 在 try_mouse_wheel 內 no-op）。
            Event::Mouse(me) => {
                self.apply_mouse_wheel(me, ctx).await;
                return Transition::Stay;
            }
            _ => return Transition::Stay,
        };

        // REQ-003 (TASK-reader-toc-02) — mode-aware dispatch。
        // Filter mode 攔截所有 printable Char（含 t/j/k/'/' 等）+ 控制鍵
        // Esc/Enter/Backspace；其餘 Normal mode 原本綁定全保留。
        if matches!(self.mode, ReaderMode::Filter { .. }) {
            self.handle_filter_key(key, ctx).await;
            return Transition::Stay;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Char('m') => {
                self.save_progress(ctx);
                return self.exit_transition();
            }
            KeyCode::Char('/') => {
                // REQ-003 S1 + S8: 進 Filter mode、強制展開 TOC。
                self.toc_collapsed = false;
                self.mode = ReaderMode::Filter {
                    query: String::new(),
                    filtered_indices: (0..self.chapters.len()).collect(),
                    selected: 0,
                };
            }
            KeyCode::Char('t') => {
                // REQ-002 (TASK-reader-toc-01): Normal mode toggle TOC；Filter
                // mode 由上方 dispatch 攔截，此處只接 Normal。
                self.toc_collapsed = !self.toc_collapsed;
            }
            KeyCode::Tab => {
                self.focus = if self.focus == Focus::Toc {
                    Focus::Content
                } else {
                    Focus::Toc
                };
            }
            KeyCode::Char('j') | KeyCode::Char('n') => {
                let next = (self.current + 1).min(self.chapters.len().saturating_sub(1));
                if next != self.current {
                    self.save_progress(ctx);
                    self.apply_jump_to(next, ctx).await;
                }
            }
            KeyCode::Char('k') | KeyCode::Char('p') => {
                if self.current > 0 {
                    let prev = self.current - 1;
                    self.save_progress(ctx);
                    self.apply_jump_to(prev, ctx).await;
                }
            }
            KeyCode::Char('J') | KeyCode::PageDown | KeyCode::Char(' ') => {
                let step = self.content_area_h.saturating_sub(2).max(1) as i32;
                self.apply_scroll(step, ctx).await;
            }
            KeyCode::Char('K') | KeyCode::PageUp => {
                let step = self.content_area_h.saturating_sub(2).max(1) as i32;
                self.apply_scroll(-step, ctx).await;
            }
            KeyCode::Char('g') => {
                self.scroll = 0;
                self.refresh_current_from_viewport();
            }
            KeyCode::Char('G') => {
                let total = buffer_total_rows(&self.buffer);
                self.scroll = total.saturating_sub(self.content_area_h).max(0);
                self.refresh_current_from_viewport();
            }
            KeyCode::Enter if self.focus == Focus::Toc => {
                if let Some(i) = self.toc_list_state.selected() {
                    if i != self.current {
                        self.save_progress(ctx);
                        self.apply_jump_to(i, ctx).await;
                    }
                }
            }
            KeyCode::Up => {
                if self.focus == Focus::Toc {
                    let i = self.toc_list_state.selected().unwrap_or(0).saturating_sub(1);
                    self.toc_list_state.select(Some(i));
                } else {
                    self.apply_scroll(-1, ctx).await;
                }
            }
            KeyCode::Down => {
                if self.focus == Focus::Toc {
                    let i = (self.toc_list_state.selected().unwrap_or(0) + 1)
                        .min(self.chapters.len().saturating_sub(1));
                    self.toc_list_state.select(Some(i));
                } else {
                    self.apply_scroll(1, ctx).await;
                }
            }
            _ => {}
        }

        Transition::Stay
    }
}

// ---------------------------------------------------------------------------
// draw / helpers — 改自 v1 reader.rs，以 buffer.combined_text 取代 raw_content。
// ---------------------------------------------------------------------------

fn draw(f: &mut Frame, area: Rect, app: &ReaderScreen) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);
    // REQ-002 (TASK-reader-toc-01): TOC pane 寬度動態 —
    // toc_collapsed=true → 0（content 100%）；false → area.width * 30%（content 70%）。
    // toc_width_cached 在 Screen::draw 內已算好（取代 buffer-02 hardcoded 28）。
    let toc_width = app.toc_width_cached;
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(toc_width), Constraint::Min(0)])
        .split(chunks[0]);

    // Left: TOC pane
    // REQ-003 (TASK-reader-toc-02): Filter mode → 拆 TOC pane 為 list(top) +
    // input bar(bottom, 1 row)；TOC list 只顯示 filtered_indices 對應章節，
    // highlight 在 filtered_indices[selected]。toc_width=0 時整個 TOC pane 跳過。
    if toc_width > 0 {
        match &app.mode {
            ReaderMode::Filter {
                query,
                filtered_indices,
                selected,
            } => {
                let toc_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Min(0), Constraint::Length(1)])
                    .split(body[0]);

                let items: Vec<ListItem> = filtered_indices
                    .iter()
                    .map(|&i| {
                        let c = &app.chapters[i];
                        let prefix = if i == app.current { "▶ " } else { "  " };
                        ListItem::new(format!("{prefix}{}", truncate(&c.name, 24)))
                    })
                    .collect();
                let toc_block = Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" {} ", truncate(&app.novel.name, 24)))
                    .border_style(focus_style(app.focus == Focus::Toc));
                let list = List::new(items).block(toc_block).highlight_style(
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                );
                let mut state = ListState::default();
                if !filtered_indices.is_empty() {
                    state.select(Some(*selected));
                }
                f.render_stateful_widget(list, toc_chunks[0], &mut state);

                // input bar: "/" + query
                let input = Paragraph::new(format!("/{}", query)).style(
                    Style::default().fg(Color::Yellow).bg(Color::Black),
                );
                f.render_widget(input, toc_chunks[1]);
            }
            ReaderMode::Normal => {
                let items: Vec<ListItem> = app
                    .chapters
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        let prefix = if i == app.current { "▶ " } else { "  " };
                        ListItem::new(format!("{prefix}{}", truncate(&c.name, 24)))
                    })
                    .collect();
                let toc_block = Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" {} ", truncate(&app.novel.name, 24)))
                    .border_style(focus_style(app.focus == Focus::Toc));
                let list = List::new(items).block(toc_block).highlight_style(
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                );
                let mut state = app.toc_list_state.clone();
                f.render_stateful_widget(list, body[0], &mut state);
            }
        }
    }

    // Right: content
    let title = app
        .chapters
        .get(app.current)
        .map(|c| c.name.clone())
        .unwrap_or_default();
    let content_block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", title))
        .border_style(focus_style(app.focus == Focus::Content));
    let para = Paragraph::new(app.buffer.combined_text.as_str())
        .block(content_block)
        .wrap(Wrap { trim: false })
        .scroll((app.scroll, 0));
    f.render_widget(para, body[1]);

    // Status bar — toast 優先（toast_active() 已處理 TTL 過期），否則
    // 顯示 progress_text。Constraint: 必須走 toast_active() getter，
    // 不直接讀 app.toast；progress 必須走 free fn，不 inline 字串組裝。
    let status_label = match app.toast_active() {
        Some(t) => format!(" {} ", t),
        None => format!(" {} ", progress_text(&app.buffer, app.scroll, app.chapters.len())),
    };
    let status = Line::from(vec![
        Span::styled(
            status_label,
            Style::default().fg(Color::Black).bg(Color::Cyan),
        ),
        Span::raw("  "),
        Span::styled(
            " j/k 章節  J/K 翻頁  n/p 下/上章  Tab 切換  g/G 頭尾  m 主菜單  q 離開 ",
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    f.render_widget(Paragraph::new(status), chunks[1]);
}

fn focus_style(active: bool) -> Style {
    if active {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

fn truncate(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        chars[..max].iter().collect::<String>() + "…"
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::library::dao::LibraryDb;
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

    /// 構造一個僅供測試使用的 AppContext（in-memory DB）。
    fn test_ctx() -> AppContext {
        let db = LibraryDb::open_in_memory().expect("open in-memory db");
        let scraper =
            crate::catalog::service::scraper::Scraper::new().expect("scraper init");
        let config = Config::default();
        AppContext { db, scraper, config }
    }

    /// Build a ChapterBuffer in-memory (no DB / no fetch) for UTs that target
    /// viewport / scroll / jump logic without exercising the fetch path.
    fn mk_buffer(
        prev_chapter_idx: Option<i64>,
        curr_chapter_idx: i64,
        next_chapter_idx: Option<i64>,
        prev_text: Option<&str>,
        curr_text: &str,
        next_text: Option<&str>,
    ) -> ChapterBuffer {
        let mut parts: Vec<String> = Vec::new();
        let prev_rows = prev_text.map(|s| s.lines().count().max(1) as u16).unwrap_or(0);
        let curr_rows = curr_text.lines().count().max(1) as u16;
        if let Some(p) = prev_text {
            parts.push(p.to_string());
        }
        parts.push(curr_text.to_string());
        if let Some(n) = next_text {
            parts.push(n.to_string());
        }
        let combined_text = parts.join("\n\n");
        let prev_end_row = prev_rows;
        let sep_after_prev = if prev_text.is_some() { 1 } else { 0 };
        let curr_end_row = prev_end_row + sep_after_prev + curr_rows;
        ChapterBuffer {
            combined_text,
            prev_chapter_idx,
            curr_chapter_idx,
            next_chapter_idx,
            prev_end_row,
            curr_end_row,
        }
    }

    /// Construct a ReaderScreen with pre-built buffer + chapters, bypassing
    /// `init_buffer` so UTs can target jump / scroll / viewport logic.
    fn mk_reader(
        mode: EntryMode,
        chapters: Vec<ChapterMeta>,
        current: usize,
        buffer: ChapterBuffer,
    ) -> ReaderScreen {
        let mut toc_list_state = ListState::default();
        toc_list_state.select(Some(current));
        let scroll = buffer.prev_end_row;
        ReaderScreen {
            entry_mode: mode,
            novel_id: 1,
            novel: Novel {
                id: Some(1),
                source_url: "https://test.example/".into(),
                book_url: "y".into(),
                name: "n".into(),
                author: None,
                intro: None,
                cover_url: None,
                toc_url: None,
            },
            chapters,
            focus: Focus::Toc,
            current,
            scroll,
            content_area_h: 20,
            buffer,
            book_source: mock_book_source(),
            toc_list_state,
            toc_collapsed: false,
            toc_width_cached: 0,
            mode: ReaderMode::Normal,
            toast: None,
            toast_expires_at: None,
        }
    }

    /// minimal mock reader (no chapters, no real buffer) — used by UNIT-4a/4b
    /// regression tests for m-key EntryMode 分流。
    fn mock_reader_min(mode: EntryMode) -> ReaderScreen {
        let buffer = mk_buffer(None, 0, None, None, "X", None);
        let chapters = vec![ChapterMeta {
            index: 0,
            name: "ch0".into(),
            url: "u0".into(),
        }];
        mk_reader(mode, chapters, 0, buffer)
    }

    /// Trait migration: 既有 UT 改為包 Event::Key(...)，行為斷言不變。
    fn press(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::empty()))
    }

    /// UNIT-4a: Reader m 鍵 + EntryMode::Menu → Transition::To(_)
    #[tokio::test]
    async fn unit4a_menu_mode_m_to_menu() {
        let mut r = mock_reader_min(EntryMode::Menu);
        let mut ctx = test_ctx();
        let t = r.handle_event(press(KeyCode::Char('m')), &mut ctx).await;
        assert!(matches!(t, Transition::To(_)));
    }

    /// UNIT-4b: Reader m 鍵 + EntryMode::DirectReader → Transition::Quit
    #[tokio::test]
    async fn unit4b_direct_mode_m_quits() {
        let mut r = mock_reader_min(EntryMode::DirectReader);
        let mut ctx = test_ctx();
        let t = r.handle_event(press(KeyCode::Char('m')), &mut ctx).await;
        assert!(matches!(t, Transition::Quit));
    }

    // -----------------------------------------------------------------------
    // INT-buffer-01..06 + INT-boundary-* — ChapterBuffer / init_buffer /
    // rebuild_buffer / ScraperLike injection seam (TASK-reader-buffer-01).
    // -----------------------------------------------------------------------

    use std::cell::RefCell;
    use std::collections::HashMap;

    use rusqlite::params;

    use crate::catalog::service::source::BookSource;

    /// In-test mock for `ScraperLike`. Maps chapter URL → response.
    /// `panic_on_call=true` → any call panics; used to assert "cache hit, no fetch".
    struct MockScraper {
        by_url: HashMap<String, Result<String>>,
        panic_on_call: bool,
        calls: RefCell<Vec<String>>,
    }

    impl MockScraper {
        fn new() -> Self {
            Self {
                by_url: HashMap::new(),
                panic_on_call: false,
                calls: RefCell::new(Vec::new()),
            }
        }
        fn with(mut self, url: &str, resp: Result<String>) -> Self {
            self.by_url.insert(url.into(), resp);
            self
        }
        fn panic_on_call(mut self) -> Self {
            self.panic_on_call = true;
            self
        }
        fn call_count(&self) -> usize {
            self.calls.borrow().len()
        }
    }

    #[async_trait::async_trait(?Send)]
    impl ScraperLike for MockScraper {
        async fn fetch_chapter_content(
            &self,
            _src: &BookSource,
            url: &str,
        ) -> Result<String> {
            if self.panic_on_call {
                panic!("scraper called when cache should hit (url={url})");
            }
            self.calls.borrow_mut().push(url.into());
            match self.by_url.get(url) {
                Some(Ok(s)) => Ok(s.clone()),
                Some(Err(e)) => Err(anyhow!(e.to_string())),
                None => Err(anyhow!("MockScraper: no mock for url {url}")),
            }
        }
    }

    /// Construct a minimal-valid BookSource (all rule groups defaulted; only the
    /// identity fields populated). The mock scraper ignores it entirely.
    fn mock_book_source() -> BookSource {
        BookSource {
            book_source_url: "https://test.example/".into(),
            book_source_name: "test".into(),
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

    /// Seed a novel with N chapters; `contents[i]` Some → that chapter has cached
    /// content; None → NULL (cache miss). Returns novel_id.
    fn seed_novel_with_chapters(
        db: &mut LibraryDb,
        contents: &[Option<&str>],
    ) -> i64 {
        let novel = Novel {
            id: None,
            source_url: "https://test.example/".into(),
            book_url: "https://test.example/book/1".into(),
            name: "Test".into(),
            author: None,
            intro: None,
            cover_url: None,
            toc_url: None,
        };
        let novel_id = db.upsert_novel(&novel).unwrap();
        let conn = db.conn_mut();
        for (i, c) in contents.iter().enumerate() {
            conn.execute(
                "INSERT INTO chapters(novel_id,idx,name,url,content) VALUES(?,?,?,?,?)",
                params![
                    novel_id,
                    i as i64,
                    format!("第 {} 章", i + 1),
                    format!("https://test.example/book/1/c{}", i),
                    c.map(|s| s.to_string()),
                ],
            )
            .unwrap();
        }
        novel_id
    }

    fn url_of(novel_idx: usize) -> String {
        format!("https://test.example/book/1/c{}", novel_idx)
    }

    #[tokio::test]
    async fn int_buffer_01_normal_3_chapters_cache_hit() {
        let mut db = LibraryDb::open_in_memory().unwrap();
        let novel_id = seed_novel_with_chapters(
            &mut db,
            &[Some("A"), Some("B"), Some("C"), Some("D")],
        );
        let chapters = library::facade::list_chapters(&db, novel_id).unwrap();
        let scraper = MockScraper::new().panic_on_call();
        let src = mock_book_source();

        let buf = init_buffer(1, novel_id, &chapters, &scraper, &src, &mut db)
            .await
            .expect("init_buffer 3-chapter happy path");

        assert_eq!(buf.prev_chapter_idx, Some(0));
        assert_eq!(buf.curr_chapter_idx, 1);
        assert_eq!(buf.next_chapter_idx, Some(2));
        assert!(buf.combined_text.contains('A') && buf.combined_text.contains('B') && buf.combined_text.contains('C'));
        assert_eq!(buf.prev_end_row, 1);
        assert!(buf.curr_end_row > buf.prev_end_row);
    }

    #[tokio::test]
    async fn int_buffer_02_first_chapter_degraded() {
        let mut db = LibraryDb::open_in_memory().unwrap();
        let novel_id =
            seed_novel_with_chapters(&mut db, &[Some("A"), Some("B"), Some("C")]);
        let chapters = library::facade::list_chapters(&db, novel_id).unwrap();
        let scraper = MockScraper::new().panic_on_call();
        let src = mock_book_source();

        let buf = init_buffer(0, novel_id, &chapters, &scraper, &src, &mut db)
            .await
            .expect("init_buffer curr=0");

        assert_eq!(buf.prev_chapter_idx, None);
        assert_eq!(buf.curr_chapter_idx, 0);
        assert_eq!(buf.next_chapter_idx, Some(1));
        assert_eq!(buf.prev_end_row, 0);
    }

    #[tokio::test]
    async fn int_buffer_03_last_chapter_degraded() {
        let mut db = LibraryDb::open_in_memory().unwrap();
        let novel_id =
            seed_novel_with_chapters(&mut db, &[Some("A"), Some("B"), Some("C")]);
        let chapters = library::facade::list_chapters(&db, novel_id).unwrap();
        let last = chapters.len() - 1;
        let scraper = MockScraper::new().panic_on_call();
        let src = mock_book_source();

        let buf = init_buffer(last, novel_id, &chapters, &scraper, &src, &mut db)
            .await
            .expect("init_buffer curr=last");

        assert_eq!(buf.prev_chapter_idx, Some((last - 1) as i64));
        assert_eq!(buf.curr_chapter_idx, last as i64);
        assert_eq!(buf.next_chapter_idx, None);
    }

    #[tokio::test]
    async fn int_buffer_04_cache_hit_no_scraper_call() {
        let mut db = LibraryDb::open_in_memory().unwrap();
        let novel_id = seed_novel_with_chapters(
            &mut db,
            &[Some("A"), Some("B"), Some("C")],
        );
        let chapters = library::facade::list_chapters(&db, novel_id).unwrap();
        let scraper = MockScraper::new().panic_on_call();
        let src = mock_book_source();

        let _buf = init_buffer(1, novel_id, &chapters, &scraper, &src, &mut db)
            .await
            .expect("ok");
    }

    #[tokio::test]
    async fn int_buffer_04b_cache_miss_save_back() {
        let mut db = LibraryDb::open_in_memory().unwrap();
        let novel_id =
            seed_novel_with_chapters(&mut db, &[None, Some("B"), None]);
        let chapters = library::facade::list_chapters(&db, novel_id).unwrap();
        let scraper = MockScraper::new()
            .with(&url_of(0), Ok("A-fetched".into()))
            .with(&url_of(2), Ok("C-fetched".into()));
        let src = mock_book_source();

        let _buf = init_buffer(1, novel_id, &chapters, &scraper, &src, &mut db)
            .await
            .expect("init_buffer cache-miss path");

        let ch0 = library::facade::get_chapter(&db, novel_id, 0).unwrap().unwrap();
        let ch2 = library::facade::get_chapter(&db, novel_id, 2).unwrap().unwrap();
        assert_eq!(ch0.content, "A-fetched");
        assert_eq!(ch2.content, "C-fetched");
    }

    #[tokio::test]
    async fn int_buffer_05_rebuild_partial_degraded_on_next_fail() {
        let mut db = LibraryDb::open_in_memory().unwrap();
        let novel_id = seed_novel_with_chapters(
            &mut db,
            &[Some("A"), Some("B"), None],
        );
        let chapters = library::facade::list_chapters(&db, novel_id).unwrap();
        let scraper = MockScraper::new()
            .with(&url_of(2), Err(anyhow!("network down")));
        let src = mock_book_source();

        let err = rebuild_buffer(1, novel_id, &chapters, &scraper, &src, &mut db)
            .await
            .expect_err("rebuild expected to flag partial degraded");

        match err {
            RebuildError::PartialDegraded(buf) => {
                assert_eq!(buf.prev_chapter_idx, Some(0));
                assert_eq!(buf.curr_chapter_idx, 1);
                assert_eq!(buf.next_chapter_idx, None);
                assert!(buf.combined_text.contains('A'));
                assert!(buf.combined_text.contains('B'));
            }
            other => panic!("expected PartialDegraded, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn int_buffer_06_rebuild_curr_failed() {
        let mut db = LibraryDb::open_in_memory().unwrap();
        let novel_id =
            seed_novel_with_chapters(&mut db, &[Some("A"), None, Some("C")]);
        let chapters = library::facade::list_chapters(&db, novel_id).unwrap();
        let scraper = MockScraper::new()
            .with(&url_of(1), Err(anyhow!("curr boom")));
        let src = mock_book_source();

        let err = rebuild_buffer(1, novel_id, &chapters, &scraper, &src, &mut db)
            .await
            .expect_err("rebuild curr-fetch fail");

        match err {
            RebuildError::CurrFailed { idx, .. } => assert_eq!(idx, 1),
            other => panic!("expected CurrFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn int_boundary_empty_chapters() {
        let mut db = LibraryDb::open_in_memory().unwrap();
        let novel_id = seed_novel_with_chapters(&mut db, &[]);
        let chapters = library::facade::list_chapters(&db, novel_id).unwrap();
        let scraper = MockScraper::new();
        let src = mock_book_source();

        let err = init_buffer(0, novel_id, &chapters, &scraper, &src, &mut db)
            .await
            .expect_err("empty chapters must Err");

        assert!(err.to_string().contains("無章節可讀"), "got: {err}");
    }

    #[tokio::test]
    async fn int_boundary_empty_content() {
        let mut db = LibraryDb::open_in_memory().unwrap();
        let novel_id =
            seed_novel_with_chapters(&mut db, &[Some("A"), None, Some("C")]);
        let chapters = library::facade::list_chapters(&db, novel_id).unwrap();
        let scraper = MockScraper::new().with(&url_of(1), Ok(String::new()));
        let src = mock_book_source();

        let buf = init_buffer(1, novel_id, &chapters, &scraper, &src, &mut db)
            .await
            .expect("init ok with empty curr content");

        assert!(
            buf.combined_text.contains("（本章空白）"),
            "expected placeholder, got: {:?}",
            buf.combined_text
        );
        assert_eq!(buf.prev_end_row, 1);
        assert!(buf.curr_end_row > buf.prev_end_row);
    }

    // -----------------------------------------------------------------------
    // TASK-reader-buffer-02 — INT-viewport / INT-scroll / INT-jump /
    // INT-boundary-recursive-rebuild — wire-in behaviour.
    // -----------------------------------------------------------------------

    fn chapters_n(n: usize) -> Vec<ChapterMeta> {
        (0..n)
            .map(|i| ChapterMeta {
                index: i as i64,
                name: format!("第 {} 章", i + 1),
                url: format!("https://test.example/book/1/c{}", i),
            })
            .collect()
    }

    #[test]
    fn int_viewport_01_in_prev_region() {
        let buf = mk_buffer(Some(0), 1, Some(2), Some("A\nA2"), "B\nB2", Some("C"));
        // prev_end_row = 2; scroll 0/1 → prev region → returns 0.
        assert_eq!(viewport_top_chapter(&buf, 0), 0);
        assert_eq!(viewport_top_chapter(&buf, 1), 0);
    }

    #[test]
    fn int_viewport_02_in_curr_region() {
        let buf = mk_buffer(Some(0), 1, Some(2), Some("A"), "B\nB2", Some("C"));
        // prev_end_row=1, curr_end_row=1+1(sep)+2=4 → scroll in [1,4) → curr (1).
        assert_eq!(viewport_top_chapter(&buf, 1), 1);
        assert_eq!(viewport_top_chapter(&buf, 3), 1);
    }

    #[test]
    fn int_viewport_03_in_next_region() {
        let buf = mk_buffer(Some(0), 1, Some(2), Some("A"), "B", Some("C\nC2"));
        // prev_end_row=1, curr_end_row=1+1+1=3 → scroll ≥ 3 → next (2).
        assert_eq!(viewport_top_chapter(&buf, 3), 2);
        assert_eq!(viewport_top_chapter(&buf, 10), 2);

        // next=None → returns curr.
        let buf2 = mk_buffer(Some(0), 1, None, Some("A"), "B", None);
        assert_eq!(viewport_top_chapter(&buf2, 99), 1);
    }

    #[tokio::test]
    async fn int_scroll_01_past_next_end_triggers_rebuild() {
        let mut db = LibraryDb::open_in_memory().unwrap();
        let _novel_id = seed_novel_with_chapters(
            &mut db,
            &[Some("A"), Some("B"), Some("C"), Some("D")],
        );
        // mk_reader doesn't need DB for the buffer itself; pass real chapters
        // so try_rebuild_to can fetch via load_content_or_fetch cache hits.
        let chapters = chapters_n(4);
        let buf = mk_buffer(Some(0), 1, Some(2), Some("A"), "B", Some("C"));
        let mut reader = mk_reader(EntryMode::Menu, chapters, 1, buf);
        reader.novel_id = 1; // matches seeded novel

        // Buffer total rows = 5 ("A\n\nB\n\nC" = 5 lines). Set scroll to max.
        let total = buffer_total_rows(&reader.buffer);
        reader.scroll = total; // already at end

        // Scraper should be hit zero times because chapters 0..3 all cached.
        let scraper = MockScraper::new(); // no panic; allow calls but expect zero
        let prev_current = reader.current;

        reader.try_scroll(1, &scraper, &mut db).await;

        // current advanced (1 → 2) and buffer rebuilt around new curr.
        assert_eq!(
            reader.current,
            prev_current + 1,
            "scroll past next-end should advance current to next chapter"
        );
        assert_eq!(reader.buffer.curr_chapter_idx, 2);
        assert_eq!(reader.toc_list_state.selected(), Some(2));
        // scroll re-positioned to new buffer's prev_end_row (viewport on new curr).
        assert_eq!(reader.scroll, reader.buffer.prev_end_row);
    }

    #[tokio::test]
    async fn int_jump_01a_apply_jump_to_internal() {
        let mut db = LibraryDb::open_in_memory().unwrap();
        let _novel_id = seed_novel_with_chapters(
            &mut db,
            &[
                Some("c0"), Some("c1"), Some("c2"), Some("c3"),
                Some("c4"), Some("c5"), Some("c6"), Some("c7"),
            ],
        );
        let chapters = chapters_n(8);
        let buf = mk_buffer(Some(0), 1, Some(2), Some("c0"), "c1", Some("c2"));
        let mut reader = mk_reader(EntryMode::Menu, chapters, 1, buf);
        reader.novel_id = 1;

        let scraper = MockScraper::new();

        reader.try_rebuild_to(5, &scraper, &mut db).await;

        assert_eq!(reader.current, 5);
        assert_eq!(reader.buffer.curr_chapter_idx, 5);
        assert_eq!(reader.toc_list_state.selected(), Some(5));
        assert_eq!(reader.scroll, reader.buffer.prev_end_row);
        assert!(reader.toast.is_none(), "successful rebuild clears toast");
    }

    #[tokio::test]
    async fn int_jump_03_curr_failed_toast_no_state_change() {
        let mut db = LibraryDb::open_in_memory().unwrap();
        // Seed chapters 0..3; chapter 2 has NULL content → mock will Err on it.
        let _novel_id = seed_novel_with_chapters(
            &mut db,
            &[Some("c0"), Some("c1"), None, Some("c3")],
        );
        let chapters = chapters_n(4);
        let buf = mk_buffer(Some(0), 1, Some(2), Some("c0"), "c1", Some("c2"));
        let mut reader = mk_reader(EntryMode::Menu, chapters, 1, buf);
        reader.novel_id = 1;
        reader.scroll = 3; // arbitrary non-default

        // Snapshot pre-rebuild state.
        let prev_current = reader.current;
        let prev_scroll = reader.scroll;
        let prev_buffer_text = reader.buffer.combined_text.clone();

        let scraper = MockScraper::new()
            .with(&url_of(2), Err(anyhow!("boom")));

        reader.try_rebuild_to(2, &scraper, &mut db).await;

        // State must be untouched.
        assert_eq!(reader.current, prev_current);
        assert_eq!(reader.scroll, prev_scroll);
        assert_eq!(reader.buffer.combined_text, prev_buffer_text);

        // Toast must be set with the failed-chapter message + TTL.
        let toast = reader.toast.as_deref().expect("toast set on CurrFailed");
        assert!(
            toast.contains("失敗"),
            "toast should mention failure, got: {toast}"
        );
        assert!(
            reader.toast_expires_at.is_some(),
            "toast_expires_at must be set on CurrFailed"
        );
    }

    #[tokio::test]
    async fn int_boundary_recursive_rebuild_first_chapter() {
        let mut db = LibraryDb::open_in_memory().unwrap();
        let _novel_id =
            seed_novel_with_chapters(&mut db, &[Some("c0"), Some("c1"), Some("c2")]);
        let chapters = chapters_n(3);
        // current = 0, prev=None
        let buf = mk_buffer(None, 0, Some(1), None, "c0", Some("c1"));
        let mut reader = mk_reader(EntryMode::Menu, chapters, 0, buf);
        reader.novel_id = 1;
        reader.scroll = 0;

        let scraper = MockScraper::new().panic_on_call();

        // Scroll up — should NOT trigger rebuild (no prev), no fetch, no panic.
        reader.try_scroll(-1, &scraper, &mut db).await;
        assert_eq!(reader.scroll, 0, "scroll locked at 0");
        assert_eq!(reader.current, 0);
        assert_eq!(scraper.call_count(), 0);
        assert!(reader.toast.is_none());

        // Symmetric: last chapter, no next, scroll down.
        let last_buf = mk_buffer(Some(1), 2, None, Some("c1"), "c2", None);
        let mut reader2 = mk_reader(EntryMode::Menu, chapters_n(3), 2, last_buf);
        reader2.novel_id = 1;
        let total = buffer_total_rows(&reader2.buffer);
        reader2.scroll = total;
        let scraper2 = MockScraper::new().panic_on_call();
        reader2.try_scroll(1, &scraper2, &mut db).await;
        assert_eq!(reader2.current, 2, "current stays at last chapter");
        assert_eq!(scraper2.call_count(), 0);
        assert!(reader2.toast.is_none());
    }

    // -----------------------------------------------------------------------
    // TASK-reader-buffer-04 — INT-progress-01: progress_text free fn.
    // Verifies the bottom status-bar progress label produced by
    // `progress_text(buffer, scroll, total)` reads:
    //   "第 X 章 / 共 N 章 (Y%)"
    // where X = viewport_top_chapter(buffer, scroll) + 1 (1-based) and
    //       Y = X * 100 / N  (integer percent, clamped to 100).
    // Three scroll points (prev/curr/next region) exercise the viewport
    // dispatch; the no-prev edge case proves `viewport_top_chapter` is
    // the source of truth even at chapter 0.
    // -----------------------------------------------------------------------

    #[test]
    fn int_progress_01_text_format_in_three_regions() {
        // 3-chapter buffer: prev=ch4 (10 rows), curr=ch5 (10 rows), next=ch6.
        // prev_end_row = 10; curr_end_row = 10 + 1 (sep) + 10 = 21.
        let buf = ChapterBuffer {
            combined_text: "x".repeat(100),
            prev_chapter_idx: Some(4),
            curr_chapter_idx: 5,
            next_chapter_idx: Some(6),
            prev_end_row: 10,
            curr_end_row: 21,
        };
        let total = 100usize;

        // scroll=0 → prev region → viewport_top=4 → X=5, Y=5
        assert_eq!(
            progress_text(&buf, 0, total),
            "第 5 章 / 共 100 章 (5%)"
        );
        // scroll=15 → curr region → viewport_top=5 → X=6, Y=6
        assert_eq!(
            progress_text(&buf, 15, total),
            "第 6 章 / 共 100 章 (6%)"
        );
        // scroll=25 → next region → viewport_top=6 → X=7, Y=7
        assert_eq!(
            progress_text(&buf, 25, total),
            "第 7 章 / 共 100 章 (7%)"
        );
    }

    #[test]
    fn int_progress_01_first_chapter_no_prev() {
        // prev=None, curr=ch0; scroll=0 falls through prev gate → curr → X=1.
        let buf = ChapterBuffer {
            combined_text: "x".repeat(50),
            prev_chapter_idx: None,
            curr_chapter_idx: 0,
            next_chapter_idx: Some(1),
            prev_end_row: 0,
            curr_end_row: 10,
        };
        assert_eq!(
            progress_text(&buf, 0, 100),
            "第 1 章 / 共 100 章 (1%)"
        );
    }

    // -----------------------------------------------------------------------
    // TASK-reader-toc-01 — INT-toggle-01: `t` key toggles toc_collapsed.
    //
    // REQ-002 S1-S3: 預設展開 → 按 t 縮合 → 再按 t 回展開。
    // 只在 Normal mode 觸發 toggle（Filter mode 由 reader-toc-02 處理）。
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn int_toggle_01_t_key_toggles_toc_collapsed() {
        let mut reader = mock_reader_min(EntryMode::Menu);
        let mut ctx = test_ctx();

        assert!(!reader.toc_collapsed, "預設展開 (toc_collapsed=false)");

        // 按 t 一次 → collapsed
        let _ = reader
            .handle_event(press(KeyCode::Char('t')), &mut ctx)
            .await;
        assert!(reader.toc_collapsed, "按 t 後 collapsed=true");

        // 再按 t → 回展開
        let _ = reader
            .handle_event(press(KeyCode::Char('t')), &mut ctx)
            .await;
        assert!(!reader.toc_collapsed, "再按 t 回 collapsed=false");
    }

    // -----------------------------------------------------------------------
    // TASK-reader-toc-02 — INT-mode-01..04 + INT-toggle-02
    // REQ-003 fuzzy filter mode + REQ-002-S4 (Filter mode 't' append, no toggle).
    // -----------------------------------------------------------------------

    /// INT-mode-01: state transition Normal → Filter → Normal
    #[tokio::test]
    async fn int_mode_01_normal_to_filter_to_normal() {
        let mut reader = mock_reader_min(EntryMode::Menu);
        let mut ctx = test_ctx();
        assert!(matches!(reader.mode, ReaderMode::Normal));

        // 按 '/' → Filter
        let _ = reader
            .handle_event(press(KeyCode::Char('/')), &mut ctx)
            .await;
        assert!(matches!(reader.mode, ReaderMode::Filter { .. }));

        // 輸入字元 → query 增長
        let _ = reader
            .handle_event(press(KeyCode::Char('x')), &mut ctx)
            .await;
        if let ReaderMode::Filter { query, .. } = &reader.mode {
            assert_eq!(query, "x");
        } else {
            panic!("expected Filter");
        }

        // Esc → Normal
        let _ = reader.handle_event(press(KeyCode::Esc), &mut ctx).await;
        assert!(matches!(reader.mode, ReaderMode::Normal));
    }

    /// INT-mode-02: fuzzy filter basic correctness
    #[test]
    fn int_mode_02_apply_fuzzy_filter_basic() {
        let chapters = vec![
            ChapterMeta {
                index: 0,
                name: "第一章".into(),
                url: "".into(),
            },
            ChapterMeta {
                index: 1,
                name: "第二章 入魔".into(),
                url: "".into(),
            },
            ChapterMeta {
                index: 2,
                name: "第三章".into(),
                url: "".into(),
            },
        ];
        let result = apply_fuzzy_filter("入魔", &chapters);
        assert_eq!(result, vec![1]);
    }

    /// INT-mode-03: fuzzy CJK + digit match
    #[test]
    fn int_mode_03_cjk_match() {
        let chapters = vec![
            ChapterMeta {
                index: 0,
                name: "第123章 XXX".into(),
                url: "".into(),
            },
            ChapterMeta {
                index: 1,
                name: "第50章 入魔之路".into(),
                url: "".into(),
            },
        ];
        let r1 = apply_fuzzy_filter("123", &chapters);
        assert!(r1.contains(&0), "123 should match 第123章");
        let r2 = apply_fuzzy_filter("入魔", &chapters);
        assert!(r2.contains(&1), "入魔 should match 第50章 入魔之路");
    }

    /// INT-mode-04: Backspace 空 query no panic + filter mode 強制展開
    #[tokio::test]
    async fn int_mode_04_backspace_empty_and_force_expand() {
        let mut reader = mock_reader_min(EntryMode::Menu);
        let mut ctx = test_ctx();
        reader.toc_collapsed = true; // 預設展開、強設收合

        // '/' → toc_collapsed 強制 false
        let _ = reader
            .handle_event(press(KeyCode::Char('/')), &mut ctx)
            .await;
        assert!(!reader.toc_collapsed, "/ 強制展開 TOC");

        // Backspace 在空 query → no panic, query 仍空
        let _ = reader
            .handle_event(press(KeyCode::Backspace), &mut ctx)
            .await;
        if let ReaderMode::Filter { query, .. } = &reader.mode {
            assert_eq!(query, "");
        } else {
            panic!("still Filter");
        }

        // Esc → 退出 filter；toc_collapsed 不還原（保持 false）
        let _ = reader.handle_event(press(KeyCode::Esc), &mut ctx).await;
        assert!(!reader.toc_collapsed, "Esc 後 toc_collapsed 不還原");
    }

    /// INT-toggle-02: filter mode 't' 不 toggle, append to query
    #[tokio::test]
    async fn int_toggle_02_filter_mode_t_appends_query() {
        let mut reader = mock_reader_min(EntryMode::Menu);
        let mut ctx = test_ctx();
        let toc_before = reader.toc_collapsed;

        // 進 Filter
        let _ = reader
            .handle_event(press(KeyCode::Char('/')), &mut ctx)
            .await;

        // 在 Filter 按 't' → append query
        let _ = reader
            .handle_event(press(KeyCode::Char('t')), &mut ctx)
            .await;
        if let ReaderMode::Filter { query, .. } = &reader.mode {
            assert_eq!(query, "t", "Filter 模式下 t 進 query");
        } else {
            panic!("still Filter");
        }

        // toc_collapsed 不變
        assert_eq!(
            reader.toc_collapsed, toc_before,
            "Filter 模式下 t 不 toggle TOC"
        );
    }

    // -----------------------------------------------------------------------
    // TASK-reader-toc-03 — int_filter_03_* (REQ-003 S4 / S6 / S7 邊界 UT)
    //
    // Fast-path regression guards：toc-02 已 wire handle_filter_key Esc /
    // Backspace / Enter 三 arm；本組 UT 為「spec-met fast path」的獨立聚焦
    // 版本，鎖死未來退化。對應：
    //   - int_filter_03_backspace_empty_query_no_panic → REQ-003 S4 邊界
    //   - int_filter_03_esc_preserves_reader_state     → REQ-003 S6
    //   - int_filter_03_empty_filtered_enter_no_op     → REQ-003 S7
    // -----------------------------------------------------------------------

    /// REQ-003 S4 邊界：filter mode + query "" + Backspace ×N → query 仍 ""、
    /// mode 仍 Filter、不 panic。比 INT-mode-04 更聚焦（無 toc_collapsed 干擾）。
    #[tokio::test]
    async fn int_filter_03_backspace_empty_query_no_panic() {
        let mut reader = mock_reader_min(EntryMode::Menu);
        let mut ctx = test_ctx();

        // 進 Filter（query 從 "" 起步）
        let _ = reader
            .handle_event(press(KeyCode::Char('/')), &mut ctx)
            .await;
        assert!(matches!(reader.mode, ReaderMode::Filter { .. }));

        // Backspace × 5 都在空 query 上 — early return path
        for _ in 0..5 {
            let _ = reader
                .handle_event(press(KeyCode::Backspace), &mut ctx)
                .await;
        }

        if let ReaderMode::Filter { query, .. } = &reader.mode {
            assert_eq!(query, "", "query 仍為空");
        } else {
            panic!("應仍在 Filter mode");
        }
    }

    /// REQ-003 S6：Esc 退出 filter mode 後，reader.buffer / reader.scroll /
    /// reader.current 三項與進 filter 前完全相同（Esc 路徑只 mutate mode，
    /// 不可碰其他 state）。
    #[tokio::test]
    async fn int_filter_03_esc_preserves_reader_state() {
        let mut reader = mock_reader_min(EntryMode::Menu);
        let mut ctx = test_ctx();

        // Snapshot reader state before entering filter
        let buffer_before = reader.buffer.combined_text.clone();
        let scroll_before = reader.scroll;
        let current_before = reader.current;

        // 進 Filter、打幾個字
        let _ = reader
            .handle_event(press(KeyCode::Char('/')), &mut ctx)
            .await;
        let _ = reader
            .handle_event(press(KeyCode::Char('x')), &mut ctx)
            .await;
        let _ = reader
            .handle_event(press(KeyCode::Char('y')), &mut ctx)
            .await;
        let _ = reader
            .handle_event(press(KeyCode::Char('z')), &mut ctx)
            .await;

        // Esc → 退出 filter
        let _ = reader.handle_event(press(KeyCode::Esc), &mut ctx).await;

        // mode 回 Normal、reader 其他 state 不變
        assert!(matches!(reader.mode, ReaderMode::Normal), "Esc → Normal");
        assert_eq!(
            reader.buffer.combined_text, buffer_before,
            "buffer 不變"
        );
        assert_eq!(reader.scroll, scroll_before, "scroll 不變");
        assert_eq!(reader.current, current_before, "current 不變");
    }

    /// REQ-003 S7：filter mode + filtered_indices 為空 + Enter → no-op；
    /// mode 仍 Filter、reader.buffer/scroll/current 全部不變。
    ///
    /// 利用 mock_reader_min 的 single chapter "ch0"：query "qzzwxxv" 字元
    /// 與 "ch0" 完全不重疊 → SkimMatcherV2 一定不命中 → filtered_indices 空。
    #[tokio::test]
    async fn int_filter_03_empty_filtered_enter_no_op() {
        let mut reader = mock_reader_min(EntryMode::Menu);
        let mut ctx = test_ctx();

        let buffer_before = reader.buffer.combined_text.clone();
        let scroll_before = reader.scroll;
        let current_before = reader.current;

        // 進 Filter
        let _ = reader
            .handle_event(press(KeyCode::Char('/')), &mut ctx)
            .await;

        // 打一串與 "ch0" 不重疊的 ASCII → filter 結果保證空
        for c in "qzzwxxv".chars() {
            let _ = reader
                .handle_event(press(KeyCode::Char(c)), &mut ctx)
                .await;
        }

        // 確認 filtered_indices 確實空（precondition）
        if let ReaderMode::Filter {
            filtered_indices, ..
        } = &reader.mode
        {
            assert!(
                filtered_indices.is_empty(),
                "precondition: query 'qzzwxxv' 不該命中 'ch0'，filtered_indices={:?}",
                filtered_indices
            );
        } else {
            panic!("應在 Filter mode");
        }

        // Enter → S7 fast path：filtered_indices.is_empty() → target=None → 不跳不退
        let _ = reader.handle_event(press(KeyCode::Enter), &mut ctx).await;

        // mode 仍 Filter
        assert!(
            matches!(reader.mode, ReaderMode::Filter { .. }),
            "Enter on empty filter 不退出 Filter mode"
        );
        // reader state 不變（沒 jump、沒 rebuild）
        assert_eq!(
            reader.buffer.combined_text, buffer_before,
            "buffer 不變（沒 rebuild）"
        );
        assert_eq!(reader.scroll, scroll_before, "scroll 不變");
        assert_eq!(reader.current, current_before, "current 不變（沒跳章）");
    }

    /// TASK-reader-buffer-03 — INT-boundary-recursive-rebuild_last_chapter.
    ///
    /// Dedicated symmetric counterpart of `_first_chapter`. The first_chapter
    /// test bundles a brief symmetric check at its tail; this test is the
    /// standalone last-chapter regression guard, with deeper invariants:
    /// - buffer.next_chapter_idx must be None (precondition)
    /// - scroll attempts that would exceed buffer end must NOT call scraper
    /// - scroll must lock at max (not advance past, not wrap, not panic)
    /// - reader.current must stay pinned at last chapter
    /// - toast must remain None (B3 is silent boundary, no user notice)
    /// - multiple consecutive down-scrolls must all stay locked (idempotent)
    #[tokio::test]
    async fn int_boundary_recursive_rebuild_last_chapter() {
        let mut db = LibraryDb::open_in_memory().unwrap();
        let _novel_id =
            seed_novel_with_chapters(&mut db, &[Some("c0"), Some("c1"), Some("c2")]);
        let chapters = chapters_n(3);
        let last = chapters.len() - 1;

        // current = last, next=None (last chapter, 2-chapter buffer).
        let buf = mk_buffer(Some(1), 2, None, Some("c1"), "c2", None);
        assert_eq!(buf.next_chapter_idx, None, "precondition: no next chapter");

        let mut reader = mk_reader(EntryMode::Menu, chapters, last, buf);
        reader.novel_id = 1;

        // Push scroll near the buffer end (max_scroll = total).
        let total = buffer_total_rows(&reader.buffer);
        reader.scroll = total;

        // panic_on_call → any scraper invocation = test fail (proves no rebuild).
        let scraper = MockScraper::new().panic_on_call();

        // Single down-scroll at the boundary.
        reader.try_scroll(1, &scraper, &mut db).await;
        assert_eq!(reader.current, last, "current pinned at last chapter");
        assert_eq!(reader.buffer.curr_chapter_idx, 2, "buffer not rebuilt");
        assert_eq!(reader.buffer.next_chapter_idx, None, "still no next");
        assert!(
            reader.scroll <= total,
            "scroll must not exceed buffer total (got {}, max {})",
            reader.scroll,
            total
        );
        assert_eq!(scraper.call_count(), 0, "no fetch on B3 boundary");
        assert!(reader.toast.is_none(), "silent boundary, no toast");

        // Larger down-scroll step (PgDn-like) — still locked, still no panic.
        reader.try_scroll(10, &scraper, &mut db).await;
        assert_eq!(reader.current, last, "current still pinned after PgDn-step");
        assert_eq!(scraper.call_count(), 0, "still no fetch");

        // Idempotency: a third consecutive down-scroll must not flip state.
        let scroll_after = reader.scroll;
        reader.try_scroll(1, &scraper, &mut db).await;
        assert_eq!(reader.scroll, scroll_after, "lock is idempotent");
        assert_eq!(scraper.call_count(), 0);
    }

    // -----------------------------------------------------------------------
    // TASK-reader-mouse-01 — hit-test helpers + toc_width_cached round-trip.
    //
    // REQ-004 / REQ-005 基礎建設：純函數 hit_test_pane / hit_test_toc_row
    // + draw() 後 toc_width_cached 反映當前 toc_collapsed。
    // -----------------------------------------------------------------------

    /// INT-hit-01: column 落在 TOC pane 內 → Pane::Toc
    #[test]
    fn int_hit_01_pane_toc_when_column_inside_toc_width() {
        assert!(matches!(hit_test_pane(10, 30), Pane::Toc));
    }

    /// INT-hit-02: column 落在 content pane 內 → Pane::Content
    #[test]
    fn int_hit_02_pane_content_when_column_outside_toc_width() {
        assert!(matches!(hit_test_pane(50, 30), Pane::Content));
    }

    /// INT-hit-03: TOC collapsed (toc_width=0) → 整面 content
    #[test]
    fn int_hit_03_pane_content_when_toc_collapsed() {
        assert!(matches!(hit_test_pane(10, 0), Pane::Content));
        assert!(matches!(hit_test_pane(0, 0), Pane::Content));
    }

    /// boundary：column == toc_width 算 content (per design pseudocode
    /// `column >= toc_width`)
    #[test]
    fn int_hit_pane_boundary_column_equals_toc_width_is_content() {
        assert!(matches!(hit_test_pane(30, 30), Pane::Content));
    }

    /// hit_test_toc_row 純 fn — row 在 list 範圍內
    #[test]
    fn int_hit_04a_toc_row_in_range() {
        // row=2, list_offset=0, items_count=10 → Some(2)
        assert_eq!(hit_test_toc_row(2, 0, 10), Some(2));
    }

    /// hit_test_toc_row 純 fn — row 超出 list items_count
    #[test]
    fn int_hit_04b_toc_row_out_of_range_none() {
        // row=15, list_offset=0, items_count=10 → None
        assert_eq!(hit_test_toc_row(15, 0, 10), None);
    }

    // -----------------------------------------------------------------------
    // TASK-reader-mouse-02 — Wheel scroll handler (pane-aware speed).
    //
    // REQ-004 S1..S6：Mouse ScrollUp/ScrollDown 在 reader 內依 hit_test_pane
    // 分派 — TOC pane = ±1 章（觸發 rebuild）、content pane = ±3 行；
    // toc_collapsed → 全畫面 content；Filter mode + TOC pane → selected ±1
    // 在 filtered_indices 內（不改 reader.current、不 rebuild）。
    // -----------------------------------------------------------------------

    /// REQ-004 S1: Normal mode + content pane wheel down → scroll += 3 (no rebuild needed)
    #[tokio::test]
    async fn int_mouse_wheel_normal_content_pane_scroll_down_increments_3() {
        use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
        let mut db = LibraryDb::open_in_memory().unwrap();
        let _novel_id = seed_novel_with_chapters(
            &mut db,
            &[Some("A"), Some("B\nB2\nB3\nB4\nB5\nB6\nB7"), Some("C")],
        );
        let chapters = chapters_n(3);
        let buf = mk_buffer(
            Some(0),
            1,
            Some(2),
            Some("A"),
            "B\nB2\nB3\nB4\nB5\nB6\nB7",
            Some("C"),
        );
        let mut reader = mk_reader(EntryMode::Menu, chapters, 1, buf);
        reader.novel_id = 1;
        reader.toc_width_cached = 30;
        // Set scroll mid-curr so a +3 won't trigger rebuild.
        reader.scroll = reader.buffer.prev_end_row + 1;
        let scroll_before = reader.scroll;

        let scraper = MockScraper::new().panic_on_call();
        let me = MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 50,
            row: 5,
            modifiers: KeyModifiers::NONE,
        };
        reader.try_mouse_wheel(me, &scraper, &mut db).await;

        assert_eq!(
            reader.scroll,
            scroll_before + 3,
            "content pane ScrollDown should add 3 to scroll"
        );
    }

    /// REQ-004 S2: Normal mode + content pane wheel up → scroll -= 3 (min 0)
    #[tokio::test]
    async fn int_mouse_wheel_normal_content_pane_scroll_up_decrements_3() {
        use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
        let mut db = LibraryDb::open_in_memory().unwrap();
        let _novel_id = seed_novel_with_chapters(
            &mut db,
            &[Some("A"), Some("B\nB2\nB3\nB4\nB5\nB6\nB7"), Some("C")],
        );
        let chapters = chapters_n(3);
        let buf = mk_buffer(
            Some(0),
            1,
            Some(2),
            Some("A"),
            "B\nB2\nB3\nB4\nB5\nB6\nB7",
            Some("C"),
        );
        let mut reader = mk_reader(EntryMode::Menu, chapters, 1, buf);
        reader.novel_id = 1;
        reader.toc_width_cached = 30;
        reader.scroll = 10;
        let scroll_before = reader.scroll;

        let scraper = MockScraper::new().panic_on_call();
        let me = MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 50,
            row: 5,
            modifiers: KeyModifiers::NONE,
        };
        reader.try_mouse_wheel(me, &scraper, &mut db).await;

        assert_eq!(
            reader.scroll,
            scroll_before - 3,
            "content pane ScrollUp should subtract 3"
        );
    }

    /// REQ-004 S3: Normal mode + TOC pane wheel down → jump to next chapter (rebuild)
    #[tokio::test]
    async fn int_mouse_wheel_normal_toc_pane_scroll_down_jumps_next_chapter() {
        use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
        let mut db = LibraryDb::open_in_memory().unwrap();
        let _novel_id = seed_novel_with_chapters(
            &mut db,
            &[Some("A"), Some("B"), Some("C"), Some("D")],
        );
        let chapters = chapters_n(4);
        let buf = mk_buffer(Some(0), 1, Some(2), Some("A"), "B", Some("C"));
        let mut reader = mk_reader(EntryMode::Menu, chapters, 1, buf);
        reader.novel_id = 1;
        reader.toc_width_cached = 30;
        let curr_before = reader.current;

        // Cache hits — no scraper call.
        let scraper = MockScraper::new();
        let me = MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        };
        reader.try_mouse_wheel(me, &scraper, &mut db).await;

        assert_eq!(
            reader.current,
            curr_before + 1,
            "TOC pane ScrollDown should jump to next chapter"
        );
        assert_eq!(
            reader.buffer.curr_chapter_idx, 2,
            "buffer should be rebuilt around new curr (idx=2)"
        );
    }

    /// REQ-004 S4: Normal mode + TOC pane wheel up → jump to prev chapter (rebuild)
    #[tokio::test]
    async fn int_mouse_wheel_normal_toc_pane_scroll_up_jumps_prev_chapter() {
        use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
        let mut db = LibraryDb::open_in_memory().unwrap();
        let _novel_id = seed_novel_with_chapters(
            &mut db,
            &[Some("A"), Some("B"), Some("C"), Some("D")],
        );
        let chapters = chapters_n(4);
        let buf = mk_buffer(Some(1), 2, Some(3), Some("B"), "C", Some("D"));
        let mut reader = mk_reader(EntryMode::Menu, chapters, 2, buf);
        reader.novel_id = 1;
        reader.toc_width_cached = 30;
        let curr_before = reader.current;

        let scraper = MockScraper::new();
        let me = MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        };
        reader.try_mouse_wheel(me, &scraper, &mut db).await;

        assert_eq!(
            reader.current,
            curr_before - 1,
            "TOC pane ScrollUp should jump to prev chapter"
        );
        assert_eq!(
            reader.buffer.curr_chapter_idx, 1,
            "buffer should be rebuilt around new curr (idx=1)"
        );
    }

    /// REQ-004 S5: TOC collapsed → entire screen is content pane (wheel scrolls 3 rows even at col=5)
    #[tokio::test]
    async fn int_mouse_wheel_toc_collapsed_scrolls_content_everywhere() {
        use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
        let mut db = LibraryDb::open_in_memory().unwrap();
        let _novel_id = seed_novel_with_chapters(
            &mut db,
            &[Some("A"), Some("B\nB2\nB3\nB4\nB5\nB6\nB7"), Some("C")],
        );
        let chapters = chapters_n(3);
        let buf = mk_buffer(
            Some(0),
            1,
            Some(2),
            Some("A"),
            "B\nB2\nB3\nB4\nB5\nB6\nB7",
            Some("C"),
        );
        let mut reader = mk_reader(EntryMode::Menu, chapters, 1, buf);
        reader.novel_id = 1;
        reader.toc_collapsed = true;
        reader.toc_width_cached = 0;
        reader.scroll = reader.buffer.prev_end_row + 1;
        let curr_before = reader.current;
        let scroll_before = reader.scroll;

        let scraper = MockScraper::new().panic_on_call();
        // column=5 would be TOC pane if expanded, but collapsed → content
        let me = MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        };
        reader.try_mouse_wheel(me, &scraper, &mut db).await;

        assert_eq!(
            reader.current, curr_before,
            "toc_collapsed wheel must not jump chapter"
        );
        assert_eq!(
            reader.scroll,
            scroll_before + 3,
            "toc_collapsed wheel should scroll content +3"
        );
    }

    /// INT-mouse-filter-01: Filter mode + TOC pane wheel → selected ± 1 in
    /// filtered_indices; reader.current unchanged; no rebuild.
    #[tokio::test]
    async fn int_mouse_filter_01_filter_mode_toc_wheel_moves_selected() {
        use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
        let mut reader = mock_reader_min(EntryMode::Menu);
        // mock_reader_min has only 1 chapter; extend to 3 so filter has list moves.
        reader.chapters = chapters_n(3);
        reader.toc_list_state.select(Some(0));
        let mut ctx = test_ctx();

        reader.toc_width_cached = 30;
        // Enter Filter mode via '/'
        let _ = reader.handle_event(press(KeyCode::Char('/')), &mut ctx).await;
        // Empty query → filtered_indices = all (0..3)
        let current_before = reader.current;

        // ScrollDown @ column=5 (TOC pane in Filter mode)
        let me_down = MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        };
        let _ = reader.handle_event(Event::Mouse(me_down), &mut ctx).await;

        if let ReaderMode::Filter {
            selected,
            filtered_indices,
            ..
        } = &reader.mode
        {
            assert_eq!(filtered_indices.len(), 3, "all chapters in filtered_indices");
            assert_eq!(*selected, 1, "Filter mode TOC wheel down → selected = 1");
        } else {
            panic!("expected still Filter mode after wheel");
        }
        assert_eq!(
            reader.current, current_before,
            "Filter mode wheel must NOT change reader.current"
        );

        // ScrollUp → selected back to 0
        let me_up = MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        };
        let _ = reader.handle_event(Event::Mouse(me_up), &mut ctx).await;
        if let ReaderMode::Filter { selected, .. } = &reader.mode {
            assert_eq!(*selected, 0, "Filter mode TOC wheel up → selected = 0");
        } else {
            panic!("expected still Filter mode after wheel up");
        }
    }

    // -----------------------------------------------------------------------
    // TASK-reader-mouse-03 — Click TOC 跳章 (REQ-005 S1..S4)
    //
    // Click 入口從 mouse-02 catch-all 拆出；REQ-005:
    //   S1：Click TOC row → apply_jump_to dispatch (INT-jump-01b)
    //   S2：Click content pane → no-op
    //   S3：Click TOC blank row (row > items) → no-op
    //   S4：Filter mode + click → jump + 退 Filter
    //
    // List_offset = 1 (top border of TOC Block::default().borders(Borders::ALL)).
    // -----------------------------------------------------------------------

    /// INT-jump-01b: Click TOC row → apply_jump_to dispatch (REQ-005 S1)
    ///
    /// 不重複驗 jump 內部（INT-jump-01a 已涵蓋）— 只驗 click 入口確實 dispatch 到
    /// apply_jump_to，導致 reader.current 改變到 hit-test 命中之 chapter pos。
    #[tokio::test]
    async fn int_jump_01b_click_toc_row_dispatches_apply_jump_to() {
        use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind, MouseButton};
        let mut db = LibraryDb::open_in_memory().unwrap();
        let _novel_id = seed_novel_with_chapters(
            &mut db,
            &[Some("A"), Some("B"), Some("C"), Some("D"), Some("E")],
        );
        let chapters = chapters_n(5);
        let buf = mk_buffer(None, 0, Some(1), None, "A", Some("B"));
        let mut reader = mk_reader(EntryMode::Menu, chapters, 0, buf);
        reader.novel_id = 1;
        reader.toc_width_cached = 30;

        let curr_before = reader.current;
        assert_eq!(curr_before, 0, "precondition: reader starts at chapter 0");

        // list_offset = 1 (Block::default().borders(Borders::ALL) top border).
        // row = 3 → hit-test idx = 3 - 1 = 2 → target chapter pos = 2.
        let scraper = MockScraper::new(); // cache hits, no panic
        let me = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 5,
            row: 3,
            modifiers: KeyModifiers::NONE,
        };
        reader.try_mouse_wheel(me, &scraper, &mut db).await;

        assert_eq!(
            reader.current, 2,
            "click TOC row=3 (list_offset=1) → apply_jump_to(2) → reader.current=2"
        );
        assert_eq!(
            reader.buffer.curr_chapter_idx, 2,
            "buffer rebuilt around new curr"
        );
        assert_eq!(reader.toc_list_state.selected(), Some(2));
    }

    /// REQ-005 S2: Click on content pane → no-op (no jump, no scroll change).
    #[tokio::test]
    async fn int_mouse_click_content_pane_no_op() {
        use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind, MouseButton};
        let mut db = LibraryDb::open_in_memory().unwrap();
        let _novel_id = seed_novel_with_chapters(
            &mut db,
            &[Some("A"), Some("B"), Some("C")],
        );
        let chapters = chapters_n(3);
        let buf = mk_buffer(Some(0), 1, Some(2), Some("A"), "B", Some("C"));
        let mut reader = mk_reader(EntryMode::Menu, chapters, 1, buf);
        reader.novel_id = 1;
        reader.toc_width_cached = 30;

        let curr_before = reader.current;
        let scroll_before = reader.scroll;

        let scraper = MockScraper::new().panic_on_call();
        let me = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 50, // >= toc_width_cached=30 → content pane
            row: 5,
            modifiers: KeyModifiers::NONE,
        };
        reader.try_mouse_wheel(me, &scraper, &mut db).await;

        assert_eq!(reader.current, curr_before, "content pane click → no jump");
        assert_eq!(reader.scroll, scroll_before, "content pane click → no scroll change");
    }

    /// REQ-005 S3: Click TOC pane but row exceeds chapters.len() → no-op.
    #[tokio::test]
    async fn int_mouse_click_toc_blank_row_no_op() {
        use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind, MouseButton};
        let mut db = LibraryDb::open_in_memory().unwrap();
        let _novel_id = seed_novel_with_chapters(
            &mut db,
            &[Some("A"), Some("B"), Some("C")],
        );
        let chapters = chapters_n(3);
        let buf = mk_buffer(Some(0), 1, Some(2), Some("A"), "B", Some("C"));
        let mut reader = mk_reader(EntryMode::Menu, chapters, 1, buf);
        reader.novel_id = 1;
        reader.toc_width_cached = 30;

        let curr_before = reader.current;
        let scroll_before = reader.scroll;

        let scraper = MockScraper::new().panic_on_call();
        let me = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 5, // TOC pane
            row: 100,  // way beyond chapters.len()
            modifiers: KeyModifiers::NONE,
        };
        reader.try_mouse_wheel(me, &scraper, &mut db).await;

        assert_eq!(reader.current, curr_before, "blank row click → no jump");
        assert_eq!(reader.scroll, scroll_before, "blank row click → no scroll change");
    }

    /// REQ-005 S4: Filter mode + click TOC row → jump using filtered_indices[row]
    /// and exit Filter mode (mode=Normal, query cleared).
    #[tokio::test]
    async fn int_mouse_click_filter_mode_jumps_and_exits_filter() {
        use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind, MouseButton};
        let mut db = LibraryDb::open_in_memory().unwrap();
        let _novel_id = seed_novel_with_chapters(
            &mut db,
            &[Some("A"), Some("B"), Some("C"), Some("D"), Some("E")],
        );
        let chapters = chapters_n(5);
        let buf = mk_buffer(None, 0, Some(1), None, "A", Some("B"));
        let mut reader = mk_reader(EntryMode::Menu, chapters, 0, buf);
        reader.novel_id = 1;
        reader.toc_width_cached = 30;
        let mut ctx = test_ctx();
        // Seed ctx db with same content so apply_jump_to (which uses ctx.db) works.
        let _ = seed_novel_with_chapters(
            &mut ctx.db,
            &[Some("A"), Some("B"), Some("C"), Some("D"), Some("E")],
        );

        // Enter Filter mode (empty query → filtered_indices = (0..5))
        let _ = reader.handle_event(press(KeyCode::Char('/')), &mut ctx).await;
        assert!(matches!(reader.mode, ReaderMode::Filter { .. }));

        // Click row=3 → hit-test idx=2 → filtered_indices[2]=2 → jump to chapter 2
        let me = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 5,
            row: 3,
            modifiers: KeyModifiers::NONE,
        };
        let _ = reader.handle_event(Event::Mouse(me), &mut ctx).await;

        assert!(
            matches!(reader.mode, ReaderMode::Normal),
            "Filter mode + click → exit Filter back to Normal"
        );
        assert_eq!(
            reader.current, 2,
            "Filter mode click row=3 → filtered_indices[2]=2 → jump to chapter 2"
        );
    }

    /// INT-hit-04: ratatui TestBackend 實際跑 draw、驗 toc_width_cached
    /// round-trip（toc_collapsed=false → 24；toc_collapsed=true → 0）。
    #[tokio::test]
    async fn int_hit_04_toc_width_cached_after_draw() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut reader = mock_reader_min(EntryMode::Menu);
        let ctx = test_ctx();

        // 預設 toc_collapsed=false → toc_width = 80 * 30 / 100 = 24
        terminal
            .draw(|f| {
                reader.draw(f, &ctx);
            })
            .unwrap();
        assert_eq!(
            reader.toc_width_cached, 24,
            "toc_collapsed=false @ width 80 → 24"
        );

        // toggle collapsed → toc_width = 0
        reader.toc_collapsed = true;
        terminal
            .draw(|f| {
                reader.draw(f, &ctx);
            })
            .unwrap();
        assert_eq!(
            reader.toc_width_cached, 0,
            "toc_collapsed=true → 0"
        );
    }
}
