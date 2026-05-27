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
use crossterm::event::{Event, KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};
use std::sync::Arc;
use std::time::Instant;

use crate::catalog;
use crate::library;
use crate::library::Novel;
use crate::presentation::handlers::tui::picker::{PickerEntry, SearchPickerScreen};
use crate::presentation::handlers::tui::{EntryMode, Screen, Transition, TOAST_TTL};
use crate::presentation::AppContext;

// ---------------------------------------------------------------------------
// Shelf 's' caller-side guard — REQ-005 entry into SearchPickerScreen.
//
// `classify_s_press` is a pure helper that inspects shelf state + DB to
// decide what should happen on 's'. The `handle_event` arm consumes the
// classification and builds the actual `Transition` (which needs `&mut ctx`
// for `Scraper::new()` and `spawn_searches`).
// ---------------------------------------------------------------------------

/// Outcome of pressing 's' on the shelf.
///
/// Pure data — `handle_event` is responsible for translating each variant
/// into a `Transition` (open picker / stay / toast back to shelf).
#[derive(Debug)]
pub(crate) enum ShelfSAction {
    /// No selection (empty shelf / `list_state.selected() == None`) or the
    /// highlighted row is missing an id — caller stays put.
    Stay,
    /// Anchor resolved (progress + chapter row both exist) — caller builds
    /// `SearchPickerScreen` with these inputs.
    OpenPicker {
        novel_id: i64,
        book_name: String,
        author: String,
        old_chapter_idx: i64,
        old_chapter_name: String,
    },
    /// Anchor lookup failed (no progress row → treated as idx 0 with no
    /// matching chapter row, OR progress idx out of range of `chapters`
    /// table). Caller stays on shelf and surfaces a toast.
    ToastBackToShelf { toast: String },
}

/// Pure classifier — decide what 's' should do given current shelf state.
///
/// Reads `library::facade::get_progress` + `library::facade::get_chapter`
/// to resolve the fuzzy anchor (chapter name) that the picker needs for
/// `picker-02`'s sync step. Any miss → `ToastBackToShelf` (REQ-002 S5).
pub(crate) fn classify_s_press(
    ctx: &AppContext,
    list_state: &ListState,
    novels: &[Novel],
) -> ShelfSAction {
    let Some(idx) = list_state.selected() else {
        return ShelfSAction::Stay;
    };
    let Some(novel) = novels.get(idx) else {
        return ShelfSAction::Stay;
    };
    let Some(novel_id) = novel.id else {
        return ShelfSAction::Stay;
    };

    // Anchor lookup: progress.chapter_index defaults to 0 if no progress row.
    let chapter_idx = library::facade::get_progress(&ctx.db, novel_id)
        .ok()
        .flatten()
        .map(|p| p.chapter_index)
        .unwrap_or(0);

    // chapter row must exist (REQ-002 S5) — we only need the *name* (anchor
    // for fuzzy match), not the cached content. `library::facade::get_chapter`
    // returns None when `content IS NULL` even if the meta row exists, so
    // use `list_chapters` + find-by-idx to get a name-only lookup.
    let chapter_name = library::facade::list_chapters(&ctx.db, novel_id)
        .ok()
        .and_then(|all| all.into_iter().find(|c| c.index == chapter_idx).map(|c| c.name));

    match chapter_name {
        Some(name) => ShelfSAction::OpenPicker {
            novel_id,
            book_name: novel.name.clone(),
            author: novel.author.clone().unwrap_or_default(),
            old_chapter_idx: chapter_idx,
            old_chapter_name: name,
        },
        None => ShelfSAction::ToastBackToShelf {
            toast: "找不到舊章節，無法換源".to_string(),
        },
    }
}

#[allow(dead_code)]
pub struct ShelfScreen {
    novels: Vec<Novel>,
    list_state: ListState,
    initial_highlight_book_url: Option<String>,
    toast: Option<String>,
    /// Toast 過期時間。`None` = 永不過期（保留 first-key-clear 行為）。
    /// `with_highlight` 在 toast 存在時自動帶 [`TOAST_TTL`]，無 toast 則 `None`。
    pub(crate) toast_expires_at: Option<Instant>,
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
            toast_expires_at: None,
            needs_refresh: true,
        }
    }

    /// 帶 hint 構造：`highlight_book_url` 在第一次 refresh 時用來 pre-select
    /// list_state；`toast` 顯示於畫面頂端，首次按鍵清除，或 [`TOAST_TTL`] 過期自消。
    #[allow(dead_code)]
    pub fn with_highlight(
        highlight_book_url: Option<String>,
        toast: Option<String>,
    ) -> Self {
        let expires = toast.as_ref().map(|_| Instant::now() + TOAST_TTL);
        Self {
            novels: Vec::new(),
            list_state: ListState::default(),
            initial_highlight_book_url: highlight_book_url,
            toast,
            toast_expires_at: expires,
            needs_refresh: true,
        }
    }

    /// 顯式指定過期時間的 ctor — 給 UT 用。
    #[allow(dead_code)]
    pub fn with_highlight_until(
        highlight_book_url: Option<String>,
        toast: Option<String>,
        expires_at: Instant,
    ) -> Self {
        Self {
            novels: Vec::new(),
            list_state: ListState::default(),
            initial_highlight_book_url: highlight_book_url,
            toast,
            toast_expires_at: Some(expires_at),
            needs_refresh: true,
        }
    }

    /// 回傳目前該顯示的 toast — toast 不存在 / 過期 → `None`。
    #[allow(dead_code)]
    pub fn toast_active(&self) -> Option<&str> {
        self.toast.as_deref().filter(|_| {
            self.toast_expires_at
                .map_or(true, |t| Instant::now() < t)
        })
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
        let active_toast = self.toast_active().map(|s| s.to_string());
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(if active_toast.is_some() { 1 } else { 0 }),
                Constraint::Min(3),
                Constraint::Length(1),
            ])
            .split(area);

        if let Some(t) = &active_toast {
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

        let hint = Line::from(" j/k 移動  Enter 閱讀  s 換源  d 刪除  Esc/q 回主菜單 ");
        frame.render_widget(
            Paragraph::new(hint).style(Style::default().fg(Color::DarkGray)),
            chunks[2],
        );
    }

    async fn handle_event(
        &mut self,
        event: Event,
        ctx: &mut AppContext,
    ) -> Transition {
        let key: KeyEvent = match event {
            Event::Key(k) => k,
            Event::Mouse(_) => return Transition::Stay,
            _ => return Transition::Stay,
        };
        // 任意鍵清 toast（首次按鍵清 highlight 提示；同時 reset expiry 避免殘留）
        self.toast = None;
        self.toast_expires_at = None;

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
                // REQ-005 entry: build SearchPickerScreen with PickerEntry::Shelf.
                // Pure helper classifies the press; we translate to Transition.
                match classify_s_press(ctx, &self.list_state, &self.novels) {
                    ShelfSAction::Stay => Transition::Stay,
                    ShelfSAction::ToastBackToShelf { toast } => Transition::To(Box::new(
                        ShelfScreen::with_highlight_until(
                            None,
                            Some(toast),
                            Instant::now() + TOAST_TTL,
                        ),
                    )),
                    ShelfSAction::OpenPicker {
                        novel_id,
                        book_name,
                        author,
                        old_chapter_idx,
                        old_chapter_name,
                    } => {
                        let mut picker = SearchPickerScreen::new(
                            PickerEntry::Shelf,
                            novel_id,
                            book_name,
                            author,
                            old_chapter_idx,
                            old_chapter_name,
                        );
                        // Build a dedicated Scraper for the parallel JoinSet —
                        // ctx.scraper is owned (no Clone derive) so we mint a
                        // fresh one and Arc-wrap it for `spawn_searches`. wreq
                        // emulation init is the cost; acceptable per-press.
                        let scraper = match catalog::service::scraper::Scraper::new() {
                            Ok(s) => Arc::new(s),
                            Err(e) => {
                                return Transition::To(Box::new(
                                    ShelfScreen::with_highlight_until(
                                        None,
                                        Some(format!("無法初始化 scraper：{:#}", e)),
                                        Instant::now() + TOAST_TTL,
                                    ),
                                ));
                            }
                        };
                        let enabled_sources: Vec<_> = catalog::facade::list_sources(&ctx.db)
                            .unwrap_or_default()
                            .into_iter()
                            .filter(|s| s.enabled)
                            .collect();
                        picker.spawn_searches(scraper, enabled_sources);
                        Transition::To(Box::new(picker))
                    }
                }
            }
            KeyCode::Char('d') => {
                // 取當前 highlight 的 novel，transition 到 DeleteConfirmScreen。
                // 空書架 / 沒選 / 缺 id → Stay（防呆同 's' / Enter）。
                if let Some(i) = self.list_state.selected() {
                    if let Some(novel) = self.novels.get(i) {
                        if let Some(novel_id) = novel.id {
                            return Transition::To(Box::new(
                                crate::presentation::handlers::tui::delete_confirm::DeleteConfirmScreen::new(
                                    novel_id,
                                    novel.name.clone(),
                                    novel.book_url.clone(),
                                ),
                            ));
                        }
                    }
                }
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
    use crate::config::Config;
    use crate::library::dao::LibraryDb;
    use crate::library::Novel;
    use crate::presentation::AppContext;
    use crossterm::event::{Event, KeyEvent, KeyModifiers};

    fn test_ctx_with_one_novel() -> (AppContext, i64, String) {
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
        (ctx, id, "https://book.example/1".into())
    }

    /// Trait migration: 既有 UT 改為包 Event::Key(...)，行為斷言不變。
    fn press(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::empty()))
    }

    #[tokio::test]
    async fn d_with_selection_transitions_to_delete_confirm() {
        let (mut ctx, _id, _book_url) = test_ctx_with_one_novel();
        let mut shelf = ShelfScreen::new();
        // 觸發第一次 refresh + select first
        shelf.refresh(&ctx).expect("refresh");
        let t = shelf.handle_event(press(KeyCode::Char('d')), &mut ctx).await;
        assert!(matches!(t, Transition::To(_)), "d 應 transition 到 DeleteConfirmScreen");
    }

    #[tokio::test]
    async fn d_on_empty_shelf_stays() {
        // 空 DB → shelf.novels 空 → d no-op
        let db = LibraryDb::open_in_memory().expect("open in-memory db");
        let scraper =
            crate::catalog::service::scraper::Scraper::new().expect("scraper init");
        let mut ctx = AppContext { db, scraper, config: Config::default() };
        let mut shelf = ShelfScreen::new();
        shelf.refresh(&ctx).expect("refresh");
        let t = shelf.handle_event(press(KeyCode::Char('d')), &mut ctx).await;
        assert!(matches!(t, Transition::Stay), "空書架的 d 應 Stay");
    }

    #[test]
    fn toast_active_returns_some_when_not_expired() {
        use std::time::{Duration, Instant};
        let s = ShelfScreen::with_highlight_until(
            Some("https://x".into()),
            Some("fresh".into()),
            Instant::now() + Duration::from_secs(10),
        );
        assert_eq!(s.toast_active(), Some("fresh"));
    }

    #[test]
    fn toast_active_returns_none_when_expired() {
        use std::time::{Duration, Instant};
        let s = ShelfScreen::with_highlight_until(
            None,
            Some("stale".into()),
            Instant::now() - Duration::from_secs(1),
        );
        assert!(s.toast_active().is_none(), "expired toast 不該顯示");
    }

    #[test]
    fn with_highlight_defaults_to_3s_ttl_when_toast_present() {
        use std::time::Instant;
        let s = ShelfScreen::with_highlight(None, Some("x".into()));
        let exp = s.toast_expires_at.expect("toast present → expiry set");
        assert!(exp > Instant::now());
        assert!(exp <= Instant::now() + std::time::Duration::from_secs(4));
    }

    #[test]
    fn with_highlight_no_toast_no_expiry() {
        let s = ShelfScreen::with_highlight(Some("https://x".into()), None);
        assert!(s.toast.is_none());
        assert!(s.toast_expires_at.is_none());
    }

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

    // ------------------------------------------------------------------
    // INT-entry-shelf-build-01 / INT-entry-shelf-empty-04 /
    // INT-entry-shelf-chapter-not-found-05 — REQ-005 's' wire to
    // SearchPickerScreen with PickerEntry::Shelf + caller-side guards.
    // ------------------------------------------------------------------

    use crate::library::{ChapterMeta as CMeta, ReadProgress};

    /// Seed a richer fixture: 1 novel + 1 source row + N chapters +
    /// optional progress. Returns (ctx, novel_id).
    fn seed_shelf_with_progress(
        novel_name: &str,
        author: &str,
        chapters: Vec<CMeta>,
        progress_idx: Option<i64>,
    ) -> (AppContext, i64) {
        let mut db = LibraryDb::open_in_memory().expect("open in-memory db");
        let novel = Novel {
            id: None,
            source_url: "https://src.example/".into(),
            book_url: "https://book.example/build01".into(),
            name: novel_name.into(),
            author: Some(author.into()),
            intro: None,
            cover_url: None,
            toc_url: None,
        };
        let id = db.upsert_novel(&novel).expect("upsert");
        if !chapters.is_empty() {
            crate::catalog::dao::replace_toc(&mut db, id, &chapters)
                .expect("replace_toc");
        }
        if let Some(pi) = progress_idx {
            db.save_progress(&ReadProgress {
                novel_id: id,
                chapter_index: pi,
                scroll_offset: 0,
            })
            .expect("save progress");
        }
        let scraper =
            crate::catalog::service::scraper::Scraper::new().expect("scraper init");
        let ctx = AppContext { db, scraper, config: Config::default() };
        (ctx, id)
    }

    // INT-entry-shelf-build-01:
    // shelf has highlight + progress.chapter_index → chapters[idx] exists →
    // classify_s_press returns OpenPicker with anchor fields.
    #[tokio::test]
    async fn int_entry_shelf_build_01() {
        let chapters = vec![
            CMeta { index: 0, name: "第一章".into(), url: "u/0".into() },
            CMeta { index: 1, name: "第二章".into(), url: "u/1".into() },
            CMeta { index: 2, name: "出關".into(), url: "u/2".into() },
            CMeta { index: 3, name: "第四章".into(), url: "u/3".into() },
        ];
        let (ctx, id) = seed_shelf_with_progress("天龍八部", "金庸", chapters, Some(2));
        let mut shelf = ShelfScreen::new();
        shelf.refresh(&ctx).expect("refresh");
        assert_eq!(shelf.list_state.selected(), Some(0));

        let action = super::classify_s_press(&ctx, &shelf.list_state, &shelf.novels);
        match action {
            super::ShelfSAction::OpenPicker {
                novel_id,
                book_name,
                author,
                old_chapter_idx,
                old_chapter_name,
            } => {
                assert_eq!(novel_id, id);
                assert_eq!(book_name, "天龍八部");
                assert_eq!(author, "金庸");
                assert_eq!(old_chapter_idx, 2);
                assert_eq!(old_chapter_name, "出關");
            }
            other => panic!("應為 OpenPicker，實際 = {:?}", other),
        }
    }

    // INT-entry-shelf-empty-04: empty shelf → press 's' → Transition::Stay.
    #[tokio::test]
    async fn int_entry_shelf_empty_04() {
        let db = LibraryDb::open_in_memory().expect("open in-memory db");
        let scraper =
            crate::catalog::service::scraper::Scraper::new().expect("scraper init");
        let mut ctx = AppContext { db, scraper, config: Config::default() };
        let mut shelf = ShelfScreen::new();
        shelf.refresh(&ctx).expect("refresh");
        assert!(shelf.novels.is_empty());
        assert_eq!(shelf.list_state.selected(), None);

        let t = shelf.handle_event(press(KeyCode::Char('s')), &mut ctx).await;
        assert!(matches!(t, Transition::Stay), "空 shelf 's' 應 Stay");
    }

    // INT-entry-shelf-chapter-not-found-05:
    // progress.chapter_index 對應 chapter row 不存在 → classify_s_press
    // returns ToastBackToShelf with 「找不到舊章節」.
    #[tokio::test]
    async fn int_entry_shelf_chapter_not_found_05() {
        let chapters = vec![
            CMeta { index: 0, name: "第一章".into(), url: "u/0".into() },
            CMeta { index: 1, name: "第二章".into(), url: "u/1".into() },
            CMeta { index: 2, name: "第三章".into(), url: "u/2".into() },
        ];
        // progress.chapter_index=99 — chapters table has no idx=99 row
        let (mut ctx, _id) =
            seed_shelf_with_progress("壞資料書", "無名", chapters, Some(99));
        let mut shelf = ShelfScreen::new();
        shelf.refresh(&ctx).expect("refresh");

        let action = super::classify_s_press(&ctx, &shelf.list_state, &shelf.novels);
        match action {
            super::ShelfSAction::ToastBackToShelf { toast } => {
                assert!(
                    toast.contains("找不到舊章節"),
                    "toast 應含「找不到舊章節」，實際 = {}",
                    toast
                );
            }
            other => panic!("應為 ToastBackToShelf，實際 = {:?}", other),
        }

        // 並驗 handle_event 也走 Transition::To（不開 picker、留 shelf）
        let t = shelf.handle_event(press(KeyCode::Char('s')), &mut ctx).await;
        assert!(
            matches!(t, Transition::To(_)),
            "chapter-not-found 's' 應 Transition::To（帶 toast 的 shelf）"
        );
    }

    // ------------------------------------------------------------------
    // INT-trait-02: shelf 收 Event::Mouse(Down(Left)) → Transition::Stay
    // ------------------------------------------------------------------
    #[tokio::test]
    async fn int_trait_02_shelf_mouse_stay() {
        use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        let (mut ctx, _id, _book_url) = test_ctx_with_one_novel();
        let mut shelf = ShelfScreen::new();
        shelf.refresh(&ctx).expect("refresh");
        let before_selected = shelf.list_state.selected();
        let mouse = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 5,
            row: 5,
            modifiers: KeyModifiers::empty(),
        });
        let t = shelf.handle_event(mouse, &mut ctx).await;
        assert!(matches!(t, Transition::Stay), "Mouse 事件應回 Stay");
        assert_eq!(
            shelf.list_state.selected(),
            before_selected,
            "Mouse 事件不該影響 list_state"
        );
    }
}
