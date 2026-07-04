//! Action menu widget — radio list with full-row highlight.

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

    let items: Vec<ListItem> = ALL_ACTIONS
        .iter()
        .map(|a| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("  {:<14}", a.as_str()),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
                Span::styled(a.display(), Style::default().fg(Color::Gray)),
            ]))
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