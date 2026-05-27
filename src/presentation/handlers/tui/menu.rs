//! MenuScreen — TUI 主菜單畫面（REQ-002）。
//!
//! 四項目主菜單：「書架 / 搜尋蒐書 / 設定 / 離開」。j/k 上下移動 highlight、
//! Enter 進入子畫面 / 退出、q 直接退出。本期「書架」「搜尋蒐書」分支因為
//! ShelfScreen / SearchScreen 還沒在 tui-03/04 完成，暫時用 `settings_stub_msg`
//! 顯示 toast 訊息 + 留在原畫面（Transition::Stay）；後續 task 完成時改成
//! 真正的 Transition::To(...)。
//!
//! 「設定」分支同樣用 settings_stub_msg 顯示「尚未實作」、任意其他鍵清除提示。

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};
use std::time::Instant;

use crate::presentation::handlers::tui::{Screen, Transition, TOAST_TTL};
use crate::presentation::AppContext;

const ITEMS: [&str; 4] = ["書架", "搜尋蒐書", "設定", "離開"];

#[allow(dead_code)]
pub struct MenuScreen {
    selected: usize,
    settings_stub_msg: Option<&'static str>,
    /// 動態 toast（owned String）— 由 `with_toast` 帶入，供 SearchScreen 入架
    /// 成功 / 書架刪除成功跳回主菜單時顯示。首次按鍵清除，或 `toast_expires_at`
    /// 過期後 draw 時不渲染（兩條件擇一）。
    toast: Option<String>,
    /// Toast 過期時間。`None` = 永不過期（保留 first-key-clear 行為）。
    /// `with_toast` 預設 [`TOAST_TTL`]；`with_toast_until` 由 caller 指定。
    pub(crate) toast_expires_at: Option<Instant>,
}

impl MenuScreen {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            selected: 0,
            settings_stub_msg: None,
            toast: None,
            toast_expires_at: None,
        }
    }

    /// 帶 toast 構造 — 預設 [`TOAST_TTL`] (3s) 後自消（仍保留 first-key-clear）。
    #[allow(dead_code)]
    pub fn with_toast(toast: String) -> Self {
        Self::with_toast_until(toast, Instant::now() + TOAST_TTL)
    }

    /// 顯式指定過期時間的 ctor — 主要給 UT 用，但也可給未來想客製 TTL 的 caller。
    #[allow(dead_code)]
    pub fn with_toast_until(toast: String, expires_at: Instant) -> Self {
        Self {
            selected: 0,
            settings_stub_msg: None,
            toast: Some(toast),
            toast_expires_at: Some(expires_at),
        }
    }

    /// 回傳「目前還該顯示的 toast」— toast 不存在 → None；過期 → None；
    /// 仍有效 → Some(&str)。draw / UT 都靠這個 helper 統一邏輯。
    #[allow(dead_code)]
    pub fn toast_active(&self) -> Option<&str> {
        self.toast.as_deref().filter(|_| {
            self.toast_expires_at
                .map_or(true, |t| Instant::now() < t)
        })
    }
}

impl Default for MenuScreen {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl Screen for MenuScreen {
    fn draw(&mut self, frame: &mut Frame, _ctx: &AppContext) {
        let area = frame.area();
        // 三段：title / list / status(+optional stub_msg).
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(2),
            ])
            .split(area);

        let title = Paragraph::new("novel-looker").block(
            Block::default()
                .borders(Borders::ALL)
                .title("主菜單"),
        );
        frame.render_widget(title, chunks[0]);

        let items: Vec<ListItem> = ITEMS
            .iter()
            .map(|s| ListItem::new(Line::from(Span::raw(*s))))
            .collect();
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .highlight_symbol("> ");
        let mut state = ListState::default();
        state.select(Some(self.selected));
        frame.render_stateful_widget(list, chunks[1], &mut state);

        let status_text = match (self.toast_active(), self.settings_stub_msg) {
            (Some(t), _) => format!("j/k 上下、Enter 進入、q 離開\n{}", t),
            (None, Some(msg)) => format!("j/k 上下、Enter 進入、q 離開\n{}", msg),
            (None, None) => "j/k 上下、Enter 進入、q 離開".to_string(),
        };
        frame.render_widget(Paragraph::new(status_text), chunks[2]);
    }

    async fn handle_event(&mut self, key: KeyEvent, _ctx: &mut AppContext) -> Transition {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.selected = (self.selected + 1) % ITEMS.len();
                self.settings_stub_msg = None;
                self.toast = None;
                self.toast_expires_at = None;
                Transition::Stay
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.selected = if self.selected == 0 {
                    ITEMS.len() - 1
                } else {
                    self.selected - 1
                };
                self.settings_stub_msg = None;
                self.toast = None;
                self.toast_expires_at = None;
                Transition::Stay
            }
            KeyCode::Enter => match self.selected {
                0 => Transition::To(Box::new(
                    crate::presentation::handlers::tui::shelf::ShelfScreen::new(),
                )),
                1 => Transition::To(Box::new(
                    crate::presentation::handlers::tui::search::SearchScreen::new(),
                )),
                2 => {
                    self.settings_stub_msg = Some("尚未實作");
                    Transition::Stay
                }
                3 => Transition::Quit,
                _ => Transition::Stay,
            },
            KeyCode::Char('q') => Transition::Quit,
            _ => {
                self.settings_stub_msg = None;
                self.toast = None;
                self.toast_expires_at = None;
                Transition::Stay
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::presentation::AppContext;
    use crossterm::event::{KeyEvent, KeyModifiers};

    /// 構造一個僅供測試使用的 AppContext（in-memory DB，real scraper）。
    fn test_ctx() -> AppContext {
        let db = crate::library::dao::LibraryDb::open_in_memory()
            .expect("open in-memory db");
        let scraper = crate::catalog::service::scraper::Scraper::new()
            .expect("scraper init");
        let config = Config::default();
        AppContext { db, scraper, config }
    }

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
    }

    #[test]
    fn new_starts_at_first_item_with_no_stub_msg() {
        let m = MenuScreen::new();
        assert_eq!(m.selected, 0);
        assert!(m.settings_stub_msg.is_none());
    }

    #[tokio::test]
    async fn j_moves_down_and_wraps() {
        let mut m = MenuScreen::new();
        let mut ctx = test_ctx();
        for expected in [1usize, 2, 3, 0] {
            let _ = m.handle_event(press(KeyCode::Char('j')), &mut ctx).await;
            assert_eq!(m.selected, expected);
        }
    }

    #[tokio::test]
    async fn k_from_zero_wraps_to_last() {
        let mut m = MenuScreen::new();
        let mut ctx = test_ctx();
        let _ = m.handle_event(press(KeyCode::Char('k')), &mut ctx).await;
        assert_eq!(m.selected, 3);
    }

    #[tokio::test]
    async fn enter_on_quit_item_returns_quit() {
        let mut m = MenuScreen::new();
        let mut ctx = test_ctx();
        m.selected = 3;
        let t = m.handle_event(press(KeyCode::Enter), &mut ctx).await;
        assert!(matches!(t, Transition::Quit));
    }

    #[tokio::test]
    async fn q_returns_quit() {
        let mut m = MenuScreen::new();
        let mut ctx = test_ctx();
        let t = m.handle_event(press(KeyCode::Char('q')), &mut ctx).await;
        assert!(matches!(t, Transition::Quit));
    }

    #[tokio::test]
    async fn enter_on_settings_sets_stub_msg_and_stays() {
        let mut m = MenuScreen::new();
        let mut ctx = test_ctx();
        m.selected = 2;
        let t = m.handle_event(press(KeyCode::Enter), &mut ctx).await;
        assert!(matches!(t, Transition::Stay));
        assert_eq!(m.settings_stub_msg, Some("尚未實作"));
    }

    #[tokio::test]
    async fn enter_on_shelf_transitions_to_shelf_screen() {
        // tui-03: Enter on 書架 should now transition to ShelfScreen
        // (replacing the earlier tui-03 stub-msg placeholder behaviour).
        let mut m = MenuScreen::new();
        let mut ctx = test_ctx();
        m.selected = 0;
        let t = m.handle_event(press(KeyCode::Enter), &mut ctx).await;
        assert!(matches!(t, Transition::To(_)));
    }

    #[tokio::test]
    async fn m_key_is_stay_no_panic() {
        let mut m = MenuScreen::new();
        let mut ctx = test_ctx();
        let t = m.handle_event(press(KeyCode::Char('m')), &mut ctx).await;
        assert!(matches!(t, Transition::Stay));
    }

    #[test]
    fn with_toast_stores_message() {
        let m = MenuScreen::with_toast("已入架 #5 超維術士".to_string());
        assert_eq!(m.selected, 0);
        assert_eq!(m.toast.as_deref(), Some("已入架 #5 超維術士"));
    }

    #[test]
    fn toast_active_returns_some_when_not_expired() {
        use std::time::{Duration, Instant};
        let m = MenuScreen::with_toast_until(
            "fresh".into(),
            Instant::now() + Duration::from_secs(10),
        );
        assert_eq!(m.toast_active(), Some("fresh"));
    }

    #[test]
    fn toast_active_returns_none_when_expired() {
        use std::time::{Duration, Instant};
        let m = MenuScreen::with_toast_until(
            "stale".into(),
            Instant::now() - Duration::from_secs(1),
        );
        assert!(m.toast_active().is_none(), "已過期 toast 不該再顯示");
    }

    #[test]
    fn with_toast_defaults_to_3s_ttl() {
        // 構造後立即查看：toast_expires_at 必須 Some 且大於 now
        use std::time::Instant;
        let m = MenuScreen::with_toast("hi".into());
        let exp = m.toast_expires_at.expect("with_toast must set TTL");
        assert!(exp > Instant::now(), "TTL 必須在未來");
        // 不能超過 4 秒（給 1 秒寬容）
        let now = Instant::now();
        assert!(
            exp <= now + std::time::Duration::from_secs(4),
            "TTL 預設應約 3 秒"
        );
    }

    #[tokio::test]
    async fn moving_clears_stub_msg() {
        let mut m = MenuScreen::new();
        let mut ctx = test_ctx();
        m.selected = 2;
        let _ = m.handle_event(press(KeyCode::Enter), &mut ctx).await;
        assert!(m.settings_stub_msg.is_some());
        let _ = m.handle_event(press(KeyCode::Char('j')), &mut ctx).await;
        assert!(m.settings_stub_msg.is_none());
    }
}
