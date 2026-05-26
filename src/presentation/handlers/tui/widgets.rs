//! Common TUI widgets shared across screens.
//!
//! Provides:
//! - `toast(frame, area, msg, kind)` — top-line transient message; Info=blue fg,
//!   Error=red background.
//! - `SingleLineInput` — single-line text input wrapping `tui_textarea::TextArea`.
//!   Intercepts Enter (→ Submit) and Esc (→ Cancel) so the underlying textarea
//!   never inserts a newline. Other keys are forwarded to the textarea.
//! - `SingleLineEvent { Submit(String), Cancel, Edit }` — return type from
//!   `SingleLineInput::handle_event`.
//! - `error_line(text)` — one-line red-fg `Paragraph` for inline error messages.
//!
//! `tui_textarea` is an implementation detail; upstream screens use only
//! `SingleLineInput`. Consumed by SearchScreen / SwitchSourceScreen
//! (REQ-003, REQ-004, REQ-005).

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use tui_textarea::TextArea;

// ---------------------------------------------------------------------------
// Toast
// ---------------------------------------------------------------------------

/// Toast level — determines color scheme used by `toast()`.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Error,
}

/// Render a one-line transient message at the top of `area`.
///
/// - `ToastKind::Info`  → blue foreground on default background.
/// - `ToastKind::Error` → white foreground on red background.
///
/// Caller controls dismissal timing; this fn just draws.
#[allow(dead_code)]
pub fn toast(frame: &mut Frame, area: Rect, msg: &str, kind: ToastKind) {
    if area.height == 0 {
        return;
    }
    let top = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: 1,
    };
    let style = match kind {
        ToastKind::Info => Style::default().fg(Color::Blue),
        ToastKind::Error => Style::default().fg(Color::White).bg(Color::Red),
    };
    let p = Paragraph::new(msg.to_string()).style(style);
    frame.render_widget(p, top);
}

// ---------------------------------------------------------------------------
// SingleLineInput
// ---------------------------------------------------------------------------

/// Outcome of feeding a `KeyEvent` to `SingleLineInput::handle_event`.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SingleLineEvent {
    /// User pressed Enter; payload is the current single-line text.
    Submit(String),
    /// User pressed Esc.
    Cancel,
    /// Any other key was consumed by the textarea (no terminal action).
    Edit,
}

/// Single-line text input with a prompt label.
///
/// Wraps `tui_textarea::TextArea` but never lets a newline be inserted —
/// `handle_event` intercepts `Enter` (→ `Submit`) and `Esc` (→ `Cancel`)
/// before the key reaches the textarea. All other keys flow through, so
/// editing primitives (Backspace, arrow keys, character input, etc.) work
/// as `tui-textarea` defines them.
#[allow(dead_code)]
pub struct SingleLineInput {
    textarea: TextArea<'static>,
    prompt: String,
}

impl SingleLineInput {
    /// Create a new empty single-line input with the given prompt label.
    ///
    /// Prompt is rendered as the block title in `draw`.
    #[allow(dead_code)]
    pub fn new(prompt: impl Into<String>) -> Self {
        let textarea = TextArea::default();
        Self {
            textarea,
            prompt: prompt.into(),
        }
    }

    /// Feed one key event. Returns the resulting transition.
    ///
    /// - `Enter` → `Submit(current_text)` — Enter is intercepted so the
    ///   textarea NEVER inserts a newline. Even after multiple Submits the
    ///   textarea remains single-line.
    /// - `Esc`   → `Cancel`.
    /// - other   → forwarded to `TextArea::input`, returns `Edit`.
    #[allow(dead_code)]
    pub fn handle_event(&mut self, key: KeyEvent) -> SingleLineEvent {
        match key.code {
            KeyCode::Enter => {
                let text = self
                    .textarea
                    .lines()
                    .first()
                    .cloned()
                    .unwrap_or_default();
                SingleLineEvent::Submit(text)
            }
            KeyCode::Esc => SingleLineEvent::Cancel,
            _ => {
                self.textarea.input(key);
                SingleLineEvent::Edit
            }
        }
    }

    /// Read the current input text (the first textarea line) without
    /// consuming it. Useful for callers that want to preview / validate.
    #[allow(dead_code)]
    pub fn text(&self) -> &str {
        self.textarea
            .lines()
            .first()
            .map(|s| s.as_str())
            .unwrap_or("")
    }

    /// Render the input box (bordered, with `prompt` as title) into `area`.
    #[allow(dead_code)]
    pub fn draw(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(self.prompt.as_str());
        // Clone the textarea so we can attach a block (which requires
        // `&mut`) without mutating `self`. tui-textarea 0.6 renders directly
        // via `Frame::render_widget(&TextArea, area)`.
        let mut view = self.textarea.clone();
        view.set_block(block);
        frame.render_widget(&view, area);
    }
}

// ---------------------------------------------------------------------------
// error_line
// ---------------------------------------------------------------------------

/// One-line `Paragraph` rendered with red foreground — for inline error
/// messages (e.g. per-source failures in the search funnel).
#[allow(dead_code)]
pub fn error_line(text: &str) -> Paragraph<'_> {
    Paragraph::new(text).style(Style::default().fg(Color::Red))
}

// ---------------------------------------------------------------------------
// Tests — UNIT-6 (key behaviour of SingleLineInput)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn single_line_input_enter_submits_text() {
        let mut input = SingleLineInput::new("test: ");
        let _ = input.handle_event(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::empty()));
        let _ = input.handle_event(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::empty()));
        let ev = input.handle_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert!(matches!(ev, SingleLineEvent::Submit(ref s) if s == "hi"));
    }

    #[test]
    fn single_line_input_esc_cancels() {
        let mut input = SingleLineInput::new("test: ");
        let ev = input.handle_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));
        assert!(matches!(ev, SingleLineEvent::Cancel));
    }

    #[test]
    fn single_line_input_no_newline_on_multiple_enter() {
        let mut input = SingleLineInput::new("test: ");
        let _ = input.handle_event(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::empty()));
        // 多按 Enter 不會換行 — Enter 一律是 Submit、textarea text 永遠單行
        let _ = input.handle_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        let _ = input.handle_event(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::empty()));
        let ev = input.handle_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        if let SingleLineEvent::Submit(s) = ev {
            assert!(!s.contains('\n'));
        } else {
            panic!("expected Submit");
        }
    }
}
