//! Centered "RECEIVING OPPONENT RESPONSE…" popup shown between
//! the player's commit and the opponent's response (plus a 300 ms
//! linger). Sits on top of the game view at row `height - 4`,
//! clearing the status line and the action menu.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::theme;
use crate::widget_spinner::braille_at;

/// Label rendered inside the popup. The literal text — emoji
/// ellipsis is intentional (matches the WOPR-tone verb phase
/// list in `widget_spinner`).
pub const POPUP_LABEL: &str = "RECEIVING OPPONENT RESPONSE…";

/// Minimum frame width to attempt rendering. Equals
/// `popup_inner_width + 2 (border)` + 2 (margin) so the no-op
/// triggers exactly when the popup would be cropped.
pub const MIN_POPUP_WIDTH: u16 = 36;

/// Minimum frame height to attempt rendering.
pub const MIN_POPUP_HEIGHT: u16 = 4;

/// Compute the centered rect for the popup inside `frame_area`.
/// Returns `Rect::default()` (zero area) when the frame is below
/// the minimum dimensions — callers treat that as "don't render".
pub fn centered_rect(frame_area: Rect) -> Rect {
    if frame_area.width < MIN_POPUP_WIDTH
        || frame_area.height < MIN_POPUP_HEIGHT
    {
        return Rect::default();
    }
    let popup_w = popup_width();
    let popup_h = popup_height();
    let x = frame_area
        .width
        .saturating_sub(popup_w)
        / 2;
    // y = height - popup_h - 1 == height - 4  (with popup_h = 3).
    // The extra `- 1` keeps a single-row gap between the popup's
    // bottom border and the frame's bottom border.
    let y = frame_area.height.saturating_sub(popup_h + 1);
    Rect {
        x,
        y,
        width: popup_w,
        height: popup_h,
    }
}

/// Total popup width including border cells.
fn popup_width() -> u16 {
    // 1 (braille) + 1 (space) + char count of label + 2 (border).
    // The `…` ellipsis is 1 char (3 UTF-8 bytes), so we count
    // chars not bytes. MIN_POPUP_WIDTH is the visual floor.
    (POPUP_LABEL.chars().count() as u16 + 4).max(MIN_POPUP_WIDTH)
}

/// Total popup height (1 content row + 2 border rows).
fn popup_height() -> u16 {
    3
}

/// Render the popup on top of whatever's already in `frame`.
/// No-op when `centered_rect` returned a degenerate area.
pub fn render(frame: &mut Frame, frame_area: Rect, frame_idx: usize) {
    let area = centered_rect(frame_area);
    if area.width == 0 || area.height == 0 {
        return;
    }
    let theme = theme::current();
    let glyph = braille_at(frame_idx).to_string();
    let label = format!("{glyph} {}", POPUP_LABEL);
    // Border in the warn colour; padded pane background uses
    // `theme.background` to match the spec's `pane_bg` intent.
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.status_warn))
        .style(Style::default().bg(theme.background));
    let paragraph = Paragraph::new(Line::from(Span::styled(
        label,
        Style::default()
            .fg(theme.status_text)
            .bg(theme.background),
    )))
    .block(block);
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centered_rect_sits_at_expected_x_y() {
        let frame = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        };
        let r = centered_rect(frame);
        let w = popup_width();
        let h = popup_height();
        assert_eq!(r.x, (80 - w) / 2);
        assert_eq!(r.y, 24 - h - 1);
        assert_eq!(r.width, w);
        assert_eq!(r.height, h);
    }

    #[test]
    fn centered_rect_is_zero_when_too_narrow() {
        let frame = Rect {
            x: 0,
            y: 0,
            width: MIN_POPUP_WIDTH - 1,
            height: 24,
        };
        assert_eq!(centered_rect(frame), Rect::default());
    }

    #[test]
    fn centered_rect_is_zero_when_too_short() {
        let frame = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: MIN_POPUP_HEIGHT - 1,
        };
        assert_eq!(centered_rect(frame), Rect::default());
    }

    #[test]
    fn popup_width_equals_minimum_for_current_label() {
        // The current label is short enough that the MIN_POPUP_WIDTH
        // floor is the binding constraint. If anyone shortens the
        // floor or lengthens the label, this pin breaks and forces
        // a deliberate update.
        assert_eq!(popup_width(), MIN_POPUP_WIDTH);
    }

    #[test]
    fn popup_height_equals_minimum_minus_one() {
        // Popup height is exactly MIN_POPUP_HEIGHT - 1: 1 row of
        // content + 2 rows of border. This encodes the invariant
        // "borders add 2 rows" so a future change to either
        // MIN_POPUP_HEIGHT or popup_height() must be conscious.
        assert_eq!(popup_height(), MIN_POPUP_HEIGHT - 1);
    }

    use crate::widget_spinner::braille_at;
    use ratatui::backend::TestBackend;
    use ratatui::{Terminal, TerminalOptions, Viewport};

    fn render_to_string(width: u16, height: u16, frame_idx: usize) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions { viewport: Viewport::Fullscreen },
        )
        .expect("terminal");
        terminal
            .draw(|f| render(f, f.area(), frame_idx))
            .expect("render");
        let buf = terminal.backend().buffer().clone();
        let mut s = String::new();
        use unicode_width::UnicodeWidthChar;
        for y in 0..buf.area.height {
            let mut x = 0u16;
            while x < buf.area.width {
                let cell = &buf[(x, y)];
                let symbol = cell.symbol();
                let mut width = 0u16;
                for c in symbol.chars() {
                    width += UnicodeWidthChar::width(c).unwrap_or(0) as u16;
                }
                s.push_str(symbol);
                if width > 1 {
                    x += width;
                } else {
                    x += 1;
                }
            }
        }
        s
    }

    #[test]
    fn render_paints_braille_glyph_and_label() {
        let s = render_to_string(80, 24, 0);
        let glyph = braille_at(0).to_string();
        assert!(s.contains(&glyph), "braille glyph missing: {s}");
        assert!(s.contains("RECEIVING OPPONENT RESPONSE…"));
    }

    #[test]
    fn render_is_noop_on_subminimum_frame() {
        let s = render_to_string(MIN_POPUP_WIDTH - 1, 24, 0);
        assert!(!s.contains("RECEIVING OPPONENT RESPONSE…"));
    }
}