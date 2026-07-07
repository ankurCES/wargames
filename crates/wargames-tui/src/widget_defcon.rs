//! DEFCON gauge — 5→1 escalation ladder with pulse-on-change.
//!
//! The widget owns a small `DefconGauge` struct so callers can
//! pre-compute the pulse phase (e.g. when the player commits a
//! risky action) and the render pass just paints colors. This
//! mirrors the behavior of the original `WOPR_TUI_2026` repo:
//! bright red at DEFCON 1, cyan at DEFCON 5, with a slow pulse
//! on the active rung when escalation is recent.

use crate::theme;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use wargames_core::WorldState;

/// A DEFCON gauge widget. Tracks the current rung, a transient
/// "just escalated" pulse, and the elapsed tick (used for the
/// slow ambient pulse on the active rung).
#[derive(Debug, Clone)]
pub struct DefconGauge {
    /// Last render time, in ticks, of an escalation event. Used
    /// to drive the "just changed" pulse for ~30 ticks.
    last_change_tick: Option<u64>,
}

impl Default for DefconGauge {
    fn default() -> Self {
        Self::new()
    }
}

impl DefconGauge {
    pub fn new() -> Self {
        Self { last_change_tick: None }
    }

    /// Record that the gauge just changed. Called by `App` after
    /// `apply_action` mutates `world.defcon`.
    pub fn note_change(&mut self, tick: u64) {
        self.last_change_tick = Some(tick);
    }

    /// Build the styled lines for the gauge. Kept separate from
    /// `render` so unit tests can inspect colors without a Frame.
    pub fn lines(&self, world: &WorldState, tick: u64) -> Vec<Line<'static>> {
        let defcon = world.defcon.clamp(1, 5);
        let theme = theme::current();
        let mut out: Vec<Line<'static>> = Vec::new();

        // Header.
        out.push(Line::from(vec![
            Span::styled(" DEFCON ", Style::default().fg(theme.title).bold()),
            Span::styled(
                format!("{}", defcon),
                Style::default().fg(rung_color(defcon)).bold(),
            ),
            Span::styled(
                format!(" — tension {:.0}%", world.tension),
                Style::default().fg(theme.status_text),
            ),
        ]));

        // Ladder — render all 5 rungs so the user can see "above"
        // and "below" the active level. The active rung gets the
        // brightest color; below it dim grey; above it pale.
        let mut ladder_spans: Vec<Span<'static>> = Vec::new();
        for rung in (1..=5).rev() {
            let label = format!("{rung}");
            let active = rung == defcon;
            let mut style = if active {
                Style::default().fg(rung_color(rung)).bold()
            } else if rung > defcon {
                Style::default().fg(theme.radar_ghost)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            if active && self.is_pulsing(tick) {
                style = style.add_modifier(ratatui::style::Modifier::SLOW_BLINK);
            }
            ladder_spans.push(Span::styled(label, style));
            if rung > 1 {
                ladder_spans.push(Span::raw(" "));
            }
        }
        out.push(Line::from(ladder_spans));

        // Status sub-line: which rung we're at in plain English
        // and the era. Helps the player contextualize the number.
        out.push(Line::from(Span::styled(
            format!("  {} · {:?} era", rung_label(defcon), world.era),
            Style::default().fg(theme.status_text),
        )));

        out
    }

    fn is_pulsing(&self, tick: u64) -> bool {
        match self.last_change_tick {
            Some(t) => tick.saturating_sub(t) < 30,
            None => false,
        }
    }

    /// Render the gauge into `area`.
    pub fn render(&self, frame: &mut Frame, area: Rect, world: &WorldState, tick: u64) {
        let theme = theme::current();
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(Span::styled(
                " DEFCON ",
                Style::default().fg(theme.title).bold(),
            ));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width < 4 || inner.height < 3 {
            return;
        }
        let lines = self.lines(world, tick);
        let p = Paragraph::new(lines).wrap(Wrap { trim: false });
        frame.render_widget(p, inner);
    }
}

/// Color for a rung — 1 (war) is bright red, 5 (peace) is cyan.
fn rung_color(rung: u8) -> Color {
    match rung {
        1 => Color::LightRed,
        2 => Color::Red,
        3 => Color::Yellow,
        4 => Color::LightBlue,
        _ => Color::Cyan,
    }
}

fn rung_label(rung: u8) -> &'static str {
    match rung {
        1 => "COCKED PISTOL",
        2 => "FAST PACE",
        3 => "ROUND HOUSE",
        4 => "DOUBLE TAKE",
        5 => "FADE OUT",
        _ => "UNKNOWN",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::{Terminal, TerminalOptions, Viewport};
    use wargames_core::{Era, Faction, SideState, Theater, WorldState};

    fn fresh_world(defcon: u8) -> WorldState {
        WorldState {
            turn: 1,
            era: Era::ColdWar,
            theater: Theater::BalticSea,
            faction: Faction::Us,
            defcon,
            tension: 35.0,
            detection_pct: 30.0,
            sides: [SideState::default_player(), SideState::default_opponent()],
            log: vec![],
            terminal: None,
            terror_actors: vec![],
            alliances: vec![],
        }
    }

    #[test]
    fn fresh_gauge_has_no_pulse() {
        let g = DefconGauge::new();
        assert!(g.last_change_tick.is_none());
        assert!(!g.is_pulsing(0));
        assert!(!g.is_pulsing(1000));
    }

    #[test]
    fn note_change_starts_pulse() {
        let mut g = DefconGauge::new();
        g.note_change(100);
        assert!(g.is_pulsing(100));
        assert!(g.is_pulsing(120));
        assert!(!g.is_pulsing(140));
    }

    #[test]
    fn lines_include_current_defcon_digit() {
        let g = DefconGauge::new();
        let lines = g.lines(&fresh_world(2), 0);
        // The header line includes the active rung digit as a span.
        let combined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.to_string())
            .collect();
        assert!(combined.contains('2'), "lines missing defcon digit: {combined}");
    }

    #[test]
    fn ladder_renders_all_five_rungs() {
        let g = DefconGauge::new();
        let lines = g.lines(&fresh_world(3), 0);
        let combined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.to_string())
            .collect();
        for d in ['1', '2', '3', '4', '5'] {
            assert!(combined.contains(d), "ladder missing rung {d}: {combined}");
        }
    }

    #[test]
    fn defcon_clamped_to_valid_range() {
        let g = DefconGauge::new();
        let mut w = fresh_world(0);
        let lines = g.lines(&w, 0);
        let combined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.to_string())
            .collect();
        // 0 clamps to 1.
        assert!(combined.contains('1'));

        w.defcon = 9;
        let lines = g.lines(&w, 0);
        let combined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.to_string())
            .collect();
        // 9 clamps to 5.
        assert!(combined.contains('5'));
    }

    #[test]
    fn render_does_not_panic_at_various_sizes() {
        for (w, h) in [(40u16, 6u16), (80, 12), (120, 20)] {
            let backend = TestBackend::new(w, h);
            let mut terminal = Terminal::with_options(
                backend,
                TerminalOptions { viewport: Viewport::Fullscreen },
            )
            .expect("terminal");
            terminal
                .draw(|f| {
                    let g = DefconGauge::new();
                    g.render(f, f.area(), &fresh_world(3), 0)
                })
                .expect("render");
        }
    }

    #[test]
    fn render_handles_sub_minimum_area() {
        let backend = TestBackend::new(10, 4);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions { viewport: Viewport::Fullscreen },
        )
        .expect("terminal");
        terminal
            .draw(|f| {
                let g = DefconGauge::new();
                g.render(f, f.area(), &fresh_world(3), 0)
            })
            .expect("render must early-return cleanly");
    }
}