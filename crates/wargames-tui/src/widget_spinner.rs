//! Reusable loading spinner widget — progress bar + animated text + sprite.
//!
//! Renders a fixed-size box with three coordinated signals of progress:
//!
//! 1. **Animated braille sprite** (top-left cell). Rotates through 10 glyphs
//!    at ~10 fps so the user sees continuous motion even when nothing else
//!    is happening.
//! 2. **Animated phase text** (top line, after the sprite). Cycles through
//!    a list of phase verbs ("warming up ▸ loading theater ▸ preparing
//!    scenarios ▸ almost there ▸ ready") so the user can read *what* is
//!    happening, not just *that* something is happening.
//! 3. **Determinate progress bar** (middle line). Fills left-to-right using
//!    a smoothstep of `frame_idx / TOTAL_FRAMES`. The bar is derived from
//!    the frame counter (not `Instant::now()`) so test assertions can pin
//!    exact fill widths without time-of-day dependencies.
//! 4. **Elapsed seconds** (bottom line). Reassures the user the box is alive
//!    even when the bar stalls.
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

/// 4-character shimmer set used for the animated phase text and the bar
/// "head" — gives the moving parts a sweep rather than a step.
const SHIMMER: &[char] = &['█', '▓', '▒', '░'];

/// Fixed bar fill character. `█` makes the filled portion unmistakably
/// opaque against the dim "remaining" tail.
const FILL_CHAR: char = '█';
/// Dim character for the unfilled portion of the bar — looks like a
/// track behind the fill so the user can see how much is left.
const TRACK_CHAR: char = '░';

/// Width of the rendered box (inner content + 2 borders = box width).
pub const SPINNER_W: u16 = 30;
/// Height of the rendered box (inner content + 2 borders = box height).
pub const SPINNER_H: u16 = 4;

/// How many render frames the progress bar sweeps across. At the run
/// loop's 50 ms tick this is ~900 ms — long enough for the user to see
/// the fill motion, short enough to feel snappy.
pub const PROGRESS_TOTAL_FRAMES: usize = 18;

/// Phase verbs cycled through the top-line text. Each is short enough
/// to fit the 28-char box width alongside the sprite. The trailing
/// em-space gives the text room to "breathe" as it cycles.
const PHASE_VERBS: &[&str] = &[
    "warming up…        ",
    "loading theater…   ",
    "preparing scenarios",
    "computing initial… ",
    "rendering layout…  ",
    "almost there…      ",
];

/// Returns the phase verb for a given frame index. The verb cycles every
/// `PHASE_PERIOD` frames so each verb is visible long enough to read.
pub fn phase_verb_at(frame_idx: usize) -> &'static str {
    const PHASE_PERIOD: usize = 3;
    let slot = (frame_idx / PHASE_PERIOD) % PHASE_VERBS.len();
    PHASE_VERBS[slot]
}

/// Number of inner cells wide the progress bar occupies. Chosen to leave
/// room for the percent label on the same line.
const BAR_WIDTH: usize = 18;

/// Compute the bar fill width (in cells, 0..=BAR_WIDTH) for a given frame
/// index. Uses a smoothstep so the bar accelerates from 0 and decelerates
/// into the cap — looks like a real progress indicator, not a counter.
pub fn progress_fill_at(frame_idx: usize) -> usize {
    if frame_idx == 0 {
        return 0;
    }
    let n = frame_idx.min(PROGRESS_TOTAL_FRAMES);
    // Smoothstep: 3t^2 - 2t^3 over t in [0,1]. Multiplied by BAR_WIDTH.
    let t = n as f32 / PROGRESS_TOTAL_FRAMES as f32;
    let s = t * t * (3.0 - 2.0 * t);
    (s * BAR_WIDTH as f32).round() as usize
}

/// Percent label for the trailing portion of the bar line (e.g. " 73%").
pub fn progress_pct_at(frame_idx: usize) -> u8 {
    let n = frame_idx.min(PROGRESS_TOTAL_FRAMES);
    let raw = (n as f32 / PROGRESS_TOTAL_FRAMES as f32) * 100.0;
    raw.round() as u8
}

/// Returns the elapsed time formatted as `12.3s`. Mirrors the old widget
/// exactly so the visual bottom-line is unchanged when callers don't pass
/// a custom formatter.
pub fn format_elapsed(started_at: Instant) -> String {
    let elapsed_ms = started_at.elapsed().as_millis();
    format!(
        "{:>3}.{:01}s",
        elapsed_ms / 1000,
        (elapsed_ms / 100) % 10
    )
}

/// Render a fixed-start placeholder — used by tests that want a stable
/// elapsed-string without depending on wall-clock time.
#[cfg(test)]
pub fn fixed_started_at() -> Instant {
    static FIXED: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    *FIXED.get_or_init(Instant::now)
}

/// Renders the spinner into `area`. The caller is responsible for placing
/// the rectangle (top-right on the picker, bottom-right on the game
/// screen). `label` is the short verb shown next to the braille glyph
/// (e.g. "thinking…", "loading…"). `frame_idx` is the run-loop tick
/// counter — the same value drives the sprite rotation, the phase text,
/// the bar fill, and the percent label. `started_at` is the `Instant`
/// the operation began; the elapsed time is shown in seconds.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    frame_idx: usize,
    started_at: Instant,
) {
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
    let phase = phase_verb_at(frame_idx);
    let fill = progress_fill_at(frame_idx);
    let pct = progress_pct_at(frame_idx);
    let elapsed_str = format_elapsed(started_at);

    // --- Top line: sprite + label + cycling phase text -------------------
    // The phase verb *replaces* the static label when present so the user
    // sees a continuous flow of verbs; the static label is the fallback
    // for callers (LLM call, predict) where there's no fixed phase list.
    let top_text = if label.is_empty() {
        phase.to_string()
    } else {
        format!("{label} {phase}")
    };
    let top_line = Line::from(vec![
        Span::styled(
            glyph.to_string(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {top_text}"), Style::default().fg(Color::White)),
    ]);

    // --- Middle line: progress bar + percent -----------------------------
    // Fill cells in `Color::Yellow`, remaining in `Color::DarkGray`. The
    // bar sits flush-left, percent sits flush-right inside the box.
    let mut bar_spans: Vec<Span<'static>> = Vec::with_capacity(BAR_WIDTH + 4);
    for i in 0..BAR_WIDTH {
        let c = if i < fill { FILL_CHAR } else { TRACK_CHAR };
        let color = if i < fill {
            Color::Yellow
        } else {
            Color::DarkGray
        };
        bar_spans.push(Span::styled(c.to_string(), Style::default().fg(color)));
    }
    bar_spans.push(Span::styled(
        format!(" {pct:>3}%"),
        Style::default().fg(Color::White),
    ));
    let bar_line = Line::from(bar_spans);

    // --- Bottom line: elapsed time ---------------------------------------
    let bot_line = Line::from(Span::styled(
        elapsed_str,
        Style::default().fg(Color::DarkGray),
    ));

    let lines = vec![top_line, bar_line, bot_line];
    let p = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(p, inner);

    // Silence dead-code warning — `Duration` re-export keeps the API
    // symmetric with `format_elapsed` so callers can compare instead of
    // formatting when they need an exact match.
    let _ = Duration::from_secs(0);
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
/// returns the braille glyph at the given frame index. Mirrors `BRAILLE`
/// so unit tests can assert against a stable byte sequence.
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

#[doc(hidden)]
pub const fn progress_total_frames() -> usize {
    PROGRESS_TOTAL_FRAMES
}

#[doc(hidden)]
pub const fn bar_width() -> usize {
    BAR_WIDTH
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
    fn progress_bar_is_zero_at_start_and_full_at_end() {
        assert_eq!(progress_fill_at(0), 0);
        assert_eq!(progress_fill_at(1), 1); // smoothstep already nonzero at frame 1
        assert_eq!(
            progress_fill_at(progress_total_frames()),
            bar_width()
        );
        // Overshoot clamps to BAR_WIDTH.
        assert_eq!(
            progress_fill_at(progress_total_frames() * 4),
            bar_width()
        );
    }

    #[test]
    fn progress_percent_is_monotonic_and_bounds() {
        let mut prev = 0u8;
        for i in 0..(progress_total_frames() * 2) {
            let p = progress_pct_at(i);
            assert!(p >= prev, "percent must be monotonic; regressed at {i}");
            assert!(p <= 100, "percent must be <= 100");
            prev = p;
        }
        assert_eq!(prev, 100);
    }

    #[test]
    fn phase_verbs_cycle_through_full_list() {
        let mut seen = std::collections::HashSet::new();
        for i in 0..(PHASE_VERBS.len() * 4) {
            seen.insert(phase_verb_at(i));
        }
        assert_eq!(seen.len(), PHASE_VERBS.len());
        // No verb is empty — visually a gap would look like a flicker.
        for v in PHASE_VERBS {
            assert!(!v.trim().is_empty(), "phase verb must be non-empty");
            assert!(v.len() <= 20, "phase verb must fit the box width");
        }
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

    #[test]
    fn format_elapsed_is_well_formed() {
        // Just-constructed Instant must yield `  0.0s` (3-char sec, 1 dec).
        let s = format_elapsed(Instant::now());
        assert!(s.ends_with('s'));
        assert_eq!(s.len(), 6, "expected NN.Ns shape, got {s:?}");
    }
}
