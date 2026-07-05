//! Country + scenario picker.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;
use wargames_core::Faction;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PickerStep {
    Country,
    Scenario,
}

#[derive(Debug, Clone)]
pub struct Picker {
    pub step: PickerStep,
    pub country_idx: usize,
    pub scenario_idx: usize,
    pub countries: Vec<Country>,
    pub scenarios: Vec<ScenarioEntry>,
    pub list_state: ListState,
    /// Cached filtered scenarios — invalidated by `next`/`prev`/`advance`.
    /// Stored as owned ids so we can return references without unsafe.
    filtered_cache: Vec<String>,
    /// Render-time message shown when the filtered scenario list is empty
    /// for the player's faction. `None` when the list is non-empty.
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Country {
    pub faction: Faction,
    pub hint: String,
}

#[derive(Debug, Clone)]
pub struct ScenarioEntry {
    pub id: String,
    pub title: String,
    pub defcon: u8,
    pub theater: String,
    pub faction: Faction,
}

impl Picker {
    pub fn new(countries: Vec<Country>, scenarios: Vec<ScenarioEntry>) -> Self {
        let mut s = ListState::default();
        s.select(Some(0));
        // NOTE: do NOT call rebuild_cache() here. The cache is meaningful only
        // in Scenario step; pre-computing it on the Country step makes the
        // renderer surface a phantom "no scenarios match this faction" error
        // before the player has even pressed Enter. The cache is built
        // lazily inside `advance()` (Country → Scenario) and inside `next`/
        // `prev` when the player rotates the country highlight.
        Self {
            step: PickerStep::Country,
            country_idx: 0,
            scenario_idx: 0,
            countries,
            scenarios,
            list_state: s,
            filtered_cache: Vec::new(),
            error: None,
        }
    }

    fn rebuild_cache(&mut self) {
        // The cache only makes sense after the player has stepped past the
        // country list. On the Country step, leave both fields empty so the
        // renderer never paints an empty-state message that doesn't belong
        // to the current step.
        if self.step != PickerStep::Scenario {
            self.filtered_cache.clear();
            self.error = None;
            return;
        }
        let faction = self.countries.get(self.country_idx).map(|c| c.faction);
        // A faction "plays" any scenario tagged for itself OR for any
        // great-power bloc in the same theater of operations. Without this
        // widening, picking PRC or DPRK (which have no JSON-tagged scenarios
        // yet) leaves the player with an empty list and no signal that the
        // picker is hung.
        //
        // Visibility matrix:
        //   Us     → Us | Nato | Soviet        (Cold-War great-power tag set)
        //   Nato   → Nato | Us                 (alliance partner)
        //   Soviet → Soviet | Nato | Us        (mirror of Us)
        //   Prc    → Prc | Us | Nato | Soviet  (modern peer; sees everything)
        //   Dprk   → Us | Nato                 (Korean theater; no Soviet)
        let accepted = |sf: Faction| -> bool {
            match faction {
                Some(Faction::Us) => {
                    matches!(sf, Faction::Us | Faction::Nato | Faction::Soviet)
                }
                Some(Faction::Nato) => matches!(sf, Faction::Nato | Faction::Us),
                Some(Faction::Soviet) => {
                    matches!(sf, Faction::Soviet | Faction::Nato | Faction::Us)
                }
                Some(Faction::Prc) => matches!(
                    sf,
                    Faction::Prc | Faction::Us | Faction::Nato | Faction::Soviet
                ),
                Some(Faction::Dprk) => matches!(sf, Faction::Us | Faction::Nato),
                None => true,
            }
        };
        self.filtered_cache = self
            .scenarios
            .iter()
            .filter(|s| accepted(s.faction))
            .map(|s| s.id.clone())
            .collect();
        // Surface an explicit empty-state message rather than a silent blank.
        self.error = if self.filtered_cache.is_empty() {
            Some("no scenarios match this faction — press Esc to go back".to_string())
        } else {
            None
        };
        // Clamp scenario_idx.
        if self.scenario_idx >= self.filtered_cache.len() {
            self.scenario_idx = self.filtered_cache.len().saturating_sub(1);
        }
    }
    pub fn next(&mut self) {
        let len = match self.step {
            PickerStep::Country => self.countries.len(),
            PickerStep::Scenario => self.filtered_cache.len(),
        };
        if len == 0 {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        let next = (i + 1) % len;
        self.list_state.select(Some(next));
        match self.step {
            PickerStep::Country => self.country_idx = next,
            PickerStep::Scenario => self.scenario_idx = next,
        }
    }

    pub fn prev(&mut self) {
        let len = match self.step {
            PickerStep::Country => self.countries.len(),
            PickerStep::Scenario => self.filtered_cache.len(),
        };
        if len == 0 {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        let prev = if i == 0 { len - 1 } else { i - 1 };
        self.list_state.select(Some(prev));
        match self.step {
            PickerStep::Country => self.country_idx = prev,
            PickerStep::Scenario => self.scenario_idx = prev,
        }
    }

    pub fn advance(&mut self) -> bool {
        match self.step {
            PickerStep::Country => {
                self.step = PickerStep::Scenario;
                self.list_state.select(Some(0));
                self.scenario_idx = 0;
                self.rebuild_cache();
                false
            }
            PickerStep::Scenario => !self.filtered_cache.is_empty(),
        }
    }

    pub fn back(&mut self) -> bool {
        if self.step == PickerStep::Scenario {
            self.step = PickerStep::Country;
            self.list_state.select(Some(self.country_idx));
            true
        } else {
            false
        }
    }

    pub fn selected_country(&self) -> Option<&Country> {
        self.countries.get(self.country_idx)
    }

    /// Returns the currently-highlighted scenario, if any.
    pub fn selected_scenario(&self) -> Option<&ScenarioEntry> {
        let id = self.filtered_cache.get(self.scenario_idx)?;
        self.scenarios.iter().find(|s| &s.id == id)
    }

    pub fn filtered_scenarios(&self) -> Vec<&ScenarioEntry> {
        self.filtered_cache
            .iter()
            .filter_map(|id| self.scenarios.iter().find(|s| &s.id == id))
            .collect()
    }
}

pub fn render_picker(frame: &mut Frame, area: Rect, picker: &mut Picker) {
    frame.render_widget(Clear, area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // bordered title strip
            Constraint::Min(1),    // list (or empty-state message)
            Constraint::Length(1), // status bar
        ])
        .split(area);

    let title = match picker.step {
        PickerStep::Country => "PICK A COUNTRY",
        PickerStep::Scenario => "PICK A SCENARIO",
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green))
        .title(Span::styled(
            format!(" {title} "),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
    frame.render_widget(block, chunks[0]);

    // Empty-state branch — explicit, no silent blank list.
    if let Some(msg) = picker.error.as_ref() {
        let p = Paragraph::new(Line::from(Span::styled(
            format!("  {msg}"),
            Style::default().fg(Color::LightRed).add_modifier(Modifier::ITALIC),
        )))
        .wrap(Wrap { trim: false });
        frame.render_widget(p, chunks[1]);
        render_picker_status(frame, chunks[2], picker, "");
        return;
    }

    let items: Vec<ListItem> = match picker.step {
        PickerStep::Country => picker
            .countries
            .iter()
            .map(|c| {
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("  {} ", c.faction.display_name()),
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        c.hint.clone(),
                        Style::default().fg(Color::Gray),
                    ),
                ]))
            })
            .collect(),
        PickerStep::Scenario => picker
            .filtered_scenarios()
            .into_iter()
            .map(|s| {
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("  DEFCON {}  ", s.defcon),
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{:<22}", s.title),
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(
                        format!("[{}]", s.theater),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
            })
            .collect(),
    };

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(52, 0, 0))
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, chunks[1], &mut picker.list_state);
    render_picker_status(frame, chunks[2], picker, "");
}

/// Renders the 1-line status bar at the bottom of the picker pane. The
/// caller passes an optional overlay string (e.g. "LOADING…") that takes
/// visual precedence when the run loop is busy with scenario work.
pub fn render_picker_status(frame: &mut Frame, area: Rect, picker: &Picker, overlay: &str) {
    let body = if !overlay.is_empty() {
        format!(" » {overlay}")
    } else {
        match picker.step {
            PickerStep::Country => format!(
                " pick a country (↑↓ select, Enter confirm) — {} available",
                picker.countries.len()
            ),
            PickerStep::Scenario => format!(
                " pick a scenario (↑↓ select, Enter confirm, Esc back) — {} filtered",
                picker.filtered_scenarios().len()
            ),
        }
    };
    let p = Paragraph::new(body).style(Style::default().bg(Color::Rgb(20, 20, 20)));
    frame.render_widget(p, area);
}

pub fn default_countries() -> Vec<Country> {
    vec![
        Country {
            faction: Faction::Us,
            hint: "Carrier groups, B-2s, NATO alliance".to_string(),
        },
        Country {
            faction: Faction::Soviet,
            hint: "Submarines, ICBMs, Spetsnaz".to_string(),
        },
        Country {
            faction: Faction::Nato,
            hint: "Collective defense, Article 5".to_string(),
        },
        Country {
            faction: Faction::Prc,
            hint: "Carrier program, ADIZ doctrine".to_string(),
        },
        Country {
            faction: Faction::Dprk,
            hint: "Limited arsenal, high noise".to_string(),
        },
        // Sentinel: select this and Enter to enter AI vs AI mode. The
        // hint string is the contract — the App layer keys off it. Kept
        // here (rather than in App) so the picker always shows it
        // without an extra App-side hookup.
        Country {
            faction: Faction::Us,
            hint: "__ai_vs_ai__ two agents fight the war end-to-end with separate personas + learning".to_string(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use wargames_core::Faction;

    fn entry(id: &str, faction: Faction) -> ScenarioEntry {
        ScenarioEntry {
            id: id.into(),
            title: id.into(),
            defcon: 3,
            theater: "test".into(),
            faction,
        }
    }

    #[test]
    fn usa_sees_us_and_nato_scenarios() {
        let mut p = Picker::new(
            vec![Country { faction: Faction::Us, hint: "".into() }],
            vec![
                entry("a", Faction::Us),
                entry("b", Faction::Nato),
                entry("c", Faction::Soviet),
                entry("d", Faction::Prc),
            ],
        );
        p.advance();
        let ids: Vec<&str> = p.filtered_scenarios().iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b", "c"], "USA must see US+NATO+Soviet scenarios");
    }

    #[test]
    fn soviet_sees_us_nato_and_soviet_scenarios() {
        let mut p = Picker::new(
            vec![Country { faction: Faction::Soviet, hint: "".into() }],
            vec![
                entry("a", Faction::Us),
                entry("b", Faction::Nato),
                entry("c", Faction::Soviet),
                entry("d", Faction::Prc),
            ],
        );
        p.advance();
        let ids: Vec<&str> = p.filtered_scenarios().iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["a", "b", "c"],
            "Soviet must mirror Us: see US+NATO+Soviet scenarios"
        );
    }

    #[test]
    fn advance_resets_scenario_index() {
        let mut p = Picker::new(
            vec![Country { faction: Faction::Us, hint: "".into() }],
            vec![entry("a", Faction::Us), entry("b", Faction::Nato)],
        );
        p.advance();
        assert_eq!(p.scenario_idx, 0);
        assert_eq!(p.list_state.selected(), Some(0));
    }

    #[test]
    fn prc_sees_modern_great_power_set() {
        let mut p = Picker::new(
            vec![Country { faction: Faction::Prc, hint: "".into() }],
            vec![
                entry("a", Faction::Us),
                entry("b", Faction::Nato),
                entry("c", Faction::Soviet),
                entry("d", Faction::Prc),
            ],
        );
        p.advance();
        let ids: Vec<&str> = p.filtered_scenarios().iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["a", "b", "c", "d"],
            "PRC sees the full modern great-power set"
        );
    }

    #[test]
    fn dprk_sees_us_nato_only() {
        let mut p = Picker::new(
            vec![Country { faction: Faction::Dprk, hint: "".into() }],
            vec![
                entry("a", Faction::Us),
                entry("b", Faction::Nato),
                entry("c", Faction::Soviet),
                entry("d", Faction::Prc),
            ],
        );
        p.advance();
        let ids: Vec<&str> = p.filtered_scenarios().iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b"], "DPRK only sees US+NATO scenarios");
    }

    #[test]
    fn empty_state_sets_error_message() {
        let mut p = Picker::new(
            vec![Country { faction: Faction::Dprk, hint: "".into() }],
            vec![entry("x", Faction::Prc), entry("y", Faction::Soviet)],
        );
        p.advance();
        assert!(p.filtered_scenarios().is_empty());
        assert!(p.error.is_some(), "must surface a render-time empty-state message");
        assert!(p.advance() == false, "Enter on an empty list must not advance into the game");
    }

    /// Regression test for the Phase 9 bug where `Picker::new` called
    /// `rebuild_cache` eagerly on the Country step. With the default-US
    /// country selected and a realistic scenario list (some tagged US/NATO/
    /// Soviet, none tagged DPRK), the renderer must NOT show "no scenarios
    /// match this faction — press Esc to go back" before the player has
    /// even pressed Enter on a country.
    #[test]
    fn fresh_picker_on_country_step_has_no_error_and_empty_cache() {
        let scenarios = vec![
            entry("us_a", Faction::Us),
            entry("nato_a", Faction::Nato),
            entry("soviet_a", Faction::Soviet),
            entry("prc_a", Faction::Prc),
            // no DPRK scenario — would have triggered the empty-state on a
            // pre-computed cache for any faction whose tag set didn't match.
        ];
        let p = Picker::new(default_countries(), scenarios);
        assert_eq!(p.step, PickerStep::Country, "fresh picker starts on Country");
        assert!(
            p.error.is_none(),
            "fresh picker must not surface an error before the user advances; got: {:?}",
            p.error
        );
        assert!(
            p.filtered_cache.is_empty(),
            "fresh picker must not pre-compute the scenario cache; got: {:?}",
            p.filtered_cache
        );
    }

    /// Defense-in-depth for the same bug: even if some future code path
    /// leaves a stale `error` set, the renderer must not paint the
    /// empty-state message while the picker is still on the Country step.
    /// This exercises the render branch (`error.is_some()`) but with a
    /// Country-step picker; the test is structural (no Frame), but the
    /// invariant `step == Country ⇒ no error displayed` is what we care
    /// about, so we encode it as a precondition the render path enforces.
    #[test]
    fn stale_error_on_country_step_is_cleared_by_rebuild_cache() {
        let scenarios = vec![entry("us_a", Faction::Us)];
        let mut p = Picker::new(default_countries(), scenarios);
        // Stale error — would have come from a previous scenario pick or a
        // pre-computed cache if `rebuild_cache` were called eagerly.
        p.error = Some("stale".into());
        // Force a rebuild (defensive — happens on advance()/next()/prev()).
        // The fix in `rebuild_cache` clears error on Country step.
        p.rebuild_cache();
        assert_eq!(p.step, PickerStep::Country);
        assert!(
            p.error.is_none(),
            "rebuild_cache must clear any stale error on the Country step; got: {:?}",
            p.error
        );
    }

    /// End-to-end reachability proof: every supported faction can move from
    /// the country step to the scenario step and see at least one scenario.
    /// This is the playability precondition for `(c)` — without it, every
    /// faction has at least one reachability gap that silently strands the
    /// user on a blank list.
    #[test]
    fn every_faction_advances_to_scenarios_with_results() {
        use std::collections::HashSet;
        // Visibility matrix (must agree with `rebuild_cache`):
        //   Us     → Us | Nato | Soviet
        //   Nato   → Nato | Us
        //   Soviet → Soviet | Nato | Us
        //   Prc    → Prc | Us | Nato | Soviet
        //   Dprk   → Us | Nato
        // Every faction in this test pool has at least one scenario in the
        // fully-stocked fixture below — so the player is never left with an
        // empty visible list after picking a country.
        let scenarios = vec![
            entry("us_a", Faction::Us),
            entry("nato_a", Faction::Nato),
            entry("soviet_a", Faction::Soviet),
            entry("prc_a", Faction::Prc),
            entry("dprk_a", Faction::Dprk),
        ];
        let cases = [
            (Faction::Us, &["us_a", "nato_a", "soviet_a"][..]),
            (Faction::Nato, &["us_a", "nato_a"][..]),
            (Faction::Soviet, &["us_a", "nato_a", "soviet_a"][..]),
            (Faction::Prc, &["us_a", "nato_a", "soviet_a", "prc_a"][..]),
            (Faction::Dprk, &["us_a", "nato_a"][..]),
        ];
        for (faction, expected_visible) in cases {
            let mut p = Picker::new(
                vec![Country { faction, hint: "".into() }],
                scenarios.clone(),
            );
            // advance() returns `true` only at the *second* call (when the
            // picker is on the scenario step AND the player confirms a
            // scenario to enter the game). The first call is the
            // Country→Scenario transition and always returns `false`. So
            // we test the precondition for entry into the game: after the
            // first advance(), the picker must be on the scenario step
            // with at least one visible scenario.
            let transitioned = p.advance();
            assert!(
                !transitioned,
                "{faction:?}: advance() at country step must return false (it is the transition arm, not the 'enter game' arm)"
            );
            assert_eq!(
                p.step,
                PickerStep::Scenario,
                "{faction:?}: must be on the scenario step after advance()"
            );
            assert!(
                !p.filtered_scenarios().is_empty(),
                "{faction:?}: must see at least one scenario after picking a country"
            );
            assert!(
                p.error.is_none(),
                "{faction:?}: must not surface an empty-state error when scenarios exist"
            );

            // Confirm the visibility set matches the documented matrix.
            let seen: HashSet<&str> = p
                .filtered_scenarios()
                .iter()
                .map(|s| s.id.as_str())
                .collect();
            let expected: HashSet<&str> = expected_visible.iter().copied().collect();
            assert_eq!(
                seen, expected,
                "{faction:?}: visibility matrix regression (expected {expected:?}, got {seen:?})"
            );

            // And the second advance() — picking a scenario — must succeed.
            let entered_game = p.advance();
            assert!(
                entered_game,
                "{faction:?}: second advance() at scenario step must succeed"
            );
        }
    }
}