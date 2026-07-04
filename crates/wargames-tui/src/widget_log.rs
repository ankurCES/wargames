//! Event log widget — scrolls inside its box, "[N earlier omitted]" hint.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use wargames_core::log::LogEntry;

pub fn render(frame: &mut Frame, area: Rect, log: &[LogEntry]) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green))
        .title(Span::styled(
            " EVENT LOG ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let height = inner.height as usize;
    if log.is_empty() {
        let p = Paragraph::new(Line::from(Span::styled(
            "  (no events yet)",
            Style::default().fg(Color::DarkGray),
        )));
        frame.render_widget(p, inner);
        return;
    }

    let skipped = log.len().saturating_sub(height.saturating_sub(1));
    let visible: &[LogEntry] = if skipped > 0 {
        &log[log.len() - (height.saturating_sub(1))..]
    } else {
        log
    };

    let mut lines: Vec<Line> = Vec::with_capacity(visible.len() + 1);
    if skipped > 0 {
        lines.push(Line::from(Span::styled(
            format!("  … {} earlier events omitted (log auto-scrolls)", skipped),
            Style::default().fg(Color::DarkGray),
        )));
    }
    for entry in visible {
        let color = match entry.side.as_str() {
            "us" => Color::Cyan,
            "opp" => Color::LightRed,
            _ => Color::Gray,
        };
        let kind_color = match entry.kind.as_str() {
            "trigger" => Color::Yellow,
            "outcome" => Color::Magenta,
            _ => color,
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("[{:>3}] ", entry.turn),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!("{:<5} ", entry.side),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{:<8} ", entry.kind),
                Style::default().fg(kind_color),
            ),
            Span::styled(entry.message.clone(), Style::default().fg(Color::White)),
        ]));
    }
    let p = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(p, inner);
}