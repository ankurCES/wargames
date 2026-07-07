//! Animation primitives — braille spinner, typewriter, pulse.
//!
//! Ported from `ankurCES/WOPR_TUI_2026` (MIT, your own repo). These
//! helpers are consumed by the Joshua login (`crate::login`) and the
//! DEFCON panel (`crate::widget_defcon`). Keep them side-effect-free
//! so widgets can stay pure functions of (state, tick).

use ratatui::style::{Color, Style};

/// 8-frame braille spinner — cycle every 8 ticks.
const BRAILLE_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧'];

/// Pick the spinner glyph for the given tick. Cycle rate is one
/// frame per tick — keeps the motion visible at 60 fps while
/// remaining flicker-free on slower terminals.
pub fn braille_spinner(tick: u64) -> char {
    let idx = (tick as usize) % BRAILLE_FRAMES.len();
    BRAILLE_FRAMES[idx]
}

/// Drives the typewriter effect on the login sequence. Advance the
/// index forward by `chars_per_tick` each render; `visible_slice`
/// returns the substring that's been "typed so far".
#[derive(Debug, Clone)]
pub struct TypewriterState {
    pub char_index: usize,
    pub complete: bool,
}

impl Default for TypewriterState {
    fn default() -> Self {
        Self::new()
    }
}

impl TypewriterState {
    pub fn new() -> Self {
        Self { char_index: 0, complete: false }
    }

    /// Push the cursor forward by `chars_per_tick` characters, capped
    /// at the text length. Marks `complete` once the cursor hits the
    /// end of `text`.
    pub fn advance(&mut self, text: &str, chars_per_tick: usize) {
        let total = text.chars().count();
        self.char_index = (self.char_index + chars_per_tick).min(total);
        self.complete = self.char_index >= total;
    }

    /// Slice `text` to the chars already typed. Always returns a
    /// valid UTF-8 prefix (we walk `char_indices` to land on a
    /// char boundary, not a byte offset).
    pub fn visible_slice<'a>(&self, text: &'a str) -> &'a str {
        let byte_end = text
            .char_indices()
            .nth(self.char_index)
            .map(|(i, _)| i)
            .unwrap_or(text.len());
        &text[..byte_end]
    }

    /// Rewind to the start. Used when the user enters an invalid
    /// username and the login screen resets for another attempt.
    pub fn reset(&mut self) {
        self.char_index = 0;
        self.complete = false;
    }
}

/// Pulse between `bright` and `dim` every `period` ticks. Used for
/// blinking DEFCON bars and other "alert" affordances.
pub fn pulse_style(tick: u64, period: u64, bright: Color, dim: Color) -> Style {
    if (tick / period) % 2 == 0 {
        Style::default().fg(bright)
    } else {
        Style::default().fg(dim)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn braille_spinner_cycles_through_eight_frames() {
        // 8-frame cycle; sample 8 distinct ticks, expect 8 distinct glyphs.
        let frames: Vec<char> = (0..8).map(|t| braille_spinner(t)).collect();
        let unique: std::collections::HashSet<char> = frames.iter().copied().collect();
        assert_eq!(unique.len(), 8, "spinner must cycle through 8 distinct frames");
    }

    #[test]
    fn braille_spinner_cycles_after_full_loop() {
        let at_8 = braille_spinner(8);
        let at_0 = braille_spinner(0);
        assert_eq!(at_8, at_0, "spinner must wrap after 8 frames");
    }

    #[test]
    fn typewriter_advances_and_caps_at_text_length() {
        let mut tw = TypewriterState::new();
        tw.advance("hello", 2);
        assert_eq!(tw.visible_slice("hello"), "he");
        assert!(!tw.complete);
        // Jump past the end — must clamp, not panic, and mark complete.
        tw.advance("hello", 99);
        assert_eq!(tw.visible_slice("hello"), "hello");
        assert!(tw.complete);
    }

    #[test]
    fn typewriter_handles_multibyte_char_boundaries() {
        // Em-dash is 3 bytes in UTF-8. A byte-offset slice would panic;
        // walking char_indices keeps us safe.
        let mut tw = TypewriterState::new();
        tw.advance("a—b", 1);
        assert_eq!(tw.visible_slice("a—b"), "a");
        tw.advance("a—b", 1);
        assert_eq!(tw.visible_slice("a—b"), "a—");
        tw.advance("a—b", 99);
        assert_eq!(tw.visible_slice("a—b"), "a—b");
    }

    #[test]
    fn typewriter_reset_returns_to_start() {
        let mut tw = TypewriterState::new();
        tw.advance("hello", 99);
        assert!(tw.complete);
        tw.reset();
        assert_eq!(tw.char_index, 0);
        assert!(!tw.complete);
    }

    #[test]
    fn pulse_style_alternates_bright_and_dim() {
        let bright = Color::Red;
        let dim = Color::Black;
        // Period 1 means toggle every tick.
        let s_even = pulse_style(0, 1, bright, dim);
        let s_odd = pulse_style(1, 1, bright, dim);
        // Verify the alternating condition directly — a Style's equality
        // isn't trivial across ratatui versions, but the fg colors are
        // observable.
        assert_eq!(s_even.fg, Some(bright));
        assert_eq!(s_odd.fg, Some(dim));
    }
}