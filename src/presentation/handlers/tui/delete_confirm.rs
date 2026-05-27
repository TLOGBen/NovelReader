//! DeleteConfirmScreen — TUI 書架刪除確認 modal（/think 2026-05-27）。
//!
//! Modal-style screen：由 [`ShelfScreen`] 按 `d` 鍵 transition 進入。畫面中央
//! 用 [`Clear`] widget 蓋背景 + 居中 layout 渲染「確定要刪除《書名》嗎？[y/N]」。
//!
//! 互動（[y/N] 預設 N）：
//! - `y` / `Y`               : 執行 [`library::facade::delete_novel`]、Transition
//!                             回 [`MenuScreen`] 並帶 toast「已刪除《書名》」。
//! - `n` / `N` / `Esc` / 其他: 取消、Transition 回 [`ShelfScreen`] 並用
//!                             `with_highlight(book_url)` 保留原 selection。
//!                             （任意非 y 鍵都當取消 — 防止誤觸成刪除）
//!
//! Atomicity：刪除事務由 [`library::facade::delete_novel`] 保證
//! （progress → chapters → novels 三表單一 transaction），失敗則整體 rollback；
//! modal 持有 `novel_name` / `book_url` 在開啟時就拷貝，避免 modal 期間
//! DB 變動導致書名漂移。

use async_trait::async_trait;
use crossterm::event::{Event, KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::library::facade as library_facade;
use crate::presentation::handlers::tui::{
    menu::MenuScreen, shelf::ShelfScreen, Screen, Transition,
};
use crate::presentation::AppContext;

pub struct DeleteConfirmScreen {
    novel_id: i64,
    novel_name: String,
    /// 取消時用來在 [`ShelfScreen::with_highlight`] 預選原本那本。modal 開啟時拷貝。
    book_url: String,
}

impl DeleteConfirmScreen {
    pub fn new(novel_id: i64, novel_name: String, book_url: String) -> Self {
        Self { novel_id, novel_name, book_url }
    }
}

#[async_trait(?Send)]
impl Screen for DeleteConfirmScreen {
    fn draw(&mut self, frame: &mut Frame, _ctx: &AppContext) {
        let area = frame.area();
        // centered modal: 中央 60% 寬 × 7 高
        let modal_w = area.width.saturating_mul(60) / 100;
        let modal_h = 7u16.min(area.height);
        let x = area.x + area.width.saturating_sub(modal_w) / 2;
        let y = area.y + area.height.saturating_sub(modal_h) / 2;
        let modal_area = Rect {
            x,
            y,
            width: modal_w,
            height: modal_h,
        };

        frame.render_widget(Clear, modal_area);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" 確認刪除 ")
            .border_style(Style::default().fg(Color::Red));
        frame.render_widget(block.clone(), modal_area);

        // 內容區（扣掉 1 行 border 各邊）
        let inner = Rect {
            x: modal_area.x + 2,
            y: modal_area.y + 2,
            width: modal_area.width.saturating_sub(4),
            height: modal_area.height.saturating_sub(4),
        };
        let prompt = Paragraph::new(format!(
            "確定要刪除《{}》嗎？\n\n[y] 確定刪除   [N] 取消（預設）",
            self.novel_name
        ));
        frame.render_widget(prompt, inner);
    }

    async fn handle_event(&mut self, event: Event, ctx: &mut AppContext) -> Transition {
        let key: KeyEvent = match event {
            Event::Key(k) => k,
            Event::Mouse(_) => return Transition::Stay,
            _ => return Transition::Stay,
        };
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                // 失敗也帶錯誤 toast 回 menu（不重試、不卡 modal）
                let toast = match library_facade::delete_novel(&mut ctx.db, self.novel_id) {
                    Ok(()) => format!("已刪除《{}》", self.novel_name),
                    Err(e) => format!("刪除失敗：{:#}", e),
                };
                Transition::To(Box::new(MenuScreen::with_toast(toast)))
            }
            // n / N / Esc / 其他鍵 → 一律取消（防誤觸）
            _ => Transition::To(Box::new(ShelfScreen::with_highlight(
                Some(self.book_url.clone()),
                None,
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::library::dao::LibraryDb;
    use crate::library::Novel;
    use crate::presentation::AppContext;
    use crossterm::event::{Event, KeyEvent, KeyModifiers};

    fn test_ctx_with_novel() -> (AppContext, i64) {
        let mut db = LibraryDb::open_in_memory().expect("open in-memory db");
        let novel = Novel {
            id: None,
            source_url: "https://src.example/".into(),
            book_url: "https://book.example/1".into(),
            name: "凡人修仙傳".into(),
            author: Some("忘語".into()),
            intro: None,
            cover_url: None,
            toc_url: None,
        };
        let id = db.upsert_novel(&novel).expect("upsert");
        let scraper =
            crate::catalog::service::scraper::Scraper::new().expect("scraper init");
        let ctx = AppContext { db, scraper, config: Config::default() };
        (ctx, id)
    }

    /// Trait migration: 既有 UT 改為包 Event::Key(...)，行為斷言不變。
    fn press(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::empty()))
    }

    #[test]
    fn new_stores_three_fields() {
        let s = DeleteConfirmScreen::new(
            42,
            "凡人修仙傳".into(),
            "https://book.example/1".into(),
        );
        assert_eq!(s.novel_id, 42);
        assert_eq!(s.novel_name, "凡人修仙傳");
        assert_eq!(s.book_url, "https://book.example/1");
    }

    #[tokio::test]
    async fn y_executes_delete_and_transitions_to_menu() {
        let (mut ctx, novel_id) = test_ctx_with_novel();
        let mut s = DeleteConfirmScreen::new(
            novel_id,
            "凡人修仙傳".into(),
            "https://book.example/1".into(),
        );

        let t = s.handle_event(press(KeyCode::Char('y')), &mut ctx).await;
        assert!(matches!(t, Transition::To(_)), "y 應轉場到 MenuScreen");

        // 驗證 DB 已刪
        let novel = library_facade::get_novel(&ctx.db, novel_id).unwrap();
        assert!(novel.is_none(), "delete_novel must have removed the row");
    }

    #[tokio::test]
    async fn capital_y_also_executes_delete() {
        let (mut ctx, novel_id) = test_ctx_with_novel();
        let mut s = DeleteConfirmScreen::new(
            novel_id,
            "凡人修仙傳".into(),
            "https://book.example/1".into(),
        );

        let _t = s.handle_event(press(KeyCode::Char('Y')), &mut ctx).await;
        let novel = library_facade::get_novel(&ctx.db, novel_id).unwrap();
        assert!(novel.is_none(), "Y should also delete");
    }

    #[tokio::test]
    async fn n_cancels_without_delete_and_transitions_to_shelf() {
        let (mut ctx, novel_id) = test_ctx_with_novel();
        let mut s = DeleteConfirmScreen::new(
            novel_id,
            "凡人修仙傳".into(),
            "https://book.example/1".into(),
        );

        let t = s.handle_event(press(KeyCode::Char('n')), &mut ctx).await;
        assert!(matches!(t, Transition::To(_)), "n 應轉場回 ShelfScreen");

        // 沒被刪
        let novel = library_facade::get_novel(&ctx.db, novel_id).unwrap();
        assert!(novel.is_some(), "n must NOT delete");
    }

    #[tokio::test]
    async fn esc_cancels_without_delete() {
        let (mut ctx, novel_id) = test_ctx_with_novel();
        let mut s = DeleteConfirmScreen::new(
            novel_id,
            "凡人修仙傳".into(),
            "https://book.example/1".into(),
        );

        let _t = s.handle_event(press(KeyCode::Esc), &mut ctx).await;
        let novel = library_facade::get_novel(&ctx.db, novel_id).unwrap();
        assert!(novel.is_some(), "Esc must NOT delete");
    }

    #[tokio::test]
    async fn other_keys_default_to_cancel() {
        // 防誤觸：任何非 y/Y 鍵都當取消
        let (mut ctx, novel_id) = test_ctx_with_novel();
        let mut s = DeleteConfirmScreen::new(
            novel_id,
            "凡人修仙傳".into(),
            "https://book.example/1".into(),
        );

        let _t = s.handle_event(press(KeyCode::Char('x')), &mut ctx).await;
        let novel = library_facade::get_novel(&ctx.db, novel_id).unwrap();
        assert!(novel.is_some(), "unknown key must default to cancel (not delete)");
    }

    // ------------------------------------------------------------------
    // INT-trait-03: delete_confirm 收 Event::Mouse(ScrollUp) → Transition::Stay
    // 並驗證：mouse 不該觸發刪除（書還在）
    // ------------------------------------------------------------------
    #[tokio::test]
    async fn int_trait_03_delete_confirm_mouse_stay() {
        use crossterm::event::{MouseEvent, MouseEventKind};
        let (mut ctx, novel_id) = test_ctx_with_novel();
        let mut s = DeleteConfirmScreen::new(
            novel_id,
            "凡人修仙傳".into(),
            "https://book.example/1".into(),
        );
        let mouse = Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::empty(),
        });
        let t = s.handle_event(mouse, &mut ctx).await;
        assert!(matches!(t, Transition::Stay), "Mouse 事件應回 Stay");
        // 確認沒被刪
        let novel = library_facade::get_novel(&ctx.db, novel_id).unwrap();
        assert!(novel.is_some(), "Mouse 事件不該觸發刪除");
    }
}
