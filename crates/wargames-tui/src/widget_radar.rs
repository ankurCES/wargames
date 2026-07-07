//! Radar widget — live contact list fed by the engine.
//!
//! Render-time layout adapts to the pane width (see the per-row sizing
//! inside [`render`]). Contacts are passed in from `App::contacts`,
//! which is regenerated each turn so the radar visibly ticks during
//! play — that is the "live" the user expects when staring at the
//! screen between opponent tool calls.

use crate::text::truncate_with_ellipsis;
use crate::theme;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

/// One row on the radar.
///
/// `side` is the contact's affiliation; the renderer maps that to a
/// color. `bearing` is the 8-point hull label shown in the table
/// column; the polar view uses `bearing_deg` (0..360, 0 = North,
/// clockwise). `range` is 0..=1, normalised against the radar's
/// outermost ring. `speed_kn` is plain ship speed in knots. `id`
/// is the contact id, used as the visible identifier (e.g. `c-04`).
#[derive(Debug, Clone, PartialEq)]
pub struct Contact {
    pub id: String,
    pub side: ContactSide,
    pub bearing: &'static str,
    /// Compass bearing in degrees: 0 = North, 90 = East, 180 = South,
    /// 270 = West. The polar grid maps this onto the radar circle.
    pub bearing_deg: u16,
    /// Normalised range 0..=1, where 0 = centre, 1 = outermost ring.
    pub range: f32,
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
        let theme = theme::current();
        match self {
            ContactSide::Us => theme.radar_us,
            ContactSide::Nato => theme.radar_nato,
            ContactSide::Soviet => theme.radar_soviet,
            ContactSide::Neutral => theme.radar_neutral,
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

#[allow(dead_code)] // Responsive-layout threshold; reused once the single-column gate is hooked to layout().
const MIN_WIDTH_FOR_FULL: u16 = 60;

/// Render the radar pane. Pass `&[]` when no contacts are known yet
/// (the pane shows a friendly empty state). `scenario_title` is the
/// theatre line printed above the contacts; can be left blank.
pub fn render(frame: &mut Frame, area: Rect, contacts: &[Contact], scenario_title: &str) {
    let theme = theme::current();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(Span::styled(
            " RADAR / CONTACTS ",
            Style::default()
                .fg(theme.title)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let inner_w = inner.width as usize;
    let inner_h = inner.height as usize;

    // If the pane is too small to draw a polar grid (need at least
    // 5 rows and 9 cols for the smallest legible radar), fall back
    // to a compact table.
    let polar_min_w: usize = 9;
    let polar_min_h: usize = 5;
    if inner_w < polar_min_w + 8 || inner_h < polar_min_h + 3 {
        render_table_fallback(frame, inner, contacts, scenario_title);
        return;
    }

    // Layout: [optional title line] [polar grid] [compact roster].
    // Roster gets the bottom two rows so the user can still read
    // individual contacts when the polar view is busy.
    let roster_rows: usize = 2.min(inner_h.saturating_sub(polar_min_h));
    let grid_h = inner_h.saturating_sub(roster_rows);
    let grid_w = inner_w;

    // Centre the polar grid in `grid_w × grid_h`. The grid is
    // always square — we use `min(grid_w, grid_h)` as the diameter
    // so the radar circle fits in the shorter axis. Contacts at
    // the outermost ring must land inside the cell buffer; if we
    // picked `grid_w` for a tall-and-narrow pane, range=1.0
    // contacts on the N/S axes would render outside the grid_h
    // rows and get silently dropped.
    let diameter = grid_w.min(grid_h).max(polar_min_w);
    let grid_x_offset = (grid_w.saturating_sub(diameter)) / 2;
    let grid_y_end = grid_h;

    // Layer order (later overwrites earlier):
    //   1. background grid: concentric rings + cardinal/diagonal spokes
    //   2. cardinal letters (N/E/S/W) on the outermost ring axis
    //   3. centre hull marker
    //   4. contact dots, plotted by (bearing_deg, range)
    let mut cells: Vec<Vec<Option<(char, Color)>>> =
        vec![vec![None; diameter]; diameter];
    let dim = Color::Rgb(60, 60, 60);
    let spoke_color = Color::Rgb(80, 80, 80);
    let accent = theme.radar_ghost;

    let cx = diameter as f32 / 2.0;
    let cy = (diameter as f32) / 2.0;
    let max_r = (diameter as f32) / 2.0;

    // Concentric rings at 25/50/75/100% of the radius. The outermost
    // ring is brighter (it's the radar's effective horizon) and the
    // inner rings fade so the centre doesn't get visually noisy.
    // We use `*` for the outer ring and `·` for the inner three.
    let ring_target: [(f32, char); 4] = [
        (0.25, '·'),
        (0.50, '·'),
        (0.75, '·'),
        (1.00, '*'),
    ];

    for y in 0..diameter {
        for x in 0..diameter {
            let dx = x as f32 - cx + 0.5;
            let dy = y as f32 - cy + 0.5;
            let dist = (dx * dx + dy * dy).sqrt();

            // Ring hit? Use a 0.6-cell tolerance so the arcs look
            // continuous rather than dotted.
            let mut ring_hit: Option<char> = None;
            for (t, ch) in ring_target.iter() {
                let r = max_r * t;
                if (dist - r).abs() < 0.6 {
                    // Outermost ring wins because we use `*` (more
                    // visible) and inner rings use `·`.
                    ring_hit = Some(*ch);
                    // Don't break — let the loop continue so the outer
                    // ring iteration overrides inner ones on the rare
                    // case two radii overlap (only when `max_r < 1`).
                }
            }

            // Spokes — 4 cardinals (N/E/S/W) plus, if there's room,
            // 4 diagonals (NE/SE/SW/NW). We test for `dx≈0` (N/S),
            // `dy≈0` (E/W), or `|dx|≈|dy|` (diagonals). Tolerance
            // widens with radius so the spokes don't appear
            // spotty near the centre.
            let ax = dx.abs();
            let ay = dy.abs();
            let diag = (ax - ay).abs();
            let cardinal = ax < 0.6 || ay < 0.6;
            let diagonal = diag < 0.6 && !cardinal;
            let on_spoke = if diameter >= 11 {
                cardinal || diagonal
            } else {
                // Tight grids: keep only the 4 cardinals so the
                // circle doesn't look like a spiderweb.
                cardinal
            };

            match (ring_hit, on_spoke) {
                (Some(_), true) => {
                    // Spoke + ring crossing — show the ring char so
                    // we don't drop the outer boundary.
                    let ch = ring_hit.unwrap_or('·');
                    cells[y][x] = Some((ch, accent));
                }
                (Some(ch), false) => {
                    cells[y][x] = Some((ch, dim));
                }
                (None, true) => {
                    cells[y][x] = Some(('·', spoke_color));
                }
                (None, false) => {}
            }
        }
    }

    // Cardinal letters on the outermost ring (the four cardinals of
    // the compass). We place them so they're inside the radar's
    // bounding box — never overwrite the outermost-ring chars we
    // just drew. N lives one row above the centre column, E one
    // column right of the centre row, etc.
    let cxi = cx as i32;
    let cyi = cy as i32;
    let outer = (max_r.round() as i32) - 1; // sit just inside the ring
    let cardinals: [(char, i32, i32); 4] = [
        ('N', cxi, cyi - outer.max(1)),
        ('S', cxi, cyi + outer.max(1)),
        ('E', cxi + outer.max(1), cyi),
        ('W', cxi - outer.max(1), cyi),
    ];
    for (ch, x, y) in cardinals {
        if x >= 0 && y >= 0 && (x as usize) < diameter && (y as usize) < diameter
        {
            cells[y as usize][x as usize] =
                Some((ch, theme.radar_ghost));
        }
    }

    // Centre marker — the player's own hull. Always overwrite the
    // background so the centre reads clearly.
    if cxi >= 0 && cyi >= 0 && (cxi as usize) < diameter && (cyi as usize) < diameter {
        cells[cyi as usize][cxi as usize] =
            Some(('✸', theme.radar_us));
    }

    // Place contacts. Map (bearing_deg, range) → (x, y). Bearing
    // 0 = North = up; we rotate clockwise as the angle increases.
    // range 0 = centre, range 1 = outermost ring. We scale range
    // by `(max_r - 1) / max_r` so contacts at the outer ring on the
    // cardinal axes don't get rounded past the buffer boundary —
    // `range=1.0` would put them at `x = cx + max_r = diameter`,
    // one past the last valid cell index. The chosen scale leaves a
    // full 1-cell margin so integer rounding never overshoots even
    // when `cx + max_r * scale` is a half-integer (e.g. `19.5` for
    // `diameter = 20`).
    for c in contacts {
        let angle = (c.bearing_deg as f32).to_radians();
        let r = c.range.clamp(0.0, 1.0) * max_r * ((max_r - 1.0) / max_r);
        let dx = angle.sin() * r;
        let dy = -angle.cos() * r; // negate so 0° is up (y-)
        let px = (cx + dx).round() as isize;
        let py = (cy + dy).round() as isize;
        if px < 0 || py < 0 || px as usize >= diameter || py as usize >= diameter {
            continue;
        }
        cells[py as usize][px as usize] = Some(('●', c.side.color()));
    }

    // Emit the polar grid as `Line`s.
    let mut lines: Vec<Line> = Vec::with_capacity(grid_y_end + roster_rows);
    let grid_color = dim;
    for y in 0..grid_y_end.min(diameter) {
        // Pad each row with the horizontal offset so the polar grid
        // is centred inside `inner_w`.
        let pad_left = " ".repeat(grid_x_offset);
        let row_str: String = (0..diameter)
            .map(|x| {
                cells[y][x]
                    .map(|(ch, _)| ch)
                    .unwrap_or(' ')
            })
            .collect();
        // Compose with manual styling — we re-walk the row to give
        // each cell its own color, since a `Span::raw` would flatten
        // them.
        let mut spans: Vec<Span<'static>> = Vec::with_capacity(diameter + 1);
        if grid_x_offset > 0 {
            spans.push(Span::raw(pad_left));
        }
        for x in 0..diameter {
            if let Some((ch, color)) = cells[y][x] {
                spans.push(Span::styled(
                    ch.to_string(),
                    Style::default().fg(color),
                ));
            } else {
                spans.push(Span::raw(" "));
            }
        }
        // Suppress the unused-variable lint for `grid_color` — kept
        // around for future themes that want a non-default dim tint.
        let _ = grid_color;
        let _ = row_str;
        lines.push(Line::from(spans));
    }

    // Bottom row: a coloured legend that maps side colours to side
    // names — "● US · ● NATO · ● SOVIET · ● NEUTRAL". Always at least
    // one row, so the user can read the side-colour mapping without
    // staring at the polar view. We lay it out row-major across all
    // 4 sides; if the pane is too narrow for the full legend we
    // tighten gaps until it fits.
    let mut legend_spans: Vec<Span<'static>> = Vec::new();
    let legend_pattern: Vec<(ContactSide, &str, &str)> = vec![
        (ContactSide::Us, "●", "US"),
        (ContactSide::Nato, "●", "NATO"),
        (ContactSide::Soviet, "●", "SOVIET"),
        (ContactSide::Neutral, "●", "NEUTRAL"),
    ];
    // Build the widest version first, then progressively tighten
    // the separator between entries until the line fits.
    let seps = [" · ", " ·", " "];
    let mut legend_fits = false;
    for sep in seps {
        legend_spans.clear();
        for (i, (side, dot, label)) in legend_pattern.iter().enumerate() {
            if i > 0 {
                legend_spans.push(Span::styled(
                    sep.to_string(),
                    Style::default().fg(theme.radar_ghost),
                ));
            }
            legend_spans.push(Span::styled(
                dot.to_string(),
                Style::default().fg(side.color()).add_modifier(Modifier::BOLD),
            ));
            legend_spans.push(Span::styled(
                format!(" {}", label),
                Style::default().fg(theme.radar_ghost),
            ));
        }
        let used: usize = legend_spans
            .iter()
            .map(|s| crate::text::display_width(&s.content))
            .sum();
        if used <= inner_w as usize {
            legend_fits = true;
            break;
        }
    }
    let _ = legend_fits;
    // Compact roster line: every contact's id + bearing (above the
    // legend). We allow up to `roster_rows - 1` rows; if none, the
    // legend still gets its full reserved line.
    let roster_rows = roster_rows.saturating_sub(1).max(0);
    let roster: Vec<&Contact> = contacts.iter().take(roster_rows * 4).collect();
    let mut roster_line_count = 0;
    for chunk in roster.chunks(4) {
        if lines.len() >= inner_h.saturating_sub(1) {
            break;
        }
        let mut spans: Vec<Span<'static>> = Vec::new();
        for (i, c) in chunk.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(
                    " · ",
                    Style::default().fg(theme.radar_ghost),
                ));
            }
            spans.push(Span::styled(
                format!("{} ", c.id),
                Style::default().fg(theme.radar_ghost),
            ));
            spans.push(Span::styled(
                format!("{}°", c.bearing_deg),
                Style::default().fg(c.side.color()),
            ));
        }
        lines.push(Line::from(spans));
        roster_line_count += 1;
    }
    // Pad with blanks between roster and legend so we always leave
    // the very last row for the legend.
    while lines.len() < inner_h.saturating_sub(1) {
        lines.push(Line::from(""));
    }
    // Legend gets the final reserved row.
    if inner_h >= 1 {
        lines.push(Line::from(legend_spans));
    }

    // If the grid is wider than `inner_w`, we may have unused
    // vertical space — pad with blank lines so the bottom row
    // doesn't get clipped. (This is a defensive belt-and-braces —
    // the loop above already pads to `inner_h - 1`.)
    while lines.len() < inner_h {
        lines.push(Line::from(""));
    }

    if !scenario_title.is_empty() {
        // The scenario title gets the very first line, displacing
        // the top of the polar grid by one row when present.
        let theatre = format!("theater: {}", scenario_title);
        let truncated = truncate_with_ellipsis(&theatre, inner_w);
        lines.insert(
            0,
            Line::from(Span::styled(
                truncated,
                Style::default().fg(theme.radar_ghost),
            )),
        );
        // Drop the last line so we still fit `inner_h` rows. The
        // legend is the most important reservation, so we always
        // trim from the tail.
        lines.pop();
    }

    let p = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(p, inner);
    let _ = roster_line_count;
}

/// Fallback renderer for panes too narrow/small for the polar grid
/// — falls back to a compact table of the same contacts.
fn render_table_fallback(
    frame: &mut Frame,
    inner: Rect,
    contacts: &[Contact],
    scenario_title: &str,
) {
    let theme = theme::current();
    let mut lines: Vec<Line> = Vec::new();
    if !scenario_title.is_empty() {
        let theatre = format!("  theater: {}", scenario_title);
        lines.push(Line::from(Span::styled(
            truncate_with_ellipsis(&theatre, inner.width as usize),
            Style::default().fg(theme.radar_ghost),
        )));
    }
    if contacts.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (no live contacts — next turn)",
            Style::default().fg(theme.radar_ghost),
        )));
    } else {
        for c in contacts.iter() {
            let id = format!("  {} ", c.id);
            let bearing = format!("{:>3}° ", c.bearing_deg);
            let speed = format!("{}kn", c.speed_kn);
            lines.push(Line::from(vec![
                Span::styled(id, Style::default().fg(theme.radar_ghost)),
                Span::styled(bearing, Style::default().fg(c.side.color())),
                Span::styled(speed, Style::default().fg(theme.radar_ghost)),
            ]));
        }
    }
    let p = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(p, inner);
}

impl ContactSide {
    #[allow(dead_code)] // Held over for a future radar legend; not consumed by the renderer today.
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
        // Numeric bearing 0..360 (0 = North, clockwise). We map
        // `r >> 32` onto the full circle so each contact lands at
        // a unique angle. Avoid 0/360 wrap confusion by keeping
        // the range [0, 360).
        let bearing_deg = ((r >> 32) as u16) % 360;
        // Range 0..=1, biased away from the dead centre (the centre
        // dot is reserved for the player's own hull). [0.15, 1.0).
        let range = 0.15 + (((r >> 48) as f32) / (u16::MAX as f32)) * 0.85;
        out.push(Contact {
            id: format!("c-{:02}", (i + 1) % 100),
            side,
            bearing,
            bearing_deg,
            range,
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

    /// Polar-layout correctness: a contact due North at range 1.0
    /// must land in the top half of the radar grid (y < centre); a
    /// contact due South at range 1.0 must land in the bottom half
    /// (y > centre); a contact due East must land to the right of
    /// centre (x > centre). We use `TestBackend::buffer` to read the
    /// rendered cells and find the `●` glyph.
    #[test]
    fn polar_radar_places_contacts_in_their_compass_quadrants() {
        use ratatui::buffer::Cell;
        let mut t = terminal(40, 24);
        let contacts = vec![
            // North, full range
            Contact {
                id: "c-N".into(),
                side: ContactSide::Nato,
                bearing: "N",
                bearing_deg: 0,
                range: 1.0,
                speed_kn: 20,
            },
            // East, full range
            Contact {
                id: "c-E".into(),
                side: ContactSide::Soviet,
                bearing: "E",
                bearing_deg: 90,
                range: 1.0,
                speed_kn: 20,
            },
            // South, full range
            Contact {
                id: "c-S".into(),
                side: ContactSide::Neutral,
                bearing: "S",
                bearing_deg: 180,
                range: 1.0,
                speed_kn: 20,
            },
            // West, full range
            Contact {
                id: "c-W".into(),
                side: ContactSide::Us,
                bearing: "W",
                bearing_deg: 270,
                range: 1.0,
                speed_kn: 20,
            },
        ];
        t.draw(|f| render(f, f.area(), &contacts, "")).expect("draw");
        let buf = t.backend().buffer().clone();
        // Find the centre cell — the ✸ marker for the player's hull.
        // We locate it by scanning for the star character.
        let (cx, cy) = {
            let mut found = None;
            for y in 0..buf.area.height {
                for x in 0..buf.area.width {
                    let cell: &Cell = &buf[(x, y)];
                    if cell.symbol() == "\u{2738}" {
                        found = Some((x, y));
                        break;
                    }
                }
                if found.is_some() {
                    break;
                }
            }
            found.expect("centre marker \u{2738} must be rendered")
        };
        // Find each contact's ● and verify it lies in the right
        // quadrant relative to (cx, cy). The `range=1.0` puts the
        // contact on the outermost ring, so its distance from
        // centre should be roughly the radar radius.
        //
        // The bottom row of the inner pane (and optionally the row
        // above it) hosts the radar legend — coloured dots marking
        // side colourings. We exclude those from the contact-dot
        // count by skipping dots whose y sits in the last 2 rows
        // of the inner pane (legend + roster rows are reserved at
        // the bottom). The legend-dots all live ≥ `inner_h - 2`.
        let inner_bottom_skip = 2u16;
        let mut dots: Vec<(u16, u16)> = Vec::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                let cell = &buf[(x, y)];
                if cell.symbol() != "\u{25CF}" {
                    continue;
                }
                // Skip anything in the last 2 rows of the inner
                // pane — the legend row always lives there. Inner
                // pane = `frame.area()` minus border (1 cell), so
                // the inner bottom row is `frame.height - 2`.
                if y + inner_bottom_skip >= buf.area.height {
                    continue;
                }
                dots.push((x, y));
            }
        }
        // Diagnostic: dump the dots we found at their rendered
        // (post-offset) positions so a future regression can be
        // debugged from the test output without rerunning locally.
        eprintln!("DEBUG dots found at rendered positions: {:?}", dots);
        // For each contact, compute the expected (px, py) in the
        // cells buffer so we can assert the dots appear at the
        // expected compass quadrants. This avoids depending on the
        // rendered buffer's offset/spacing — we test the geometry
        // directly.
        let centre_x_f = cx as f32;
        let centre_y_f = cy as f32;
        let max_r_dbg: f32 = centre_x_f;
        let cells_positions: Vec<(u16, u16, u16)> = contacts
            .iter()
            .map(|c| {
                let angle = (c.bearing_deg as f32).to_radians();
                let r = c.range.clamp(0.0, 1.0) * max_r_dbg * 0.95;
                let dx = angle.sin() * r;
                let dy = -angle.cos() * r;
                let px = (centre_x_f + dx).round() as i32;
                let py = (centre_y_f + dy).round() as i32;
                (c.bearing_deg, px as u16, py as u16)
            })
            .collect();
        assert_eq!(
            dots.len(),
            4,
            "expected exactly 4 contact dots in the polar grid, found {} at {:?}; \
             cx={}, cy={}, expected cells positions={:?}",
            dots.len(),
            dots,
            cx,
            cy,
            cells_positions
        );
        // Quadrant expectations: north (y < cy, x ≈ cx), east (x > cx,
        // y ≈ cy), south (y > cy, x ≈ cx), west (x < cx, y ≈ cy).
        let mut north = false;
        let mut east = false;
        let mut south = false;
        let mut west = false;
        for (x, y) in dots {
            let dx = (x as i32 - cx as i32).abs();
            let dy = (y as i32 - cy as i32).abs();
            // Allow up to 1 cell of jitter from the grid layout.
            if (y as i32) < cy as i32 && dx <= 2 {
                north = true;
            } else if (x as i32) > cx as i32 && dy <= 2 {
                east = true;
            } else if (y as i32) > cy as i32 && dx <= 2 {
                south = true;
            } else if (x as i32) < cx as i32 && dy <= 2 {
                west = true;
            }
        }
        assert!(north, "north contact must land above centre");
        assert!(east, "east contact must land right of centre");
        assert!(south, "south contact must land below centre");
        assert!(west, "west contact must land left of centre");
    }

    /// `sample_contacts` must populate the new polar fields with
    /// valid ranges — bearings in [0, 360) and ranges in [0.15, 1.0).
    /// Catches a regression where `bearing_deg` or `range` is left
    /// at its default (0) and all contacts stack on the centre.
    #[test]
    fn sample_contacts_populates_polar_fields() {
        let contacts = sample_contacts(99, 12);
        assert_eq!(contacts.len(), 12);
        for c in &contacts {
            assert!(
                c.bearing_deg < 360,
                "bearing_deg must be in [0, 360), got {}",
                c.bearing_deg
            );
            assert!(
                (0.15..=1.0).contains(&c.range),
                "range must be in [0.15, 1.0], got {}",
                c.range
            );
        }
        // At least one contact should not be at bearing 0 (otherwise
        // they all stack on the North axis).
        let unique_bearings: std::collections::HashSet<u16> =
            contacts.iter().map(|c| c.bearing_deg).collect();
        assert!(
            unique_bearings.len() >= 3,
            "12 contacts should land on at least 3 distinct bearings, got {}",
            unique_bearings.len()
        );
    }

    /// The radar must render cardinal letters (N/E/S/W) inside the
    /// polar grid and a coloured legend below it. The legend is the
    /// user's only way to map dot colour to side name without
    /// referencing docs, so its presence is contract-level.
    #[test]
    fn polar_radar_renders_cardinals_and_legend() {
        let mut t = terminal(40, 24);
        let contacts = vec![
            Contact {
                id: "c-N".into(),
                side: ContactSide::Nato,
                bearing: "N",
                bearing_deg: 0,
                range: 0.5,
                speed_kn: 18,
            },
        ];
        t.draw(|f| render(f, f.area(), &contacts, "")).expect("draw");
        let buf = t.backend().buffer().clone();
        // Walk the rendered buffer and look for the legend tokens —
        // any of these being absent means the user has no way to
        // map dot colour to side.
        let s = buffer_to_string_radar(&buf);
        assert!(
            s.contains("US"),
            "legend must contain US marker, rendered buffer:\n{s:?}"
        );
        assert!(
            s.contains("NATO"),
            "legend must contain NATO marker, rendered buffer:\n{s:?}"
        );
        assert!(
            s.contains("SOVIET"),
            "legend must contain SOVIET marker, rendered buffer:\n{s:?}"
        );
        assert!(
            s.contains("NEUTRAL"),
            "legend must contain NEUTRAL marker, rendered buffer:\n{s:?}"
        );

        // Cardinal letter test: locate the centre ✸, then expect
        // letters N (above), S (below), E (right), W (left) along the
        // outermost ring axis. We allow some jitter (the cardinal
        // letters are placed by integer rounding).
        let (cx, cy) = find_centre_radar(&buf);
        let north = find_cardinal_north(&buf, cx, cy);
        let east = find_cardinal_east(&buf, cx, cy);
        let south = find_cardinal_south(&buf, cx, cy);
        let west = find_cardinal_west(&buf, cx, cy);
        assert_eq!(north, Some('N'), "north cardinal 'N' must render above centre; got {north:?}");
        assert_eq!(south, Some('S'), "south cardinal 'S' must render below centre; got {south:?}");
        assert_eq!(east, Some('E'), "east cardinal 'E' must render right of centre; got {east:?}");
        assert_eq!(west, Some('W'), "west cardinal 'W' must render left of centre; got {west:?}");
    }

    fn buffer_to_string_radar(buf: &ratatui::buffer::Buffer) -> String {
        let mut s = String::with_capacity(
            (buf.area.width as usize) * (buf.area.height as usize),
        );
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                s.push_str(buf[(x, y)].symbol());
            }
        }
        s
    }

    fn find_centre_radar(buf: &ratatui::buffer::Buffer) -> (u16, u16) {
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                if buf[(x, y)].symbol() == "\u{2738}" {
                    return (x, y);
                }
            }
        }
        panic!("centre marker not found");
    }

    fn find_cardinal_north(
        buf: &ratatui::buffer::Buffer,
        cx: u16,
        cy: u16,
    ) -> Option<char> {
        // Iterate from centre row upward; the N letter sits on the
        // vertical axis (`x ≈ cx`) just inside the outermost ring.
        // The legend lives in the very last rows so any y above cy
        // is fair game.
        for y in (0..cy).rev() {
            if let Some(c) = nearest_axis_letter(buf, cx, y) {
                return Some(c);
            }
        }
        None
    }

    fn find_cardinal_south(
        buf: &ratatui::buffer::Buffer,
        cx: u16,
        cy: u16,
    ) -> Option<char> {
        // The legend lives in the last 1-2 rows; scan up to the row
        // immediately above the legend.
        let bottom = buf.area.height.saturating_sub(2);
        for y in (cy + 1)..bottom {
            if let Some(c) = nearest_axis_letter(buf, cx, y) {
                return Some(c);
            }
        }
        None
    }

    fn find_cardinal_east(
        buf: &ratatui::buffer::Buffer,
        cx: u16,
        cy: u16,
    ) -> Option<char> {
        for x in (cx + 1)..buf.area.width {
            if let Some(c) = nearest_axis_letter(buf, x, cy) {
                return Some(c);
            }
        }
        None
    }

    fn find_cardinal_west(
        buf: &ratatui::buffer::Buffer,
        cx: u16,
        cy: u16,
    ) -> Option<char> {
        for x in (0..cx).rev() {
            if let Some(c) = nearest_axis_letter(buf, x, cy) {
                return Some(c);
            }
        }
        None
    }

    fn nearest_axis_letter(
        buf: &ratatui::buffer::Buffer,
        x: u16,
        y: u16,
    ) -> Option<char> {
        let s = buf[(x, y)].symbol();
        if s == "N" {
            Some('N')
        } else if s == "S" {
            Some('S')
        } else if s == "E" {
            Some('E')
        } else if s == "W" {
            Some('W')
        } else {
            None
        }
    }
}
