//! ShelfScreen — TUI 書架畫面（TASK-tui-03）。
//!
//! 顯示 `library::facade::list_shelf` 結果為一個列表；j/k 上下、Enter 進
//! ReaderScreen（EntryMode::Menu）、`s` 觸發換源（tui-05 才實裝
//! SwitchSourceScreen，目前以 toast stub）、Esc/q 回 MenuScreen。
//!
//! 兩種 ctor：
//! - [`ShelfScreen::new`] — 一般進入。
//! - [`ShelfScreen::with_highlight`] — 給「搜尋重複入架」/「換源完成回 shelf」
//!   等場景，攜帶 book_url 預選 + toast 訊息（首次按鍵清除）。
//!
//! 書架資料採 lazy-load：ctor 只存欄位，第一次 `draw` 時呼
//! `library::facade::list_shelf` 拉取（純 sync DB read，不需 await）。
//! 後續 task 若需要強制 refresh（換源完成回 shelf 後），把 `needs_refresh`
//! 設回 `true` 即可。

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::library;
use crate::library::Novel;
use crate::presentation::handlers::tui::{EntryMode, Screen, Transition};
use crate::presentation::AppContext;

#[allow(dead_code)]
pub struct ShelfScreen {
    novels: Vec<Novel>,
    list_state: ListState,
    initial_highlight_book_url: Option<String>,
    toast: Option<String>,
    needs_refresh: bool,
}

impl ShelfScreen {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            novels: Vec::new(),
            list_state: ListState::default(),
            initial_highlight_book_url: None,
            toast: None,
            needs_refresh: true,
        }
    }

    /// 帶 hint 構造：`highlight_book_url` 在第一次 refresh 時用來 pre-select
    /// list_state；`toast` 顯示於畫面頂端、首次按鍵清除。
    #[allow(dead_code)]
    pub fn with_highlight(
        highlight_book_url: Option<String>,
        toast: Option<String>,
    ) -> Self {
        Self {
            novels: Vec::new(),
            list_state: ListState::default(),
            initial_highlight_book_url: highlight_book_url,
            toast,
            needs_refresh: true,
        }
    }

    fn refresh(&mut self, ctx: &AppContext) -> Result<()> {
        self.novels = library::facade::list_shelf(&ctx.db)?;
        // apply initial_highlight_book_url 一次性
        let selected = if let Some(url) = self.initial_highlight_book_url.take() {
            self.novels
                .iter()
                .position(|n| n.book_url == url)
                .unwrap_or(0)
        } else {
            0
        };
        if !self.novels.is_empty() {
            self.list_state.select(Some(selected));
        }
        self.needs_refresh = false;
        Ok(())
    }
}

impl Default for ShelfScreen {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl Screen for ShelfScreen {
    fn draw(&mut self, frame: &mut Frame, ctx: &AppContext) {
        // 第一次 draw 觸發 refresh（純 sync DB read，無 await 需要）
        if self.needs_refresh {
            // 忽略 err、就讓 list 空著
            let _ = self.refresh(ctx);
        }
        let area = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(if self.toast.is_some() { 1 } else { 0 }),
                Constraint::Min(3),
                Constraint::Length(1),
            ])
            .split(area);

        if let Some(t) = &self.toast {
            let p = Paragraph::new(t.as_str())
                .style(Style::default().fg(Color::Cyan));
            frame.render_widget(p, chunks[0]);
        }

        if self.novels.is_empty() {
            let p = Paragraph::new("（書架空、回主菜單按 q）");
            frame.render_widget(p, chunks[1]);
        } else {
            let items: Vec<ListItem> = self
                .novels
                .iter()
                .map(|n| {
                    ListItem::new(format!(
                        "#{} {} / {} [{}]",
                        n.id.unwrap_or(0),
                        n.name,
                        n.author.as_deref().unwrap_or("-"),
                        n.source_url
                    ))
                })
                .collect();
            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title(" 書架 "))
                .highlight_style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                );
            frame.render_stateful_widget(list, chunks[1], &mut self.list_state);
        }

        let hint = Line::from(" j/k 移動  Enter 閱讀  s 換源  Esc/q 回主菜單 ");
        frame.render_widget(
            Paragraph::new(hint).style(Style::default().fg(Color::DarkGray)),
            chunks[2],
        );
    }

    async fn handle_event(
        &mut self,
        key: KeyEvent,
        ctx: &mut AppContext,
    ) -> Transition {
        // 任意鍵清 toast（首次按鍵清 highlight 提示）
        self.toast = None;

        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                let i = self.list_state.selected().unwrap_or(0);
                let next = (i + 1).min(self.novels.len().saturating_sub(1));
                self.list_state.select(Some(next));
                Transition::Stay
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let i = self.list_state.selected().unwrap_or(0);
                self.list_state.select(Some(i.saturating_sub(1)));
                Transition::Stay
            }
            KeyCode::Enter => {
                if let Some(i) = self.list_state.selected() {
                    if let Some(novel) = self.novels.get(i) {
                        if let Some(novel_id) = novel.id {
                            match crate::presentation::handlers::tui::reader::ReaderScreen::new(
                                EntryMode::Menu,
                                ctx,
                                novel_id,
                            )
                            .await
                            {
                                Ok(reader) => return Transition::To(Box::new(reader)),
                                Err(e) => {
                                    self.toast =
                                        Some(format!("無法開啟 reader：{:#}", e));
                                }
                            }
                        }
                    }
                }
                Transition::Stay
            }
            KeyCode::Char('s') => {
                // SwitchSourceScreen 尚未實作（tui-05 才建）。
                // 暫時以 toast stub，後續 task wire 成 Transition::To。
                self.toast = Some("（換源等 tui-05 實裝）".into());
                Transition::Stay
            }
            KeyCode::Esc | KeyCode::Char('q') => Transition::To(Box::new(
                crate::presentation::handlers::tui::menu::MenuScreen::new(),
            )),
            _ => Transition::Stay,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit7_with_highlight_stores_toast() {
        let shelf = ShelfScreen::with_highlight(
            Some("https://x".into()),
            Some("已在書架第 1 本".into()),
        );
        assert_eq!(shelf.toast.as_deref(), Some("已在書架第 1 本"));
        assert_eq!(shelf.initial_highlight_book_url.as_deref(), Some("https://x"));
    }

    #[test]
    fn new_no_toast_no_highlight() {
        let shelf = ShelfScreen::new();
        assert!(shelf.toast.is_none());
        assert!(shelf.initial_highlight_book_url.is_none());
    }
}
