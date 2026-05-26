//! SwitchSourceScreen — TUI 換源 modal（TASK-tui-05 / REQ-005）。
//!
//! Modal-style screen：由 [`ShelfScreen`] 按 `s` 鍵 transition 進入。畫面中央
//! 用 [`Clear`] widget 蓋背景 + 居中 layout 渲染兩個 [`SingleLineInput`]：
//! 第一行收「新書 URL」、第二行收「新源 URL」，[`Focus`] 標示當前焦點。
//!
//! 互動：
//! - `Tab`              : 切換焦點。
//! - `Esc`              : 取消、回 [`ShelfScreen`]（**不**呼叫任何 catalog /
//!                        library facade，atomicity 保證）。
//! - `Enter`（BookUrl） : 切到 SourceUrl 欄。
//! - `Enter`（SourceUrl）: 取兩欄 text、`await switch_source_core::run(...)`；
//!                        成功 / 失敗都 transition 回 [`ShelfScreen`] 帶 toast。
//!
//! 不直接寫 DB —— atomicity 由 `switch_source_core::run` 保證（五類 abort
//! 任一觸發時不呼叫 `library::facade::switch_source_tx`）。

use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::presentation::handlers::switch_source_core;
use crate::presentation::handlers::tui::{
    shelf::ShelfScreen,
    widgets::{SingleLineEvent, SingleLineInput},
    Screen, Transition,
};
use crate::presentation::AppContext;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    BookUrl,
    SourceUrl,
}

#[allow(dead_code)]
pub struct SwitchSourceScreen {
    novel_id: i64,
    book_url_input: SingleLineInput,
    source_url_input: SingleLineInput,
    focus: Focus,
}

impl SwitchSourceScreen {
    #[allow(dead_code)]
    pub fn new(novel_id: i64) -> Self {
        Self {
            novel_id,
            book_url_input: SingleLineInput::new(" 新書 URL "),
            source_url_input: SingleLineInput::new(" 新源 URL "),
            focus: Focus::BookUrl,
        }
    }
}

#[async_trait(?Send)]
impl Screen for SwitchSourceScreen {
    fn draw(&mut self, frame: &mut Frame, _ctx: &AppContext) {
        let area = frame.area();
        // centered modal: 中央 60% 寬 × 10 高（不超過畫面）
        let modal_w = area.width.saturating_mul(60) / 100;
        let modal_h = 10u16.min(area.height);
        let x = area.x + area.width.saturating_sub(modal_w) / 2;
        let y = area.y + area.height.saturating_sub(modal_h) / 2;
        let modal_area = Rect {
            x,
            y,
            width: modal_w,
            height: modal_h,
        };

        // Clear 蓋背景，再畫邊框 + 標題
        frame.render_widget(Clear, modal_area);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" 換源 #{} ", self.novel_id))
            .border_style(Style::default().fg(Color::Cyan));
        frame.render_widget(block, modal_area);

        // 內部 layout：兩個 input（高 3）+ 提示列（剩餘）
        let inner = Rect {
            x: modal_area.x + 1,
            y: modal_area.y + 1,
            width: modal_area.width.saturating_sub(2),
            height: modal_area.height.saturating_sub(2),
        };
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(1),
            ])
            .split(inner);

        let book_focused = self.focus == Focus::BookUrl;
        let src_focused = self.focus == Focus::SourceUrl;

        // 渲染兩個 input；focus 者用原生 draw（含游標），非 focus 者退化為
        // 灰邊靜態 Paragraph（避免兩個游標互相干擾）。
        if book_focused {
            self.book_url_input.draw(frame, rows[0]);
        } else {
            let p = Paragraph::new(self.book_url_input.text()).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" 新書 URL ")
                    .border_style(Style::default().fg(Color::DarkGray)),
            );
            frame.render_widget(p, rows[0]);
        }
        if src_focused {
            self.source_url_input.draw(frame, rows[1]);
        } else {
            let p = Paragraph::new(self.source_url_input.text()).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" 新源 URL ")
                    .border_style(Style::default().fg(Color::DarkGray)),
            );
            frame.render_widget(p, rows[1]);
        }

        let hint = Paragraph::new(" Tab 切換  Enter(在新源 URL) 執行  Esc 取消 ")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(hint, rows[2]);
    }

    async fn handle_event(&mut self, key: KeyEvent, ctx: &mut AppContext) -> Transition {
        match key.code {
            KeyCode::Tab => {
                self.focus = if self.focus == Focus::BookUrl {
                    Focus::SourceUrl
                } else {
                    Focus::BookUrl
                };
                Transition::Stay
            }
            KeyCode::Esc => Transition::To(Box::new(ShelfScreen::new())),
            _ => {
                // 餵給目前 focus 的 input；只有 Submit / Cancel 需要路由
                let event = match self.focus {
                    Focus::BookUrl => self.book_url_input.handle_event(key),
                    Focus::SourceUrl => self.source_url_input.handle_event(key),
                };
                match event {
                    SingleLineEvent::Cancel => Transition::To(Box::new(ShelfScreen::new())),
                    SingleLineEvent::Edit => Transition::Stay,
                    SingleLineEvent::Submit(_) => {
                        // BookUrl Submit → 切到 SourceUrl；SourceUrl Submit → 執行
                        if self.focus == Focus::BookUrl {
                            self.focus = Focus::SourceUrl;
                            return Transition::Stay;
                        }
                        let book_url = self.book_url_input.text().trim().to_string();
                        let source_url = self.source_url_input.text().trim().to_string();
                        if book_url.is_empty() || source_url.is_empty() {
                            // 缺欄位、回第一欄讓使用者補
                            self.focus = Focus::BookUrl;
                            return Transition::Stay;
                        }
                        match switch_source_core::run(
                            ctx,
                            self.novel_id,
                            &source_url,
                            &book_url,
                        )
                        .await
                        {
                            Ok(outcome) => {
                                // 規格 line 145：成功時用 None highlight + toast。
                                // 不再 highlight 新 book_url（即使 UX 上會更友善）。
                                let toast = format!(
                                    "✓ 已換源至 {}，進度重置到第 {} 章: {}",
                                    source_url,
                                    outcome.new_progress_idx + 1,
                                    outcome.new_first_chapter_name
                                );
                                Transition::To(Box::new(ShelfScreen::with_highlight(
                                    None,
                                    Some(toast),
                                )))
                            }
                            Err(e) => {
                                let toast = format!("換源失敗：{:#}", e);
                                Transition::To(Box::new(ShelfScreen::with_highlight(
                                    None,
                                    Some(toast),
                                )))
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_starts_with_book_url_focus() {
        let s = SwitchSourceScreen::new(42);
        assert_eq!(s.novel_id, 42);
        assert!(matches!(s.focus, Focus::BookUrl));
    }
}
