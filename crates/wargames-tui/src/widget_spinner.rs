//! Reusable loading spinner widget.
//!
//! Renders a fixed-size 28×3 box with:
//! - top line: animated braille glyph + label (e.g. "thinking…", "LOADING…")
//! - middle line: animated marquis "LOADING" where each character cycles
//!   through `█ ░ ▒ ▓` independently based on `frame_idx` so it looks
//!   alive without a busy redraw loop.
//! - bottom line: elapsed milliseconds since the operation started.
//!
//! The widget is shared by the picker (during scenario load) and the game
//! screen (during LLM calls and prediction refresh). Both screens anchor
//! the box to a corner of the frame; the caller passes the desired
//! rectangle so the widget itself stays layout-agnostic.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;
use std::time::{Duration, Instant};

/// 10-frame braille spinner. Each glyph occupies one terminal cell and
/// rotates to give the impression of motion at ~10 fps with the run
/// loop's 50 ms tick.
const BRAILLE: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// The 4-character shimmer set used by the animated LOADING marquis.
const SHIMMER: &[char] = &['█', '▓', '▒', '░'];

/// Width of the rendered box (inner content + 2 borders = box width).
pub const SPINNER_W: u16 = 30;
/// Height of the rendered box (inner content + 2 borders = box height).
pub const SPINNER_H: u16 = 3;

/// Renders the spinner into `area`. The caller is responsible for placing
/// the rectangle (top-right on the picker, bottom-right on the game
/// screen). `label` is the short verb shown next to the braille glyph
/// (e.g. "thinking…", "loading…"). `frame_idx` is the run-loop tick
/// counter — the same value is used for the braille rotation and the
/// shimmer phase. `started_at` is the `Instant` the operation began;
/// the elapsed time is shown in seconds with one decimal place.
pub fn render(frame: &mut Frame, area: Rect, label: &str, frame_idx: usize, started_at: Instant) {
    let w = SPINNER_W.min(area.width);
    let h = SPINNER_H.min(area.height);
    if w == 0 || h == 0 {
        return;
    }
    let box_area = Rect {
        x: area.x,
        y: area.y,
        width: w,
        height: h,
    };
    frame.render_widget(Clear, box_area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(box_area);
    frame.render_widget(block, box_area);

    let glyph = BRAILLE[frame_idx % BRAILLE.len()];
    let elapsed_ms = started_at.elapsed().as_millis();
    let secs_str = format!("{:>4}.{:01}s", elapsed_ms / 1000, (elapsed_ms / 100) % 10);

    // Animated marquis: 8 characters, each cycling through the shimmer
    // set on its own phase so the row looks like it's rippling.
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(8);
    for i in 0..8 {
        let phase = (frame_idx.wrapping_add(i * 2)) % (SHIMMER.len() * 2);
        let c = if phase < SHIMMER.len() { SHIMMER[phase] } else { SHIMMER[0] };
        spans.push(Span::styled(
            c.to_string(),
            Style::default().fg(if i % 2 == 0 { Color::Yellow } else { Color::DarkGray }),
        ));
    }

    let lines = vec![
        Line::from(vec![
            Span::styled(
                glyph.to_string(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {label}"),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(spans),
        Line::from(Span::styled(
            secs_str,
            Style::default().fg(Color::DarkGray),
        )),
    ];
    let p = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(p, inner);
}

/// Convenience: top-right anchor for the spinner inside `frame_area`.
pub fn top_right_rect(frame_area: Rect) -> Rect {
    let w = SPINNER_W.min(frame_area.width);
    let h = SPINNER_H.min(frame_area.height);
    Rect {
        x: frame_area.width.saturating_sub(w + 1),
        y: 0,
        width: w,
        height: h,
    }
}

/// Convenience: bottom-right anchor — used on the game screen so the
/// spinner never covers the action menu.
pub fn bottom_right_rect(frame_area: Rect) -> Rect {
    let w = SPINNER_W.min(frame_area.width);
    let h = SPINNER_H.min(frame_area.height);
    Rect {
        x: frame_area.width.saturating_sub(w + 1),
        y: frame_area.height.saturating_sub(h + 3),
        width: w,
        height: h,
    }
}

/// Helper for tests and call sites that don't care about clock time:
/// returns a label-safe, ascii-clean string for the braille glyph at
/// the given frame index. Mirrors `BRAILLE` so unit tests can assert
/// against a stable byte sequence.
#[doc(hidden)]
pub fn braille_at(frame_idx: usize) -> char {
    BRAILLE[frame_idx % BRAILLE.len()]
}

#[doc(hidden)]
pub const fn shimmer_len() -> usize {
    SHIMMER.len()
}

#[doc(hidden)]
pub const fn braille_len() -> usize {
    BRAILLE.len()
}

/// Convenience used in unit tests to construct a fixed-start `Instant`
/// relative to a known baseline — keeps tests deterministic.
#[cfg(test)]
pub fn fixed_started_at() -> Instant {
    // Use a fixed reference so elapsed is computable in tests that
    // want to assert against format strings.
    static FIXED: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    *FIXED.get_or_init(Instant::now)
}

#[doc(hidden)]
pub fn _sample_duration() -> Duration {
    Duration::from_millis(1234)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn braille_cycles_through_all_frames() {
        let mut seen = std::collections::HashSet::new();
        for i in 0..(braille_len() * 2) {
            seen.insert(braille_at(i));
        }
        assert_eq!(seen.len(), braille_len(), "every braille glyph must appear");
    }

    #[test]
    fn shimmer_set_has_four_glyphs() {
        assert_eq!(shimmer_len(), 4);
        assert!(SHIMMER.contains(&'█'));
        assert!(SHIMMER.contains(&'░'));
    }

    #[test]
    fn rect_helpers_never_overflow_frame() {
        let frame = Rect {
            x: 0,
            y: 0,
            width: 20,
            height: 8,
        };
        let r = top_right_rect(frame);
        assert!(r.x + r.width <= frame.width);
        assert!(r.y + r.height <= frame.height);
        let r = bottom_right_rect(frame);
        assert!(r.x + r.width <= frame.width);
        assert!(r.y + r.height <= frame.height);
    }

    #[test]
    fn rect_helpers_clamp_on_tiny_frames() {
        let tiny = Rect {
            x: 0,
            y: 0,
            width: 4,
            height: 2,
        };
        let r = top_right_rect(tiny);
        assert_eq!(r.width, 4);
        assert_eq!(r.height, 2);
    }
}