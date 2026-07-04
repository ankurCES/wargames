//! State widget — DEFCON, posture, budget, tension, detection.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use wargames_core::{Side, WorldState};

pub fn render(frame: &mut Frame, area: Rect, state: &WorldState) {
    let defcon_color = match state.defcon {
        5 => Color::Green,
        4 => Color::Yellow,
        3 => Color::LightYellow,
        2 => Color::LightRed,
        1 => Color::Red,
        _ => Color::Magenta,
    };
    let title = Span::styled(
        " STATE ",
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green))
        .title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let us = state.side(Side::Us);
    let opp = state.side(Side::Opp);

    let lines = vec![
        Line::from(vec![
            Span::styled("DEFCON ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{}", state.defcon),
                Style::default().fg(defcon_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled("   TURN ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{}", state.turn),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from({
            let mut v = vec![Span::styled("TENSION ", Style::default().fg(Color::Gray))];
            v.extend(bar(state.tension as u16, Color::Yellow));
            v.push(Span::styled(
                format!(" {:>3.0}", state.tension),
                Style::default().fg(Color::White),
            ));
            v
        }),
        Line::from({
            let mut v = vec![Span::styled("DETECT  ", Style::default().fg(Color::Gray))];
            v.extend(bar(state.detection_pct as u16, Color::Cyan));
            v.push(Span::styled(
                format!(" {:>3.0}", state.detection_pct),
                Style::default().fg(Color::White),
            ));
            v
        }),
        Line::from(Span::raw("")),
        Line::from(vec![
            Span::styled("US   ", Style::default().fg(Color::Cyan)),
            Span::styled(format!("{:<14}", format!("{:?}", us.posture).to_lowercase()), Style::default().fg(Color::White)),
            Span::styled(format!("${:>3}  C{:>2}  S{:>2}", us.escalation_budget, us.carriers_operational, us.subs_at_sea), Style::default().fg(Color::Gray)),
        ]),
        Line::from(vec![
            Span::styled("OPP  ", Style::default().fg(Color::LightRed)),
            Span::styled(format!("{:<14}", format!("{:?}", opp.posture).to_lowercase()), Style::default().fg(Color::White)),
            Span::styled(format!("${:>3}  C{:>2}  S{:>2}", opp.escalation_budget, opp.carriers_operational, opp.subs_at_sea), Style::default().fg(Color::Gray)),
        ]),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            format!("theater: {} · era: {:?}", state.theater.display_name(), state.era),
            Style::default().fg(Color::DarkGray),
        )),
    ];
    let p = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(p, inner);
}

fn bar(pct: u16, color: Color) -> Vec<Span<'static>> {
    let width = 12;
    let filled = ((pct.min(100) as usize) * width) / 100;
    let mut s = String::new();
    s.push('▇');
    for _ in 0..filled.saturating_sub(1).min(width - 1) {
        s.push('▇');
    }
    let mut out = Vec::new();
    out.push(Span::styled(s, Style::default().fg(color)));
    let pad = width.saturating_sub(filled);
    if pad > 0 {
        out.push(Span::styled("▁".repeat(pad), Style::default().fg(Color::DarkGray)));
    }
    out
}