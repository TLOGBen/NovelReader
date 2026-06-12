//! TUI subcommand handler + main-menu shell (Screen trait routing).
//!
//! This module hosts two things:
//!   1. The `handle(novel_id, ctx)` entry point invoked by
//!      `presentation::cli::run` for `novel-looker tui <id>`. It constructs
//!      a `ReaderScreen` with `EntryMode::DirectReader` and dispatches to
//!      `run_loop` (REQ-006 — `m` 鍵在 DirectReader mode 退出 process)。
//!   2. The TUI shell scaffolding: `Screen` trait, `Transition` enum,
//!      `EntryMode` enum, `App` struct, `run_loop`, and the `RawTerm` RAII
//!      guard. Wired up by TASK-tui-01 (MenuScreen) and TASK-tui-02
//!      (ReaderScreen)。
//!
//! `StubMenuScreen` 仍保留作為 fallback / 教學用，加 `#[allow(dead_code)]`
//! —— 與 `catalog/service/rule.rs::select_within` 同樣的 dead-code pattern。

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    widgets::Paragraph,
    Frame, Terminal,
};
use std::io;
use std::time::Duration;

use crate::presentation::AppContext;

/// Default TTL for toasts displayed on transient screens (delete-success,
/// switch-source-success, search-add-success). 3s 是 /think 2026-05-27 KD #4
/// 取的折衷 — 200ms event-poll tick 容忍下實際延遲約 3.2s。共用於
/// [`MenuScreen`] 與 [`ShelfScreen`]，避免 sibling 反向依賴。
pub(crate) const TOAST_TTL: Duration = Duration::from_secs(3);

pub mod widgets;
pub mod menu;
pub mod reader;
pub mod search;
pub mod shelf;
pub mod delete_confirm;
pub mod picker;

// ============================================================================
// CLI entry point for `novel-looker tui <id>` — DirectReader mode.
// ============================================================================
//
// Takes `ctx` by value because `App` owns `AppContext` (run_loop / screens
// access it via `&mut app.ctx`)。對應 cli.rs 的 dispatch arm 同樣 by-value。

pub async fn handle(novel_id: i64, ctx: AppContext) -> Result<()> {
    let app = App::new_with_direct_reader(ctx, novel_id).await?;
    run_loop(app).await
}

// ============================================================================
// Shell scaffolding: Screen / Transition / EntryMode / App / run_loop.
// ============================================================================

/// 啟動入口 — 決定 reader 中 `m` 鍵語意（見 REQ-006）。
///
/// - `Menu`: 經主菜單進入 reader；`m` → 回主菜單。
/// - `DirectReader`: 經 `novel-looker tui <id>` 直入 reader；`m` → exit process。
///
/// 必須是 `Copy` 以便建構 reader screen 時抄入而非搬走。
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryMode {
    Menu,
    DirectReader,
}

/// 單軌路由的 transition 結果（見 REQ-002）。
///
/// `Screen::handle_event` 回傳這個 enum 告訴 event loop 該做什麼：
/// - `To(next)`: 替換 `App.current` 為 `next`。
/// - `Stay`: 保留 `App.current`，繼續下一輪 draw / event。
/// - `Quit`: 結束 event loop、清理 terminal、process exit 0。
#[allow(dead_code)]
pub enum Transition {
    To(Box<dyn Screen>),
    Stay,
    Quit,
}

/// 所有 TUI screen 共用的介面。
///
/// `draw` 渲染當前畫面到 `Frame`；`handle_event` 收一個按鍵事件並回傳
/// `Transition`。事件 polling、terminal 生命週期都由 `run_loop` 負責，
/// screen 實作不需要碰。
///
/// `handle_event` 是 async 因為 reader screen 在切章節時需要 inline
/// `await` fetch_chapter_content（與 v1 reader.rs 行為一致）。用
/// `#[async_trait(?Send)]` 因為 `run_loop` 是單執行緒，不需要 Send bound
/// 也避開 `Scraper` / `LibraryDb` 等型別未實作 `Send` 的限制。
#[allow(dead_code)]
#[async_trait::async_trait(?Send)]
pub trait Screen {
    fn draw(&mut self, frame: &mut Frame, ctx: &AppContext);
    async fn handle_event(&mut self, event: Event, ctx: &mut AppContext) -> Transition;
}

/// Test seam — abstract event source for `run_inner`.
///
/// Production impl ([`CrosstermEventSource`]) wraps `crossterm::event::poll` /
/// `event::read`. Tests provide a `Vec<Event>`-backed mock so we can drive
/// `run_inner` deterministically (INT-trait-04 / INT-trait-05).
#[allow(dead_code)]
pub(crate) trait EventSource {
    fn poll(&mut self, dur: Duration) -> Result<bool>;
    fn read(&mut self) -> Result<Event>;
}

/// Production [`EventSource`] — thin wrapper around crossterm's polling API.
#[allow(dead_code)]
pub(crate) struct CrosstermEventSource;

impl EventSource for CrosstermEventSource {
    fn poll(&mut self, dur: Duration) -> Result<bool> {
        Ok(event::poll(dur)?)
    }
    fn read(&mut self) -> Result<Event> {
        Ok(event::read()?)
    }
}

/// Test seam — abstract terminal-like draw target for `run_inner`.
///
/// Production uses [`ratatui::Terminal`]; tests can provide a no-op impl
/// that swallows `draw_frame` (INT-trait-04 / INT-trait-05 don't assert on
/// rendered output, only on event forwarding).
#[allow(dead_code)]
pub(crate) trait TerminalLike {
    /// Invoke `f` with a [`Frame`]; the production impl borrows the
    /// internal `Terminal` and forwards to `Terminal::draw`. Test impls
    /// may skip drawing entirely (return Ok(())) since UT for `run_inner`
    /// only cares about event dispatch.
    fn draw_frame(&mut self, f: &mut dyn FnMut(&mut Frame)) -> Result<()>;
}

impl TerminalLike for RawTerm {
    fn draw_frame(&mut self, f: &mut dyn FnMut(&mut Frame)) -> Result<()> {
        self.terminal.draw(|frame| f(frame))?;
        Ok(())
    }
}

/// TUI app state。
///
/// 三欄位 owned 結構：`current` 為當前 screen，`entry_mode` 記錄啟動入口
/// （見 REQ-006），`ctx` 為 `AppContext`（move 進來，含 `LibraryDb` /
/// `Scraper` / `Config`）。`AppContext` 不需要 `Clone` —— ownership transfer
/// 即可：bootstrap 建出來後直接 move 進 `App`，screen 透過 `&mut app.ctx`
/// 操作。
///
/// 既有 v1 `handlers::tui::handle(novel_id, ctx: &mut AppContext)` 是兩條
/// 獨立路徑：v1 用借用、新 `run_loop` 走 owned。
#[allow(dead_code)]
pub struct App {
    pub current: Box<dyn Screen>,
    pub entry_mode: EntryMode,
    pub ctx: AppContext,
}

impl App {
    #[allow(dead_code)]
    pub fn new(current: Box<dyn Screen>, entry_mode: EntryMode, ctx: AppContext) -> Self {
        Self { current, entry_mode, ctx }
    }

    /// Convenience ctor — start with `MenuScreen`, `EntryMode::Menu`.
    ///
    /// 由 `presentation::handlers::menu::handle`（`novel-looker` 無子命令入口）
    /// 呼叫，封裝「Box<MenuScreen> + EntryMode::Menu」這對固定組合。
    pub fn new_with_menu(ctx: AppContext) -> Self {
        Self::new(Box::new(menu::MenuScreen::new()), EntryMode::Menu, ctx)
    }

    /// Convenience ctor — start with `ReaderScreen`, `EntryMode::DirectReader`.
    ///
    /// 由 `presentation::handlers::tui::handle`（`novel-looker tui <id>` 入口）
    /// 呼叫，封裝「pre-load ReaderScreen + EntryMode::DirectReader」流程；
    /// `ReaderScreen::new` 借 `&mut ctx` 完成 inline await 後再把 ctx move
    /// 進 `App`。
    pub async fn new_with_direct_reader(mut ctx: AppContext, novel_id: i64) -> Result<Self> {
        let reader =
            reader::ReaderScreen::new(EntryMode::DirectReader, &mut ctx, novel_id).await?;
        Ok(Self::new(Box::new(reader), EntryMode::DirectReader, ctx))
    }
}

/// RAII guard: enable raw mode + alternate screen on `new`, restore on `Drop`.
///
/// 與 `presentation/reader.rs` 的 setup/teardown 對稱（見該檔 line 88-99）。
/// `Drop` 是 best-effort —— 失敗也不能 panic，因為我們可能本來就在 panic
/// 路徑上清理。
#[allow(dead_code)]
struct RawTerm {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl RawTerm {
    #[allow(dead_code)]
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        let mut out = io::stdout();
        execute!(out, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(out);
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal })
    }
}

impl Drop for RawTerm {
    fn drop(&mut self) {
        // best-effort cleanup; ignore individual errors so we still attempt
        // all three steps even if one fails.
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        );
        let _ = self.terminal.show_cursor();
    }
}

/// 安裝 panic hook，確保 panic 路徑下 terminal 也能被清理。
///
/// 雙保險：`RawTerm::Drop` 處理正常 + 受控結束；panic hook 處理 abort 路徑
/// 中 stack unwind 不會跑到 Drop 的情況（例如 `panic = "abort"` profile）。
#[allow(dead_code)]
fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        original(info);
    }));
}

/// 主事件迴圈骨架。
///
/// 流程：enter raw terminal → loop { draw → poll → dispatch → 根據
/// Transition 切換 / Stay / break } → Drop 自動清理 terminal。
///
/// 只接 owned `App`，內部用 `app.ctx` / `&mut app.ctx` 操作（screens
/// 之後拿到 `&mut app.ctx` 來呼叫 facades）。
///
/// 注意：`app` 用 `let mut`，因為 `Transition::To` 需要替換 `app.current`。
#[allow(dead_code)]
pub async fn run_loop(app: App) -> Result<()> {
    install_panic_hook();
    let mut term = RawTerm::enter()?;
    let mut events = CrosstermEventSource;
    run_inner(app, &mut events, &mut term).await
}

/// Testable seam — the real event loop. Same behaviour as the previous
/// monolithic `run_loop`, but `events` and `term` are injected so tests can
/// feed canned events (INT-trait-04 / INT-trait-05).
///
/// Behaviour:
/// - `Event::Key` with `kind == KeyEventKind::Press` → forward to screen.
/// - Other `Event::Key` kinds (Release / Repeat) → ignore (`continue`).
/// - `Event::Mouse(_)` → forward to screen (REQ-001 S2; previously dropped).
/// - `Event::Resize / Paste / FocusGained / FocusLost` → forward to screen.
///   Most screens treat these as no-ops; the reader uses Paste to extend its
///   Filter-mode query, and Resize is absorbed by per-frame layout recompute.
///   (Supersedes the original REQ-001 S2 "do not forward" clause — see README
///   TODO "resize / paste / focus event 處理".)
#[allow(dead_code)]
pub(crate) async fn run_inner<E: EventSource, T: TerminalLike>(
    app: App,
    events: &mut E,
    term: &mut T,
) -> Result<()> {
    let mut app = app;

    loop {
        // Split-borrow trick: take disjoint &mut references to `current` and
        // `ctx` so we can pass `&ctx` into the draw closure (which would
        // otherwise conflict with `&mut current`).
        let App { current, ctx, .. } = &mut app;
        let ctx_ref: &AppContext = ctx;
        term.draw_frame(&mut |f| current.draw(f, ctx_ref))?;

        if !events.poll(Duration::from_millis(200))? {
            continue;
        }
        let event = events.read()?;
        // Forward everything to the current screen EXCEPT non-Press key events
        // (Release / Repeat) which would double-fire bindings. Mouse / Resize /
        // Paste / Focus are all forwarded; screens that don't care return
        // `Transition::Stay` (their `handle_event` has a catch-all arm).
        if let Event::Key(key) = &event {
            if key.kind != KeyEventKind::Press {
                continue;
            }
        }

        match current.handle_event(event, ctx).await {
            Transition::Stay => {}
            Transition::To(next) => {
                *current = next;
            }
            Transition::Quit => break,
        }
    }

    Ok(())
}

// ============================================================================
// StubMenuScreen — placeholder so `cargo build` passes; TASK-tui-01 will
// replace this with the real MenuScreen in `menu.rs`.
// ============================================================================

#[allow(dead_code)]
pub struct StubMenuScreen;

#[async_trait::async_trait(?Send)]
impl Screen for StubMenuScreen {
    fn draw(&mut self, frame: &mut Frame, _ctx: &AppContext) {
        let p = Paragraph::new("Stub menu — TASK-tui-01 will replace this");
        frame.render_widget(p, frame.area());
    }

    async fn handle_event(&mut self, event: Event, _ctx: &mut AppContext) -> Transition {
        use crossterm::event::KeyCode;
        match event {
            Event::Key(key) => match key.code {
                KeyCode::Char('q') => Transition::Quit,
                _ => Transition::Stay,
            },
            Event::Mouse(_) => Transition::Stay,
            _ => Transition::Stay,
        }
    }
}

// ============================================================================
// Tests — INT-trait-04 / INT-trait-05 (run_inner event dispatch invariants)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };

    fn test_ctx() -> AppContext {
        let db = crate::library::dao::LibraryDb::open_in_memory().expect("open in-memory db");
        let scraper = crate::catalog::service::scraper::Scraper::new().expect("scraper init");
        let config = Config::default();
        AppContext { db, scraper, config }
    }

    // -- Mock EventSource ---------------------------------------------------

    struct MockEvents {
        queue: std::collections::VecDeque<Event>,
    }

    impl MockEvents {
        fn new(events: Vec<Event>) -> Self {
            Self {
                queue: events.into(),
            }
        }
    }

    impl EventSource for MockEvents {
        fn poll(&mut self, _dur: Duration) -> Result<bool> {
            Ok(!self.queue.is_empty())
        }
        fn read(&mut self) -> Result<Event> {
            self.queue
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("queue empty"))
        }
    }

    // -- Mock TerminalLike ---------------------------------------------------
    // 不實際 draw（UT 不 assert render output）；接 closure 但跳過呼叫即可。
    // 跳過呼叫亦避免「mock screen 沒有真實 draw 邏輯」造成的問題。

    struct NoopTerm;

    impl TerminalLike for NoopTerm {
        fn draw_frame(&mut self, _f: &mut dyn FnMut(&mut Frame)) -> Result<()> {
            Ok(())
        }
    }

    // -- Mock Screen --------------------------------------------------------
    // 記錄收到的 events；達到 quit_after 筆數後回 Quit 結束 run_inner。

    struct RecordingScreen {
        received: std::rc::Rc<std::cell::RefCell<Vec<Event>>>,
        quit_after: usize,
        count: usize,
    }

    impl RecordingScreen {
        fn new(
            received: std::rc::Rc<std::cell::RefCell<Vec<Event>>>,
            quit_after: usize,
        ) -> Self {
            Self {
                received,
                quit_after,
                count: 0,
            }
        }
    }

    #[async_trait::async_trait(?Send)]
    impl Screen for RecordingScreen {
        fn draw(&mut self, _frame: &mut Frame, _ctx: &AppContext) {}
        async fn handle_event(&mut self, event: Event, _ctx: &mut AppContext) -> Transition {
            self.received.borrow_mut().push(event);
            self.count += 1;
            if self.count >= self.quit_after {
                Transition::Quit
            } else {
                Transition::Stay
            }
        }
    }

    /// INT-trait-04: run_inner forwards Mouse events to current screen.
    ///
    /// Queue: [Mouse(ScrollUp), Key('q')(quit sentinel via screen)]
    /// Mock screen quits after receiving 2 events. Verify the Mouse event
    /// is in the received list (NOT dropped by run_inner like the old
    /// `else { continue }` path).
    #[tokio::test]
    async fn int_trait_04_run_loop_forwards_mouse() {
        let received = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Event>::new()));
        let screen = Box::new(RecordingScreen::new(received.clone(), 2));
        let ctx = test_ctx();
        let app = App::new(screen, EntryMode::Menu, ctx);

        let mouse_event = Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 1,
            row: 1,
            modifiers: KeyModifiers::empty(),
        });
        let quit_key = Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::empty()));
        let mut events = MockEvents::new(vec![mouse_event.clone(), quit_key]);
        let mut term = NoopTerm;

        run_inner(app, &mut events, &mut term)
            .await
            .expect("run_inner ok");

        let got = received.borrow();
        assert_eq!(got.len(), 2, "screen 應收到 2 個 events");
        assert!(
            matches!(got[0], Event::Mouse(_)),
            "第一筆應為 Mouse event (run_inner forward 而非 drop)"
        );
        assert!(
            matches!(got[1], Event::Key(_)),
            "第二筆應為 Key event（quit sentinel）"
        );
    }

    /// INT-trait-05: run_inner now FORWARDS Resize / Paste / FocusGained to the
    /// current screen (README TODO "resize / paste / focus event 處理" — this
    /// supersedes the original "ignore" invariant).
    ///
    /// Queue: [Resize, Paste, FocusGained, Key('q')]
    /// Mock screen quits after 4 events. Verify all four were forwarded in order.
    #[tokio::test]
    async fn int_trait_05_run_loop_forwards_resize_paste_focus() {
        let received = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Event>::new()));
        let screen = Box::new(RecordingScreen::new(received.clone(), 4));
        let ctx = test_ctx();
        let app = App::new(screen, EntryMode::Menu, ctx);

        let quit_key = Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::empty()));
        let mut events = MockEvents::new(vec![
            Event::Resize(80, 24),
            Event::Paste("clipboard data".into()),
            Event::FocusGained,
            quit_key,
        ]);
        let mut term = NoopTerm;

        run_inner(app, &mut events, &mut term)
            .await
            .expect("run_inner ok (must not panic on Resize/Paste/FocusGained)");

        let got = received.borrow();
        assert_eq!(
            got.len(),
            4,
            "Resize/Paste/FocusGained + Key 應全部 forward；screen 收到 4 個 event"
        );
        assert!(matches!(got[0], Event::Resize(80, 24)), "第一筆應為 Resize");
        assert!(matches!(got[1], Event::Paste(_)), "第二筆應為 Paste");
        assert!(matches!(got[2], Event::FocusGained), "第三筆應為 FocusGained");
        assert!(matches!(got[3], Event::Key(_)), "第四筆應為 Key（quit sentinel）");
    }

    /// Extra: Key event with kind != Press 仍然被 run_inner ignore（既有行為不退化）。
    /// 這條 hint 是 trait migration 期間順手驗證 existing filter 仍然生效，不在
    /// task ctx 的 5 條 INT-trait 必交清單，但有助 regression coverage。
    #[tokio::test]
    async fn run_loop_ignores_key_release() {
        use crossterm::event::KeyEventState;
        let received = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Event>::new()));
        let screen = Box::new(RecordingScreen::new(received.clone(), 1));
        let ctx = test_ctx();
        let app = App::new(screen, EntryMode::Menu, ctx);

        let release = Event::Key(KeyEvent {
            code: KeyCode::Char('a'),
            modifiers: KeyModifiers::empty(),
            kind: KeyEventKind::Release,
            state: KeyEventState::empty(),
        });
        let quit_key = Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::empty()));
        let mut events = MockEvents::new(vec![release, quit_key]);
        let mut term = NoopTerm;

        run_inner(app, &mut events, &mut term).await.expect("ok");

        let got = received.borrow();
        assert_eq!(got.len(), 1, "Release event 不該 forward");
    }

    // 抑制未使用 import 警告（MouseButton 在這檔 stub 範例給未來複用）
    #[allow(dead_code)]
    fn _silence_unused_mousebutton() -> MouseButton {
        MouseButton::Left
    }
}
