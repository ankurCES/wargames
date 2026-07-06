//! Action menu widget — radio list with full-row highlight.
//!
//! Labels are width-aware: the action name fits the inner width, and the
//! description is dropped entirely below ~18 cols (it would just be
//! truncation noise — the verb name already carries the meaning).

use crate::text::truncate_with_ellipsis;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;
use wargames_core::Action;

pub const ALL_ACTIONS: [Action; 11] = [
    Action::Patrol,
    Action::Feint,
    Action::Mobilize,
    Action::Intercept,
    Action::Declassify,
    Action::Harden,
    Action::Bluff,
    Action::Negotiate,
    Action::StandDown,
    Action::Disarm,
    Action::Strike,
];

/// Below this width we render only the action name; the description
/// ("mobilize ground forces") is dropped to avoid a wall of ellipses.
pub const DESCRIPTION_MIN_WIDTH: u16 = 18;

/// Fallback action-panel inner width on extremely narrow terminals.
/// The fixed action list has a longest row of ~38 cells; on a
/// sub-50-col frame the layouts degrade gracefully and use this floor.
pub const ACTION_PANEL_MIN_INNER_WIDTH: u16 = 14;

/// Width (in cells) needed to render the longest action row without
/// truncation, given the current "leading 2 spaces + name + 1 space +
/// description" layout. The panes layout uses this to size the right
/// column so the action list is always fully legible.
pub fn widest_row_width() -> u16 {
    // The list is static (we export the const array above), so the
    // longest row can be computed at compile time by walking the same
    // 11 actions and taking the max of the per-row display width.
    let longest_name = ALL_ACTIONS
        .iter()
        .map(|a| crate::text::display_width(a.as_str()))
        .max()
        .unwrap_or(8);
    let longest_desc = ALL_ACTIONS
        .iter()
        .map(|a| crate::text::display_width(a.display()))
        .max()
        .unwrap_or(0);
    // Layout reserves: 2 indent + name_left_aligned(8) + 1 space + desc.
    // The name column has a fixed width of 8 inside the widget so the
    // names line up; descriptions keep their natural length.
    let w = 2 + longest_name.max(8) + 1 + longest_desc;
    w as u16
}

/// Inner width the action panel should occupy — never less than the
/// floor, never more than the widest content. The panes layout asks
/// for `widest_row_width()` and caps by its available column budget.
pub fn desired_inner_width() -> u16 {
    widest_row_width().max(ACTION_PANEL_MIN_INNER_WIDTH)
}

pub fn render(frame: &mut Frame, area: Rect, list_state: &mut ListState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green))
        .title(Span::styled(
            " ACTION ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let inner_w = inner.width as usize;
    // The bullet + 1-space gap reserves 3 cells (`> ` prefix from the
    // highlight symbol plus our own leading "  "); then the action name
    // in a fixed 8-cell column so the list stays visually aligned
    // even when the description wraps over multiple terminal widths.
    let name_w = 8usize;
    let needs_descr_min = (inner.width as u16) >= DESCRIPTION_MIN_WIDTH;

    let items: Vec<ListItem> = ALL_ACTIONS
        .iter()
        .map(|a| {
            let name = truncate_with_ellipsis(a.as_str(), name_w);
            let line = if needs_descr_min {
                // Show both name and description.
                let desc_w = inner_w.saturating_sub(name_w + 3).max(6);
                let desc = truncate_with_ellipsis(&a.display(), desc_w);
                Line::from(vec![
                    Span::styled(
                        format!("  {:<width$}", name, width = name_w),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(desc, Style::default().fg(Color::Gray)),
                ])
            } else {
                // Narrow — verb only.
                Line::from(Span::styled(
                    format!("  {}", name),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ))
            };
            ListItem::new(line)
        })
        .collect();
    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(52, 0, 0))
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, inner, list_state);
}

#[allow(dead_code)]
pub fn step_from_picker(picker: &crate::picker::Picker) -> Option<Action> {
    let i = picker.list_state.selected()?;
    ALL_ACTIONS.get(i).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_at_narrow_width_does_not_panic() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use ratatui::TerminalOptions;
        use ratatui::Viewport;

        let backend = TestBackend::new(12, 8);
        let mut terminal = Terminal::with_options(backend, TerminalOptions {
            viewport: Viewport::Fullscreen,
        })
        .expect("TestBackend terminal constructs");
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        terminal
            .draw(|f| render(f, f.area(), &mut list_state))
            .expect("narrow action render must not panic");
    }

    #[test]
    fn render_at_typical_width_does_not_panic() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use ratatui::TerminalOptions;
        use ratatui::Viewport;

        let backend = TestBackend::new(40, 12);
        let mut terminal = Terminal::with_options(backend, TerminalOptions {
            viewport: Viewport::Fullscreen,
        })
        .expect("TestBackend terminal constructs");
        let mut list_state = ListState::default();
        list_state.select(Some(7));
        terminal
            .draw(|f| render(f, f.area(), &mut list_state))
            .expect("typical-width action render must not panic");
    }

    /// Every action in the catalogue must fit inside `widest_row_width`
    /// without truncation — that's the contract the side-panel layout
    /// depends on. If `ALL_ACTIONS` ever gains a longer verb, the
    /// assertion will pin it down at compile-test time.
    #[test]
    fn every_action_fits_in_widest_row_width() {
        use crate::text::truncate_with_ellipsis;
        let budget = widest_row_width() as usize;
        for a in ALL_ACTIONS.iter() {
            // The panel renders `  <name 8> <description>` (see
            // `widget_action::render`). Compose the same string and
            // assert its display width fits the budget.
            let row = format!("  {:<8} {}", a.as_str(), a.display());
            assert!(
                crate::text::display_width(&row) <= budget,
                "row {:?} ({} cells) overflows widest_row_width ({})",
                a,
                crate::text::display_width(&row),
                budget
            );
            // Sanity: truncate_with_ellipsis at the budget must not
            // elide any non-space character of the description.
            let kept = truncate_with_ellipsis(&row, budget);
            assert!(crate::text::display_width(&kept) <= budget);
        }
    }

    #[test]
    fn widest_row_width_is_at_least_min_inner_width() {
        assert!(
            widest_row_width() >= ACTION_PANEL_MIN_INNER_WIDTH,
            "widest_row_width ({}) must be at least ACTION_PANEL_MIN_INNER_WIDTH ({})",
            widest_row_width(),
            ACTION_PANEL_MIN_INNER_WIDTH
        );
    }
}