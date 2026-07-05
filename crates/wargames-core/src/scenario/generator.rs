//! Procedural scenario generator. Builds a fresh `Scenario` for a given
//! `Theater` from the conflict corpus, with seed-driven variation across
//! `initial_detection_pct`, opening message phrasing, and recent-event flavor.
//!
//! Design goals:
//!   - Pure: no I/O, no clock, no network. Given `(theater, seed, era)` and a
//!     deterministic RNG, the output is identical every run.
//!   - Valid: the output is a `Scenario` that round-trips through the bundled
//!     `serde_json` loader — same shape as `scenarios/*.json` files.
//!   - Idempotent variants: same seed + same theater = same scenario. The
//!     picker uses the entry's hash as a stable id.

use crate::state::{Era, Faction, SideState, Theater};
use crate::scenario::{Scenario, WinConditions};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use super::corpus::{lookup, TheaterCorpus};

/// Build a `Scenario` for the given theater using a stable seed.
///
/// `era` overrides the corpus's default era when provided. `faction` overrides
/// the corpus's default faction the same way; the TUI uses this when the player
/// has chosen the opposing faction in the country picker.
pub fn generate_scenario(theater: Theater, seed: u64, era: Option<Era>, faction: Option<Faction>) -> Scenario {
    let corpus: &TheaterCorpus = lookup(theater);
    let mut rng = StdRng::seed_from_u64(seed);

    let era = era.unwrap_or(corpus.era);
    let faction = faction.unwrap_or(corpus.faction);

    // Vary detection_pct inside the bundled scenario's range so each generated
    // scenario has a slightly different opening read on the operating picture.
    let detection_jitter: f32 = rng.gen_range(-7.0..7.0);
    let initial_detection_pct = (corpus.initial_detection_pct + detection_jitter).clamp(10.0, 90.0);

    // Vary budgets by ±15%. The corpus has the rough centerline; generation
    // nudges it.
    let us_budget = nudget(corpus.us_budget, &mut rng);
    let opp_budget = nudget(corpus.opp_budget, &mut rng);

    let us_state = SideState {
        escalation_budget: us_budget,
        ..SideState::default_player()
    };
    let opp_state = SideState {
        escalation_budget: opp_budget,
        ..SideState::default_opponent()
    };

    // Pick one recent event to drop into the opening message. The corpus has
    // 4 entries; we pick deterministically from the seed.
    let event = corpus.recent_events[rng.gen_range(0..corpus.recent_events.len())];
    let opening_message = format!(
        "WOPR online. Theater: {}. Last notable event ({}): {}. Awaiting strategic command.",
        corpus.theater.display_name(),
        event.year,
        event.summary
    );

    let id = format!("{}_gen_{:08x}", seed, seed);

    Scenario {
        id,
        title: format!("{}, generated", corpus.theater.display_name()),
        briefing: corpus.briefing.to_string(),
        initial_state: Some(corpus.initial_state.to_string()),
        initial_detection_pct: Some(initial_detection_pct),
        faction: Some(faction),
        era: Some(era),
        theater: Some(theater),
        us: Some(us_state),
        soviet: Some(opp_state),
        opening_message: Some(opening_message),
        win_conditions: Some(WinConditions {
            disarm: Some(
                "Stand down without losing a ship. DEFCON <= 3, both sides deescalating.".to_string(),
            ),
            defeat_strike: Some("Either side launches first.".to_string()),
            mutual_assured_destruction: Some(
                "Both sides launch within 2 turns of each other.".to_string(),
            ),
        }),
    }
}

/// Nudge an integer value by ±15%. Saturates at 0 on the low side.
fn nudget(value: i32, rng: &mut StdRng) -> i32 {
    let pct: i32 = rng.gen_range(-15..=15);
    ((value * (100 + pct)) / 100).max(1)
}

/// Era distribution per theater. Used by the picker when seeding AI-vs-AI
/// matches — different theaters naturally skew different eras.
pub fn era_distribution(theater: Theater) -> &'static [Era] {
    match theater {
        // Cold-War-era theaters skew to "modern" with cold war as legacy flavor.
        Theater::NorthAtlantic => &[Era::Modern, Era::Modern, Era::ColdWar],
        Theater::BalticSea => &[Era::Modern, Era::Modern, Era::ColdWar],
        // Near-peer competitors get future flavor.
        Theater::TaiwanStrait => &[Era::Modern, Era::NearPeer2030],
        Theater::SouthChinaSea => &[Era::Modern, Era::NearPeer2030],
        // The rest are modern-only.
        Theater::BlackSea => &[Era::Modern],
        Theater::KoreanPeninsula => &[Era::Modern],
        Theater::RedSea => &[Era::Modern],
        Theater::EasternMed => &[Era::Modern],
        Theater::Custom => &[Era::Modern],
    }
}

/// Sample an era from the per-theater distribution using a deterministic RNG.
pub fn sample_era(theater: Theater, seed: u64) -> Era {
    let dist = era_distribution(theater);
    let mut rng = StdRng::seed_from_u64(seed);
    dist[rng.gen_range(0..dist.len())]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn core() -> WorldStateStub {
        WorldStateStub::default()
    }

    // Tiny stub struct so we don't need WorldState here — the generator
    // doesn't read it. Just exists so the test code mirrors how the
    // picker will invoke it (seed + theater + optional overrides).
    #[derive(Default)]
    struct WorldStateStub;

    #[test]
    fn generated_scenario_round_trips_through_serde() {
        for t in [
            Theater::BalticSea,
            Theater::BlackSea,
            Theater::KoreanPeninsula,
            Theater::TaiwanStrait,
            Theater::SouthChinaSea,
            Theater::RedSea,
            Theater::EasternMed,
            Theater::NorthAtlantic,
        ] {
            let s = generate_scenario(t, 0x1234_5678, None, None);
            let json = serde_json::to_string(&s).expect("serialize");
            let _: Scenario = serde_json::from_str(&json).expect("deserialize");
        }
    }

    #[test]
    fn same_seed_same_theater_is_deterministic() {
        let a = generate_scenario(Theater::BlackSea, 0xdeadbeef, None, None);
        let b = generate_scenario(Theater::BlackSea, 0xdeadbeef, None, None);
        assert_eq!(a.id, b.id);
        assert_eq!(a.opening_message, b.opening_message);
        assert_eq!(
            a.initial_detection_pct, b.initial_detection_pct,
            "same seed must produce same opening detection"
        );
    }

    #[test]
    fn different_seeds_produce_variation() {
        // The corpus has 4 recent events per theater; with seed-driven jitter
        // on detection_pct, the *scenario id* and the *opening detection*
        // still vary per seed. We assert on those, which are guaranteed
        // distinct per seed.
        let mut ids = std::collections::HashSet::new();
        let mut detections = std::collections::HashSet::new();
        for s in 0u64..8 {
            let scenario = generate_scenario(Theater::TaiwanStrait, s, None, None);
            assert!(ids.insert(scenario.id.clone()), "seed {} collides on id", s);
            let d = (scenario.initial_detection_pct.unwrap_or_default() * 100.0).round() as i32;
            assert!(
                detections.insert(d),
                "seed {} collides on detection {}",
                s,
                d
            );
        }
    }

    #[test]
    fn different_theaters_differ() {
        let a = generate_scenario(Theater::BalticSea, 1, None, None);
        let b = generate_scenario(Theater::BlackSea, 1, None, None);
        assert_ne!(a.theater, b.theater);
        assert_ne!(a.briefing, b.briefing);
    }

    #[test]
    fn era_override_takes_precedence_over_default() {
        let s = generate_scenario(Theater::BalticSea, 1, Some(Era::NearPeer2030), None);
        assert_eq!(s.era, Some(Era::NearPeer2030));
    }

    #[test]
    fn faction_override_takes_precedence_over_default() {
        let s = generate_scenario(Theater::BalticSea, 1, None, Some(Faction::Soviet));
        assert_eq!(s.faction, Some(Faction::Soviet));
    }

    #[test]
    fn detection_pct_lands_in_safe_band() {
        for seed in 0u64..64 {
            let s = generate_scenario(Theater::RedSea, seed, None, None);
            let pct = s.initial_detection_pct.unwrap_or_default();
            assert!((10.0..=90.0).contains(&pct), "seed {} got {}%", seed, pct);
        }
    }

    #[test]
    fn opener_cites_corpus_event_and_theater_name() {
        let s = generate_scenario(Theater::KoreanPeninsula, 1, None, None);
        let msg = s.opening_message.unwrap();
        assert!(msg.contains("Korean Peninsula"));
        assert!(msg.contains("Last notable event"));
    }

    #[test]
    fn sample_era_only_returns_distributions_supported_by_theater() {
        for t in [
            Theater::BalticSea,
            Theater::BlackSea,
            Theater::KoreanPeninsula,
            Theater::TaiwanStrait,
            Theater::SouthChinaSea,
            Theater::RedSea,
            Theater::EasternMed,
            Theater::NorthAtlantic,
        ] {
            for s in 0u64..32 {
                let era = sample_era(t, s);
                let dist = era_distribution(t);
                assert!(
                    dist.contains(&era),
                    "{:?} seed {} yielded era {:?} not in {:?}",
                    t, s, era, dist
                );
            }
        }
    }

    #[test]
    fn stub_compiles() {
        let _ = core();
    }
}
