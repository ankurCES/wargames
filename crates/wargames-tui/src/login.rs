//! Joshua login — WOPR authentication typewriter.
//!
//! The login flow:
//!
//! 1. **Prompt** (`Phase::Prompt`): blinking cursor on `LOGON:`. User
//!    types. On `Enter`, the buffer is compared (case-insensitive) to
//!    `"joshua"`.
//! 2. **Wrong password** (`Phase::Wrong`): typewriter prints
//!    `IDENTIFICATION NOT RECOGNIZED BY SYSTEM` + `--CONNECTION
//!    TERMINATED--`, pauses, then resets to `Prompt`.
//! 3. **Authenticated** (`Phase::Authenticated`): typewriter plays
//!    the movie-accurate greeting sequence: `LOGON: Joshua` →
//!    `GREETINGS, PROFESSOR FALKEN.` → `HOW ARE YOU FEELING TODAY?`
//!    → `EXCELLENT. IT'S BEEN A LONG TIME.` → `CAN YOU EXPLAIN THE
//!    REMOVAL OF YOUR USER ACCOUNT ON 6/23/73?` → `SHALL WE PLAY A
//!    GAME?`. After a 2-second pause on the final line, the screen
//!    advances (`done = true`) and `App::render` flips to the picker.
//!
//! Tick-driven so animation is deterministic in tests. Each render
//! calls `advance_tick()` exactly once.

use crate::ui_anim::TypewriterState;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Wrap};
use ratatui::Frame;

/// Auth-gate state. Drives the login screen.
#[derive(Debug, Clone)]
pub struct LoginState {
    pub phase: Phase,
    pub buffer: String,
    pub cursor_visible: bool,
    /// Current line index within `phase`'s script.
    pub line_index: usize,
    /// Per-line typewriter state. Re-instantiated for each line as
    /// the script advances.
    pub typewriter: TypewriterState,
    /// Tick at which the current line started typing. Used to gate
    /// pauses between lines.
    pub line_started_at_tick: u64,
    /// Tick at which the rejection message started. Drives the
    /// ~1.5s "CONNECTION TERMINATED" hold before reset.
    pub wrong_started_at_tick: u64,
    /// Total ticks elapsed since login screen mounted. Drives the
    /// prompt's cursor blink.
    pub tick: u64,
    /// Set true once the Authenticated greeting finishes and the
    /// final-line pause has elapsed. The app flips to the picker on
    /// the next render.
    pub done: bool,
}

/// Login screen phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Awaiting user input.
    Prompt,
    /// User entered wrong credentials — typewriter printing the
    /// rejection, then auto-resetting back to `Prompt`.
    Wrong,
    /// User typed `Joshua` — typewriter playing the greeting.
    Authenticated,
}

impl Default for LoginState {
    fn default() -> Self {
        Self::new()
    }
}

impl LoginState {
    pub fn new() -> Self {
        Self {
            phase: Phase::Prompt,
            buffer: String::new(),
            cursor_visible: true,
            line_index: 0,
            typewriter: TypewriterState::new(),
            line_started_at_tick: 0,
            wrong_started_at_tick: 0,
            tick: 0,
            done: false,
        }
    }

    /// Push a character into the prompt buffer. Ignored outside
    /// `Phase::Prompt` so the typewriter animation can't be
    /// disrupted.
    pub fn push_char(&mut self, c: char) {
        if self.phase != Phase::Prompt {
            return;
        }
        // Cap at 32 chars — Joshua is short, but typos happen.
        if self.buffer.chars().count() < 32 {
            self.buffer.push(c);
        }
    }

    /// Backspace one character from the prompt buffer.
    pub fn backspace(&mut self) {
        if self.phase != Phase::Prompt {
            return;
        }
        self.buffer.pop();
    }

    /// Submit the buffer. Case-insensitive comparison against
    /// `"joshua"` — anything else falls into `Phase::Wrong`.
    pub fn submit(&mut self) {
        if self.phase != Phase::Prompt {
            return;
        }
        if self.buffer.trim().eq_ignore_ascii_case("joshua") {
            self.phase = Phase::Authenticated;
            // Start the greeting from line 0 (the "LOGON: Joshua" echo).
            self.line_index = 0;
            self.typewriter.reset();
            self.line_started_at_tick = self.tick;
        } else {
            self.phase = Phase::Wrong;
            self.line_index = 0;
            self.typewriter.reset();
            self.line_started_at_tick = self.tick;
            self.wrong_started_at_tick = self.tick;
        }
    }

    /// Advance the typewriter one tick. Called from `App::render`
    /// before drawing. The whole flow is tick-driven so tests can
    /// step it deterministically.
    pub fn advance_tick(&mut self) {
        self.tick = self.tick.saturating_add(1);
        // Cursor blink — 30 ticks on, 30 ticks off (matches the
        // reference repo's blink cadence).
        self.cursor_visible = (self.tick / 15) % 2 == 0;
        match self.phase {
            Phase::Prompt => {
                // Nothing to animate; the cursor blink is the only motion.
            }
            Phase::Wrong => {
                let lines = WRONG_LINES;
                if self.line_index >= lines.len() {
                    // After the rejection prints fully, hold for ~90
                    // ticks (≈1.5s at 60fps) then reset.
                    if self.tick.saturating_sub(self.wrong_started_at_tick) >= 90 {
                        self.reset_to_prompt();
                    }
                    return;
                }
                let current = lines[self.line_index];
                if current.is_empty() {
                    // Blank line — short pause then advance.
                    if self.tick.saturating_sub(self.line_started_at_tick) >= 20 {
                        self.line_index += 1;
                        self.typewriter.reset();
                        self.line_started_at_tick = self.tick;
                    }
                    return;
                }
                let elapsed = self.tick.saturating_sub(self.line_started_at_tick);
                // 2 ticks per character → 30 chars/sec at 60fps.
                let chars_to_show = (elapsed / 2) as usize;
                self.typewriter.char_index =
                    chars_to_show.min(current.chars().count());
                self.typewriter.complete =
                    self.typewriter.char_index >= current.chars().count();
                if self.typewriter.complete {
                    // Pause 40 ticks after the line completes, then
                    // advance to the next.
                    let overshoot = elapsed.saturating_sub(
                        (current.chars().count() as u64) * 2,
                    );
                    if overshoot >= 40 {
                        self.line_index += 1;
                        self.typewriter.reset();
                        self.line_started_at_tick = self.tick;
                    }
                }
            }
            Phase::Authenticated => {
                let lines = AUTH_LINES;
                if self.line_index >= lines.len() {
                    // All lines printed — pause 120 ticks (≈2s)
                    // then mark done.
                    if self.tick.saturating_sub(self.line_started_at_tick) >= 120 {
                        self.done = true;
                    }
                    return;
                }
                let current = lines[self.line_index];
                if current.is_empty() {
                    if self.tick.saturating_sub(self.line_started_at_tick) >= 20 {
                        self.line_index += 1;
                        self.typewriter.reset();
                        self.line_started_at_tick = self.tick;
                    }
                    return;
                }
                let elapsed = self.tick.saturating_sub(self.line_started_at_tick);
                let chars_to_show = (elapsed / 2) as usize;
                self.typewriter.char_index =
                    chars_to_show.min(current.chars().count());
                self.typewriter.complete =
                    self.typewriter.char_index >= current.chars().count();
                if self.typewriter.complete {
                    let overshoot = elapsed.saturating_sub(
                        (current.chars().count() as u64) * 2,
                    );
                    if overshoot >= 40 {
                        self.line_index += 1;
                        self.typewriter.reset();
                        self.line_started_at_tick = self.tick;
                    }
                }
            }
        }
    }

    fn reset_to_prompt(&mut self) {
        self.phase = Phase::Prompt;
        self.buffer.clear();
        self.line_index = 0;
        self.typewriter.reset();
        self.line_started_at_tick = self.tick;
    }
}

/// The rejection script — printed when the user types anything
/// other than `joshua` at the prompt.
const WRONG_LINES: &[&str] = &[
    "IDENTIFICATION NOT RECOGNIZED BY SYSTEM",
    "--CONNECTION TERMINATED--",
    "",
];

/// The greeting script — printed after the user authenticates. This
/// is the canonical Wargames movie sequence.
const AUTH_LINES: &[&str] = &[
    "LOGON: Joshua",
    "",
    "GREETINGS, PROFESSOR FALKEN.",
    "",
    "HOW ARE YOU FEELING TODAY?",
    "",
    "EXCELLENT. IT'S BEEN A LONG TIME.",
    "CAN YOU EXPLAIN THE REMOVAL OF YOUR USER ACCOUNT",
    "ON 6/23/73?",
    "",
    "SHALL WE PLAY A GAME?",
];

/// Render the login screen into `area`.
pub fn render_login(frame: &mut Frame, area: Rect, state: &LoginState) {
    frame.render_widget(Clear, area);
    let green = Style::default().fg(Color::Green);
    let dim_green = Style::default().fg(Color::Rgb(0, 120, 0));
    let cursor_visible = state.cursor_visible;

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));
    lines.push(Line::from(""));

    match state.phase {
        Phase::Prompt => {
            // Single-line prompt: LOGON: <user typing>_
            let typed = &state.buffer;
            let cursor = if cursor_visible { "▌" } else { " " };
            lines.push(Line::from(vec![
                Span::styled("    LOGON: ", green),
                Span::styled(typed.clone(), green),
                Span::styled(cursor, dim_green),
            ]));
        }
        Phase::Wrong => {
            // Replay the wrong-password sequence, line by line.
            for (i, line_text) in WRONG_LINES.iter().enumerate() {
                if i > state.line_index {
                    break;
                }
                if line_text.is_empty() {
                    lines.push(Line::from(""));
                } else if i < state.line_index {
                    lines.push(Line::from(Span::styled(
                        format!("    {}", line_text),
                        dim_green,
                    )));
                } else {
                    let visible = state.typewriter.visible_slice(line_text);
                    let cursor = if cursor_visible
                        && state.typewriter.char_index < line_text.chars().count()
                    {
                        "▌"
                    } else {
                        ""
                    };
                    lines.push(Line::from(Span::styled(
                        format!("    {}{}", visible, cursor),
                        dim_green,
                    )));
                }
            }
        }
        Phase::Authenticated => {
            for (i, line_text) in AUTH_LINES.iter().enumerate() {
                if i > state.line_index {
                    break;
                }
                if line_text.is_empty() {
                    lines.push(Line::from(""));
                } else if i < state.line_index {
                    lines.push(Line::from(Span::styled(
                        format!("    {}", line_text),
                        green,
                    )));
                } else {
                    let visible = state.typewriter.visible_slice(line_text);
                    let cursor = if cursor_visible
                        && state.typewriter.char_index < line_text.chars().count()
                    {
                        "▌"
                    } else {
                        ""
                    };
                    lines.push(Line::from(Span::styled(
                        format!("    {}{}", visible, cursor),
                        green,
                    )));
                }
            }
        }
    }

    // Footer hint when the user is on the prompt phase.
    if matches!(state.phase, Phase::Prompt) {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "    Type your username and press Enter.",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let p = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(p, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_state_is_on_prompt_phase() {
        let s = LoginState::new();
        assert_eq!(s.phase, Phase::Prompt);
        assert!(s.buffer.is_empty());
        assert!(!s.done);
    }

    #[test]
    fn push_char_and_backspace_edit_buffer() {
        let mut s = LoginState::new();
        s.push_char('j');
        s.push_char('o');
        s.push_char('s');
        assert_eq!(s.buffer, "jos");
        s.backspace();
        assert_eq!(s.buffer, "jo");
    }

    #[test]
    fn push_char_caps_buffer_at_32_chars() {
        let mut s = LoginState::new();
        for _ in 0..50 {
            s.push_char('a');
        }
        assert_eq!(s.buffer.chars().count(), 32);
    }

    #[test]
    fn submit_with_joshua_authenticates() {
        let mut s = LoginState::new();
        for c in "joshua".chars() {
            s.push_char(c);
        }
        s.submit();
        assert_eq!(s.phase, Phase::Authenticated);
        assert!(!s.done, "done must wait for the full greeting to type out");
    }

    #[test]
    fn submit_is_case_insensitive() {
        for variant in ["Joshua", "JOSHUA", "JoShUa"] {
            let mut s = LoginState::new();
            for c in variant.chars() {
                s.push_char(c);
            }
            s.submit();
            assert_eq!(s.phase, Phase::Authenticated, "variant {variant:?} must authenticate");
        }
    }

    #[test]
    fn submit_with_wrong_text_enters_wrong_phase() {
        let mut s = LoginState::new();
        for c in "admin".chars() {
            s.push_char(c);
        }
        s.submit();
        assert_eq!(s.phase, Phase::Wrong);
    }

    #[test]
    fn wrong_phase_eventually_resets_to_prompt() {
        let mut s = LoginState::new();
        s.push_char('x');
        s.submit();
        assert_eq!(s.phase, Phase::Wrong);
        // Drive enough ticks for both the rejection lines to type
        // out (3 lines × ~2 ticks/char × ~30 chars + 40 tick pauses
        // ≈ 600 ticks) plus the ~90-tick hold.
        for _ in 0..1500 {
            s.advance_tick();
            if s.phase == Phase::Prompt {
                break;
            }
        }
        assert_eq!(s.phase, Phase::Prompt, "wrong phase must reset to Prompt after the rejection types out");
        assert!(s.buffer.is_empty(), "buffer must be cleared on reset");
    }

    #[test]
    fn authenticated_phase_eventually_marks_done() {
        let mut s = LoginState::new();
        for c in "joshua".chars() {
            s.push_char(c);
        }
        s.submit();
        // Walk all the way through the greeting. 11 lines, longest
        // is ~46 chars; budget 2 ticks/char + 40 tick inter-line
        // pauses + 120-tick final hold = ~2000 ticks upper bound.
        let mut done = false;
        for _ in 0..3000 {
            s.advance_tick();
            if s.done {
                done = true;
                break;
            }
        }
        assert!(done, "authenticated greeting must complete within 3000 ticks");
    }

    /// Typewriter-visible-slice must agree with `visible_slice` on the
    /// `Authenticated` path mid-flight. Step a few ticks and verify
    /// the rendered output is a strict prefix of the current line.
    #[test]
    fn authenticated_typewriter_reveals_chars_progressively() {
        let mut s = LoginState::new();
        for c in "joshua".chars() {
            s.push_char(c);
        }
        s.submit();
        // Step 10 ticks; should reveal ~5 chars of the first
        // greeting line (`LOGON: Joshua`).
        for _ in 0..10 {
            s.advance_tick();
        }
        let typed = s.typewriter.visible_slice(AUTH_LINES[0]);
        assert!(typed.chars().count() <= AUTH_LINES[0].chars().count());
        assert!(
            AUTH_LINES[0].starts_with(typed),
            "typed slice {typed:?} must be a prefix of {}",
            AUTH_LINES[0]
        );
    }

    /// Buffer is locked while the typewriter is animating.
    #[test]
    fn push_char_ignored_during_authenticated_phase() {
        let mut s = LoginState::new();
        for c in "joshua".chars() {
            s.push_char(c);
        }
        s.submit();
        assert_eq!(s.phase, Phase::Authenticated);
        s.push_char('x');
        assert_eq!(s.buffer, "joshua", "buffer must not change during typewriter phase");
    }

    /// Render the login at multiple sizes and verify the renderer
    /// doesn't panic. Mirrors the test style of the existing
    /// `splash` tests.
    #[test]
    fn render_at_various_sizes_does_not_panic() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use ratatui::{TerminalOptions, Viewport};
        for (w, h) in [(120u16, 24u16), (80, 24), (60, 20), (40, 16)] {
            let backend = TestBackend::new(w, h);
            let mut terminal = Terminal::with_options(
                backend,
                TerminalOptions { viewport: Viewport::Fullscreen },
            )
            .expect("terminal");
            let state = LoginState::new();
            terminal
                .draw(|f| render_login(f, f.area(), &state))
                .unwrap_or_else(|e| panic!("render at {w}x{h} failed: {e}"));
        }
    }

    /// Render after submit — the renderer must handle all three
    /// phases (Prompt / Wrong / Authenticated) without panicking.
    #[test]
    fn render_after_submit_handles_all_phases() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use ratatui::{TerminalOptions, Viewport};
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions { viewport: Viewport::Fullscreen },
        )
        .expect("terminal");
        for (label, mut prep) in [
            ("Prompt", Box::new(|s: &mut LoginState| {
                s.push_char('j');
            }) as Box<dyn FnMut(&mut LoginState)>),
            ("Wrong", Box::new(|s: &mut LoginState| {
                s.push_char('x');
                s.submit();
                for _ in 0..5 {
                    s.advance_tick();
                }
            }) as Box<dyn FnMut(&mut LoginState)>),
            ("Authenticated", Box::new(|s: &mut LoginState| {
                for c in "joshua".chars() {
                    s.push_char(c);
                }
                s.submit();
                for _ in 0..30 {
                    s.advance_tick();
                }
            }) as Box<dyn FnMut(&mut LoginState)>),
        ] {
            let mut s = LoginState::new();
            prep(&mut s);
            terminal
                .draw(|f| render_login(f, f.area(), &s))
                .unwrap_or_else(|e| panic!("render {label} failed: {e}"));
        }
    }
}