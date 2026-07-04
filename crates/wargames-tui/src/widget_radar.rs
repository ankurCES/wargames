//! Radar widget — minimal ASCII contact list (compact alternative to the JS canvas).

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use wargames_core::scenario::Scenario;

pub fn render(frame: &mut Frame, area: Rect, scenario: Option<&Scenario>) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green))
        .title(Span::styled(
            " RADAR / CONTACTS ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = vec![Line::from(Span::styled(
        "  ID    HULL       FACTION  SPD",
        Style::default().fg(Color::DarkGray),
    ))];

    if let Some(s) = scenario {
        // Scenario may or may not carry ship_tracks; render what we have.
        let raw = s.title.clone();
        // We don't parse ship_tracks (the JSON shape varies); just show
        // the scenario summary plus a deterministic placeholder for the
        // theater's contact list. Real feed integration is opt-in via --live.
        lines.push(Line::from(Span::styled(
            format!("  theater: {}", raw),
            Style::default().fg(Color::White),
        )));
        lines.push(Line::from(Span::raw("")));
        // Stable demo contacts derived from the scenario id so each scenario
        // has a distinct radar picture.
        let seed: u32 = s.id.bytes().map(|b| b as u32).sum();
        for i in 0..6 {
            let kind = match (seed.wrapping_add(i)) % 3 {
                0 => "us",
                1 => "nato",
                _ => "soviet",
            };
            let bearing = ["NW", "NE", "SE", "SW", "N", "S"][(seed.wrapping_add(i) % 6) as usize];
            let speed = 12 + (seed.wrapping_add(i * 7) % 22);
            let color = match kind {
                "us" | "nato" => Color::Cyan,
                _ => Color::LightRed,
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  c-{:02}   ", i + 1),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(
                    format!("{:<10}", bearing),
                    Style::default().fg(color),
                ),
                Span::styled(
                    format!("{:<8}", kind),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{}kn", speed),
                    Style::default().fg(Color::White),
                ),
            ]));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "  (no scenario loaded)",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let p = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(p, inner);
}