//! Splash — a 5-second "WAR GAMES OG" banner that paints the frame.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

const SPLASH_ART: &str = r#"
 _    _    ___     __ ______ ____   ___  ____   ____   ___  __  __ ____  ____
| |  / \  / _ \   / /| ____|  _ \ / _ \|  _ \ / ___| |  _ \|  \/  / __ )|  _ \
| | / _ \| | | | / /_|  _| | |_) | | | | |_) | |  _  | |_) | |\/| |  _ \| |_) |
| |/ ___ \ |_| |/ ___ | |___|  _ <| |_| |  _ <| |_| | |  __/| |  | | |_) |  _ <
|__/_/   \_\___/_/   |_____|_| \_\\___/|_| \_\\____| |_|   |_|  |_|____/|_| \_\
"#;

pub fn render_splash(frame: &mut Frame, area: Rect, seconds_remaining: u8) {
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green))
        .title(Span::styled(
            " WARGAMES / WOPR ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let cyan = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(Color::DarkGray);
    let mut lines: Vec<Line> = SPLASH_ART
        .lines()
        .map(|l| Line::from(Span::styled(l.to_string(), cyan)))
        .collect();
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Strategic Defense Initiative Online",
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        "All scenarios derived from real-world events.",
        Style::default().fg(Color::Gray),
    )));
    lines.push(Line::from(Span::styled(
        "Predictions update each turn from a Monte Carlo roll.",
        Style::default().fg(Color::Gray),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!(
            "Press any key to skip — splash ends in {}s",
            seconds_remaining
        ),
        dim,
    )));
    let p = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(p, inner);
}