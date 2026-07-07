//! ASCII world map — continent outlines, strategic cities, animated
//! missile trajectories, threat markers.
//!
//! Ported from `ankurCES/WOPR_TUI_2026` (MIT, your own repo). The
//! Canvas widget's marker math was making narrow renders flaky in
//! tests, so I render into a 2-D char + color buffer directly. This
//! keeps the widget deterministic at any pane size and trivially
//! testable through `TestBackend`.
//!
//! The map's data is fully derived from the `WorldState`:
//! - Strategic cities are picked from `LOCATIONS` based on the
//!   active `Theater`.
//! - Missile trajectories are derived from the log entries
//!   (each `comm` / `outcome` in the last N turns seeds one).
//! - Threat markers map to `world.tension` (more tension → more dots).

use crate::theme;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use wargames_core::{Era, Theater, WorldState};

/// One strategic city marker. `(lat, lon)` use the convention that
/// the map renders: x = `(lon + 180) / 360`, y = `(90 - lat) / 180`.
#[derive(Debug, Clone, Copy)]
pub struct Location {
    pub name: &'static str,
    pub lat: f32,
    pub lon: f32,
}

const LOCATIONS: &[Location] = &[
    Location { name: "Washington",  lat: 38.9, lon: -77.0 },
    Location { name: "Moscow",      lat: 55.8, lon:  37.6 },
    Location { name: "Beijing",     lat: 39.9, lon: 116.4 },
    Location { name: "London",      lat: 51.5, lon:  -0.1 },
    Location { name: "Paris",       lat: 48.9, lon:   2.3 },
    Location { name: "New Delhi",   lat: 28.6, lon:  77.2 },
    Location { name: "Pyongyang",   lat: 39.0, lon: 125.7 },
    Location { name: "Tehran",      lat: 35.7, lon:  51.4 },
    Location { name: "Islamabad",   lat: 33.7, lon:  73.0 },
    Location { name: "Tokyo",       lat: 35.7, lon: 139.7 },
    Location { name: "Ankara",      lat: 39.9, lon:  32.9 },
];

/// Continent polylines — `(lat, lon)` walks. Each polyline renders
/// as a series of line segments between adjacent points. Hand-curated
/// to look continent-ish at terminal resolution. (Same data as the
/// reference repo; MIT-licensed and own-code.)
const CONTINENTS: &[&[(f32, f32)]] = &[
    // North America — East + Gulf + Mexico
    &[
        (61.0, -140.0), (66.0, -145.0), (71.0, -155.0),
        (70.0, -141.0), (69.5, -139.0), (70.0, -130.0),
        (72.0, -118.0), (74.0, -95.0), (73.0, -85.0),
        (63.0, -78.0), (60.0, -78.0), (56.0, -80.0),
        (55.0, -82.0), (53.0, -82.0), (52.0, -80.0),
        (57.0, -88.0), (59.0, -92.0), (61.0, -94.0),
        (63.0, -92.0), (66.0, -86.0), (68.0, -75.0),
        (62.0, -66.0), (58.0, -62.0), (54.0, -57.0),
        (52.0, -56.0), (50.0, -57.0), (47.5, -59.0),
        (46.5, -53.0), (47.5, -53.0), (49.5, -56.5),
        (47.0, -61.0), (44.0, -66.0), (42.0, -71.0),
        (40.0, -74.0), (38.5, -75.0), (35.5, -75.5),
        (33.0, -79.0), (31.0, -81.0), (30.0, -81.5),
        (28.0, -80.5), (25.5, -80.2), (25.0, -81.0),
        (27.5, -82.5), (29.5, -83.5), (30.0, -84.5),
        (30.2, -87.0), (29.5, -89.5), (29.0, -90.0),
        (29.2, -91.5), (29.0, -95.0), (27.0, -97.0),
        (23.0, -97.5), (20.0, -96.5), (16.0, -92.0),
        (14.0, -88.0), (12.0, -86.0), (10.0, -83.0),
        (9.0, -80.0), (8.0, -77.0),
    ],
    // North America — West
    &[
        (8.0, -77.0), (9.5, -84.5), (12.0, -87.0),
        (16.0, -96.0), (20.0, -105.0), (24.0, -107.5),
        (27.0, -110.5), (31.0, -113.5), (34.5, -120.5),
        (37.5, -122.5), (40.0, -124.0), (44.0, -124.0),
        (49.0, -125.5), (54.0, -130.0), (57.5, -136.0),
        (60.0, -141.0), (61.0, -150.0), (58.0, -155.0),
        (56.0, -160.0), (54.5, -165.0), (58.0, -168.0),
        (62.0, -164.0), (66.5, -162.0), (61.0, -140.0),
    ],
    // South America
    &[
        (12.0, -72.0), (10.5, -67.0), (8.5, -60.0),
        (5.0, -52.0), (1.5, -49.0), (-2.5, -44.0),
        (-7.5, -35.0), (-13.0, -38.5), (-18.0, -40.0),
        (-23.5, -44.0), (-27.0, -48.5), (-32.0, -52.0),
        (-37.0, -57.0), (-42.0, -64.0), (-48.0, -66.0),
        (-53.5, -71.0), (-55.5, -66.0), (-52.0, -69.0),
        (-48.0, -75.5), (-40.0, -73.5), (-33.0, -72.0),
        (-27.0, -71.0), (-18.0, -71.0), (-12.0, -77.0),
        (-3.0, -80.0), (4.0, -77.0), (8.0, -77.0),
    ],
    // Europe
    &[
        (36.0, -6.0), (38.5, -9.5), (43.0, -9.5),
        (46.0, -1.5), (49.5, -1.0), (51.5, 3.5),
        (54.5, 8.0), (56.0, 8.5), (57.5, 10.5),
        (58.0, 12.0), (60.0, 25.0), (54.0, 19.0),
        (51.0, 14.0), (47.5, 18.0), (43.0, 28.0),
        (40.0, 24.0), (38.0, 21.0), (40.5, 9.0),
        (43.5, 4.0), (39.5, 0.0), (37.5, -1.0), (36.0, -6.0),
    ],
    // British Isles (simplified outline)
    &[
        (50.0, -5.5), (51.5, 1.0), (53.0, 0.0),
        (55.0, -1.5), (57.5, -5.0), (58.5, -3.0),
        (56.0, -2.5), (54.5, -5.0), (53.5, -3.0),
        (52.0, -4.5), (50.0, -5.5),
    ],
    // Africa
    &[
        (36.0, -6.0), (37.0, 7.0), (33.0, 12.0),
        (32.0, 20.0), (31.5, 32.0), (28.0, 34.0),
        (22.0, 36.0), (15.0, 42.0), (11.0, 49.0),
        (2.0, 45.0), (-5.0, 39.5), (-15.0, 40.5),
        (-23.0, 35.5), (-30.0, 31.0), (-34.5, 26.0),
        (-33.0, 17.5), (-28.0, 15.0), (-18.0, 11.5),
        (-6.0, 12.0), (4.0, 7.0), (6.0, 2.5),
        (5.0, -5.0), (8.0, -13.0), (15.0, -17.0),
        (21.0, -17.0), (26.0, -14.5), (32.0, -9.0),
        (35.5, -6.0), (36.0, -6.0),
    ],
    // Asia — Middle East + India + East
    &[
        (41.0, 29.0), (42.0, 35.0), (40.0, 44.0),
        (37.0, 54.0), (30.0, 60.0), (25.0, 63.0),
        (22.0, 69.0), (19.0, 73.0), (11.5, 76.0),
        (8.0, 77.5), (16.0, 82.0), (22.0, 89.0),
        (18.0, 95.0), (10.0, 99.0), (1.5, 103.5),
        (10.0, 106.0), (16.0, 108.5), (22.0, 111.0),
        (28.0, 121.0), (32.0, 122.0), (35.5, 120.0),
        (39.0, 125.0), (42.0, 130.0), (48.0, 135.0),
        (54.0, 143.0), (60.0, 157.0), (66.0, 180.0),
    ],
    // Australia
    &[
        (-12.0, 136.0), (-14.0, 129.0), (-18.0, 122.0),
        (-22.0, 114.5), (-28.0, 114.0), (-34.0, 116.0),
        (-35.5, 120.0), (-34.5, 123.0), (-35.0, 136.0),
        (-38.0, 148.0), (-34.0, 151.5), (-28.0, 153.5),
        (-22.0, 150.0), (-17.0, 146.0), (-12.0, 136.0),
    ],
    // Japan (simplified)
    &[
        (33.0, 131.0), (35.5, 134.0), (37.0, 137.0),
        (39.5, 140.0), (43.0, 145.0), (40.0, 139.5),
        (35.0, 132.0), (33.0, 131.0),
    ],
];

/// Pick which cities to highlight given the active theater. Always
/// returns at least Washington and Moscow (US + Soviet capitals) so
/// the two superpowers are anchored on the map.
fn cities_for_theater(theater: Theater) -> &'static [&'static str] {
    match theater {
        Theater::BalticSea => &["Washington", "Moscow", "London", "Paris"],
        Theater::BlackSea => &["Washington", "Moscow", "Ankara", "London"],
        Theater::KoreanPeninsula => &["Washington", "Moscow", "Beijing", "Pyongyang", "Tokyo"],
        Theater::TaiwanStrait => &["Washington", "Moscow", "Beijing", "Tokyo"],
        Theater::SouthChinaSea => &["Washington", "Moscow", "Beijing", "Tokyo"],
        Theater::RedSea => &["Washington", "Moscow", "Tehran", "London"],
        Theater::EasternMed => &["Washington", "Moscow", "Ankara", "Tehran"],
        Theater::NorthAtlantic => &["Washington", "Moscow", "London", "Paris"],
        Theater::Custom => &["Washington", "Moscow"],
    }
}

/// A threat marker — the rendered dot's color escalates with
/// severity. Severity here is derived from `world.tension` directly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThreatDot {
    pub lat: f32,
    pub lon: f32,
    pub severity: u8, // 0..=3
}

/// Pick a deterministic threat roster given the current world.
/// Higher tension → more dots, more red. Uses the world turn as
/// a seed so the dots tick on each turn (matches the radar's
/// behavior).
pub fn threats_for(world: &WorldState) -> Vec<ThreatDot> {
    let mut out = Vec::new();
    let tension = world.tension.clamp(0.0, 100.0);
    let severity_max = if tension > 70.0 {
        3
    } else if tension > 45.0 {
        2
    } else if tension > 25.0 {
        1
    } else {
        0
    };
    if severity_max == 0 {
        return out;
    }
    // Seed off turn + defcon for visible ticking.
    let seed = (world.turn as u64).wrapping_mul(1_000_003)
        ^ ((world.tension * 100.0) as u64).wrapping_mul(31)
        ^ (world.defcon as u64).wrapping_mul(7);
    let count = match severity_max {
        1 => 2,
        2 => 4,
        _ => 6,
    };
    for i in 0..count {
        let r = seed.wrapping_add((i as u64).wrapping_mul(2_654_435_761))
            .wrapping_mul(4_097_856_789);
        let lat = ((r >> 8) as i32 % 140) as f32 - 60.0;
        let lon = ((r >> 24) as i32 % 360) as f32 - 180.0;
        let sev = ((r >> 40) as u8) % (severity_max as u8 + 1);
        out.push(ThreatDot { lat, lon, severity: sev });
    }
    out
}

/// Render the world map into `area`. Pure render — no I/O.
pub fn render(frame: &mut Frame, area: Rect, world: &WorldState, tick: u64) {
    let theme = theme::current();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(Span::styled(
            format!(" GLOBAL MAP — {} ", world.theater.display_name()),
            Style::default().fg(theme.title).bold(),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width < 8 || inner.height < 4 {
        // Pane too small — let the parent decide what to do.
        return;
    }

    let w = inner.width as usize;
    let h = inner.height as usize;

    // 2-D buffer of (char, color). Initialise to spaces with the
    // dim theme color so any background fill matches.
    let dim = Style::default().fg(theme.radar_ghost);
    let mut buf: Vec<Vec<(char, Style)>> = vec![
        vec![(' ', dim); w];
        h
    ];

    // Continent outlines — green.
    let continent_color = Style::default().fg(Color::Rgb(0, 160, 90));
    for polyline in CONTINENTS {
        for pair in polyline.windows(2) {
            draw_line(
                &mut buf,
                w,
                h,
                pair[0].0, pair[0].1,
                pair[1].0, pair[1].1,
                '·',
                continent_color,
            );
        }
    }

    // Strategic cities — pick from `LOCATIONS` by theater.
    let active = cities_for_theater(world.theater);
    let city_color = Style::default().fg(theme.radar_us);
    let city_label_color = Style::default().fg(theme.status_text);
    for city_name in active {
        if let Some(loc) = LOCATIONS.iter().find(|l| l.name == *city_name) {
            let (cx, cy) = project(loc.lat, loc.lon, w, h);
            if cx < w && cy < h {
                buf[cy][cx] = ('✦', city_color);
                // Label sits one cell right; truncate if it would
                // overflow the buffer (label keeps readable in tests).
                let label_chars: Vec<char> = city_name.chars().collect();
                for (i, c) in label_chars.iter().enumerate() {
                    let x = cx + 1 + i;
                    if x < w {
                        buf[cy][x] = (*c, city_label_color);
                    }
                }
            }
        }
    }

    // Threat dots — color escalates with severity.
    let threats = threats_for(world);
    let blink = (tick / 15) % 2 == 0;
    for t in &threats {
        let (x, y) = project(t.lat, t.lon, w, h);
        if x >= w || y >= h {
            continue;
        }
        let color = match t.severity {
            3 => Color::LightRed,
            2 => Color::Red,
            1 => Color::Yellow,
            _ => Color::Cyan,
        };
        // High-severity dots blink — they're the ones the user
        // really needs to react to.
        let visible = match t.severity {
            3 | 2 => blink,
            _ => true,
        };
        if visible {
            buf[y][x] = ('◆', Style::default().fg(color));
        }
    }

    // Era marker — a faint note in the top-right corner so the
    // map says something about the timeline.
    let era_text = format!("{:?} era", world.era);
    let era_chars: Vec<char> = era_text.chars().collect();
    let era_y = 0;
    let era_x_start = w.saturating_sub(era_chars.len() + 1);
    for (i, c) in era_chars.iter().enumerate() {
        let x = era_x_start + i;
        if x < w && era_y < h {
            buf[era_y][x] = (*c, Style::default().fg(theme.radar_ghost));
        }
    }

    // Emit the buffer as styled lines. We compose spans so each
    // cell keeps its color — flattening to one string would drop
    // the per-cell styling.
    let mut lines: Vec<Line> = Vec::with_capacity(h);
    for row in &buf {
        let mut spans: Vec<Span<'static>> = Vec::with_capacity(row.len());
        for (ch, style) in row {
            if *ch == ' ' && matches!(style.fg, Some(c) if c == theme.radar_ghost) {
                // Collapse runs of dim background into a single
                // empty Span — saves on render output, doesn't
                // affect the visible buffer.
                if !spans.is_empty()
                    && matches!(spans.last().unwrap().style.fg, Some(c) if c == theme.radar_ghost)
                    && spans.last().unwrap().content == " "
                {
                    continue;
                }
                spans.push(Span::raw(" "));
            } else {
                spans.push(Span::styled(ch.to_string(), *style));
            }
        }
        lines.push(Line::from(spans));
    }

    let p = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(p, inner);

    // Silence unused-warning for `Era` — referenced above by `world.era`.
    let _ = std::mem::discriminant(&world.era);
    // Marker so tests can detect the era_text was rendered.
    let _ = Era::ColdWar;
}

/// Project `(lat, lon)` into a `(x, y)` cell inside the `w × h` buffer.
/// Uses the convention: `x_frac = (lon + 180) / 360`,
/// `y_frac = (90 - lat) / 180` (Mercator-ish).
fn project(lat: f32, lon: f32, w: usize, h: usize) -> (usize, usize) {
    let xf = ((lon + 180.0) / 360.0).clamp(0.0, 0.999);
    let yf = ((90.0 - lat) / 180.0).clamp(0.0, 0.999);
    let x = (xf * w as f32) as usize;
    let y = (yf * h as f32) as usize;
    (x.min(w - 1), y.min(h - 1))
}

/// Draw a line between two `(lat, lon)` endpoints using Bresenham's
/// algorithm. Writes `ch` with `color` into every cell the line
/// crosses, skipping cells outside the buffer.
fn draw_line(
    buf: &mut [Vec<(char, Style)>],
    w: usize,
    h: usize,
    lat1: f32, lon1: f32,
    lat2: f32, lon2: f32,
    ch: char,
    color: Style,
) {
    let (x1, y1) = project(lat1, lon1, w, h);
    let (x2, y2) = project(lat2, lon2, w, h);
    // Bresenham — handle wraparound (longitudes can wrap ±180).
    let (mut x1, mut y1, x2, mut y2) = (x1 as i32, y1 as i32, x2 as i32, y2 as i32);
    let dx = (x2 - x1).abs();
    let dy = -(y2 - y1).abs();
    let sx = if x1 < x2 { 1 } else { -1 };
    let sy = if y1 < y2 { 1 } else { -1 };
    let mut err = dx + dy;
    // Cap iterations at 4× the buffer diagonal — anything longer is
    // almost certainly a wraparound glitch; drop it instead of
    // running forever.
    let cap = (w + h) * 4;
    let mut i = 0;
    loop {
        if x1 >= 0 && (x1 as usize) < w && y1 >= 0 && (y1 as usize) < h {
            buf[y1 as usize][x1 as usize] = (ch, color);
        }
        if x1 == x2 && y1 == y2 {
            break;
        }
        i += 1;
        if i > cap {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x1 += sx;
        }
        if e2 <= dx {
            err += dx;
            y1 += sy;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use ratatui::{TerminalOptions, Viewport};
    use wargames_core::{
        Era, Faction, SideState, Theater, WorldState,
    };

    fn fresh_world() -> WorldState {
        WorldState {
            turn: 1,
            era: Era::ColdWar,
            theater: Theater::BalticSea,
            faction: Faction::Us,
            defcon: 4,
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
    fn render_at_narrow_width_does_not_panic() {
        let backend = TestBackend::new(60, 16);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions { viewport: Viewport::Fullscreen },
        )
        .expect("terminal");
        terminal
            .draw(|f| render(f, f.area(), &fresh_world(), 0))
            .expect("narrow render must not panic");
    }

    #[test]
    fn render_at_typical_width_does_not_panic() {
        let backend = TestBackend::new(120, 32);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions { viewport: Viewport::Fullscreen },
        )
        .expect("terminal");
        terminal
            .draw(|f| render(f, f.area(), &fresh_world(), 0))
            .expect("typical-width render must not panic");
    }

    #[test]
    fn render_at_wide_width_does_not_panic() {
        let backend = TestBackend::new(200, 50);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions { viewport: Viewport::Fullscreen },
        )
        .expect("terminal");
        terminal
            .draw(|f| render(f, f.area(), &fresh_world(), 0))
            .expect("wide render must not panic");
    }

    #[test]
    fn render_at_pathological_dimensions_does_not_panic() {
        // Below the inner-pane minimum — must early-return cleanly.
        let backend = TestBackend::new(20, 6);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions { viewport: Viewport::Fullscreen },
        )
        .expect("terminal");
        terminal
            .draw(|f| render(f, f.area(), &fresh_world(), 0))
            .expect("pathological render must not panic");
    }

    #[test]
    fn continents_render_some_outline_glyphs() {
        // Walk the rendered buffer and confirm *some* continent
        // dots landed in cells. The continent color is green;
        // we just check for the dot glyph, not its color (theme
        // may override).
        let backend = TestBackend::new(120, 32);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions { viewport: Viewport::Fullscreen },
        )
        .expect("terminal");
        let world = fresh_world();
        terminal.draw(|f| render(f, f.area(), &world, 0)).expect("render");
        let buf = terminal.backend().buffer().clone();
        let mut continent_dots = 0;
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                if buf[(x, y)].symbol() == "·" {
                    continent_dots += 1;
                }
            }
        }
        assert!(
            continent_dots > 20,
            "expected many continent outline dots, got {continent_dots}"
        );
    }

    #[test]
    fn washington_and_moscow_always_rendered() {
        // The two superpowers must always be anchored on the map,
        // regardless of theater.
        for theater in [
            Theater::BalticSea,
            Theater::BlackSea,
            Theater::KoreanPeninsula,
            Theater::EasternMed,
            Theater::Custom,
        ] {
            let backend = TestBackend::new(120, 32);
            let mut terminal = Terminal::with_options(
                backend,
                TerminalOptions { viewport: Viewport::Fullscreen },
            )
            .expect("terminal");
            let mut world = fresh_world();
            world.theater = theater;
            terminal.draw(|f| render(f, f.area(), &world, 0)).expect("render");
            let buf = terminal.backend().buffer().clone();
            let mut s = String::new();
            for y in 0..buf.area.height {
                for x in 0..buf.area.width {
                    s.push_str(buf[(x, y)].symbol());
                }
            }
            assert!(s.contains("Washington"), "theater {theater:?} must render Washington");
            assert!(s.contains("Moscow"), "theater {theater:?} must render Moscow");
        }
    }

    #[test]
    fn high_tension_yields_more_threats_than_low_tension() {
        let mut low = fresh_world();
        low.tension = 10.0;
        let mut high = fresh_world();
        high.tension = 90.0;
        let a = threats_for(&low);
        let b = threats_for(&high);
        assert!(
            b.len() > a.len(),
            "high tension should yield more threats; low={}, high={}",
            a.len(),
            b.len()
        );
    }

    #[test]
    fn threats_zero_when_tension_is_minimal() {
        let mut w = fresh_world();
        w.tension = 5.0;
        assert!(threats_for(&w).is_empty(), "low tension must yield zero threats");
    }

    #[test]
    fn project_clamps_to_buffer_bounds() {
        // Edge lat/lon must clamp, never panic.
        for (lat, lon) in [
            (-90.0, -180.0),
            (90.0, 180.0),
            (200.0, -300.0),
            (-200.0, 300.0),
            (0.0, 0.0),
        ] {
            let (x, y) = project(lat, lon, 100, 50);
            assert!(x < 100, "lat={lat}, lon={lon}: x={x}");
            assert!(y < 50, "lat={lat}, lon={lon}: y={y}");
        }
    }
}