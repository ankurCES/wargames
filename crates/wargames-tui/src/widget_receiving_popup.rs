//! Centered "RECEIVING OPPONENT RESPONSE…" popup shown between
//! the player's commit and the opponent's response (plus a 300 ms
//! linger). Sits on top of the game view at row `height - 4`,
//! clearing the status line and the action menu.

use ratatui::layout::Rect;

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
}