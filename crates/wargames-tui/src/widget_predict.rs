//! Prediction widget — Monte Carlo bars.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use wargames_core::Prediction;

pub fn render(frame: &mut Frame, area: Rect, pred: Option<Prediction>) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green))
        .title(Span::styled(
            " PREDICTIONS (Monte Carlo, 1000 sims × 5 turns) ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines = match pred {
        Some(p) => vec![
            Line::from(Span::styled(
                format!("horizon Δdefcon {:+.2}    Δtension {:+.1}", p.expected_defcon_delta, p.expected_tension_delta),
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::raw("")),
            labeled_bar("STRIKE  ", p.p_strike, Color::Red),
            labeled_bar("DISARM  ", p.p_disarm, Color::Green),
            labeled_bar("DEFECT  ", p.p_defect, Color::Magenta),
            labeled_bar("NEGOT   ", p.p_negotiate, Color::Cyan),
            Line::from(Span::raw("")),
            Line::from(Span::styled(
                "(bars are probability of each terminal within 5 turns from current state)",
                Style::default().fg(Color::DarkGray),
            )),
        ],
        None => vec![Line::from(Span::styled(
            "(computing first prediction…)",
            Style::default().fg(Color::DarkGray),
        ))],
    };
    let p = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(p, inner);
}

fn labeled_bar(label: &'static str, prob: f32, color: Color) -> Line<'static> {
    let width = 22;
    let filled = ((prob.clamp(0.0, 1.0) * width as f32) as usize).min(width);
    let mut s = String::new();
    for _ in 0..filled {
        s.push('█');
    }
    let mut pad = String::new();
    for _ in 0..(width - filled) {
        pad.push('░');
    }
    Line::from(vec![
        Span::styled(label, Style::default().fg(Color::Gray)),
        Span::styled(s, Style::default().fg(color)),
        Span::styled(pad, Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!(" {:>3}%", (prob * 100.0).round() as u32),
            Style::default().fg(Color::White),
        ),
    ])
}