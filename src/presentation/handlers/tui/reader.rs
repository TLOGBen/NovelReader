//! ReaderScreen — TUI 兩 pane 閱讀器（搬遷自 v1 `presentation/reader.rs`）。
//!
//! REQ-006：構造時帶 `EntryMode`，`m` 鍵依入口分流：
//!   - `EntryMode::Menu`         → `Transition::To(MenuScreen::new())`（回主菜單）
//!   - `EntryMode::DirectReader` → `Transition::Quit`（exit process）
//!
//! `q` 鍵在本實作中與 `m` 相同（task constraint line 170）— 維持 v1
//! 「q 是退出」的手感，Menu 模式下兩鍵都回主菜單、Direct 模式下兩鍵都 exit。
//!
//! 既有 v1 鍵綁定（j/k/J/K/Space/PgUp/PgDn/n/p/Tab/g/G/Enter/Up/Down）行為不變。
//! 章節 fetch 依然 inline `await` —— `Screen::handle_event` 已 async 化。
//! `save_progress` 時機：每次章節切換、`m`/`q` 觸發 transition 時皆呼一次。

use anyhow::{anyhow, Result};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::catalog;
use crate::catalog::BookSource;
use crate::library;
use crate::library::{ChapterMeta, Novel, ReadProgress};
use crate::presentation::handlers::tui::{
    menu::MenuScreen, EntryMode, Screen, Transition,
};
use crate::presentation::AppContext;

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Focus {
    Toc,
    Content,
}

pub struct ReaderScreen {
    pub entry_mode: EntryMode,
    pub novel_id: i64,
    pub novel: Novel,
    pub chapters: Vec<ChapterMeta>,
    pub toc_state: ListState,
    pub current: usize,
    pub content: Vec<String>,
    pub raw_content: String,
    pub scroll: u16,
    pub focus: Focus,
    pub status: String,
    /// content area height cached on last draw（用來計算翻頁步幅）。
    content_area_h: u16,
}

impl ReaderScreen {
    /// 構造 + 預載：抓 novel / chapters / progress，inline await 第一章內容。
    ///
    /// 與 v1 `reader::run` pre-load 階段（line 66-86）等價，只是把後續的
    /// terminal setup / event loop 都委派給 `run_loop`。
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
        let src = catalog::facade::get_source(&ctx.db, &novel.source_url)?
            .ok_or_else(|| anyhow!("書源不存在: {}", novel.source_url))?;

        let progress = library::facade::get_progress(&ctx.db, novel_id)?;
        let start_idx = progress
            .as_ref()
            .map(|p| p.chapter_index as usize)
            .unwrap_or(0);
        let start_scroll = progress.as_ref().map(|p| p.scroll_offset).unwrap_or(0);

        let mut toc_state = ListState::default();
        toc_state.select(Some(start_idx.min(chapters.len().saturating_sub(1))));

        let mut screen = Self {
            entry_mode,
            novel_id,
            novel,
            chapters,
            toc_state,
            current: start_idx,
            content: Vec::new(),
            raw_content: String::new(),
            scroll: 0,
            focus: Focus::Toc,
            status: "載入中…".into(),
            content_area_h: 20,
        };
        screen.load_chapter(ctx, &src, start_idx).await;
        screen.scroll = start_scroll;
        Ok(screen)
    }

    /// 載入章節 idx 到 self.content / raw_content；inline await fetch on cache miss。
    /// 行為對齊 v1 `load_chapter`（line 225-262）。
    async fn load_chapter(&mut self, ctx: &mut AppContext, src: &BookSource, idx: usize) {
        self.current = idx;
        self.toc_state.select(Some(idx));
        let Some(meta) = self.chapters.get(idx).cloned() else {
            self.status = "章節索引超界".into();
            return;
        };

        let content = match library::facade::get_chapter(&ctx.db, self.novel_id, idx as i64) {
            Ok(Some(c)) => Some(c.content),
            _ => None,
        };
        let text = match content {
            Some(c) => c,
            None => match catalog::facade::fetch_chapter_content(&ctx.scraper, src, &meta.url).await {
                Ok(c) => {
                    let _ = library::facade::save_chapter_content(
                        &mut ctx.db,
                        self.novel_id,
                        idx as i64,
                        &c,
                    );
                    c
                }
                Err(e) => {
                    self.status = format!("抓取失敗: {e}");
                    self.raw_content = String::new();
                    self.content = Vec::new();
                    return;
                }
            },
        };
        self.raw_content = text.clone();
        self.content = text.lines().map(|s| s.to_string()).collect();
        self.status = format!("第 {}/{} 章", idx + 1, self.chapters.len());
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
        // cache content area height for J/K paging step。
        self.content_area_h = area.height.saturating_sub(3);
        draw(frame, area, self);
    }

    async fn handle_event(&mut self, key: KeyEvent, ctx: &mut AppContext) -> Transition {
        // chapter_change: 若有設值，在 match 之後 inline await 切換章節。
        let mut chapter_change: Option<usize> = None;

        match key.code {
            KeyCode::Char('q') | KeyCode::Char('m') => {
                self.save_progress(ctx);
                return self.exit_transition();
            }
            KeyCode::Tab => {
                self.focus = if self.focus == Focus::Toc {
                    Focus::Content
                } else {
                    Focus::Toc
                };
            }
            KeyCode::Char('j') => match self.focus {
                Focus::Toc => {
                    let next = (self.current + 1).min(self.chapters.len().saturating_sub(1));
                    if next != self.current {
                        chapter_change = Some(next);
                    }
                }
                Focus::Content => {
                    self.scroll = self.scroll.saturating_add(1);
                }
            },
            KeyCode::Char('k') => match self.focus {
                Focus::Toc => {
                    if self.current > 0 {
                        chapter_change = Some(self.current - 1);
                    }
                }
                Focus::Content => {
                    self.scroll = self.scroll.saturating_sub(1);
                }
            },
            KeyCode::Char('J') | KeyCode::PageDown | KeyCode::Char(' ') => {
                self.scroll = self
                    .scroll
                    .saturating_add(self.content_area_h.saturating_sub(2));
            }
            KeyCode::Char('K') | KeyCode::PageUp => {
                self.scroll = self
                    .scroll
                    .saturating_sub(self.content_area_h.saturating_sub(2));
            }
            KeyCode::Char('g') => self.scroll = 0,
            KeyCode::Char('G') => {
                self.scroll = self
                    .content
                    .len()
                    .saturating_sub(self.content_area_h as usize) as u16;
            }
            KeyCode::Char('n') => {
                let next = (self.current + 1).min(self.chapters.len().saturating_sub(1));
                if next != self.current {
                    chapter_change = Some(next);
                }
            }
            KeyCode::Char('p') => {
                if self.current > 0 {
                    chapter_change = Some(self.current - 1);
                }
            }
            KeyCode::Enter if self.focus == Focus::Toc => {
                if let Some(i) = self.toc_state.selected() {
                    if i != self.current {
                        chapter_change = Some(i);
                    }
                }
            }
            KeyCode::Up => {
                if self.focus == Focus::Toc {
                    let i = self.toc_state.selected().unwrap_or(0).saturating_sub(1);
                    self.toc_state.select(Some(i));
                } else {
                    self.scroll = self.scroll.saturating_sub(1);
                }
            }
            KeyCode::Down => {
                if self.focus == Focus::Toc {
                    let i = (self.toc_state.selected().unwrap_or(0) + 1)
                        .min(self.chapters.len().saturating_sub(1));
                    self.toc_state.select(Some(i));
                } else {
                    self.scroll = self.scroll.saturating_add(1);
                }
            }
            _ => {}
        }

        if let Some(next) = chapter_change {
            // 1) 為離開的章節儲存進度。
            self.save_progress(ctx);
            self.status = format!("載入第 {} 章…", next + 1);
            // 2) 取書源（不存在直接顯示錯誤、不換章）。
            let src = match catalog::facade::get_source(&ctx.db, &self.novel.source_url) {
                Ok(Some(s)) => s,
                _ => {
                    self.status = format!("書源不存在: {}", self.novel.source_url);
                    return Transition::Stay;
                }
            };
            // 3) inline await fetch。
            self.load_chapter(ctx, &src, next).await;
            self.scroll = 0;
        }

        Transition::Stay
    }
}

// ---------------------------------------------------------------------------
// draw / helpers — 1:1 從 v1 reader.rs 搬遷（line 264-338）。
// ---------------------------------------------------------------------------

fn draw(f: &mut Frame, area: Rect, app: &ReaderScreen) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(28), Constraint::Min(20)])
        .split(chunks[0]);

    // Left: TOC
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
    let list = List::new(items)
        .block(toc_block)
        .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
    let mut state = app.toc_state.clone();
    f.render_stateful_widget(list, body[0], &mut state);

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
    let para = Paragraph::new(app.raw_content.as_str())
        .block(content_block)
        .wrap(Wrap { trim: false })
        .scroll((app.scroll, 0));
    f.render_widget(para, body[1]);

    // Status bar
    let pct = if app.content.is_empty() {
        0
    } else {
        ((app.scroll as f32 / app.content.len().max(1) as f32) * 100.0) as u16
    };
    let status = Line::from(vec![
        Span::styled(
            format!(" {} ", app.status),
            Style::default().fg(Color::Black).bg(Color::Cyan),
        ),
        Span::raw("  "),
        Span::raw(format!("{pct}%")),
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
// Tests — UNIT-4a / UNIT-4b（m 鍵 EntryMode 分流）。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::library::dao::LibraryDb;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    /// 構造一個僅供測試使用的 AppContext（in-memory DB）。
    fn test_ctx() -> AppContext {
        let db = LibraryDb::open_in_memory().expect("open in-memory db");
        let scraper =
            crate::catalog::service::scraper::Scraper::new().expect("scraper init");
        let config = Config::default();
        AppContext { db, scraper, config }
    }

    /// 直接構造 ReaderScreen 不走 ctor — UT 不需要真 DB / 章節列表。
    fn mock_reader(mode: EntryMode) -> ReaderScreen {
        ReaderScreen {
            entry_mode: mode,
            novel_id: 1,
            novel: Novel {
                id: Some(1),
                source_url: "x".into(),
                book_url: "y".into(),
                name: "n".into(),
                author: None,
                intro: None,
                cover_url: None,
                toc_url: None,
            },
            chapters: vec![],
            toc_state: ListState::default(),
            current: 0,
            content: vec![],
            raw_content: String::new(),
            scroll: 0,
            focus: Focus::Toc,
            status: "test".into(),
            content_area_h: 20,
        }
    }

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
    }

    /// UNIT-4a: Reader m 鍵 + EntryMode::Menu → Transition::To(_)
    #[tokio::test]
    async fn unit4a_menu_mode_m_to_menu() {
        let mut r = mock_reader(EntryMode::Menu);
        let mut ctx = test_ctx();
        let t = r.handle_event(press(KeyCode::Char('m')), &mut ctx).await;
        assert!(matches!(t, Transition::To(_)));
    }

    /// UNIT-4b: Reader m 鍵 + EntryMode::DirectReader → Transition::Quit
    #[tokio::test]
    async fn unit4b_direct_mode_m_quits() {
        let mut r = mock_reader(EntryMode::DirectReader);
        let mut ctx = test_ctx();
        let t = r.handle_event(press(KeyCode::Char('m')), &mut ctx).await;
        assert!(matches!(t, Transition::Quit));
    }
}
