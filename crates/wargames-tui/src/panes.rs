//! Herdr-style 2×2 + log paned layout for the game screen.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Returns (state_pane, predict_pane, radar_pane, action_pane, log_pane).
pub fn game_layout(area: Rect) -> (Rect, Rect, Rect, Rect, Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(8),    // body
            Constraint::Length(8), // log strip
            Constraint::Length(1), // status line
        ])
        .split(area);

    let body_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(rows[0]);

    let left_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(body_cols[0]);

    let right_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(body_cols[1]);

    (left_rows[0], left_rows[1], right_rows[0], right_rows[1], rows[1])
}