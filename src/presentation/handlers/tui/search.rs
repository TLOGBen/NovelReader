//! SearchScreen — 跨源搜尋蒐書 funnel + 重複入架偵測（REQ-003 / REQ-004）。
//!
//! 流程：
//! 1. Input：`SingleLineInput` 收關鍵字；Enter → 進入逐源序列查；Esc → 回主菜單。
//! 2. 搜尋：列舉所有 `enabled` 書源 → 對每源跑 `scraper.search` with
//!    `per_source = max(2s, remaining/remaining_count)`；總 deadline 15s；單源
//!    timeout / Err 各記下一行狀態列；總 deadline 到時剩下源全標「未查」、break。
//! 3. Results：列出（書名 / 作者 / [來源]）和「源 X 錯誤」/「源 Y 逾時」/
//!    「源 Z 未查」狀態行。
//! 4. Enter on hit：先查 `library::facade::get_novel_by_book_url`；
//!    - Some → `Transition::To(ShelfScreen::with_highlight(book_url, "已在書架第 N 本"))`，
//!      **不** UPSERT 任何欄位。
//!    - None → `catalog::facade::fetch_novel_info` + `library::facade::add_novel`
//!      → `Transition::To(MenuScreen::with_toast("已入架 #ID 書名"))`。
//!
//! Out-of-scope（spec 已宣告）：
//! - 真正的 per-source redraw（受 Screen trait 簽名所限：handle_event 內 await
//!   只在 source 之間更新 state，下一輪 run_loop tick 才會 redraw — 與 spec
//!   `從_user_prompt_實作取捨` 一致）。
//! - 多源結果摺疊 / dedup（同一本書在 N 源出現顯示 N 行）。

use std::time::{Duration, Instant};

use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::catalog::{self, SearchHit};
use crate::library;
use crate::presentation::handlers::tui::{
    widgets::{SingleLineEvent, SingleLineInput},
    Screen, Transition,
};
use crate::presentation::AppContext;

/// 搜尋結果一行：要嘛是命中、要嘛是狀態訊息（錯誤 / 逾時 / 未查）。
#[allow(dead_code)]
pub enum HitOrStatus {
    /// 一個書源的命中；`source_name` 由 SearchScreen 從 BookSource 帶入
    /// （SearchHit 本身不含 source_name —— Catalog PL 不亂改）。
    Hit { hit: SearchHit, source_name: String },
    /// 狀態訊息列（紅字）：「源 X：錯誤」/「源 Y：逾時」/「源 Z 未查（時間預算用盡）」。
    StatusLine(String),
}

#[allow(dead_code)]
pub enum SearchState {
    Input(SingleLineInput),
    Results {
        rows: Vec<HitOrStatus>,
        list_state: ListState,
    },
}

#[allow(dead_code)]
pub struct SearchScreen {
    state: SearchState,
    /// 搜尋過程中的 progress；目前只在 Results 切換瞬間被覆寫，留欄位給未來 mpsc 改造。
    progress: Option<String>,
}

impl SearchScreen {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            state: SearchState::Input(SingleLineInput::new(" 關鍵字（Enter 搜尋、Esc 取消）")),
            progress: None,
        }
    }

    fn append_status(&mut self, msg: String) {
        if let SearchState::Results { rows, .. } = &mut self.state {
            rows.push(HitOrStatus::StatusLine(msg));
        }
    }
}

impl Default for SearchScreen {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl Screen for SearchScreen {
    fn draw(&mut self, frame: &mut Frame, _ctx: &AppContext) {
        let area = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // 輸入框 / 進度條
                Constraint::Min(3),    // 結果列
                Constraint::Length(1), // hint
            ])
            .split(area);

        match &mut self.state {
            SearchState::Input(input) => {
                if let Some(prog) = &self.progress {
                    let p = Paragraph::new(prog.as_str())
                        .style(Style::default().fg(Color::Yellow))
                        .block(Block::default().borders(Borders::ALL).title(" 搜尋中 "));
                    frame.render_widget(p, chunks[0]);
                } else {
                    input.draw(frame, chunks[0]);
                }
                let hint = Paragraph::new(" Enter 開始搜尋  Esc 回主菜單 ")
                    .style(Style::default().fg(Color::DarkGray));
                frame.render_widget(hint, chunks[2]);
            }
            SearchState::Results { rows, list_state } => {
                let items: Vec<ListItem> = rows
                    .iter()
                    .map(|r| match r {
                        HitOrStatus::Hit { hit, source_name } => ListItem::new(format!(
                            "{} / {} [{}]",
                            hit.name,
                            hit.author.as_deref().unwrap_or("-"),
                            source_name
                        )),
                        HitOrStatus::StatusLine(s) => ListItem::new(s.as_str())
                            .style(Style::default().fg(Color::Red)),
                    })
                    .collect();
                let list = List::new(items)
                    .block(Block::default().borders(Borders::ALL).title(" 搜尋結果 "))
                    .highlight_style(Style::default().fg(Color::Yellow));
                frame.render_stateful_widget(list, chunks[1], list_state);
                let hint = Paragraph::new(" j/k 移動  Enter 加入書架  Esc 回主菜單 ")
                    .style(Style::default().fg(Color::DarkGray));
                frame.render_widget(hint, chunks[2]);
            }
        }
    }

    async fn handle_event(&mut self, key: KeyEvent, ctx: &mut AppContext) -> Transition {
        // 任意鍵清 progress（舊狀態訊息不殘留）
        self.progress = None;

        // Take ownership of state to allow swap.
        match &mut self.state {
            SearchState::Input(input) => match input.handle_event(key) {
                SingleLineEvent::Cancel => Transition::To(Box::new(
                    crate::presentation::handlers::tui::menu::MenuScreen::new(),
                )),
                SingleLineEvent::Submit(keyword) => {
                    let kw = keyword.trim().to_string();
                    if kw.is_empty() {
                        return Transition::Stay;
                    }
                    let rows = do_search(ctx, &kw).await;
                    let mut list_state = ListState::default();
                    // 把第一個 Hit 預選；若全是 StatusLine 則選 0（仍能 j/k 移動）。
                    let first_hit_idx = rows
                        .iter()
                        .position(|r| matches!(r, HitOrStatus::Hit { .. }))
                        .unwrap_or(0);
                    if !rows.is_empty() {
                        list_state.select(Some(first_hit_idx));
                    }
                    self.state = SearchState::Results { rows, list_state };
                    Transition::Stay
                }
                SingleLineEvent::Edit => Transition::Stay,
            },
            SearchState::Results { rows, list_state } => match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    let i = list_state.selected().unwrap_or(0);
                    let next = (i + 1).min(rows.len().saturating_sub(1));
                    list_state.select(Some(next));
                    Transition::Stay
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    let i = list_state.selected().unwrap_or(0);
                    list_state.select(Some(i.saturating_sub(1)));
                    Transition::Stay
                }
                KeyCode::Enter => {
                    // Extract selected hit data (cloned) so we can release the
                    // &mut borrow on self.state before calling helpers / facades.
                    let selected_hit = list_state.selected().and_then(|i| {
                        rows.get(i).and_then(|r| match r {
                            HitOrStatus::Hit { hit, .. } => Some(hit.clone()),
                            HitOrStatus::StatusLine(_) => None,
                        })
                    });
                    let Some(hit) = selected_hit else {
                        return Transition::Stay;
                    };
                    handle_enter_on_hit(self, ctx, hit).await
                }
                KeyCode::Esc => Transition::To(Box::new(
                    crate::presentation::handlers::tui::menu::MenuScreen::new(),
                )),
                _ => Transition::Stay,
            },
        }
    }
}

// ----------------------------------------------------------------------------
// Search funnel core
// ----------------------------------------------------------------------------

/// 逐源序列查 — 全局 15s deadline + 單源 timeout（per_source = remaining /
/// remaining_count，下限 2s）。單源錯誤 / timeout 不中斷整體 funnel。
async fn do_search(ctx: &mut AppContext, keyword: &str) -> Vec<HitOrStatus> {
    let sources: Vec<catalog::BookSource> = match catalog::facade::list_sources(&ctx.db) {
        Ok(v) => v.into_iter().filter(|s| s.enabled).collect(),
        Err(e) => {
            return vec![HitOrStatus::StatusLine(format!(
                "讀取書源清單失敗：{:#}",
                e
            ))]
        }
    };
    if sources.is_empty() {
        return vec![HitOrStatus::StatusLine(
            "（尚無 enabled 書源、請先 source import）".into(),
        )];
    }

    let mut rows: Vec<HitOrStatus> = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(15);
    let total = sources.len();

    for (i, src) in sources.iter().enumerate() {
        let now = Instant::now();
        if now >= deadline {
            rows.push(HitOrStatus::StatusLine(format!(
                "源 {} 未查（時間預算用盡）",
                src.book_source_name
            )));
            continue;
        }
        let remaining = deadline.saturating_duration_since(now);
        let remaining_count = (total - i) as u32;
        let per_source = (remaining / remaining_count.max(1)).max(Duration::from_secs(2));

        match tokio::time::timeout(per_source, ctx.scraper.search(src, keyword)).await {
            Ok(Ok(hits)) => {
                for hit in hits {
                    rows.push(HitOrStatus::Hit {
                        hit,
                        source_name: src.book_source_name.clone(),
                    });
                }
            }
            Ok(Err(e)) => {
                rows.push(HitOrStatus::StatusLine(format!(
                    "源 {}：錯誤 {:#}",
                    src.book_source_name, e
                )));
            }
            Err(_elapsed) => {
                rows.push(HitOrStatus::StatusLine(format!(
                    "源 {}：逾時",
                    src.book_source_name
                )));
            }
        }
    }
    rows
}

// ----------------------------------------------------------------------------
// Enter-on-hit branching: duplicate detection vs new add
// ----------------------------------------------------------------------------

async fn handle_enter_on_hit(
    screen: &mut SearchScreen,
    ctx: &mut AppContext,
    hit: SearchHit,
) -> Transition {
    // Duplicate detection by natural key.
    match library::facade::get_novel_by_book_url(&ctx.db, &hit.book_url) {
        Ok(Some(existing)) => {
            let position = shelf_position(ctx, existing.id.unwrap_or(0));
            let toast = format!("已在書架第 {} 本", position);
            Transition::To(Box::new(
                crate::presentation::handlers::tui::shelf::ShelfScreen::with_highlight(
                    Some(existing.book_url),
                    Some(toast),
                ),
            ))
        }
        Ok(None) => add_new_novel(screen, ctx, hit).await,
        Err(e) => {
            screen.append_status(format!("查詢失敗：{:#}", e));
            Transition::Stay
        }
    }
}

async fn add_new_novel(
    screen: &mut SearchScreen,
    ctx: &mut AppContext,
    hit: SearchHit,
) -> Transition {
    let source = match catalog::facade::get_source(&ctx.db, &hit.source_url) {
        Ok(Some(s)) => s,
        Ok(None) => {
            screen.append_status(format!("找不到書源：{}", hit.source_url));
            return Transition::Stay;
        }
        Err(e) => {
            screen.append_status(format!("查書源失敗：{:#}", e));
            return Transition::Stay;
        }
    };
    let novel = match catalog::facade::fetch_novel_info(&ctx.scraper, &source, &hit.book_url).await
    {
        Ok(n) => n,
        Err(e) => {
            screen.append_status(format!("取得詳情失敗：{:#}", e));
            return Transition::Stay;
        }
    };
    match library::facade::add_novel(&mut ctx.db, &novel) {
        Ok(id) => {
            let toast = format!("已入架 #{} {}", id, novel.name);
            Transition::To(Box::new(
                crate::presentation::handlers::tui::menu::MenuScreen::with_toast(toast),
            ))
        }
        Err(e) => {
            screen.append_status(format!("入架失敗：{:#}", e));
            Transition::Stay
        }
    }
}

/// 找出 novel_id 在書架（list_shelf 順序）中的 1-based 位置；找不到回 0。
fn shelf_position(ctx: &AppContext, novel_id: i64) -> usize {
    library::facade::list_shelf(&ctx.db)
        .map(|v| {
            v.iter()
                .position(|n| n.id == Some(novel_id))
                .map(|i| i + 1)
                .unwrap_or(0)
        })
        .unwrap_or(0)
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_starts_in_input_state_with_no_progress() {
        let s = SearchScreen::new();
        assert!(matches!(s.state, SearchState::Input(_)));
        assert!(s.progress.is_none());
    }
}
