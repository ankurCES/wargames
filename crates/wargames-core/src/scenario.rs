//! Scenario JSON loader — matches the JS impl's contract.

use crate::state::{Era, Faction, SideState, Theater};
use serde::{Deserialize, Serialize};
use std::path::Path;

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
}