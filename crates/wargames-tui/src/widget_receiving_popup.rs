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
    // 1 (braille) + 1 (space) + label.len() + 2 (border) = label.len() + 4
    // Pad to MIN_POPUP_WIDTH for visual breathing room.
    (POPUP_LABEL.len() as u16 + 4).max(MIN_POPUP_WIDTH)
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
    fn popup_width_is_at_least_minimum() {
        assert!(popup_width() >= MIN_POPUP_WIDTH);
    }

    #[test]
    fn popup_height_matches_minimum() {
        assert_eq!(popup_height(), 3);
    }
}