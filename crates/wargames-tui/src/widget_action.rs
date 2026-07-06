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
const DESCRIPTION_MIN_WIDTH: u16 = 18;

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
    // highlight symbol plus our own leading "  "); then the action name.
    let name_w = inner_w.saturating_sub(3).max(6);

    let items: Vec<ListItem> = ALL_ACTIONS
        .iter()
        .map(|a| {
            let name = truncate_with_ellipsis(a.as_str(), name_w);
            let line = if (inner.width as u16) >= DESCRIPTION_MIN_WIDTH {
                // Show both name and description (description fits after
                // we trim the name to leave at least 6 cells for it).
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
}