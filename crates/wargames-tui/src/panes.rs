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

use crate::widget_action;

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
/// always populated; `Compact` mode draws `tabs + left + log + action`
/// in a single column; `Medium`/`Wide` skip the tab strip and place
/// `left` and `log` side by side above a full-width `action` strip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GameRects {
    pub breakpoint: Breakpoint,
    /// Total body area (full game pane minus status row). Equal to
    /// `left` in Medium/Wide.
    pub body: Rect,
    /// Left pane — hosts the currently-active cycling pane
    /// (`State`/`Predict`/`Radar`). Spans the full top-area width in
    /// Compact; takes the leftmost columns in Medium/Wide.
    pub left: Rect,
    /// Event log rectangle. Always visible at Medium/Wide; sits at the
    /// bottom of the body column in Compact.
    pub log: Rect,
    /// Action strip rectangle — full-width bottom bar in Medium/Wide;
    /// 3-row strip in Compact. Holds the action list.
    pub action: Rect,
    /// Tab strip — only used in Compact. `Rect::default()` at
    /// Medium/Wide; widgets ignore it there.
    pub tabs: Rect,
    /// Status line — single row at the bottom. Distinct from `log`.
    pub status: Rect,
    /// Currently-active tab-cyclable pane (`State` / `Predict` /
    /// `Radar`). `Action` is *not* a tab-cyclable variant — it lives
    /// in the action strip and doesn't move.
    pub active_pane: PaneKind,
}

/// The game panels the player can land on. There are now two distinct
/// kinds:
///
/// 1. **Tab-cyclable**: `State`, `Predict`, `Radar`. These are the panes
///    that occupy the *left* half of the body in Medium/Wide mode and
///    cycle with Tab/Shift+Tab.
/// 2. **Pinned**: `Action` and `Log`. `Action` is the bottom-bar action
///    strip; `Log` is the always-on right side at Medium/Wide. Neither
///    is part of the Tab cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneKind {
    State,
    Predict,
    Radar,
    Action,
}

impl PaneKind {
    /// Pane-cyclable variants in their canonical order. `Action` is
    /// intentionally absent — it lives in the bottom strip, not in
    /// the tab cycle.
    pub fn tab_order() -> &'static [PaneKind] {
        &[PaneKind::State, PaneKind::Predict, PaneKind::Radar]
    }

    /// Next tab-cyclable pane clockwise (Tab). Wraps around the cycle.
    pub fn next(self) -> Self {
        let order = Self::tab_order();
        let idx = order.iter().position(|p| *p == self).unwrap_or(0);
        order[(idx + 1) % order.len()]
    }

    /// Previous tab-cyclable pane (Shift+Tab).
    pub fn prev(self) -> Self {
        let order = Self::tab_order();
        let idx = order.iter().position(|p| *p == self).unwrap_or(0);
        let prev = if idx == 0 { order.len() - 1 } else { idx - 1 };
        order[prev]
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
///
/// Layout (Medium / Wide):
///
/// ```text
/// +-----------------------------------+------+
/// |  left = active tab-cyclable pane  | log  |
/// |  (state, predict, or radar)       |      |
/// +-----------------------------------+------+
/// |       action strip (full width)             |
/// +--------------------------------------------+
/// | status (1 row)                             |
/// +--------------------------------------------+
/// ```
///
/// In Compact mode the layout collapses to a single column with the
/// same vertical order — tabs strip + active pane + log + action —
/// but the column widths are full.
///
/// Tab-cycling acts on the *left* pane only. The log is always visible
/// at Medium/Wide, the action strip is always at the bottom.
pub fn game_layout(area: Rect) -> GameRects {
    let breakpoint = Breakpoint::classify(area);

    if matches!(breakpoint, Breakpoint::TooSmall) {
        return GameRects {
            breakpoint,
            body: area,
            left: area,
            log: Rect::default(),
            action: Rect::default(),
            tabs: Rect::default(),
            status: Rect::default(),
            active_pane: PaneKind::State,
        };
    }

    // Body = area minus status (1 row). Everything else (left, log,
    // action strip, tabs) lives inside `body`.
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);
    let status = vertical[1];
    let body = vertical[0];

    if matches!(breakpoint, Breakpoint::Compact) {
        // Side-by-side layout: top row is [tabs | horizontal split
        // of (active pane | event log)]; bottom is the full-width
        // action strip. The user wanted the cycling pane and the
        // log visible at the same time so the truncated opp message
        // at the bottom (now also streamed into the log) isn't the
        // only way to read it.
        //
        // Min heights: 1 (tabs) + 4 (top row) + 3 (action) = 8 rows
        // of body plus 1 of status.
        let compact_v = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),  // tabs
                Constraint::Min(4),    // top row: pane + log
                Constraint::Length(3), // action strip
            ])
            .split(body);
        let tabs = compact_v[0];
        let top_area = compact_v[1];
        let action = compact_v[2];
        // Width split inside the top row — log gets ~40% with a
        // floor of 24 cells so the cycling pane stays legible.
        let log_min: u16 = 24;
        let log_width = if top_area.width * 2 / 5 >= log_min {
            top_area.width * 2 / 5
        } else if top_area.width >= log_min + 1 {
            log_min
        } else {
            top_area.width / 2
        };
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(0),
                Constraint::Length(log_width),
            ])
            .split(top_area);
        let left = cols[0];
        let log = cols[1];
        return GameRects {
            breakpoint,
            body,
            left,
            log,
            action,
            tabs,
            status,
            active_pane: PaneKind::State,
        };
    }

    // Medium / Wide: split body horizontally into [top-area | action-bar]
    // and vertically inside the top-area into [left | log].
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(6),          // top-area (left + log)
            Constraint::Length(3),       // action strip
        ])
        .split(body);
    let top_area = rows[0];
    let action = rows[1];

    // Pick a log width that always shows enough content to be useful
    // (≥ 24 cells, otherwise narrow it further to keep the left pane
    // legible). The right side is always the log; the left side hosts
    // the active cycling pane.
    let log_min: u16 = 24;
    let log_width = if top_area.width / 2 >= log_min {
        top_area.width / 3 // ~1/3 of the top area stays on the right
    } else if top_area.width >= log_min + 1 {
        // Just under half on a tighter pane so the left still gets
        // most of the screen for the cycling pane.
        log_min
    } else {
        // Extremely narrow medium — split halfway.
        top_area.width / 2
    };
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(log_width),
        ])
        .split(top_area);
    let left = cols[0];
    let log = cols[1];

    GameRects {
        breakpoint,
        body,
        left,
        log,
        action,
        tabs: Rect::default(),
        status,
        active_pane: PaneKind::State,
    }
}

/// Convenience: the 5-tuple form the original `game_layout` returned,
/// preserved so existing callers don't break. New callers should migrate
/// to `game_layout` (returns `GameRects`).
///
/// Returns `(body, left, log, action, status)`.
pub fn legacy_game_layout(
    area: Rect,
) -> (Rect, Rect, Rect, Rect, Rect) {
    let r = game_layout(area);
    (r.body, r.left, r.log, r.action, r.status)
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
        assert_eq!(r.left, r.body);
        // log + action + tabs are zero-sized on TooSmall.
        assert_eq!(r.log.width, 0);
        assert_eq!(r.action.width, 0);
        assert_eq!(r.tabs.width, 0);
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
        for pane in [r.body, r.left, r.tabs, r.log, r.action, r.status] {
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
        // Tabs strip is only present in Compact.
        assert!(r.tabs.height >= 1);
        // Log and cycling pane sit side-by-side in Compact now
        // (was vertical-stack before). Log must be to the right
        // of the cycling pane with both having non-zero width.
        assert!(r.log.width > 0);
        assert!(r.left.width > 0);
        assert!(
            r.left.x + r.left.width <= r.log.x,
            "Compact log must start at or after the cycling pane ends \
             (left.x+left.width={}, log.x={})",
            r.left.x + r.left.width,
            r.log.x
        );
        // Action strip is present and spans full body width.
        assert!(r.action.width > 0);
        assert_eq!(r.action.width, r.body.width);
    }

    #[test]
    fn game_layout_compact_log_and_left_are_side_by_side() {
        // The user-visible guarantee: in Compact, the cycling pane
        // and the event log share the same row band (no log-only
        // vertical strip between them and the action panel).
        let area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        };
        let r = game_layout(area);
        assert_eq!(r.breakpoint, Breakpoint::Compact);
        // Same vertical band — their y ranges overlap fully.
        assert_eq!(r.left.y, r.log.y);
        assert_eq!(r.left.height, r.log.height);
        // Log starts strictly after the cycling pane — they're
        // side-by-side, not overlapping.
        assert!(r.log.x >= r.left.x + r.left.width);
        // Action strip sits below both of them at full body width.
        assert!(r.action.y >= r.left.y + r.left.height);
        assert_eq!(r.action.width, r.body.width);
    }

    #[test]
    fn game_layout_compact_handles_narrow_terminal_without_panic() {
        // 60×18 is the canonical Compact size used in the other tests
        // — verify the new horizontal split doesn't push panes off
        // the right edge when the log floor (24 cells) would over-
        // flow the available width.
        let area = Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 18,
        };
        let r = game_layout(area);
        assert_eq!(r.breakpoint, Breakpoint::Compact);
        // Every rect must still fit within the area.
        for pane in [r.body, r.left, r.log, r.action, r.tabs] {
            assert!(pane.x + pane.width <= area.width);
            assert!(pane.y + pane.height <= area.height);
        }
        // Both halves still visible (zero width would be a regression
        // from the old vertical-stack).
        assert!(r.left.width > 0);
        assert!(r.log.width > 0);
    }

    #[test]
    fn game_layout_medium_shows_split_left_and_log_plus_action_strip() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 30,
        };
        let r = game_layout(area);
        assert_eq!(r.breakpoint, Breakpoint::Medium);
        assert!(r.left.width > 0);
        assert!(r.log.width > 0);
        assert!(r.action.width > 0);
        // The log sits on the *right* of the body — invariant of the
        // new shape.
        assert!(
            r.log.x > r.left.x,
            "log must be right of the left pane (left.x={}, log.x={})",
            r.left.x,
            r.log.x
        );
        // The action strip is below the body — invariant of the
        // bottom-bar shape.
        assert!(
            r.action.y > r.left.y,
            "action strip must be below the body (left.y={}, action.y={})",
            r.left.y,
            r.action.y
        );
        // The action strip is full body width.
        assert_eq!(
            r.action.width, r.body.width,
            "action strip width ({}) must equal body width ({})",
            r.action.width, r.body.width
        );
    }

    #[test]
    fn game_layout_wide_keeps_action_panel_full_width_and_below_log() {
        // The new shape: action strip is full body width at the
        // bottom; event log always-on right; cycling pane on the left.
        for (w, h) in [(160u16, 40u16), (120, 32), (200, 50), (240, 60)] {
            let area = Rect { x: 0, y: 0, width: w, height: h };
            let r = game_layout(area);
            // Action strip must be full body width.
            assert_eq!(
                r.action.width, r.body.width,
                "{w}x{h}: action strip width ({}) must equal body width ({})",
                r.action.width, r.body.width
            );
            // Action strip must be below the body proper.
            assert!(
                r.action.y >= r.log.y + r.log.height.saturating_sub(1)
                    || r.action.y > r.left.y + r.left.height.saturating_sub(1),
                "{w}x{h}: action strip must sit below the body"
            );
            // Log must be to the right of the left pane.
            assert!(
                r.log.x > r.left.x,
                "{w}x{h}: log must be right of the left pane"
            );
        }
    }

    #[test]
    fn game_layout_wide_left_pane_takes_more_than_log() {
        // The action strip is full-width and there's no separate
        // radar/predict pane — the left pane hosts the cycling pane
        // and inherits the larger share of the horizontal space.
        let area = Rect { x: 0, y: 0, width: 200, height: 50 };
        let r = game_layout(area);
        assert_eq!(r.breakpoint, Breakpoint::Wide);
        assert!(
            r.left.width > r.log.width,
            "left pane should take more horizontal space than the log (left={}, log={})",
            r.left.width,
            r.log.width
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
            for pane in [r.left, r.log, r.action, r.tabs, r.status] {
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
        // Smoke test the legacy 5-tuple form. We now return
        // `(body, left, log, action, status)`.
        let area = Rect {
            x: 0,
            y: 0,
            width: 120,
            height: 40,
        };
        let (body, left, log, action, status) = legacy_game_layout(area);
        let g = game_layout(area);
        assert_eq!(body, g.body);
        assert_eq!(left, g.left);
        assert_eq!(log, g.log);
        assert_eq!(action, g.action);
        assert_eq!(status, g.status);
    }

    #[test]
    fn pane_kind_cycles_through_three_tab_panes() {
        // Tab-cycling now goes State → Predict → Radar → State.
        // `Action` is the bottom-bar strip and is not in the cycle.
        use PaneKind::*;
        assert_eq!(State.next(), Predict);
        assert_eq!(Predict.next(), Radar);
        assert_eq!(Radar.next(), State);
        // Reverse:
        assert_eq!(State.prev(), Radar);
        assert_eq!(Radar.prev(), Predict);
        assert_eq!(Predict.prev(), State);
        // tab_order excludes Action.
        assert_eq!(PaneKind::tab_order().len(), 3);
        assert!(!PaneKind::tab_order().contains(&Action));
    }
}
