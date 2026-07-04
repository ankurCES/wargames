# LEARNINGS — harvested from the JS implementation

> Insights carried forward into the Rust rewrite. Anything not here is
> archived in `old_data/` and is no longer authoritative.

## Rules / mechanics

- **Turn order**: Soviet (LLM-driven) acts first, US (player) reacts. This
  pacing matters — it gives the LLM initiative without handing it control.
  Source: `src/game/turn-engine.js`.
- **Action matrix**: 8 actions × 2 sides, each with effects on posture,
  escalation_budget, defcon (sometimes), and detection_pct. Mapped to a
  table-driven evaluator in Rust.
- **Defection rule**: if either side's escalation_budget hits 0 *while*
  posture is `hardened`, the game enters `DEFECT` (loss of command control).
  This is one of three terminal states (`STRIKE` / `DISARM` / `DEFECT`).
- **Detection drift**: detection rises when both sides are aggressive, falls
  when both are quiet. Source: `src/game/state.js` `applyAction`.
- **Resource decay**: every turn both sides lose 1 from escalation_budget.

## Engine

- **State is cloned per turn** (`structuredClone`) — pure transitions, no
  shared mutable state. Same in Rust: `WorldState` is `Clone + Copy`-friendly
  via `#[derive(Clone)]`.
- **State for LLM is filtered**: `stateForLLM()` strips secrets. Same shape
  goes into the Rust LLM client.
- **Replay**: every turn is recorded as `{side, action, beforeState,
  afterState, sitrep}`. Replay is the durable ground truth.

## Networking / resilience

- **12s ceiling on every fetch.** Anywhere a network call happens
  (`commander.js`, `tts.js`, `settings-panel._testLlm`, `wopr-shell`,
  `config/loader.js`, `tools/tui.mjs`) the JS code uses `AbortSignal.timeout`.
  In Rust this is `tokio::time::timeout(Duration::from_secs(12), ...)`.
- **Env override**: `WOPR_NET_TIMEOUT_MS` is honored across all call sites.
  We keep this in Rust via the same env name.
- **Failure mode is a deterministic action, not a hang.** When the LLM
  times out, the engine still moves (a fallback action). Same contract in
  Rust: `predict.rs` returns a `Default` action on timeout.

## TUI

- **Box rendering**: right border `▌` on every interior row + a bottom border
  row, to eliminate visual bleed between adjacent panels.
  Source: `tools/tui.mjs:55-67` `box()`.
- **Cursor row**: full-row `BG(bgTitle, ...)` highlight so the selected row
  is unmistakable. Source: `tools/tui.mjs:189-249`.
- **Log overflow**: when log > pane height, draw `[N earlier omitted — log
  auto-scrolls]` hint. Same pattern in `widget_log.rs`.
- **State panel pinned to fixed rows**: in JS the state panel was pinned to
  `STATE_ROWS = 16` to prevent layout churn. We apply the same discipline
  via a `MinHeight` constraint on the state pane.
- **Action panel pinned to a deterministic row**: same approach. In Rust,
  the layout uses `Constraint::Length(...)` for the action pane.

## Data

- **Scenario JSON contract**: `id`, `title`, `briefing`, `initial_state`,
  `initial_detection_pct`, `faction`, `era`, `us`, `soviet`, `map`,
  `opening_message`, `win_conditions`. We load this shape verbatim with
  `serde_json`.
- **Live feeds**: GDELT, OpenSky, satellite tracks. TTY-mode doesn't fetch;
  a `--live` flag opts into the network with the same 12s ceiling.

## What we explicitly drop

- Browser surface (Leaflet, WOPR shell, settings panel). Out of scope for the
  Rust rewrite; archived under `old_data/`.
- The 19 ava specs not related to scenario shape — replaced by Rust `cargo test`
  in the new test cadence. See `docs/THIS_PLAN.md` §7.
- Procedural scenario generator (GDELT-driven) — reimplemented in Rust as
  `wargames-core::scenario::generator` (deferred to a follow-up release).

## Conventions we adopt from herdr

- ratatui 0.30 + crossterm 0.29 + clap 4 (same versions, same idiom).
- Pane system with title bars.
- Deterministic tick loop at ~60fps.
- Single binary, single config, single license.
- AGPL-3.0-or-later (matches herdr's license).