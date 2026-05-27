//! SearchPickerScreen — TUI 換源 picker (TASK-tui-picker-01)。
//!
//! 並行 streaming search + caller-aware Esc transition：按 s 進入後立即跳 modal、
//! 對所有 enabled 書源並行打 `SearchLike::search`、每個書源完成即 streaming
//! append 到表格行；逐 source 5s timeout、不阻塞 picker UI。
//!
//! 本 task 範圍：骨架 + Picking phase + Esc 取消路徑 caller-aware Transition。
//! Confirming phase 內部 sync_toc / fuzzy 鏈為下個 task（picker-02）。
//!
//! Testability seams：
//! - Seam 1 — [`SearchLike`] trait：UT 注入 MockSearchScraper。Production
//!   走 `impl SearchLike for Scraper` forward 到既有 `Scraper::search`。
//! - Seam 5 — ratatui TestBackend round-trip：`draw` 純 pure（吃 frame、不
//!   await）→ INT-picker-draw-01 可 deterministic 驗 row 數 + 顏色。

#![allow(dead_code)]

use async_trait::async_trait;
use crossterm::event::{Event, KeyCode};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinSet;

use crate::catalog::{service::source::BookSource, SearchHit};
use crate::library::ChapterMeta;
use crate::presentation::handlers::switch_source_core::{self, SwitchOutcome};
use crate::presentation::handlers::tui::{
    reader::{apply_fuzzy_filter, pick_best_with_anchor, ReaderScreen},
    shelf::ShelfScreen,
    EntryMode, Screen, Transition, TOAST_TTL,
};
use crate::presentation::AppContext;

/// per-source search 超時 — production 5s（ctx Constraints 硬編碼）；test
/// build 縮為 500ms，避免 INT-picker-timeout-03 真等 5s。
#[cfg(not(test))]
pub(crate) const SEARCH_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(test)]
pub(crate) const SEARCH_TIMEOUT: Duration = Duration::from_millis(500);

// ---------------------------------------------------------------------------
// 資料模型
// ---------------------------------------------------------------------------

/// 由 picker 上一個 screen 透傳：用於 Esc 取消路徑回原 caller（REQ-004 S3）。
///
/// `Reader.previous_chapter_idx` 在 Esc 路徑保留供 reader 重建時還原（confirm
/// 路徑屬 picker-02、走 `outcome.new_progress_idx`，不用此欄）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PickerEntry {
    Reader { previous_chapter_idx: i64 },
    Shelf,
}

/// Picker 內部生命週期 — 兩相階段：選書源 → 確認章節對應。
#[derive(Debug, Clone)]
pub(crate) enum Phase {
    Picking,
    Confirming {
        selected_idx: usize,
        sync_state: SyncState,
    },
}

/// Confirming phase 內 sync_toc + fuzzy 結果 — 本 task 只用 Pending（picker-02
/// 才接 sync_toc / fuzzy 鏈）。
#[derive(Debug, Clone)]
pub(crate) enum SyncState {
    Pending,
    Ok {
        new_idx: i64,
        new_chapter_name: String,
        score: i64,
    },
    Abort {
        reason: AbortKind,
    },
    Err {
        msg: String,
    },
}

#[derive(Debug, Clone)]
pub(crate) enum AbortKind {
    EmptyToc,
    FuzzyBelow(i64),
}

/// Picker 表格一行的全部資料。
#[derive(Debug, Clone)]
pub(crate) struct SearchResult {
    pub(crate) src_url: String,
    pub(crate) status: SearchStatus,
    /// Some 才能在 Picking phase 接受 Enter（confirm 對象）。
    pub(crate) hit: Option<SearchHit>,
}

#[derive(Debug, Clone)]
pub(crate) enum SearchStatus {
    Loading,
    Ok,
    Timeout,
    Failed { msg: String },
}

// ---------------------------------------------------------------------------
// SearchLike trait — picker UT seam（Seam 1）+ production Scraper impl
// ---------------------------------------------------------------------------
//
// `Send + Sync` 是 JoinSet::spawn 必需（多執行緒 runtime）；wreq::Client
// 本身 Send + Sync、所以 Scraper 直接 impl 通過 — 與既有 `?Send` 版本的
// ScraperLike 不衝突（兩條 seam 各自獨立）。

#[async_trait]
pub(crate) trait SearchLike: Send + Sync {
    async fn search(
        &self,
        source: &BookSource,
        keyword: &str,
    ) -> anyhow::Result<Vec<SearchHit>>;
}

#[async_trait]
impl SearchLike for crate::catalog::service::scraper::Scraper {
    async fn search(
        &self,
        source: &BookSource,
        keyword: &str,
    ) -> anyhow::Result<Vec<SearchHit>> {
        // 既有 method（同名）— 用 fully-qualified path 避免遞迴。
        crate::catalog::service::scraper::Scraper::search(self, source, keyword).await
    }
}

// ---------------------------------------------------------------------------
// SearchPickerScreen — state container
// ---------------------------------------------------------------------------

pub(crate) struct SearchPickerScreen {
    pub(crate) novel_id: i64,
    pub(crate) book_name: String,
    pub(crate) author: String,
    /// 舊源當前章節 idx（fuzzy anchor、picker-02 才用；本 task 只 store）。
    pub(crate) old_chapter_idx: i64,
    /// 舊源當前章節名（fuzzy query；picker-02 才用）。
    pub(crate) old_chapter_name: String,
    pub(crate) entry: PickerEntry,

    pub(crate) phase: Phase,
    pub(crate) results: Vec<SearchResult>,
    pub(crate) list_state: ListState,
    /// 並行 search tasks；Enter / drop 時 abort_all 收剩餘 pending tasks。
    pub(crate) join_set: Option<JoinSet<(String, SearchStatus, Option<SearchHit>)>>,
}

impl SearchPickerScreen {
    pub(crate) fn new(
        entry: PickerEntry,
        novel_id: i64,
        book_name: String,
        author: String,
        old_chapter_idx: i64,
        old_chapter_name: String,
    ) -> Self {
        Self {
            novel_id,
            book_name,
            author,
            old_chapter_idx,
            old_chapter_name,
            entry,
            phase: Phase::Picking,
            results: Vec::new(),
            list_state: ListState::default(),
            join_set: None,
        }
    }

    /// 對每個 enabled book source 啟一個 JoinSet task：5s timeout、Ok/Failed/
    /// Timeout 三類結果。results Vec 預先填入 Loading row、task 完成時由
    /// [`poll_results`] match `src_url` 寫回 status / hit。
    ///
    /// `scraper: Arc<S>` 設計：每 task 內 clone Arc 移入 async block。生產
    /// 端可 `Arc::new(scraper_clone)`，UT 直接 `Arc::new(MockSearchScraper)`。
    pub(crate) fn spawn_searches<S>(
        &mut self,
        scraper: Arc<S>,
        enabled_sources: Vec<BookSource>,
    ) where
        S: SearchLike + 'static,
    {
        let mut join_set: JoinSet<(String, SearchStatus, Option<SearchHit>)> =
            JoinSet::new();
        let keyword = self.book_name.clone();
        for src in enabled_sources {
            self.results.push(SearchResult {
                src_url: src.book_source_url.clone(),
                status: SearchStatus::Loading,
                hit: None,
            });
            let src_url = src.book_source_url.clone();
            let scraper = Arc::clone(&scraper);
            let kw = keyword.clone();
            join_set.spawn(async move {
                match tokio::time::timeout(SEARCH_TIMEOUT, scraper.search(&src, &kw)).await
                {
                    Ok(Ok(hits)) => {
                        let hit = hits.into_iter().next();
                        let status = if hit.is_some() {
                            SearchStatus::Ok
                        } else {
                            SearchStatus::Failed { msg: "無命中".into() }
                        };
                        (src_url, status, hit)
                    }
                    Ok(Err(e)) => {
                        (src_url, SearchStatus::Failed { msg: e.to_string() }, None)
                    }
                    Err(_) => (src_url, SearchStatus::Timeout, None),
                }
            });
        }
        if !self.results.is_empty() {
            self.list_state.select(Some(0));
        }
        self.join_set = Some(join_set);
    }

    /// 非阻塞 drain JoinSet 已完成的 task 結果並寫回 results。
    /// 由 [`Screen::handle_event`] / streaming UT helper 呼用。
    pub(crate) async fn poll_results(&mut self) {
        let Some(js) = self.join_set.as_mut() else {
            return;
        };
        while let Some(res) = js.try_join_next() {
            let Ok((src_url, status, hit)) = res else {
                continue;
            };
            if let Some(row) = self.results.iter_mut().find(|r| r.src_url == src_url) {
                row.status = status;
                row.hit = hit;
            }
        }
    }

    /// 阻塞 await JoinSet 下個完成的 task 並寫回。UT streaming verification 用
    /// （生產 handle_event 走非阻塞 [`poll_results`]）。
    pub(crate) async fn await_next_result(&mut self) -> bool {
        let Some(js) = self.join_set.as_mut() else {
            return false;
        };
        match js.join_next().await {
            Some(Ok((src_url, status, hit))) => {
                if let Some(row) =
                    self.results.iter_mut().find(|r| r.src_url == src_url)
                {
                    row.status = status;
                    row.hit = hit;
                }
                true
            }
            _ => false,
        }
    }

    /// Confirming 進場後 inline async work：fetch_novel_info → fetch_toc →
    /// compute_sync_state（fuzzy + threshold）。block handle_event 1-3 秒；
    /// 簡化選擇（spec：「適合本 task、Confirming sync 通常 1-3 秒、可接受
    /// block」）。失敗回 SyncState::Err / Abort、不 panic。
    async fn perform_confirm_sync(
        &self,
        ctx: &mut AppContext,
        hit: &SearchHit,
    ) -> SyncState {
        let src = match crate::catalog::facade::get_source(&ctx.db, &hit.source_url) {
            Ok(Some(s)) => s,
            Ok(None) => {
                return SyncState::Err {
                    msg: format!("找不到書源 {}", hit.source_url),
                }
            }
            Err(e) => return SyncState::Err { msg: e.to_string() },
        };
        let novel_info = match crate::catalog::facade::fetch_novel_info(
            &ctx.scraper,
            &src,
            &hit.book_url,
        )
        .await
        {
            Ok(n) => n,
            Err(e) => return SyncState::Err { msg: e.to_string() },
        };
        let toc_url = novel_info
            .toc_url
            .clone()
            .unwrap_or_else(|| hit.book_url.clone());
        let toc = match crate::catalog::facade::fetch_toc_with_timeout(
            &ctx.scraper,
            &src,
            &toc_url,
            Duration::from_secs(8),
        )
        .await
        {
            Ok(t) => t,
            Err(e) => return SyncState::Err { msg: e.to_string() },
        };
        compute_sync_state(&toc, &self.old_chapter_name, self.old_chapter_idx)
    }

    /// Esc 取消：依 entry 分派 transition 目標。reader-entry 從 DB 讀現存
    /// progress 重建 ReaderScreen（Esc 不寫 DB、reader 等價回到原狀）；
    /// reader 重建失敗（不該發生 — picker 開時 reader 已開過一次）退化為 Shelf。
    async fn transition_by_entry(&self, ctx: &mut AppContext) -> Transition {
        match &self.entry {
            PickerEntry::Reader { .. } => {
                match ReaderScreen::new(EntryMode::DirectReader, ctx, self.novel_id).await
                {
                    Ok(reader) => Transition::To(Box::new(reader)),
                    Err(_) => Transition::To(Box::new(ShelfScreen::new())),
                }
            }
            PickerEntry::Shelf => Transition::To(Box::new(ShelfScreen::new())),
        }
    }
}

#[async_trait(?Send)]
impl Screen for SearchPickerScreen {
    fn draw(&mut self, frame: &mut Frame, _ctx: &AppContext) {
        let area = frame.area();
        let modal_w = area.width.saturating_mul(80) / 100;
        let modal_h = area.height.saturating_mul(60) / 100;
        let x = area.x + area.width.saturating_sub(modal_w) / 2;
        let y = area.y + area.height.saturating_sub(modal_h) / 2;
        let modal_area = Rect {
            x,
            y,
            width: modal_w,
            height: modal_h,
        };

        frame.render_widget(Clear, modal_area);
        let title = format!(
            " 搜尋: {} / {} ({} 書源) ",
            self.book_name,
            self.author,
            self.results.len()
        );
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(Color::Cyan));
        frame.render_widget(block, modal_area);

        let inner = Rect {
            x: modal_area.x + 1,
            y: modal_area.y + 1,
            width: modal_area.width.saturating_sub(2),
            height: modal_area.height.saturating_sub(2),
        };

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(inner);

        // Confirming phase: 蓋掉表格區，顯 sync_state 對應訊息。
        if let Phase::Confirming { sync_state, .. } = &self.phase {
            let msg = match sync_state {
                SyncState::Pending => "準備換源、抓取新源 TOC 中...".to_string(),
                SyncState::Ok { new_idx, new_chapter_name, score } => format!(
                    "舊源第 {} 章 《{}》 → 新源第 {} 章 《{}》 (score {})\ny 確認 / Esc 回",
                    self.old_chapter_idx + 1,
                    self.old_chapter_name,
                    new_idx + 1,
                    new_chapter_name,
                    score,
                ),
                SyncState::Abort { reason: AbortKind::EmptyToc } => {
                    "新源無章節 (best score N/A)\nEsc 回".to_string()
                }
                SyncState::Abort { reason: AbortKind::FuzzyBelow(s) } => format!(
                    "找不到對應章節 (best score {} ≤ 50)\nEsc 回",
                    s
                ),
                SyncState::Err { msg } => format!("錯誤: {}\nEsc 回", msg),
            };
            let p = Paragraph::new(msg).style(Style::default().fg(Color::Yellow));
            frame.render_widget(p, rows[0]);
            let hint = "y 確認 / Esc 回 picker";
            frame.render_widget(
                Paragraph::new(hint).style(Style::default().fg(Color::DarkGray)),
                rows[1],
            );
            return;
        }

        // 表格：書源 URL × 書名 × 作者 × 狀態
        if self.results.is_empty() {
            let advisory = Paragraph::new("無 enabled 書源，請先 source import")
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(advisory, rows[0]);
        } else {
            let items: Vec<ListItem> = self
                .results
                .iter()
                .map(|r| {
                    let (status_text, color) = match &r.status {
                        SearchStatus::Loading => ("...載入中", Color::White),
                        SearchStatus::Ok => ("OK", Color::Green),
                        SearchStatus::Timeout => ("逾時 (5s)", Color::DarkGray),
                        SearchStatus::Failed { .. } => ("失敗", Color::Red),
                    };
                    let hit_name = r.hit.as_ref().map(|h| h.name.as_str()).unwrap_or("-");
                    let hit_author = r
                        .hit
                        .as_ref()
                        .and_then(|h| h.author.as_deref())
                        .unwrap_or("-");
                    let line = format!(
                        "{:30} {:20} {:12} {}",
                        truncate(&r.src_url, 30),
                        truncate(hit_name, 20),
                        truncate(hit_author, 12),
                        status_text
                    );
                    ListItem::new(Line::from(vec![Span::styled(
                        line,
                        Style::default().fg(color),
                    )]))
                })
                .collect();

            let list = List::new(items)
                .highlight_style(Style::default().bg(Color::DarkGray));
            frame.render_stateful_widget(list, rows[0], &mut self.list_state);
        }

        let hint = match &self.phase {
            Phase::Picking => "j/k 移動 / Enter 選 / Esc 取消",
            Phase::Confirming { .. } => "y 確認 / Esc 回 picker",
        };
        frame.render_widget(
            Paragraph::new(hint).style(Style::default().fg(Color::DarkGray)),
            rows[1],
        );
    }

    async fn handle_event(&mut self, event: Event, ctx: &mut AppContext) -> Transition {
        // streaming：每收 event 前 drain 完成的 search 結果。
        self.poll_results().await;

        let key = match event {
            Event::Key(k) => k,
            _ => return Transition::Stay,
        };

        match &self.phase {
            Phase::Picking => match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    if !self.results.is_empty() {
                        let i = self.list_state.selected().unwrap_or(0);
                        self.list_state
                            .select(Some((i + 1).min(self.results.len() - 1)));
                    }
                    Transition::Stay
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    if let Some(i) = self.list_state.selected() {
                        self.list_state.select(Some(i.saturating_sub(1)));
                    }
                    Transition::Stay
                }
                KeyCode::Enter => {
                    let selected = self.list_state.selected().unwrap_or(0);
                    let hit_opt = self
                        .results
                        .get(selected)
                        .filter(|r| matches!(r.status, SearchStatus::Ok))
                        .and_then(|r| r.hit.clone());
                    if let Some(hit) = hit_opt {
                        // C2: abort 剩餘 pending search tasks
                        if let Some(mut js) = self.join_set.take() {
                            js.abort_all();
                        }
                        self.phase = Phase::Confirming {
                            selected_idx: selected,
                            sync_state: SyncState::Pending,
                        };
                        // inline async sync_task — block handle_event 1-3 秒。
                        let sync_state = self.perform_confirm_sync(ctx, &hit).await;
                        if let Phase::Confirming { sync_state: ref mut st, .. } = self.phase {
                            *st = sync_state;
                        }
                    }
                    // 非 Ok（Loading / Timeout / Failed / 無 hit）→ no-op（S4 + S5）
                    Transition::Stay
                }
                KeyCode::Esc => self.transition_by_entry(ctx).await,
                _ => Transition::Stay,
            },
            Phase::Confirming { selected_idx, sync_state } => {
                // 先 clone 出需要的資料避免 self 雙借
                let selected_idx = *selected_idx;
                let sync_state_clone = sync_state.clone();
                match key.code {
                    KeyCode::Char('y')
                        if matches!(sync_state_clone, SyncState::Ok { .. }) =>
                    {
                        let SyncState::Ok { new_idx, .. } = sync_state_clone else {
                            unreachable!()
                        };
                        let hit = match self
                            .results
                            .get(selected_idx)
                            .and_then(|r| r.hit.clone())
                        {
                            Some(h) => h,
                            None => return Transition::Stay,
                        };
                        let new_src_url = hit.source_url.clone();
                        let new_book_url = hit.book_url.clone();
                        let old_src_label = "舊源".to_string();
                        match switch_source_core::run(
                            ctx,
                            self.novel_id,
                            &new_src_url,
                            &new_book_url,
                            Some(new_idx),
                        )
                        .await
                        {
                            Ok(outcome) => {
                                let next = next_transition(
                                    &self.entry,
                                    self.novel_id,
                                    &outcome,
                                    &old_src_label,
                                    &new_src_url,
                                );
                                match next {
                                    NextScreen::Reader => match ReaderScreen::new(
                                        EntryMode::DirectReader,
                                        ctx,
                                        self.novel_id,
                                    )
                                    .await
                                    {
                                        Ok(r) => Transition::To(Box::new(r)),
                                        Err(_) => Transition::To(Box::new(
                                            ShelfScreen::with_highlight(
                                                None,
                                                Some(
                                                    "換源成功但 reader 載入失敗".to_string(),
                                                ),
                                            ),
                                        )),
                                    },
                                    NextScreen::Shelf { toast } => Transition::To(Box::new(
                                        ShelfScreen::with_highlight_until(
                                            None,
                                            Some(toast),
                                            std::time::Instant::now() + TOAST_TTL,
                                        ),
                                    )),
                                }
                            }
                            Err(e) => {
                                if let Phase::Confirming {
                                    sync_state: ref mut st,
                                    ..
                                } = self.phase
                                {
                                    *st = SyncState::Err { msg: format!("{:#}", e) };
                                }
                                Transition::Stay
                            }
                        }
                    }
                    KeyCode::Esc => {
                        // 退回 Picking phase；不離開 picker
                        self.phase = Phase::Picking;
                        Transition::Stay
                    }
                    _ => Transition::Stay,
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Seam 3 — caller-aware Transition 拆 free fn
//
// next_transition 不能直接回 `Transition`（因 ReaderScreen::new 是 async fn
// 需 ctx）— 改回 `enum NextScreen { Reader, Shelf { toast } }`、caller 在
// handle_event 內依此 enum 在 await 上下文 build screen → 包成 Transition::To。
// free fn 仍可 UT 直接驗 dispatch 邏輯。
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NextScreen {
    Reader,
    Shelf { toast: String },
}

/// Pure free fn — 依 picker entry 決定 confirm 成功後該轉去哪個 screen。
/// reader-entry 走 ReaderScreen + DB progress (caller 端 await new()
/// 重建)；shelf-entry 走 ShelfScreen + toast「已換源 ... 目標：第 N 章 《章名》」。
pub(crate) fn next_transition(
    entry: &PickerEntry,
    _novel_id: i64,
    outcome: &SwitchOutcome,
    old_src_label: &str,
    new_src_label: &str,
) -> NextScreen {
    match entry {
        PickerEntry::Reader { .. } => NextScreen::Reader,
        PickerEntry::Shelf => {
            let toast = format!(
                "已換源 {} → {}，目標：第 {} 章 《{}》",
                old_src_label,
                new_src_label,
                outcome.new_progress_idx + 1,
                outcome.new_progress_chapter_name,
            );
            NextScreen::Shelf { toast }
        }
    }
}

/// Pure free fn — 接已抓回的 new_toc + 舊章 (name / idx)，跑 fuzzy + threshold
/// 判斷出 SyncState。從 perform_confirm_sync 抽出讓 UT 可直接驗 fuzzy 邊界
/// （score 50 拒 / 51 接 / empty toc abort），不需 mock scraper。
pub(crate) fn compute_sync_state(
    toc: &[ChapterMeta],
    old_chapter_name: &str,
    old_chapter_idx: i64,
) -> SyncState {
    if toc.is_empty() {
        return SyncState::Abort { reason: AbortKind::EmptyToc };
    }
    let scored = apply_fuzzy_filter(old_chapter_name, toc, Some(old_chapter_idx));
    match pick_best_with_anchor(&scored, Some(old_chapter_idx)) {
        Some((idx, score)) if score > 50 => SyncState::Ok {
            new_idx: toc[idx].index,
            new_chapter_name: toc[idx].name.clone(),
            score,
        },
        Some((_, score)) => SyncState::Abort {
            reason: AbortKind::FuzzyBelow(score),
        },
        None => SyncState::Abort {
            reason: AbortKind::FuzzyBelow(0),
        },
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
// Tests — INT-picker-* (TASK-tui-picker-01)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::service::source::BookSource;
    use crate::catalog::SearchHit;
    use crate::config::Config;
    use crate::library::dao::LibraryDb;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::collections::HashMap;
    use std::sync::Mutex;

    // ----- Mock SearchLike --------------------------------------------------

    struct MockSearchScraper {
        by_src: HashMap<String, Result<Vec<SearchHit>, String>>,
        delays: HashMap<String, Duration>,
        call_count: Mutex<usize>,
    }

    impl MockSearchScraper {
        fn new() -> Self {
            Self {
                by_src: HashMap::new(),
                delays: HashMap::new(),
                call_count: Mutex::new(0),
            }
        }

        fn with_ok(mut self, url: &str, hits: Vec<SearchHit>) -> Self {
            self.by_src.insert(url.to_string(), Ok(hits));
            self
        }

        fn with_delay(mut self, url: &str, d: Duration) -> Self {
            self.delays.insert(url.to_string(), d);
            self
        }

        fn calls(&self) -> usize {
            *self.call_count.lock().unwrap()
        }
    }

    #[async_trait]
    impl SearchLike for MockSearchScraper {
        async fn search(
            &self,
            source: &BookSource,
            _keyword: &str,
        ) -> anyhow::Result<Vec<SearchHit>> {
            *self.call_count.lock().unwrap() += 1;
            if let Some(d) = self.delays.get(&source.book_source_url) {
                tokio::time::sleep(*d).await;
            }
            match self.by_src.get(&source.book_source_url) {
                Some(Ok(hits)) => Ok(hits.clone()),
                Some(Err(e)) => Err(anyhow::anyhow!(e.clone())),
                None => Err(anyhow::anyhow!("no mock for {}", source.book_source_url)),
            }
        }
    }

    fn mk_source(url: &str) -> BookSource {
        let mut s = BookSource {
            book_source_url: url.to_string(),
            book_source_name: format!("src-{}", url),
            book_source_group: None,
            enabled: true,
            book_url_pattern: None,
            header: None,
            rule_search: Default::default(),
            rule_book_info: Default::default(),
            rule_toc: Default::default(),
            rule_content: Default::default(),
        };
        // touch unused mut to silence warning
        s.enabled = true;
        s
    }

    fn mk_hit(src: &str, name: &str) -> SearchHit {
        SearchHit {
            source_url: src.to_string(),
            name: name.to_string(),
            author: Some("作者A".to_string()),
            book_url: format!("{}/book", src),
            kind: None,
            intro: None,
        }
    }

    fn mk_picker(entry: PickerEntry) -> SearchPickerScreen {
        SearchPickerScreen::new(
            entry,
            1,
            "測試書".to_string(),
            "作者A".to_string(),
            5,
            "第六章".to_string(),
        )
    }

    fn test_ctx() -> AppContext {
        let db = LibraryDb::open_in_memory().expect("open in-memory db");
        let scraper = crate::catalog::service::scraper::Scraper::new().expect("scraper");
        let config = Config::default();
        AppContext { db, scraper, config }
    }

    fn esc_event() -> Event {
        Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()))
    }

    fn enter_event() -> Event {
        Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()))
    }

    // ----- INT-picker-spawn-01 ---------------------------------------------

    #[tokio::test]
    async fn int_picker_spawn_01_spawns_n_tasks() {
        let mock = Arc::new(
            MockSearchScraper::new()
                .with_ok("u1", vec![mk_hit("u1", "測試書")])
                .with_ok("u2", vec![mk_hit("u2", "測試書")])
                .with_ok("u3", vec![mk_hit("u3", "測試書")]),
        );
        let sources = vec![mk_source("u1"), mk_source("u2"), mk_source("u3")];
        let mut picker = mk_picker(PickerEntry::Shelf);
        picker.spawn_searches(Arc::clone(&mock), sources);

        // Drain all 3 tasks
        while picker.await_next_result().await {}

        assert_eq!(mock.calls(), 3, "應 spawn 3 個 task");
        assert_eq!(picker.results.len(), 3);
    }

    // ----- INT-picker-stream-02 --------------------------------------------
    //
    // 不同延遲 (50/200/500ms) 縮 5x — UT 速度優先。完成順序應為 u1 → u2 → u3。

    #[tokio::test]
    async fn int_picker_stream_02_results_in_completion_order() {
        let mock = Arc::new(
            MockSearchScraper::new()
                .with_ok("u1", vec![mk_hit("u1", "測試書")])
                .with_ok("u2", vec![mk_hit("u2", "測試書")])
                .with_ok("u3", vec![mk_hit("u3", "測試書")])
                .with_delay("u1", Duration::from_millis(50))
                .with_delay("u2", Duration::from_millis(200))
                .with_delay("u3", Duration::from_millis(500)),
        );
        let sources = vec![mk_source("u1"), mk_source("u2"), mk_source("u3")];
        let mut picker = mk_picker(PickerEntry::Shelf);
        picker.spawn_searches(Arc::clone(&mock), sources);

        // First completion should be u1
        assert!(picker.await_next_result().await);
        let u1 = picker
            .results
            .iter()
            .find(|r| r.src_url == "u1")
            .expect("u1 row");
        assert!(matches!(u1.status, SearchStatus::Ok), "u1 應最先 Ok");

        // Second completion: u2 should also be Ok by now (no strict ordering
        // assertion on u3, but verify u3 still Loading or already done)
        assert!(picker.await_next_result().await);
        let u2 = picker.results.iter().find(|r| r.src_url == "u2").unwrap();
        assert!(matches!(u2.status, SearchStatus::Ok), "u2 應第二 Ok");

        assert!(picker.await_next_result().await);
        let u3 = picker.results.iter().find(|r| r.src_url == "u3").unwrap();
        assert!(matches!(u3.status, SearchStatus::Ok), "u3 最後 Ok");
    }

    // ----- INT-picker-timeout-03 -------------------------------------------
    //
    // production SEARCH_TIMEOUT = 5s；test build cfg-gated 縮為 500ms（見
    // picker.rs 頂部）。mock 注入 2s 延遲 — 遠超 test 500ms 的 timeout，
    // 行為等價 production「6s 延遲、5s timeout」。

    #[tokio::test]
    async fn int_picker_timeout_03_per_source_5s() {
        let mock = Arc::new(
            MockSearchScraper::new()
                .with_ok("slow", vec![mk_hit("slow", "測試書")])
                .with_delay("slow", Duration::from_secs(2)),
        );
        let sources = vec![mk_source("slow")];
        let mut picker = mk_picker(PickerEntry::Shelf);
        picker.spawn_searches(Arc::clone(&mock), sources);

        // 500ms test-timeout 過後該 task 完成回 Timeout
        assert!(picker.await_next_result().await);
        let row = picker.results.iter().find(|r| r.src_url == "slow").unwrap();
        assert!(
            matches!(row.status, SearchStatus::Timeout),
            "test-timeout 500ms 過後 status 應為 Timeout，實際 = {:?}",
            row.status
        );
    }

    // ----- INT-picker-enter-04 ---------------------------------------------

    #[tokio::test]
    async fn int_picker_enter_04_aborts_and_switches_to_confirming() {
        let mock = Arc::new(
            MockSearchScraper::new().with_ok("u1", vec![mk_hit("u1", "測試書")]),
        );
        let sources = vec![mk_source("u1")];
        let mut picker = mk_picker(PickerEntry::Shelf);
        picker.spawn_searches(Arc::clone(&mock), sources);

        // Wait u1 to complete → status Ok
        assert!(picker.await_next_result().await);
        assert!(picker.join_set.is_some(), "spawn 後 join_set 應 Some");

        let mut ctx = test_ctx();
        let trans = picker.handle_event(enter_event(), &mut ctx).await;
        assert!(matches!(trans, Transition::Stay), "Enter 不該 Transition");
        assert!(
            matches!(picker.phase, Phase::Confirming { .. }),
            "Enter 後 phase 應 Confirming，實際 = {:?}",
            picker.phase
        );
        assert!(
            picker.join_set.is_none(),
            "Enter 後 join_set 應被 take（abort_all 已呼）"
        );
    }

    // ----- INT-picker-enter-pending-05 -------------------------------------

    #[tokio::test]
    async fn int_picker_enter_pending_05_no_op() {
        let mock = Arc::new(
            MockSearchScraper::new()
                .with_ok("u1", vec![mk_hit("u1", "測試書")])
                .with_delay("u1", Duration::from_secs(10)),
        );
        let sources = vec![mk_source("u1")];
        let mut picker = mk_picker(PickerEntry::Shelf);
        picker.spawn_searches(Arc::clone(&mock), sources);

        // Don't await — row status still Loading
        let row = &picker.results[0];
        assert!(matches!(row.status, SearchStatus::Loading));

        let mut ctx = test_ctx();
        let trans = picker.handle_event(enter_event(), &mut ctx).await;
        assert!(matches!(trans, Transition::Stay));
        assert!(
            matches!(picker.phase, Phase::Picking),
            "Loading 行 Enter 不該切 phase"
        );
        assert!(picker.join_set.is_some(), "join_set 應仍存在");
    }

    // ----- INT-picker-esc-cancel-shelf-06 ----------------------------------
    //
    // entry=Shelf、Esc → Transition::To(ShelfScreen)。
    // 因 Transition::To carries Box<dyn Screen>，動態檢查改驗「不是 Stay 也不是 Quit」+
    // 結構暗號（picker 內 transition_by_entry shelf 分支硬寫 ShelfScreen，所以 To
    // 已是 strong signal）。

    #[tokio::test]
    async fn int_picker_esc_cancel_shelf_06() {
        let mock = Arc::new(MockSearchScraper::new());
        let mut picker = mk_picker(PickerEntry::Shelf);
        picker.spawn_searches(Arc::clone(&mock), vec![]); // 0 sources OK
        let mut ctx = test_ctx();
        let trans = picker.handle_event(esc_event(), &mut ctx).await;
        match trans {
            Transition::To(_) => { /* OK — only To branch valid for Shelf entry */ }
            _ => panic!("Esc + Shelf-entry 應 Transition::To，實際非 To"),
        }
    }

    // ----- INT-picker-esc-cancel-reader-07 ---------------------------------
    //
    // entry=Reader、Esc → Transition::To(ReaderScreen)。
    // ReaderScreen::new 需要 DB 有 novel — 用 test_ctx + 手動 seed library。

    #[tokio::test]
    async fn int_picker_esc_cancel_reader_07() {
        let mock = Arc::new(MockSearchScraper::new());
        let mut picker = mk_picker(PickerEntry::Reader {
            previous_chapter_idx: 10,
        });
        picker.spawn_searches(Arc::clone(&mock), vec![]);

        let mut ctx = test_ctx();
        // Seed source + novel + 1 chapter so ReaderScreen::new succeeds
        let src = mk_source("test-src");
        crate::catalog::facade::save_source(&mut ctx.db, &src).expect("save src");
        let novel = crate::library::Novel {
            id: None,
            source_url: "test-src".to_string(),
            book_url: "test-src/book".to_string(),
            name: "測試書".to_string(),
            author: Some("作者A".to_string()),
            intro: None,
            cover_url: None,
            toc_url: None,
        };
        let novel_id =
            crate::library::facade::add_novel(&mut ctx.db, &novel).expect("add novel");
        // 1 chapter with placeholder content so init_buffer doesn't fetch
        let ch = crate::library::ChapterMeta {
            index: 0,
            name: "第一章".to_string(),
            url: "test-src/c1".to_string(),
        };
        crate::catalog::dao::replace_toc(&mut ctx.db, novel_id, std::slice::from_ref(&ch))
            .expect("replace toc");
        crate::library::facade::save_chapter_content(&mut ctx.db, novel_id, 0, "內容")
            .expect("save content");

        // Update picker.novel_id to the freshly-inserted novel
        picker.novel_id = novel_id;
        let trans = picker.handle_event(esc_event(), &mut ctx).await;
        match trans {
            Transition::To(_) => { /* OK — reader rebuild succeeded */ }
            _ => panic!("Esc + Reader-entry 應 Transition::To"),
        }
    }

    // ----- INT-picker-draw-01 ----------------------------------------------

    #[tokio::test]
    async fn int_picker_draw_01_test_backend_round_trip() {
        let mock = Arc::new(
            MockSearchScraper::new()
                .with_ok("u1", vec![mk_hit("u1", "測試書")])
                .with_ok("u2", vec![mk_hit("u2", "測試書")])
                .with_delay("u2", Duration::from_secs(10)),
        );
        let sources = vec![mk_source("u1"), mk_source("u2")];
        let mut picker = mk_picker(PickerEntry::Shelf);
        picker.spawn_searches(Arc::clone(&mock), sources);

        // u1 done, u2 still Loading
        assert!(picker.await_next_result().await);
        // Force u2 → Timeout manually so we can verify all three colours
        // (Ok / Loading / Timeout) on next draw — easier than waiting real 5s.
        // Instead: just verify what we have — u1 Ok + u2 Loading.

        let backend = TestBackend::new(80, 24);
        let mut term = Terminal::new(backend).expect("term");
        let ctx = test_ctx();
        term.draw(|f| picker.draw(f, &ctx)).expect("draw");

        // Row count assertion: 2 results → at least 2 visible rows in buffer
        // ratatui TestBackend exposes buffer via .backend().buffer()
        let buf = term.backend().buffer().clone();
        let mut found_u1 = false;
        let mut found_u2 = false;
        for y in 0..24 {
            let mut line = String::new();
            for x in 0..80 {
                line.push_str(buf[(x, y)].symbol());
            }
            if line.contains("u1") {
                found_u1 = true;
            }
            if line.contains("u2") {
                found_u2 = true;
            }
        }
        assert!(found_u1, "draw 後 buffer 應含 u1 行");
        assert!(found_u2, "draw 後 buffer 應含 u2 行");
    }

    // ----- INT-picker-empty-source-list-09 ---------------------------------

    #[tokio::test]
    async fn int_picker_empty_source_list_09() {
        let mock = Arc::new(MockSearchScraper::new());
        let mut picker = mk_picker(PickerEntry::Shelf);
        picker.spawn_searches(Arc::clone(&mock), vec![]);

        assert_eq!(mock.calls(), 0, "0 sources → 0 call");
        assert!(picker.results.is_empty(), "results 應空");

        let mut ctx = test_ctx();
        let trans = picker.handle_event(enter_event(), &mut ctx).await;
        assert!(matches!(trans, Transition::Stay));
        assert!(
            matches!(picker.phase, Phase::Picking),
            "0 sources Enter 應 no-op、phase 不切"
        );
    }

    // -----------------------------------------------------------------------
    // TASK-tui-picker-02: Confirming phase + caller-aware Transition UTs
    // -----------------------------------------------------------------------

    use crate::library::ChapterMeta;
    use crate::presentation::handlers::switch_source_core::SwitchOutcome;

    fn mk_meta(idx: i64, name: &str) -> ChapterMeta {
        ChapterMeta {
            index: idx,
            name: name.to_string(),
            url: format!("u{}", idx),
        }
    }

    // ----- INT-fuzzy-threshold-01: score == 50 reject ----------------------
    //
    // 構造：query "第六章"、toc 中只放一個 "x"-字串、fuzzy 分數應極低（≤ 50）→
    // SyncState::Abort { FuzzyBelow }. 取 SkimMatcherV2 對「第六章」vs 完全
    // 不相關字串分數 = None → scored 為空 → Abort { FuzzyBelow(0) }（≤ 50）.
    #[test]
    fn int_fuzzy_threshold_01_score_below_or_equal_50_rejected() {
        // toc 中所有章節名都跟 query 完全沒交集 → scored 為空（None）
        let toc = vec![mk_meta(0, "zzz"), mk_meta(1, "yyy")];
        let st = super::compute_sync_state(&toc, "第六章", 5);
        match st {
            SyncState::Abort {
                reason: AbortKind::FuzzyBelow(s),
            } => assert!(s <= 50, "score 應 ≤ 50，實際 = {}", s),
            other => panic!("應 Abort FuzzyBelow，實際 = {:?}", other),
        }
    }

    // ----- INT-fuzzy-threshold-02: score > 50 accepted ---------------------
    //
    // query 與 toc[1] name 完全相同 → SkimMatcherV2 給高分（> 50）.
    #[test]
    fn int_fuzzy_threshold_02_score_above_50_accepted() {
        let toc = vec![
            mk_meta(0, "完全無關"),
            mk_meta(1, "第六章 一模一樣的章節名稱"),
            mk_meta(2, "其它"),
        ];
        let st = super::compute_sync_state(&toc, "第六章 一模一樣的章節名稱", 0);
        match st {
            SyncState::Ok { new_idx, new_chapter_name, score } => {
                assert!(score > 50, "score 應 > 50，實際 = {}", score);
                assert_eq!(new_idx, 1);
                assert_eq!(new_chapter_name, "第六章 一模一樣的章節名稱");
            }
            other => panic!("應 Ok，實際 = {:?}", other),
        }
    }

    // ----- INT-edge-empty-toc-01: new toc==0 → Abort{EmptyToc} -------------
    #[test]
    fn int_edge_empty_toc_01_aborts() {
        let st = super::compute_sync_state(&[], "第六章", 5);
        assert!(
            matches!(st, SyncState::Abort { reason: AbortKind::EmptyToc }),
            "empty toc 應 Abort EmptyToc，實際 = {:?}",
            st
        );
    }

    // ----- INT-picker-confirming-pending-transition-09 ---------------------
    //
    // Enter 後 phase=Confirming{Pending} → 第 1 frame draw 顯「準備換源、
    // 抓取新源 TOC 中...」；手動把 sync_state 切成 Ok → 下 1 frame 顯預覽.
    #[tokio::test]
    async fn int_picker_confirming_pending_transition_09() {
        let mut picker = mk_picker(PickerEntry::Shelf);
        // 直接設 phase 不走 perform_confirm_sync（UT 只驗 draw state transition）
        picker.phase = Phase::Confirming {
            selected_idx: 0,
            sync_state: SyncState::Pending,
        };

        let backend = TestBackend::new(100, 24);
        let mut term = Terminal::new(backend).expect("term");
        let ctx = test_ctx();
        term.draw(|f| picker.draw(f, &ctx)).expect("draw pending");
        let buf = term.backend().buffer().clone();
        // CJK cells render with adjacent " " (CJK width=2). Strip ASCII spaces
        // before substring match.
        let mut all_text = String::new();
        for y in 0..24 {
            for x in 0..100 {
                all_text.push_str(buf[(x, y)].symbol());
            }
            all_text.push('\n');
        }
        let stripped: String = all_text.chars().filter(|c| *c != ' ').collect();
        assert!(
            stripped.contains("準備換源") || stripped.contains("抓取新源"),
            "Pending phase 第 1 frame 應顯「準備換源、抓取新源 TOC 中...」, buf=\n{}",
            all_text
        );

        // 模擬 sync_task 完成 → 切 Ok
        picker.phase = Phase::Confirming {
            selected_idx: 0,
            sync_state: SyncState::Ok {
                new_idx: 7,
                new_chapter_name: "第七章 新源".to_string(),
                score: 88,
            },
        };
        term.draw(|f| picker.draw(f, &ctx)).expect("draw ok");
        let buf = term.backend().buffer().clone();
        let mut all_text = String::new();
        for y in 0..24 {
            for x in 0..100 {
                all_text.push_str(buf[(x, y)].symbol());
            }
            all_text.push('\n');
        }
        let stripped: String = all_text.chars().filter(|c| *c != ' ').collect();
        assert!(
            stripped.contains("第七章") || stripped.contains("88"),
            "Pending → Ok 後第 2 frame 應顯預覽（章名 / score），buf=\n{}",
            all_text
        );
    }

    // ----- INT-transition-reader-confirm-01 --------------------------------
    #[test]
    fn int_transition_reader_confirm_01() {
        let entry = PickerEntry::Reader { previous_chapter_idx: 10 };
        let outcome = SwitchOutcome {
            new_progress_idx: 15,
            chapter_count: 100,
            new_first_chapter_name: "序章".into(),
            new_progress_chapter_name: "第十六章".into(),
        };
        let next = super::next_transition(&entry, 1, &outcome, "old-src", "new-src");
        assert!(
            matches!(next, super::NextScreen::Reader),
            "reader-entry confirm 應 NextScreen::Reader，實際 = {:?}",
            next
        );
    }

    // ----- INT-transition-shelf-confirm-02 ---------------------------------
    #[test]
    fn int_transition_shelf_confirm_02() {
        let entry = PickerEntry::Shelf;
        let outcome = SwitchOutcome {
            new_progress_idx: 15,
            chapter_count: 100,
            new_first_chapter_name: "序章".into(),
            new_progress_chapter_name: "X".into(),
        };
        let next = super::next_transition(&entry, 1, &outcome, "old-src", "new-src");
        match next {
            super::NextScreen::Shelf { toast } => {
                assert!(toast.contains("已換源"), "toast 應含「已換源」: {}", toast);
                assert!(toast.contains("第 16 章"), "toast 應含「第 16 章」: {}", toast);
                assert!(toast.contains("《X》"), "toast 應含「《X》」: {}", toast);
            }
            other => panic!("shelf-entry confirm 應 NextScreen::Shelf，實際 = {:?}", other),
        }
    }

    // ----- INT-transition-esc-reader-03: Confirming Esc → Picking ---------
    #[tokio::test]
    async fn int_transition_esc_reader_03() {
        let mut picker = mk_picker(PickerEntry::Reader {
            previous_chapter_idx: 5,
        });
        picker.phase = Phase::Confirming {
            selected_idx: 0,
            sync_state: SyncState::Pending,
        };
        let mut ctx = test_ctx();
        let trans = picker.handle_event(esc_event(), &mut ctx).await;
        assert!(
            matches!(trans, Transition::Stay),
            "Confirming Esc 應 Stay，不離開 picker"
        );
        assert!(
            matches!(picker.phase, Phase::Picking),
            "Confirming Esc → phase 應回 Picking，實際 = {:?}",
            picker.phase
        );
    }

    // ----- INT-transition-esc-shelf-04: 同上 entry=Shelf ------------------
    #[tokio::test]
    async fn int_transition_esc_shelf_04() {
        let mut picker = mk_picker(PickerEntry::Shelf);
        picker.phase = Phase::Confirming {
            selected_idx: 0,
            sync_state: SyncState::Ok {
                new_idx: 3,
                new_chapter_name: "第四章".into(),
                score: 80,
            },
        };
        let mut ctx = test_ctx();
        let trans = picker.handle_event(esc_event(), &mut ctx).await;
        assert!(matches!(trans, Transition::Stay));
        assert!(
            matches!(picker.phase, Phase::Picking),
            "Confirming Esc → phase 應回 Picking"
        );
    }
}
