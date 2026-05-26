//! ratatui TUI reader.
//! ReaderApp 內含 reading session state (current_chapter, scroll, content cache) —
//! 未來若拆出 Reading bounded context (OQ-2: annotation / highlight / 多 session) 會搬出。
//! 目前居此處 with full Reading concept inline.

use anyhow::{anyhow, Result};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Terminal,
};
use ratatui::backend::CrosstermBackend;
use std::io;
use std::time::Duration;

use crate::catalog;
use crate::catalog::BookSource;
use crate::catalog::service::scraper::Scraper;
use crate::library;
use crate::library::dao::LibraryDb;
use crate::library::{ChapterMeta, Novel, ReadProgress};

#[derive(PartialEq, Eq, Clone, Copy)]
enum Focus {
    Toc,
    Content,
}

struct App {
    novel: Novel,
    chapters: Vec<ChapterMeta>,
    toc_state: ListState,
    current: usize,
    content: Vec<String>, // wrapped lines per current chapter
    raw_content: String,
    scroll: u16,
    focus: Focus,
    status: String,
}

impl App {
    fn new(novel: Novel, chapters: Vec<ChapterMeta>, start: usize) -> Self {
        let mut toc_state = ListState::default();
        toc_state.select(Some(start.min(chapters.len().saturating_sub(1))));
        Self {
            novel,
            chapters,
            toc_state,
            current: start,
            content: Vec::new(),
            raw_content: String::new(),
            scroll: 0,
            focus: Focus::Toc,
            status: "載入中…".into(),
        }
    }
}

pub async fn run(store: &mut LibraryDb, novel_id: i64) -> Result<()> {
    let novel = library::facade::get_novel(store, novel_id)?
        .ok_or_else(|| anyhow!("找不到小說 #{novel_id}"))?;
    let chapters = library::facade::list_chapters(store, novel_id)?;
    if chapters.is_empty() {
        anyhow::bail!("尚無章節，請先 `sync {novel_id}`");
    }
    let src = catalog::facade::get_source(store, &novel.source_url)?
        .ok_or_else(|| anyhow!("書源不存在: {}", novel.source_url))?;

    let progress = library::facade::get_progress(store, novel_id)?;
    let start_idx = progress.as_ref().map(|p| p.chapter_index as usize).unwrap_or(0);
    let start_scroll = progress.as_ref().map(|p| p.scroll_offset).unwrap_or(0);

    let scraper = Scraper::new()?;

    // Pre-load first chapter (synchronously, blocking) before drawing.
    let mut app = App::new(novel, chapters, start_idx);
    load_chapter(&mut app, store, &src, &scraper, novel_id, start_idx).await;
    app.scroll = start_scroll;

    // Enter TUI.
    enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(out, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(out);
    let mut term = Terminal::new(backend)?;

    let result = event_loop(&mut term, &mut app, store, &src, &scraper, novel_id).await;

    // Restore terminal.
    disable_raw_mode()?;
    execute!(term.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    term.show_cursor()?;

    // Save progress.
    library::facade::save_progress(store, &ReadProgress {
        novel_id,
        chapter_index: app.current as i64,
        scroll_offset: app.scroll,
    })?;

    result
}

async fn event_loop<B: ratatui::backend::Backend>(
    term: &mut Terminal<B>,
    app: &mut App,
    store: &mut LibraryDb,
    src: &BookSource,
    scraper: &Scraper,
    novel_id: i64,
) -> Result<()> {
    let mut content_area_h: u16 = 20;
    loop {
        term.draw(|f| {
            let size = f.area();
            draw(f, size, app);
            content_area_h = size.height.saturating_sub(3);
        })?;

        if !event::poll(Duration::from_millis(200))? {
            continue;
        }
        let Event::Key(key) = event::read()? else { continue };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        let mut chapter_change: Option<usize> = None;
        match key.code {
            KeyCode::Char('q') => break,
            KeyCode::Tab => {
                app.focus = if app.focus == Focus::Toc { Focus::Content } else { Focus::Toc };
            }
            KeyCode::Char('j') => match app.focus {
                Focus::Toc => {
                    let next = (app.current + 1).min(app.chapters.len().saturating_sub(1));
                    if next != app.current {
                        chapter_change = Some(next);
                    }
                }
                Focus::Content => {
                    app.scroll = app.scroll.saturating_add(1);
                }
            },
            KeyCode::Char('k') => match app.focus {
                Focus::Toc => {
                    if app.current > 0 {
                        chapter_change = Some(app.current - 1);
                    }
                }
                Focus::Content => {
                    app.scroll = app.scroll.saturating_sub(1);
                }
            },
            KeyCode::Char('J') | KeyCode::PageDown | KeyCode::Char(' ') => {
                app.scroll = app.scroll.saturating_add(content_area_h.saturating_sub(2));
            }
            KeyCode::Char('K') | KeyCode::PageUp => {
                app.scroll = app.scroll.saturating_sub(content_area_h.saturating_sub(2));
            }
            KeyCode::Char('g') => app.scroll = 0,
            KeyCode::Char('G') => {
                app.scroll = app.content.len().saturating_sub(content_area_h as usize) as u16;
            }
            KeyCode::Char('n') => {
                let next = (app.current + 1).min(app.chapters.len().saturating_sub(1));
                if next != app.current {
                    chapter_change = Some(next);
                }
            }
            KeyCode::Char('p') => {
                if app.current > 0 {
                    chapter_change = Some(app.current - 1);
                }
            }
            KeyCode::Enter if app.focus == Focus::Toc => {
                if let Some(i) = app.toc_state.selected() {
                    if i != app.current {
                        chapter_change = Some(i);
                    }
                }
            }
            KeyCode::Up => {
                if app.focus == Focus::Toc {
                    let i = app.toc_state.selected().unwrap_or(0).saturating_sub(1);
                    app.toc_state.select(Some(i));
                } else {
                    app.scroll = app.scroll.saturating_sub(1);
                }
            }
            KeyCode::Down => {
                if app.focus == Focus::Toc {
                    let i = (app.toc_state.selected().unwrap_or(0) + 1)
                        .min(app.chapters.len().saturating_sub(1));
                    app.toc_state.select(Some(i));
                } else {
                    app.scroll = app.scroll.saturating_add(1);
                }
            }
            _ => {}
        }

        if let Some(next) = chapter_change {
            // Save progress for the chapter we're leaving.
            library::facade::save_progress(store, &ReadProgress {
                novel_id,
                chapter_index: app.current as i64,
                scroll_offset: app.scroll,
            })?;
            app.status = format!("載入第 {} 章…", next + 1);
            term.draw(|f| draw(f, f.area(), app))?;
            load_chapter(app, store, src, scraper, novel_id, next).await;
            app.scroll = 0;
        }
    }
    Ok(())
}

async fn load_chapter(
    app: &mut App,
    store: &mut LibraryDb,
    src: &BookSource,
    scraper: &Scraper,
    novel_id: i64,
    idx: usize,
) {
    app.current = idx;
    app.toc_state.select(Some(idx));
    let Some(meta) = app.chapters.get(idx).cloned() else {
        app.status = "章節索引超界".into();
        return;
    };

    let content = match library::facade::get_chapter(store, novel_id, idx as i64) {
        Ok(Some(c)) => Some(c.content),
        _ => None,
    };
    let text = match content {
        Some(c) => c,
        None => match catalog::facade::fetch_chapter_content(scraper, src, &meta.url).await {
            Ok(c) => {
                let _ = library::facade::save_chapter_content(store, novel_id, idx as i64, &c);
                c
            }
            Err(e) => {
                app.status = format!("抓取失敗: {e}");
                app.raw_content = String::new();
                app.content = Vec::new();
                return;
            }
        },
    };
    app.raw_content = text.clone();
    app.content = text.lines().map(|s| s.to_string()).collect();
    app.status = format!("第 {}/{} 章", idx + 1, app.chapters.len());
}

fn draw(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(28), Constraint::Min(20)])
        .split(chunks[0]);

    // Left: TOC
    let items: Vec<ListItem> = app.chapters.iter().enumerate().map(|(i, c)| {
        let prefix = if i == app.current { "▶ " } else { "  " };
        ListItem::new(format!("{prefix}{}", truncate(&c.name, 24)))
    }).collect();
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
    let title = app.chapters.get(app.current).map(|c| c.name.clone()).unwrap_or_default();
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
            " j/k 章節  J/K 翻頁  n/p 下/上章  Tab 切換  g/G 頭尾  q 離開 ",
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
