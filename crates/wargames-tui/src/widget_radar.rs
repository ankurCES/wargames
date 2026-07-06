//! Radar widget — live contact list fed by the engine.
//!
//! Render-time layout adapts to the pane width (see the per-row sizing
//! inside [`render`]). Contacts are passed in from `App::contacts`,
//! which is regenerated each turn so the radar visibly ticks during
//! play — that is the "live" the user expects when staring at the
//! screen between opponent tool calls.

use crate::text::truncate_with_ellipsis;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

/// One row on the radar.
///
/// `side` is the contact's affiliation; the renderer maps that to a
/// color. `bearing` is one of `NW NE SE SW N S`; rendered as the
/// hull column. `speed_kn` is plain ship speed in knots. `id` is the
/// contact id, used as the visible identifier (e.g. `c-04`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contact {
    pub id: String,
    pub side: ContactSide,
    pub bearing: &'static str,
    pub speed_kn: u32,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ContactSide {
    Us,
    Nato,
    Soviet,
    Neutral,
}

impl ContactSide {
    fn color(self) -> Color {
        match self {
            ContactSide::Us | ContactSide::Nato => Color::Cyan,
            ContactSide::Soviet => Color::LightRed,
            ContactSide::Neutral => Color::Yellow,
        }
    }
}

const BEARINGS: &[&str] = &["NW", "NE", "SE", "SW", "N", "S"];
const SAMPLE_SIDES: &[ContactSide] = &[
    ContactSide::Us,
    ContactSide::Nato,
    ContactSide::Soviet,
    ContactSide::Neutral,
];

const MIN_WIDTH_FOR_FULL: u16 = 60;

/// Render the radar pane. Pass `&[]` when no contacts are known yet
/// (the pane shows a friendly empty state). `scenario_title` is the
/// theatre line printed above the contacts; can be left blank.
pub fn render(frame: &mut Frame, area: Rect, contacts: &[Contact], scenario_title: &str) {
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

    let inner_w = inner.width as usize;
    let mut lines: Vec<Line> = Vec::new();

    // Header row depends on the column budget.
    if inner.width >= MIN_WIDTH_FOR_FULL {
        lines.push(Line::from(Span::styled(
            "  ID    HULL       FACTION  SPD",
            Style::default().fg(Color::DarkGray),
        )));
    } else if inner.width >= 28 {
        lines.push(Line::from(Span::styled(
            "  ID    HULL       SPD",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        // Skip the header — it would be wider than the rows; just
        // render the contacts directly.
    }

    if !scenario_title.is_empty() {
        let theatre = format!("  theater: {}", scenario_title);
        lines.push(Line::from(Span::styled(
            truncate_with_ellipsis(&theatre, inner_w),
            Style::default().fg(Color::White),
        )));
        lines.push(Line::from(Span::raw("")));
    }

    if contacts.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (no live contacts — next turn)",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for c in contacts.iter() {
            // Per-row layout: depends on whether we have a full-width or
            // compact header. Spacing rules adapt so the row never
            // exceeds `inner_w`.
            let id_w = "  c-99 ".chars().count();
            let speed_w = "99kn".chars().count();
            let remaining = inner_w.saturating_sub(id_w + speed_w + 2);
            let (hull_w, faction_w) = if inner.width >= MIN_WIDTH_FOR_FULL {
                (10usize, 8usize)
            } else if inner.width >= 28 {
                (10usize, 0usize)
            } else {
                // Very narrow — bearing consumes everything that's left.
                let take = remaining.saturating_sub(2).max(4);
                (take, 0usize)
            };
            let hull_w = hull_w.min(remaining.saturating_sub(faction_w + 1).max(2));
            let faction_w = faction_w.min(remaining.saturating_sub(hull_w + 1));

            let id = format!("  {} ", c.id);
            let hull = truncate_with_ellipsis(c.bearing, hull_w);
            let row = if faction_w > 0 {
                let faction_str = truncate_with_ellipsis(c.side.label(), faction_w);
                vec![
                    Span::styled(id, Style::default().fg(Color::Gray)),
                    Span::styled(
                        format!("{:<hull_w$} ", hull, hull_w = hull_w),
                        Style::default().fg(c.side.color()),
                    ),
                    Span::styled(
                        format!("{:<faction_w$} ", faction_str, faction_w = faction_w),
                        Style::default().fg(c.side.color()).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!("{}kn", c.speed_kn), Style::default().fg(Color::White)),
                ]
            } else {
                vec![
                    Span::styled(id, Style::default().fg(Color::Gray)),
                    Span::styled(
                        format!("{:<hull_w$} ", hull, hull_w = hull_w),
                        Style::default().fg(c.side.color()),
                    ),
                    Span::styled(format!("{}kn", c.speed_kn), Style::default().fg(Color::White)),
                ]
            };
            lines.push(Line::from(row));
        }
    }

    let p = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(p, inner);
}

impl ContactSide {
    fn label(self) -> &'static str {
        match self {
            ContactSide::Us => "us",
            ContactSide::Nato => "nato",
            ContactSide::Soviet => "soviet",
            ContactSide::Neutral => "neutral",
        }
    }
}

/// Deterministic generator — seeded from `seed` (typically
/// `world.turn`). Yields `count` contacts whose side / bearing /
/// speed always agree for the same seed, but change as soon as the
/// seed changes (i.e. next turn).
pub fn sample_contacts(seed: u64, count: usize) -> Vec<Contact> {
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        // LCG so each row index deterministically lands somewhere.
        let r = seed
            .wrapping_add((i as u64).wrapping_mul(2_654_435_761))
            .wrapping_mul(4_097_856_789);
        let side = SAMPLE_SIDES[(r as usize) % SAMPLE_SIDES.len()];
        let bearing = BEARINGS[((r >> 8) as usize) % BEARINGS.len()];
        let speed_kn = 12 + (r >> 16) as u32 % 22;
        out.push(Contact {
            id: format!("c-{:02}", (i + 1) % 100),
            side,
            bearing,
            speed_kn,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use ratatui::{TerminalOptions, Viewport};

    fn terminal(w: u16, h: u16) -> Terminal<TestBackend> {
        let backend = TestBackend::new(w, h);
        Terminal::with_options(backend, TerminalOptions {
            viewport: Viewport::Fullscreen,
        })
        .expect("TestBackend terminal constructs")
    }

    fn sample() -> Vec<Contact> {
        sample_contacts(7, 4)
    }

    #[test]
    fn render_at_narrow_width_does_not_panic() {
        let mut t = terminal(20, 8);
        t.draw(|f| render(f, f.area(), &sample(), "Test Theatre"))
            .expect("narrow radar render must not panic");
    }

    #[test]
    fn render_at_typical_width_does_not_panic() {
        let mut t = terminal(80, 12);
        t.draw(|f| render(f, f.area(), &sample(), "Test Theatre"))
            .expect("typical-width radar render must not panic");
    }

    #[test]
    fn render_with_empty_contacts_does_not_panic() {
        let mut t = terminal(40, 8);
        t.draw(|f| render(f, f.area(), &[], ""))
            .expect("empty-contacts radar render must not panic");
    }

    #[test]
    fn render_at_pathological_dimensions_does_not_panic() {
        let mut t = terminal(8, 4);
        t.draw(|f| render(f, f.area(), &sample(), ""))
            .expect("pathological radar render must not panic");
    }

    #[test]
    fn sample_contacts_is_deterministic_for_a_seed() {
        let a = sample_contacts(42, 6);
        let b = sample_contacts(42, 6);
        assert_eq!(a, b, "same seed must yield identical rows");
    }

    #[test]
    fn sample_contacts_changes_with_seed() {
        let a = sample_contacts(1, 6);
        let b = sample_contacts(2, 6);
        assert_ne!(
            a, b,
            "different seeds must produce different contact rosters (otherwise the radar never ticks)"
        );
    }
}
