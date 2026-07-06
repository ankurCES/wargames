//! Herdr-style 2×2 + log paned layout for the game screen.
//!
//! The layout is **breakpoint-aware**: a single `game_layout(area)` call
//! classifies the available area and returns the rectangles that match it.
//! Widgets then read `inner.width` from their rectangle and scale their
//! own contents; the layout itself never hard-codes column widths.
//!
//! Breakpoints (cell dimensions, where cells == `inner.width`/height):
//!
//! - `TooSmall` — area < 24×8. The caller should paint a "Terminal too small"
//!   overlay instead of rendering the game.
//! - `Compact` — width ≤ 80 or height ≤ 24. One pane at a time, with a
//!   Tab/Shift+Tab bar; the remaining widgets are not drawn (no overlap).
//! - `Medium` — 81 ≤ width < 120, default height. 2×2 + log strip, 50/50
//!   columns (the 35/65 split from the original only breathes at ≥120 cols).
//! - `Wide` — width ≥ 120. Original 35/65 split, PREDICT gets its own column.
//!
//! At every breakpoint the layout *guarantees* all returned `Rect`s fit
//! inside the input area (no `x + width > area.width`, no negative origin).

use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Width below which we give up and ask the user to enlarge the terminal.
/// 24 cols is the smallest width at which any of the four game widgets
/// (state, predict, radar, action) can render something readable — under
/// that width the caller should draw the `TooSmall` overlay.
pub const MIN_WIDTH: u16 = 24;
/// Height below which we give up. 8 rows leaves room for one compact
/// pane (≥4 rows) + a tabs strip (1 row) + status line (1 row) + the
/// mandatory frame border (2 rows).
pub const MIN_HEIGHT: u16 = 8;
/// Compact breakpoint upper bound. 80 cols is the canonical "small but
/// usable" terminal (tmux in a small pane, low-zoom laptop, etc.).
pub const COMPACT_MAX_WIDTH: u16 = 80;

/// Width above which we use the original 35/65 split (the original layout
/// was designed for ≥120 cols).
pub const WIDE_MIN_WIDTH: u16 = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Breakpoint {
    /// `area.width < MIN_WIDTH` *or* `area.height < MIN_HEIGHT`. Caller
    /// paints a friendly overlay instead of the game.
    TooSmall,
    /// `width <= 80` or `height <= 24`. Single-column mode, one pane +
    /// tab strip.
    Compact,
    /// 81 ≤ width < 120. 2×2 grid with balanced (50/50) columns.
    Medium,
    /// width ≥ 120. 2×2 grid with the original 35/65 weighted columns.
    Wide,
}

impl Breakpoint {
    /// Classify an area. Pure function — no side effects, no allocation.
    pub fn classify(area: Rect) -> Breakpoint {
        if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
            Breakpoint::TooSmall
        } else if area.width <= COMPACT_MAX_WIDTH || area.height <= 24 {
            Breakpoint::Compact
        } else if area.width < WIDE_MIN_WIDTH {
            Breakpoint::Medium
        } else {
            Breakpoint::Wide
        }
    }
}

/// All the rectangles the game screen needs to paint. The fields are
/// always populated; `Compact` mode zeros out the unused panes and
/// surfaces only `active_pane` for the renderer to draw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GameRects {
    pub breakpoint: Breakpoint,
    /// Where the top-level body goes. The split into columns + rows
    /// depends on the breakpoint.
    pub body: Rect,
    /// STATE pane (top-left in 2×2; sole pane in Compact).
    pub state: Rect,
    /// PREDICT pane (bottom-left in 2×2; hidden in Compact).
    pub predict: Rect,
    /// RADAR pane (top-right in 2×2; hidden in Compact).
    pub radar: Rect,
    /// ACTION menu (bottom-right in 2×2; shown as overlay in Compact).
    pub action: Rect,
    /// EVENT LOG strip — always at the bottom across all breakpoints.
    pub log: Rect,
    /// Tab strip rendered above the active pane in Compact mode. Width
    /// spans the body; for non-Compact layouts it's `Rect::default()` and
    /// the widgets ignore it.
    pub tabs: Rect,
    /// STATUS line — single row at the bottom. Distinct from `log` so the
    /// game status never overlaps event messages.
    pub status: Rect,
    /// Which pane is currently active in Compact mode. For other
    /// breakpoints this defaults to `PaneKind::State` and is unused.
    pub active_pane: PaneKind,
}

/// The four "core" game panels. Used by the tab strip and the
/// `render_game` dispatcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneKind {
    State,
    Predict,
    Radar,
    Action,
}

impl PaneKind {
    /// Next pane clockwise (Tab). Wraps around.
    pub fn next(self) -> Self {
        match self {
            PaneKind::State => PaneKind::Predict,
            PaneKind::Predict => PaneKind::Radar,
            PaneKind::Radar => PaneKind::Action,
            PaneKind::Action => PaneKind::State,
        }
    }

    /// Previous pane (Shift+Tab).
    pub fn prev(self) -> Self {
        match self {
            PaneKind::State => PaneKind::Action,
            PaneKind::Action => PaneKind::Radar,
            PaneKind::Radar => PaneKind::Predict,
            PaneKind::Predict => PaneKind::State,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PaneKind::State => "STATE",
            PaneKind::Predict => "PRED",
            PaneKind::Radar => "RADAR",
            PaneKind::Action => "ACT",
        }
    }
}

impl Default for PaneKind {
    fn default() -> Self {
        PaneKind::State
    }
}

/// Compute the `GameRects` for an arbitrary area. Always returns
/// rectangles fully contained in `area`; never panics on `area.width=0`.
pub fn game_layout(area: Rect) -> GameRects {
    let breakpoint = Breakpoint::classify(area);

    if matches!(breakpoint, Breakpoint::TooSmall) {
        // No layout to compute; the caller paints an overlay using
        // `area` directly.
        return GameRects {
            breakpoint,
            body: area,
            state: area,
            predict: Rect::default(),
            radar: Rect::default(),
            action: Rect::default(),
            log: Rect::default(),
            tabs: Rect::default(),
            status: Rect::default(),
            active_pane: PaneKind::State,
        };
    }

    // Reserve the bottom row for the status line.
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1), // status
        ])
        .split(area);
    let status = vertical[1];
    let body = vertical[0];

    if matches!(breakpoint, Breakpoint::Compact) {
        // Single column. Tab strip is 3 rows; remaining rows show the
        // active pane. The log is omitted in Compact to leave room — the
        // user moves between panes with Tab/Shift+Tab and reads the log
        // by switching to the dedicated log surface (the existing event
        // log stays available via the action pane in a future iteration;
        // for now we still paint the log at the bottom inside the body).
        // Actually: to preserve the log always-visible behaviour users
        // rely on, we shrink it to 5 rows and the tab+active pane share
        // the remaining rows.
        let compact_v = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),  // tabs
                Constraint::Min(4),    // active pane
                Constraint::Length(5), // log
            ])
            .split(body);
        let tabs = compact_v[0];
        let active = compact_v[1];
        let log = compact_v[2];
        // Per-breakpoint defaults: state pane gets the body; the rest
        // are zero-area and the renderer skips them.
        return GameRects {
            breakpoint,
            body: active,
            state: active,
            predict: Rect::default(),
            radar: Rect::default(),
            action: Rect::default(),
            log,
            tabs,
            status,
            active_pane: PaneKind::State,
        };
    }

    // Medium & Wide: 2×2 grid + log strip in the body.
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(8),
            Constraint::Length(8), // log
        ])
        .split(body);

    let col_split: [u16; 2] = match breakpoint {
        Breakpoint::Medium => [50, 50],
        // Wide (and any future >= 120): original 35/65 split.
        Breakpoint::Wide | _ => [35, 65],
    };
    let body_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(col_split[0]),
            Constraint::Percentage(col_split[1]),
        ])
        .split(rows[0]);

    let left_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(body_cols[0]);

    let right_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(body_cols[1]);

    GameRects {
        breakpoint,
        body: rows[0],
        state: left_rows[0],
        predict: left_rows[1],
        radar: right_rows[0],
        action: right_rows[1],
        log: rows[1],
        tabs: Rect::default(),
        status,
        active_pane: PaneKind::State,
    }
}

/// Convenience: the 5-tuple form the original `game_layout` returned,
/// preserved so existing callers don't break. New callers should migrate
/// to `game_layout` (returns `GameRects`).
///
/// Returns `(state, predict, radar, action, log)`.
pub fn legacy_game_layout(
    area: Rect,
) -> (Rect, Rect, Rect, Rect, Rect) {
    let r = game_layout(area);
    (r.state, r.predict, r.radar, r.action, r.log)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_marks_too_small_when_below_minimum() {
        let tiny = Rect {
            x: 0,
            y: 0,
            width: 20,
            height: 7,
        };
        assert_eq!(Breakpoint::classify(tiny), Breakpoint::TooSmall);
        // Boundary: exactly MIN_WIDTH-1.
        let almost = Rect {
            x: 0,
            y: 0,
            width: MIN_WIDTH - 1,
            height: MIN_HEIGHT,
        };
        assert_eq!(Breakpoint::classify(almost), Breakpoint::TooSmall);
    }

    #[test]
    fn classify_picks_compact_at_or_below_80_cols() {
        // 80 is the upper bound — still Compact.
        let at_80 = Rect {
            x: 0,
            y: 0,
            width: COMPACT_MAX_WIDTH,
            height: 30,
        };
        assert_eq!(Breakpoint::classify(at_80), Breakpoint::Compact);
        // 81 cols crosses into Medium.
        let at_81 = Rect {
            x: 0,
            y: 0,
            width: COMPACT_MAX_WIDTH + 1,
            height: 30,
        };
        assert_eq!(Breakpoint::classify(at_81), Breakpoint::Medium);
    }

    #[test]
    fn classify_picks_wide_at_or_above_120_cols() {
        // 119 → Medium.
        let m = Rect {
            x: 0,
            y: 0,
            width: WIDE_MIN_WIDTH - 1,
            height: 30,
        };
        assert_eq!(Breakpoint::classify(m), Breakpoint::Medium);
        // 120 → Wide.
        let w = Rect {
            x: 0,
            y: 0,
            width: WIDE_MIN_WIDTH,
            height: 30,
        };
        assert_eq!(Breakpoint::classify(w), Breakpoint::Wide);
    }

    #[test]
    fn classify_promotes_to_compact_at_small_height() {
        // Tall but narrow is Medium; short but wide is Compact (height
        // < 25 wins regardless of width).
        let short = Rect {
            x: 0,
            y: 0,
            width: 200,
            height: 20,
        };
        assert_eq!(Breakpoint::classify(short), Breakpoint::Compact);
    }

    #[test]
    fn game_layout_too_small_returns_default_rects() {
        let r = game_layout(Rect {
            x: 0,
            y: 0,
            width: 20,
            height: 7,
        });
        assert_eq!(r.breakpoint, Breakpoint::TooSmall);
        assert_eq!(r.state, r.body);
        // Remaining rects are zero-size so the renderer skips them.
        assert_eq!(r.predict.width, 0);
        assert_eq!(r.radar.width, 0);
        assert_eq!(r.action.width, 0);
        assert_eq!(r.log.width, 0);
    }

    #[test]
    fn game_layout_compact_fits_inside_area() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 60,
            height: 18,
        };
        let r = game_layout(area);
        assert_eq!(r.breakpoint, Breakpoint::Compact);
        // Every returned rect must lie within `area`.
        for pane in [r.body, r.state, r.tabs, r.log, r.status] {
            assert!(
                pane.x + pane.width <= area.width,
                "pane right edge {}+{} > area.width {}",
                pane.x,
                pane.width,
                area.width
            );
            assert!(
                pane.y + pane.height <= area.height,
                "pane bottom edge {}+{} > area.height {}",
                pane.y,
                pane.height,
                area.height
            );
        }
        // In Compact we only show the active pane (state by default) —
        // the others are zero-area so the renderer skips them.
        assert_eq!(r.predict.width, 0);
        assert_eq!(r.radar.width, 0);
        assert_eq!(r.action.width, 0);
    }

    #[test]
    fn game_layout_medium_keeps_all_panes_visible() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 30,
        };
        let r = game_layout(area);
        assert_eq!(r.breakpoint, Breakpoint::Medium);
        assert!(r.state.width > 0);
        assert!(r.predict.width > 0);
        assert!(r.radar.width > 0);
        assert!(r.action.width > 0);
        assert!(r.log.width > 0);
        // Two columns at 50/50 — each pane should be ~half the width.
        // (It's a percentage split, so allow a 1-cell slop for rounding.)
        let diff = (r.state.width as i32 - r.radar.width as i32).abs();
        assert!(
            diff <= 2,
            "Medium should be 50/50 (diff {diff}, state={}, radar={})",
            r.state.width,
            r.radar.width
        );
    }

    #[test]
    fn game_layout_wide_uses_35_65_split() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 160,
            height: 40,
        };
        let r = game_layout(area);
        assert_eq!(r.breakpoint, Breakpoint::Wide);
        // 35/65 → state column should be noticeably narrower than
        // radar+action column. For 158 inner cols (160 - 2 borders),
        // state ≈ 55 and radar ≈ 103.
        assert!(
            r.state.width + 10 < r.radar.width,
            "Wide should be 35/65 (state={}, radar={})",
            r.state.width,
            r.radar.width
        );
    }

    #[test]
    fn game_layout_no_pane_exceeds_area_at_any_size() {
        // Sweep a representative set of sizes from 24×8 (boundary) up to
        // 240×60. Every rectangle returned must fit inside `area`.
        let sizes = [
            (24u16, 8u16),
            (40, 18),
            (60, 24),
            (80, 30),
            (81, 30),
            (100, 40),
            (120, 40),
            (160, 50),
            (240, 60),
            // Pathological: weird near-boundary sizes.
            (79, 25),
            (119, 30),
            (121, 40),
        ];
        for (w, h) in sizes {
            let area = Rect {
                x: 0,
                y: 0,
                width: w,
                height: h,
            };
            let r = game_layout(area);
            for pane in [r.state, r.predict, r.radar, r.action, r.log, r.tabs, r.status] {
                if pane.width == 0 && pane.height == 0 {
                    continue; // unused in this breakpoint
                }
                assert!(
                    pane.x + pane.width <= area.width,
                    "size {w}x{h}: pane right {}+{} > area.width {}",
                    pane.x,
                    pane.width,
                    area.width
                );
                assert!(
                    pane.y + pane.height <= area.height,
                    "size {w}x{h}: pane bottom {}+{} > area.height {}",
                    pane.y,
                    pane.height,
                    area.height
                );
            }
        }
    }

    #[test]
    fn legacy_game_layout_matches_new_layout() {
        // Smoke test the legacy 5-tuple form.
        let area = Rect {
            x: 0,
            y: 0,
            width: 120,
            height: 40,
        };
        let (s, p, r, a, l) = legacy_game_layout(area);
        let g = game_layout(area);
        assert_eq!(s, g.state);
        assert_eq!(p, g.predict);
        assert_eq!(r, g.radar);
        assert_eq!(a, g.action);
        assert_eq!(l, g.log);
    }

    #[test]
    fn pane_kind_cycles_through_tabs() {
        use PaneKind::*;
        assert_eq!(State.next(), Predict);
        assert_eq!(Predict.next(), Radar);
        assert_eq!(Radar.next(), Action);
        assert_eq!(Action.next(), State);
        // Reverse:
        assert_eq!(State.prev(), Action);
        assert_eq!(Action.prev(), Radar);
        assert_eq!(Radar.prev(), Predict);
        assert_eq!(Predict.prev(), State);
    }
}
