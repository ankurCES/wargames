//! Theater conflict corpus — short factual briefings of real-world inflection
//! points from approximately the last 10 years.
//!
//! Intentionally deterministic: no HTTP, no live feeds, no third-party data.
//! This is the offline seed surface for `generate_scenario`. Live feeds
//! (GDELT, OpenSky, etc.) remain gated behind the `--live` flag in the TUI;
//! wiring the corpus into a network-backed feed is a separate, follow-up
//! feature.
//!
//! Each theater has:
//!   - a one-paragraph `briefing` written for in-game flavor,
//!   - an `era` default (caller may override),
//!   - a default `faction` (who controls the picker in the bundled scenario;
//!     reversed by the player when they pick the opposing faction),
//!   - a default `initial_state` / `initial_detection_pct`,
//!   - side escalations budgets from public reporting as ballpark figures —
//!     not live, not real-time, just "what does starting posture look like",
//!   - a list of `recent_events` with year + 1-sentence summary, used for
//!     seed-driven variation.
//!
//! Sourcing: paraphrased from publicly reported events. Years only. Avoids
//! quoting specific figures (no "3.7 billion dollars" or "seventeen ships" —
//! the corpus is flavor text, not a fact database).

use crate::state::{Era, Faction, Theater};

#[derive(Debug, Clone, Copy)]
pub struct RecentEvent {
    pub year: u16,
    pub summary: &'static str,
}

#[derive(Debug, Clone)]
pub struct TheaterCorpus {
    pub theater: Theater,
    pub briefing: &'static str,
    pub era: Era,
    pub faction: Faction,
    pub initial_state: &'static str,
    pub initial_detection_pct: f32,
    pub us_budget: i32,
    pub opp_budget: i32,
    pub gdelt_query: &'static str,
    pub recent_events: &'static [RecentEvent],
}

/// Look up the corpus entry for a theater. Falls back to `Custom` if no
/// theater-specific entry exists (which should never happen — all 8 bundled
/// theaters are covered).
pub fn lookup(theater: Theater) -> &'static TheaterCorpus {
    match theater {
        Theater::BalticSea => &BALTIC_SEA,
        Theater::BlackSea => &BLACK_SEA,
        Theater::KoreanPeninsula => &KOREAN_PENINSULA,
        Theater::TaiwanStrait => &TAIWAN_STRAIT,
        Theater::SouthChinaSea => &SOUTH_CHINA_SEA,
        Theater::RedSea => &RED_SEA,
        Theater::EasternMed => &EASTERN_MED,
        Theater::NorthAtlantic => &NORTH_ATLANTIC,
        Theater::Custom => &NORTH_ATLANTIC, // fallback path; should be unreachable from picker
    }
}

const BALTIC_SEA: TheaterCorpus = TheaterCorpus {
    theater: Theater::BalticSea,
    briefing: "DEFCON 4. Persistent NATO air policing over the Baltic republics has been the baseline posture since 2014, with periodic scrambles into Russian airspace and tighter EEZ surveillance around the Suwalki gap and the Danish straits. The energy footprint of the Baltic — LNG terminals, subsea cables, the Nord Stream scars — keeps maritime incidents politically consequential. A miscalc on either side could draw every NATO capital in.",
    era: Era::Modern,
    faction: Faction::Nato,
    initial_state: "DEFCON_4",
    initial_detection_pct: 35.0,
    us_budget: 50,
    opp_budget: 45,
    gdelt_query: "\"Baltic Sea\" OR Suwalki OR Estonia OR Latvia OR Lithuania OR \"air policing\"",
    recent_events: &[
        RecentEvent { year: 2014, summary: "Wales Summit commits NATO persistent air policing; Baltic airspace vigil formalized." },
        RecentEvent { year: 2022, summary: "Nord Stream pipeline sabotage in the Danish strait — unresolved attribution." },
        RecentEvent { year: 2023, summary: "Estonia declares a naval zone violation; Latvia reports a coastal shadow." },
        RecentEvent { year: 2024, summary: "NATO BALTOPS exercises rehearse amphibious entry across the Baltic." },
    ],
};

const BLACK_SEA: TheaterCorpus = TheaterCorpus {
    theater: Theater::BlackSea,
    briefing: "DEFCON 3. Sevastopol-based naval assets continue to make high-tempo sorties; NATO air policing runs out of Romania and Bulgaria track activity. Grain-corridor logistics, the Kerch bridge, and the Bosporus chokepoint all sit inside the same operating picture. A mishap over the Bosporus or an accidental intercept in the western Black Sea would pull every capital in.",
    era: Era::Modern,
    faction: Faction::Nato,
    initial_state: "DEFCON_3",
    initial_detection_pct: 45.0,
    us_budget: 55,
    opp_budget: 50,
    gdelt_query: "\"Black Sea\" OR Sevastopol OR Romania OR Bulgaria navy OR \"grain corridor\"",
    recent_events: &[
        RecentEvent { year: 2014, summary: "Crimea annexed; Sevastopol naval basing reaffirmed." },
        RecentEvent { year: 2018, summary: "Kerch bridge opens; tightening choke around the Azov." },
        RecentEvent { year: 2022, summary: "Full-scale invasion; flagship of the Black Sea Fleet lost." },
        RecentEvent { year: 2023, summary: "Grain corridor diplomacy keeps the Bosporus in the loop." },
    ],
};

const KOREAN_PENINSULA: TheaterCorpus = TheaterCorpus {
    theater: Theater::KoreanPeninsula,
    briefing: "DEFCON 3. The DPRK's ICBM tempo has continued to climb since the 2017 Hwasong tests, with longer-range tests and shorter cadence in recent cycles. ROK and US combined exercises around K-INMP and the Marine Corps' Korean Deployment Program keep alliance readiness in the public eye. A further long-range test against an unexpected trajectory would shorten decision windows across the region.",
    era: Era::Modern,
    faction: Faction::Us,
    initial_state: "DEFCON_3",
    initial_detection_pct: 40.0,
    us_budget: 60,
    opp_budget: 35,
    gdelt_query: "\"North Korea\" OR DPRK OR Hwasong OR \"Korean Deployment Program\" OR KINMP",
    recent_events: &[
        RecentEvent { year: 2017, summary: "Hwasong-15 ICBM test; peninsula creeps to DEFCON 2 headlines." },
        RecentEvent { year: 2018, summary: "Inter-Korean diplomacy; summits cool the visible tempo." },
        RecentEvent { year: 2022, summary: "ICBM cadence returns after the pandemic lull." },
        RecentEvent { year: 2024, summary: "Solid-fuel ICBM test follows a year of unprecedented tempo." },
    ],
};

const TAIWAN_STRAIT: TheaterCorpus = TheaterCorpus {
    theater: Theater::TaiwanStrait,
    briefing: "DEFCON 3. PLAAF ADIZ incursions around Taiwan have continued at an elevated cadence, with longer-duration tracks and more aircraft per incursion. US and allied naval transits through the strait and around the Bashi Channel are now a routine — and visibly signaled — posture. A miscalculation around an election cycle or an unattributable subsea cable cut would trigger rapid escalation.",
    era: Era::Modern,
    faction: Faction::Us,
    initial_state: "DEFCON_3",
    initial_detection_pct: 50.0,
    us_budget: 55,
    opp_budget: 45,
    gdelt_query: "\"Taiwan Strait\" OR ADIZ OR PLAAF OR Taipei OR Bashi OR Kinmen",
    recent_events: &[
        RecentEvent { year: 2022, summary: "ADIZ incursions surge after the August exercises." },
        RecentEvent { year: 2023, summary: "Subsea cable cuts in the Luzon Strait; unresolved attribution." },
        RecentEvent { year: 2024, summary: "Election cycle; visible cross-strait signaling activity." },
        RecentEvent { year: 2025, summary: "Continued ADIZ tempo; combined-arms exercises around the strait." },
    ],
};

const SOUTH_CHINA_SEA: TheaterCorpus = TheaterCorpus {
    theater: Theater::SouthChinaSea,
    briefing: "DEFCON 4. The 2016 PCA ruling is now a fixture in the doctrinal backdrop but not in the daily moves on the water. Continued reefing reports, militia incidents around Second Thomas Shoal, and EEZ shadowing run alongside allied FONOPS and rotation deployments. A kinetic incident at Second Thomas or a third-party casualty in a militia encounter would escalate fast.",
    era: Era::Modern,
    faction: Faction::Nato,
    initial_state: "DEFCON_4",
    initial_detection_pct: 35.0,
    us_budget: 55,
    opp_budget: 50,
    gdelt_query: "\"South China Sea\" OR \"Second Thomas\" OR Scarborough OR FONOPS OR militia",
    recent_events: &[
        RecentEvent { year: 2016, summary: "PCA ruling rejects expansive historical claims in the SCS." },
        RecentEvent { year: 2020, summary: "Periodic militia surges around Whitsun Reef escalate." },
        RecentEvent { year: 2023, summary: "Second Thomas Shoal barrier incidents." },
        RecentEvent { year: 2024, summary: "Allied FONOPS cadence increases under AUKUS shadow." },
    ],
};

const RED_SEA: TheaterCorpus = TheaterCorpus {
    theater: Theater::RedSea,
    briefing: "DEFCON 3. Houthi attacks on commercial shipping through the Bab el-Mandeb have ranged from missiles to hijackings, with coalition strikes into Houthi-controlled territory as the response. Re-routing via the Cape of Good Hope has become the default for major shipping interests; the coalition task force and a widening list of national responders keep the naval balance tight. A carrier-level incident or a missile hit on a major commercial vessel would reset the picture overnight.",
    era: Era::Modern,
    faction: Faction::Nato,
    initial_state: "DEFCON_3",
    initial_detection_pct: 50.0,
    us_budget: 50,
    opp_budget: 40,
    gdelt_query: "\"Red Sea\" OR \"Bab el-Mandeb\" OR Houthi OR \"Operation Prosperity Guardian\" OR shipping",
    recent_events: &[
        RecentEvent { year: 2023, summary: "Houthi seizures of the Galaxy Leader kick off the campaign." },
        RecentEvent { year: 2024, summary: "Combined task force forms; coalition strike cycle begins." },
        RecentEvent { year: 2024, summary: "Sea Champion and other commercial casualties." },
        RecentEvent { year: 2025, summary: "Cadence stabilizes; coalition expands beyond initial members." },
    ],
};

const EASTERN_MED: TheaterCorpus = TheaterCorpus {
    theater: Theater::EasternMed,
    briefing: "DEFCON 3. The 2020 Turkey–Greece EEZ standoff and the 2023 Israel–Gaza war have both written new rules of engagement into the regional picture. Cypriot EEZ disputes, Lebanese platform negotiations, and the Egypt–Greece–Cyprus power grid sit alongside ongoing NATO and EU force posture in the eastern Mediterranean. A second Lebanon front or a direct Turkey–Greece hot incident would draw NATO's southern flank in fast.",
    era: Era::Modern,
    faction: Faction::Nato,
    initial_state: "DEFCON_3",
    initial_detection_pct: 40.0,
    us_budget: 50,
    opp_budget: 40,
    gdelt_query: "\"Eastern Mediterranean\" OR Turkey Greece EEZ OR Cyprus OR Lebanon OR Gaza",
    recent_events: &[
        RecentEvent { year: 2020, summary: "Turkey–Greece EEZ standoff; Oruc Reis goes to sea." },
        RecentEvent { year: 2022, summary: "Egypt–Greece–Cyprus power grid MOU cements a trilateral posture." },
        RecentEvent { year: 2023, summary: "Israel–Gaza war opens a second-front risk in Lebanon." },
        RecentEvent { year: 2024, summary: "Cyprus examines its own posture as the Gaza spillover settles." },
    ],
};

const NORTH_ATLANTIC: TheaterCorpus = TheaterCorpus {
    theater: Theater::NorthAtlantic,
    briefing: "DEFCON 3. NATO's northern flank has come back into focus with a string of subsea incidents, persistent Russian naval activity around the Greenland–UK gap, and high-end exercises like Steadfast Defender. The High North and Arctic access routes sit squarely in the operating picture. A cable cut mapped to a specific platform or an unintended intercept north of the GIUK line would force immediate NATO consultations.",
    era: Era::Modern,
    faction: Faction::Nato,
    initial_state: "DEFCON_3",
    initial_detection_pct: 45.0,
    us_budget: 60,
    opp_budget: 50,
    gdelt_query: "\"North Atlantic\" OR GIUK OR HighNorth OR \"subsea cable\" OR \"Steadfast Defender\"",
    recent_events: &[
        RecentEvent { year: 2018, summary: "Russian navy returns to the North Atlantic in numbers unseen since 1991." },
        RecentEvent { year: 2022, summary: "Nord Stream cable cuts; the High North becomes a contested domain." },
        RecentEvent { year: 2024, summary: "Steadfast Defender rehearses the largest NATO posture in decades." },
        RecentEvent { year: 2025, summary: "Continued cable incidents; Greenland becomes a strategic-level topic." },
    ],
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_resolves_all_bundled_theaters() {
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
            assert_eq!(lookup(t).theater, t, "theater {:?} missing", t);
        }
    }

    #[test]
    fn every_corpus_has_at_least_three_recent_events() {
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
            assert!(
                lookup(t).recent_events.len() >= 3,
                "{:?} has fewer than 3 recent events",
                t
            );
        }
    }

    #[test]
    fn briefings_are_nonempty() {
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
            assert!(
                !lookup(t).briefing.trim().is_empty(),
                "{:?} briefing is empty",
                t
            );
        }
    }
}
