//! Prediction widget — Monte Carlo bars, scaled to the pane width.
//!
//! Bars are sized from `inner.width` minus the label + percent columns
//! they share the row with. No fixed widths anywhere — the widget fits
//! narrow (≤30 col) panes and breathes on wide ones.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use wargames_core::Prediction;

/// How many columns the percent label occupies (` 100%` → 6 chars
/// including the leading space). Static — column alignment is the
/// single source of truth for the widget's row layout.
const PERCENT_COLS: usize = 6;

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
                format!(
                    "horizon Δdefcon {:+.2}    Δtension {:+.1}",
                    p.expected_defcon_delta, p.expected_tension_delta
                ),
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::raw("")),
            labeled_bar(
                "STRIKE  ",
                p.p_strike,
                Color::Red,
                bar_width_for(inner.width, "STRIKE  "),
            ),
            labeled_bar(
                "DISARM  ",
                p.p_disarm,
                Color::Green,
                bar_width_for(inner.width, "DISARM  "),
            ),
            labeled_bar(
                "DEFECT  ",
                p.p_defect,
                Color::Magenta,
                bar_width_for(inner.width, "DEFECT  "),
            ),
            labeled_bar(
                "NEGOT   ",
                p.p_negotiate,
                Color::Cyan,
                bar_width_for(inner.width, "NEGOT   "),
            ),
            Line::from(Span::raw("")),
            Line::from(Span::styled(
                "(bars are probability of each terminal within 5 turns)",
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

/// How many cells the bar should occupy inside a row of total width
/// `inner_width`, leaving room for `label` cells and `PERCENT_COLS`
/// (the trailing " XXX%"). Clamped to [4, 22] so we never make a bar
/// wider than the original layout nor thinner than a pixel of meaning.
fn bar_width_for(inner_width: u16, label: &str) -> usize {
    let label_cols = label.chars().count();
    let budget = (inner_width as usize).saturating_sub(label_cols + PERCENT_COLS);
    budget.clamp(4, 22)
}

fn labeled_bar(label: &'static str, prob: f32, color: Color, width: usize) -> Line<'static> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_width_scales_with_pane() {
        // 80-col pane: ample room → bar is at the original 22-cell max.
        assert_eq!(bar_width_for(80, "STRIKE  "), 22);
        // 40-col pane: still > 22 after accounting for label + percent.
        assert_eq!(bar_width_for(40, "STRIKE  "), 22);
        // 30-col pane: `STRIKE  ` (8) + 6 (pct) = 14; budget = 16,
        // still at the upper bound.
        assert_eq!(bar_width_for(30, "STRIKE  "), 16);
        // 16-col pane: tighter — 8 + 6 = 14; budget = 2, clamped to 4.
        assert_eq!(bar_width_for(16, "STRIKE  "), 4);
        // Pathological narrow — clamped to the 4-cell floor.
        assert_eq!(bar_width_for(8, "STRIKE  "), 4);
    }

    #[test]
    fn bar_width_floor_keeps_a_readable_bar() {
        // No matter how narrow the pane is, the bar is at least 4 cells —
        // the user can still see "some signal".
        for w in 0u16..=16 {
            let bw = bar_width_for(w, "STRIKE  ");
            assert!(
                (4..=22).contains(&bw),
                "bar must be within [4, 22], got {bw} for pane width {w}"
            );
        }
    }

    #[test]
    fn render_at_narrow_width_does_not_panic() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use ratatui::TerminalOptions;
        use ratatui::Viewport;

        let backend = TestBackend::new(20, 8);
        let mut terminal = Terminal::with_options(backend, TerminalOptions {
            viewport: Viewport::Fullscreen,
        })
        .expect("TestBackend terminal constructs");
        // A Prediction with all four bars exercised.
        let p = Prediction {
            p_strike: 0.4,
            p_disarm: 0.1,
            p_defect: 0.2,
            p_negotiate: 0.3,
            expected_defcon_delta: -0.5,
            expected_tension_delta: 2.0,
        };
        terminal
            .draw(|f| render(f, f.area(), Some(p)))
            .expect("narrow predict render must not panic");
    }
}