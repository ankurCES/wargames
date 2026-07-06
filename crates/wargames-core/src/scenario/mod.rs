//! Scenario JSON loader — matches the JS impl's contract.
//!
//! Layout:
//! - [`Scenario`] is the JSON shape (matches `scenarios/*.json` files).
//! - [`corpus`] is the in-Rust theater conflict corpus for procedural
//!   generation.
//! - [`generator`] builds a `Scenario` from the corpus, seed-driven and
//!   deterministic — used by `--ai-vs-ai` and `--regen` in the TUI.

use crate::proxies::{Alliance, TerrorActor};
use crate::state::{Era, Faction, SideState, Theater};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub mod corpus;
pub mod generator;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub briefing: String,
    pub initial_state: Option<String>,
    pub initial_detection_pct: Option<f32>,
    #[serde(default)]
    pub faction: Option<Faction>,
    #[serde(default)]
    pub era: Option<Era>,
    #[serde(default)]
    pub theater: Option<Theater>,
    #[serde(default)]
    pub us: Option<SideState>,
    #[serde(default)]
    pub soviet: Option<SideState>,
    pub opening_message: Option<String>,
    pub win_conditions: Option<WinConditions>,
    /// Non-state actors present at scenario start. Defaults to
    /// empty so legacy scenarios without this field still load.
    #[serde(default)]
    pub terror_actors: Vec<TerrorActor>,
    /// Bilateral alliances (treaties, proxy sponsorships,
    /// rivalries) present at scenario start. Defaults to empty.
    #[serde(default)]
    pub alliances: Vec<Alliance>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WinConditions {
    pub disarm: Option<String>,
    pub defeat_strike: Option<String>,
    pub mutual_assured_destruction: Option<String>,
}

impl Scenario {
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, ScenarioError> {
        let raw = std::fs::read_to_string(path.as_ref())?;
        let s: Scenario = serde_json::from_str(&raw)?;
        Ok(s)
    }

    /// Best-effort theater inference from the id (e.g. `baltic_sea_2024`).
    pub fn infer_theater(&self) -> Theater {
        if let Some(t) = self.theater {
            return t;
        }
        let id = self.id.to_ascii_lowercase();
        if id.contains("baltic") {
            Theater::BalticSea
        } else if id.contains("black") {
            Theater::BlackSea
        } else if id.contains("korea") {
            Theater::KoreanPeninsula
        } else if id.contains("taiwan") {
            Theater::TaiwanStrait
        } else if id.contains("south_china") {
            Theater::SouthChinaSea
        } else if id.contains("red_sea") {
            Theater::RedSea
        } else if id.contains("eastern_med") {
            Theater::EasternMed
        } else if id.contains("north_atlantic") {
            Theater::NorthAtlantic
        } else {
            Theater::Custom
        }
    }

    pub fn infer_era(&self) -> Era {
        if let Some(e) = self.era {
            return e;
        }
        let id = self.id.to_ascii_lowercase();
        if id.contains("1983") || id.contains("cold_war") {
            Era::ColdWar
        } else if id.contains("2030") {
            Era::NearPeer2030
        } else {
            Era::Modern
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ScenarioError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json parse error: {0}")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_scenario_shape() {
        let raw = r#"{
            "id": "north_atlantic_1983",
            "title": "North Atlantic, 1983",
            "briefing": "DEFCON 3.",
            "initial_state": "DEFCON_3",
            "initial_detection_pct": 45,
            "faction": "nato",
            "us": { "posture": "routine", "carriers_operational": 2, "silos_ready": 100, "escalation_budget": 60, "subs_at_sea": 0 },
            "soviet": { "posture": "routine", "carriers_operational": 0, "silos_ready": 100, "escalation_budget": 55, "subs_at_sea": 8 }
        }"#;
        let s: Scenario = serde_json::from_str(raw).unwrap();
        assert_eq!(s.id, "north_atlantic_1983");
        assert_eq!(s.infer_theater(), Theater::NorthAtlantic);
        assert_eq!(s.infer_era(), Era::ColdWar);
    }

    #[test]
    fn infers_taiwan_theater() {
        let raw = r#"{
            "id": "taiwan_strait_2024",
            "title": "Taiwan Strait",
            "briefing": "ADIZ breach.",
            "initial_state": "DEFCON_3"
        }"#;
        let s: Scenario = serde_json::from_str(raw).unwrap();
        assert_eq!(s.infer_theater(), Theater::TaiwanStrait);
    }

    /// M7: the two new scenarios exercising proxy mechanics must
    /// load cleanly from disk and carry terror actors / alliances
    /// through. We resolve paths relative to the crate manifest
    /// dir (`CARGO_MANIFEST_DIR/../../scenarios/...`) so the test
    /// works in both `cargo test` and `cargo test --manifest-path`.
    #[test]
    fn proxy_scenarios_load_with_terror_actors_and_alliances() {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        // <repo>/crates/wargames-core/Cargo.toml → <repo>/scenarios/
        let scenarios_dir = manifest_dir
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("scenarios"))
            .expect("repo scenarios dir resolves");

        for (filename, expected_actors, expected_alliances) in [
            ("eastern_med_proxy.json", 2, 1),
            ("red_sea_freelance.json", 3, 0),
        ] {
            let path = scenarios_dir.join(filename);
            let s = Scenario::from_path(&path).unwrap_or_else(|e| {
                panic!("{filename} must load from {path:?}, got: {e}")
            });
            assert_eq!(
                s.terror_actors.len(),
                expected_actors,
                "{filename}: expected {expected_actors} terror actors, got {} ({:?})",
                s.terror_actors.len(),
                s.terror_actors.iter().map(|a| &a.id).collect::<Vec<_>>()
            );
            assert_eq!(
                s.alliances.len(),
                expected_alliances,
                "{filename}: expected {expected_alliances} alliances"
            );
        }
    }

    /// Eastern Med scenario: the two actors must have *opposing*
    /// sponsors (one US, one Opp). A regression where sponsors
    /// collapse to the default would make proxy vs proxy
    /// meaningless.
    #[test]
    fn eastern_med_proxy_actors_have_opposing_sponsors() {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let path = manifest_dir
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("scenarios/eastern_med_proxy.json"))
            .expect("path resolves");
        let s = Scenario::from_path(&path).unwrap();
        assert_eq!(s.terror_actors.len(), 2);
        let sponsors: std::collections::HashSet<_> = s
            .terror_actors
            .iter()
            .map(|a| a.sponsor)
            .collect();
        assert!(sponsors.contains(&crate::proxies::Sponsor::Us));
        assert!(sponsors.contains(&crate::proxies::Sponsor::Opp));
    }

    /// Red Sea freelance: every actor must be Independent (no
    /// sponsor). If a sponsor leaks in, the player could exploit
    /// the supply-chain toolchain — these actors are deliberately
    /// outside the bipolar framework.
    #[test]
    fn red_sea_freelance_actors_are_all_independent() {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let path = manifest_dir
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("scenarios/red_sea_freelance.json"))
            .expect("path resolves");
        let s = Scenario::from_path(&path).unwrap();
        assert!(!s.terror_actors.is_empty());
        for actor in &s.terror_actors {
            assert_eq!(
                actor.sponsor,
                crate::proxies::Sponsor::Independent,
                "{} must be Independent (no sponsor), got {:?}",
                actor.id,
                actor.sponsor
            );
        }
    }

    /// End-to-end: load the Eastern Med scenario, build a
    /// `WorldState` from it, confirm the terror actors + alliance
    /// are present. This is the contract `app::build_world` uses.
    #[test]
    fn eastern_med_world_state_includes_terror_actors() {
        use crate::state::{SideState, WorldState};
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let path = manifest_dir
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("scenarios/eastern_med_proxy.json"))
            .expect("path resolves");
        let scenario = Scenario::from_path(&path).unwrap();
        // Replicate the `build_world` shape, only asserting the
        // proxy fields. Mirrors what app.rs::build_world does.
        let sides = [
            SideState::default_player(),
            SideState::default_opponent(),
        ];
        let world = WorldState {
            turn: 1,
            era: scenario.infer_era(),
            theater: scenario.infer_theater(),
            faction: scenario.faction.unwrap_or(crate::state::Faction::Us),
            defcon: 4,
            tension: 40.0,
            detection_pct: 45.0,
            sides,
            log: vec![],
            terminal: None,
            terror_actors: scenario.terror_actors.clone(),
            alliances: scenario.alliances.clone(),
        };
        assert_eq!(world.terror_actors.len(), 2);
        assert_eq!(world.alliances.len(), 1);
        assert_eq!(world.alliances[0].kind, crate::proxies::AllianceKind::Rivalry);
    }
}