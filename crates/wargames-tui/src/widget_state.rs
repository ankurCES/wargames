//! State widget — DEFCON, posture, budget, tension, detection.
//!
//! All horizontal columns are sized from `inner.width`. There is no
//! fixed-width layout in the body — at 24 cols the line for "US
//! routine $50 C 3 S 2" still fits on a single row, at 12 cols it
//! collapses to its essential information (the posture names).
//!
//! Colour comes from `crate::theme::current()`. The DEFCON ladder
//! maps to the three `state_value_*` roles (ok → warn → crit) and a
//! fallback "off the chart" magenta inherited from the seed themes.

use crate::text::{self, pad_right, wrap_to_width};
use crate::theme;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use wargames_core::{Side, WorldState};

pub fn render(frame: &mut Frame, area: Rect, state: &WorldState) {
    let theme = theme::current();
    let defcon_color = match state.defcon {
        5 => theme.state_value_ok,
        4 => theme.state_value_warn,
        3 => theme.predict_bar_mid, // a "yellow-ish" mid tier — themes map this to LightYellow
        2 => theme.log_trigger,    // light-red/upcoming-warning
        1 => theme.state_value_crit,
        _ => theme.splash_accent,  // off the chart — magenta-ish
    };
    let title = Span::styled(
        " STATE ",
        Style::default()
            .fg(theme.title)
            .add_modifier(Modifier::BOLD),
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let us = state.side(Side::Us);
    let opp = state.side(Side::Opp);

    let inner_w = inner.width as usize;

    // Bar widths — leave room for the row's label + 3-digit percent.
    let bar_w = inner_w.saturating_sub("TENSION ".len() + 4).clamp(4, 24);
    let detect_w = inner_w.saturating_sub("DETECT  ".len() + 4).clamp(4, 24);
    // Theatre footer is the longest line; if it doesn't fit, wrap.
    let theatre = format!(
        "theater: {} · era: {:?}",
        state.theater.display_name(),
        state.era
    );
    let theatre_lines = wrap_to_width(&theatre, inner_w.max(4));

    // Per-faction summary text — width-aware (no truncated or padded
    // formatting; we just take whatever fits).
    let us_summary = format!(
        "US   {}  ${}  C{}  S{}",
        format!("{:?}", us.posture).to_lowercase(),
        us.escalation_budget,
        us.carriers_operational,
        us.subs_at_sea,
    );
    let opp_summary = format!(
        "OPP  {}  ${}  C{}  S{}",
        format!("{:?}", opp.posture).to_lowercase(),
        opp.escalation_budget,
        opp.carriers_operational,
        opp.subs_at_sea,
    );
    let us_lines = wrap_to_width(&us_summary, inner_w.max(4));
    let opp_lines = wrap_to_width(&opp_summary, inner_w.max(4));

    let mut lines: Vec<Line> = vec![
        Line::from(vec![
            Span::styled("DEFCON ", Style::default().fg(theme.state_dim)),
            Span::styled(
                format!("{}", state.defcon),
                Style::default().fg(defcon_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled("   TURN ", Style::default().fg(theme.state_dim)),
            Span::styled(
                format!("{}", state.turn),
                Style::default().fg(theme.state_text).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from({
            let mut v = vec![Span::styled("TENSION ", Style::default().fg(theme.state_dim))];
            v.extend(bar(state.tension as u16, theme.predict_bar_mid, bar_w));
            v.push(Span::styled(
                format!(" {:>3.0}", state.tension),
                Style::default().fg(theme.state_text),
            ));
            v
        }),
        Line::from({
            let mut v = vec![Span::styled("DETECT  ", Style::default().fg(theme.state_dim))];
            v.extend(bar(state.detection_pct as u16, theme.radar_us, detect_w));
            v.push(Span::styled(
                format!(" {:>3.0}", state.detection_pct),
                Style::default().fg(theme.state_text),
            ));
            v
        }),
        Line::from(Span::raw("")),
    ];
    // US summary may wrap to multiple rows; each row keeps the prefix.
    for (i, l) in us_lines.iter().enumerate() {
        if i == 0 {
            lines.push(Line::from(vec![
                Span::styled("US   ", Style::default().fg(theme.state_us)),
                Span::styled(l.clone(), Style::default().fg(theme.state_text)),
            ]));
        } else {
            // Continuation rows stay indented under "US".
            lines.push(Line::from(vec![
                Span::styled("     ", Style::default().fg(theme.state_dim)),
                Span::styled(l.clone(), Style::default().fg(theme.state_text)),
            ]));
        }
    }
    for (i, l) in opp_lines.iter().enumerate() {
        if i == 0 {
            lines.push(Line::from(vec![
                Span::styled("OPP  ", Style::default().fg(theme.state_opp)),
                Span::styled(l.clone(), Style::default().fg(theme.state_text)),
            ]));
        } else {
            // Continuation rows stay indented under "OPP".
            lines.push(Line::from(vec![
                Span::styled("     ", Style::default().fg(theme.state_dim)),
                Span::styled(l.clone(), Style::default().fg(theme.state_text)),
            ]));
        }
    }
    lines.push(Line::from(Span::raw("")));
    for l in theatre_lines {
        lines.push(Line::from(Span::styled(
            l,
            Style::default().fg(theme.state_dim),
        )));
    }

    let p = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(p, inner);

    // Silence unused-warning for `pad_right` — kept in the import list
    // because downstream callers will want it for the wide-terminal path.
    let _ = pad_right;
}

fn bar(pct: u16, color: Color, width: usize) -> Vec<Span<'static>> {
    let theme = theme::current();
    let width = width.max(1);
    let filled = ((pct.min(100) as usize) * width) / 100;
    let mut s = String::new();
    for _ in 0..filled {
        s.push('▇');
    }
    let mut out = Vec::new();
    out.push(Span::styled(s, Style::default().fg(color)));
    let pad = width.saturating_sub(filled);
    if pad > 0 {
        out.push(Span::styled(
            "▁".repeat(pad),
            Style::default().fg(theme.state_dim),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_uses_requested_width() {
        // Bar must honour the width passed in by the renderer, not be
        // hardcoded to 12.
        let v = bar(50, Color::Yellow, 20);
        // 10 filled + 10 unfilled (we render in pairs).
        let total_chars: usize = v.iter().map(|s| s.content.chars().count()).sum();
        assert_eq!(total_chars, 20);
    }

    #[test]
    fn bar_clamps_to_minimum_one_cell() {
        // Even at width 1, we render exactly one cell (the floor).
        let v = bar(50, Color::Yellow, 1);
        let total_chars: usize = v.iter().map(|s| s.content.chars().count()).sum();
        assert!(total_chars >= 1);
    }

    #[test]
    fn bar_zero_percent_is_all_pad() {
        let v = bar(0, Color::Yellow, 10);
        let filled: usize = v
            .iter()
            .take(1)
            .map(|s| s.content.chars().count())
            .sum();
        assert_eq!(filled, 0);
    }

    #[test]
    fn render_at_narrow_width_does_not_panic() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use ratatui::TerminalOptions;
        use ratatui::Viewport;
        use wargames_core::{
            Era, Faction, SideState, Theater, WorldState,
        };

        // A minimal world — values don't matter; we just want to prove
        // the render survives narrow panes without panicking.
        let state = WorldState {
            turn: 7,
            era: Era::ColdWar,
            theater: Theater::BalticSea,
            faction: Faction::Us,
            defcon: 3,
            tension: 65.0,
            detection_pct: 40.0,
            sides: [SideState::default_player(), SideState::default_opponent()],
            log: vec![],
            terminal: None,
            terror_actors: vec![],
            alliances: vec![],
        };
        let backend = TestBackend::new(24, 8);
        let mut terminal = Terminal::with_options(backend, TerminalOptions {
            viewport: Viewport::Fullscreen,
        })
        .expect("TestBackend terminal constructs");
        terminal
            .draw(|f| render(f, f.area(), &state))
            .expect("narrow state render must not panic");
    }

    #[test]
    fn text_helper_width_can_be_used_for_padding() {
        // Sanity check that the import is correct.
        assert_eq!(text::pad_right("ab", 5), "ab   ");
    }
}