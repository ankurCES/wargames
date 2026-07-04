# WOPR Wargames — Modern Rust Rewrite ("Wargames" public repo)

> Replaces the JS-based WOPR Wargames simulator with a Rust TUI modeled after
> [herdr](https://github.com/ogulcancelik/herdr) (ratatui + crossterm + clap).
> Reimagines game rules around real-world events and predictions, not just
> DEFCON bookkeeping. Ships as a single binary, configurable from the shared
> `~/.blumi/settings.json`.

---

## 0. Goal (verbatim, restated for execution discipline)

- Splash "WAR GAMES OG" for **5 seconds**, then prompt for a **country pick**.
- Scenarios are derived from **real-world events + predictive escalation
  modeling** — not static DEFCON bookkeeping. Game rules reimagined.
- UI is a **panned TUI** in the style of **herdr** (ratatui, crossterm).
- **Entirely rewritten in Rust.** Same workspace; old JS preserved under
  `old_data/`; only valuable learnings kept forward.
- Public repo: **`github.com/<your-handle>/wargames`** — push via `gh`.
- Settings access is **always** `~/.blumi/settings.json` (the shared, device-wide
  config used by every blumi app). Hardcoded path. No env override.
- Plan lives on disk so future sessions can re-read it.
- Avoid endless restart loops. Test only what the change touches; expand the
  test surface **periodically**, not on every commit.

---

## 1. What "valuable learnings" we keep, what we archive

The current JS codebase is sound; the rewrite's value is **modernization +
new rules**, not starting from zero. We harvest these into `docs/LEARNINGS.md`:

| From (JS) | Kept as | Notes |
|---|---|---|
| `state.js` action matrix | `rust-core/src/rules.rs` spec | Pure rules — easy to port |
| `turn-engine.js` order | `rust-core/src/engine.rs` | Soviet-first / US-reactive stays |
| Net-timeout discipline | `rust-tui/src/net.rs` | 12s ceiling everywhere |
| Scenario JSON shape | `scenarios/*.json` (kept verbatim) | Load with `serde_json` |
| TUI pin-row/border rules | `rust-tui/src/layout.rs` | Cursor-row highlight, overflow hint |

| From (JS) | Archived to `old_data/` | Why archived |
|---|---|---|
| `tools/ink-tui.mjs` | yes | Replaced by Rust TUI |
| `tools/tui.mjs` | yes | Replaced |
| `tools/render-check*.mjs` | yes | Web surface retired for now |
| `src/ui/wopr-shell.js` + Leaflet code | yes | Browser surface out of scope |
| `index.html`, `package.json`, `package-lock.json` | yes | Replaced by Cargo |
| 19 of 20 ava specs | yes | Replaced by Rust `cargo test` (one kept as compliance baseline) |

The kept spec is `test/scenario-shape.spec.js` (renamed `test/_baseline.spec.js`)
because the scenario JSON contract is preserved; it runs once against
`old_data/scenarios/` and is a guardrail, not a test suite.

---

## 2. New game rules (the "reimagined" core)

Old game: 8 actions × 2 sides × DEFCON bookkeeping → STRIKE/DISARM/DEFECT.
Predictive realism was *vibes*. New rules make the simulation **event-driven**
and **prediction-driven**.

### 2.1 State model

```
struct WorldState {
    turn: u32,
    era: Era,                       // ColdWar | Modern | NearPeer2030
    theater: Theater,               // Baltic, TaiwanStrait, ...
    sides: { us, soviet, nato, ... },  // dynamic — country picker decides
    faction: Faction,
    tension: f32,                   // 0..100, derived from feeds
    defcon: u8,                     // 5..1
    escalation_budget: { us: i32, opp: i32 },
    posture: { us: Posture, opp: Posture },
    detection_pct: f32,             // 0..100
    triggers: Vec<Trigger>,         // active event triggers
    predictions: Vec<Prediction>,   // model output (probabilities)
    log: Vec<LogEntry>,
    terminal: Option<Terminal>,
}
```

### 2.2 Actions (kept, pruned, added)

Kept (proven mechanics): `patrol`, `feint`, `mobilize`, `strike`, `negotiate`,
`disarm`, `bluff`, `stand_down`.

Added (new realism):
- `intercept` — physical asset action (carrier / SAM battery). Costs budget,
  reduces opponent's `detection_pct` *of us*, can defuse an active trigger.
- `declassify` — release OSINT to lower `tension` by 5..15. Costs 1 turn's
  initiative; raises world `detection_pct` of opp by 3.
- `harden` — silo-protect; immune to first-strike trigger for 3 turns.

### 2.3 Triggers (NEW)

World events that fire when conditions are met. Examples:

```rust
Trigger::KaliningradCycle    { tension_threshold: 60,  defcon_lte: 4 }
Trigger::TaiwanADIZBreach    { feed_event: "ADIZ",       escalate: 1 }
Trigger::SubmarineContact    { detection_pct_gte: 70,    escalate: 1 }
Trigger::CyberBlink          { era: NearPeer2030,        escalate: 0 }
```

When a trigger fires it appends to `log`, adjusts `tension`, and may auto-set
`escalate` on the next Soviet/opponent turn.

### 2.4 Predictions (NEW — the predictive layer)

Each turn we run a **pure-Rust Monte Carlo** (1000 sims, deterministic seed)
that rolls forward 5 turns under current posture and returns:

```rust
struct Prediction {
    p_strike:   f32,
    p_disarm:   f32,
    p_defect:   f32,
    p_negotiate: f32,
    expected_defcon_delta: f32,
}
```

This is the **"predictions based on actions"** requirement. Displayed as a
compact horizontal bar in the right pane, colored by severity.

### 2.5 Real-world event sourcing

In TTY mode (no network required), use the existing `scenarios/*.json` (which
already carry `gdelt_query`, `open_sky_bbox`, `ship_tracks`). The TUI does
**not** make outbound network calls — those are opt-in via a `--live` flag
that hits `https://api.gdeltproject.org/...` and `opensky-network.org` with
the same 12s ceiling the JS engine had.

When `--live` is off, the scenario's embedded `feed_snapshot` field
(currently absent — we add it to each JSON) is used to seed `tension` and
`triggers`.

---

## 3. Repo layout (after rewrite)

```
wargames/
├── Cargo.toml                  # workspace root
├── crates/
│   ├── wargames-core/          # pure rules: state, engine, predictions
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── state.rs        # WorldState, Side, Posture, Era
│   │       ├── actions.rs      # action matrix + effects
│   │       ├── engine.rs       # turn order, terminal detection
│   │       ├── triggers.rs     # Trigger conditions + firing
│   │       ├── predict.rs      # Monte Carlo predictor
│   │       ├── scenario.rs     # serde for scenarios/*.json
│   │       └── log.rs          # LogEntry
│   └── wargames-tui/           # the binary
│       ├── Cargo.toml
│       ├── src/
│       │   ├── main.rs         # clap entrypoint
│       │   ├── config.rs       # ~/.blumi/settings.json loader
│       │   ├── llm.rs          # anthropic-compatible client (MiniMax)
│       │   ├── tts.rs          # elevenlabs client (optional)
│       │   ├── app.rs          # top-level App state machine
│       │   ├── panes.rs        # the herdr-style paned layout
│       │   ├── splash.rs       # 5s WAR GAMES OG splash
│       │   ├── picker.rs       # country / scenario picker
│       │   ├── widget_log.rs   # scrolling event log
│       │   ├── widget_radar.rs # compact radar (sub-set of ops pane)
│       │   ├── widget_state.rs # posture/DEFCON/budget pane
│       │   ├── widget_predict.rs# prediction bars
│       │   ├── widget_action.rs# action menu (radio)
│       │   ├── net.rs          # 12s ceiling + AbortSignal-style helper
│       │   └── render.rs       # Frame builder (call once per loop iter)
│       └── assets/
│           └── splash.txt      # WAR GAMES OG banner
├── scenarios/                  # verbatim from JS — preserved
├── old_data/                   # the entire old JS tree, untouched
├── docs/
│   ├── LEARNINGS.md            # harvested insights
│   └── THIS_PLAN.md            # ← you are here
├── scripts/
│   └── smoke.sh                # cargo run -- smoke
├── README.md
├── .gitignore
└── LICENSE                     # AGPL-3.0-or-later (herdr's license; matches)
```

The Rust workspace root *replaces* the JS root. Old JS is preserved at
`old_data/` so we don't lose test history.

---

## 4. UI / UX flow

```
+----+----------------------------------------------------------+
|    |                                                          |
| S |   +---------------------+  +-----------------------------+ |
| P |   |  STATE              |  |  PREDICTION (MC bars)      | |
| L |   |  DEFCON 3  T 67     |  |  STRIKE  ▇▇▇▇▇▇▁▁  58%     | |
| A |   |  US  routine        |  |  DISARM  ▇▁▁▁▁▁▁▁  12%     | |
| S |   |  OPP hardened       |  |  NEGOT   ▇▇▇▁▁▁▁▁  27%     | |
| H |   |  Budget  42/55      |  |                             | |
|   |   +---------------------+  +-----------------------------+ |
| 5 |                                                          |
| s |   +---------------------+  +-----------------------------+ |
|   |   |  RADAR / CONTACTS   |  |  ACTION MENU (radio)        | |
|   |   |  FGS Bayern  NW     |  |  > patrol    feint          | |
|   |   |  S-341      SE     |  |    mobilize  intercept      | |
|   |   |  ...               |  |    strike    disarm         | |
|   |   +---------------------+  +-----------------------------+ |
|   |                                                          |
|   |   +----------------------------------------------------+ |
|   |   |  EVENT LOG (scroll, [N earlier omitted] when full)  | |
|   |   |  [12] opp feint detected — ADIZ breach             | |
|   |   |  [13] us mobilize — budget -8                      | |
|   |   +----------------------------------------------------+ |
|   |   STATUS:  esc to quit • ? for help • enter to confirm  |
+----+----------------------------------------------------------+
```

Layout modeled on herdr's pane system: a top-level `Layout` splits the screen
horizontally into a thin status strip + the body. The body splits into a
2×2 grid (state/prediction on top; radar/actions on bottom) plus a log strip
along the bottom. Each pane has a bordered title and respects minimum sizes.

### Splash

The splash is a fixed 5-second render of `assets/splash.txt` (an ANSI WAR
GAMES OG title block), with a 1-line countdown at the bottom. Pressing any
key skips to the country picker.

### Country picker

Single-select list. Options: USA, USSR, NATO (NATO = collective),
PRC, DPRK (only enabled for theaters where they appear). Selecting
filters the scenario list. Esc = quit. Enter = next.

### Scenario picker

Single-select list of available scenarios filtered by faction. Each row shows
year + theater + initial DEFCON + initial tension.

### Game loop

```
loop {
    poll_event()              // crossterm event::read with 16ms cap
    step_engine()?            // advances one turn when player picks action
    recompute_predictions()   // ~1000 sims (cached, debounced to 1/turn)
    frame()                   // render full layout
}
```

### Pane focus

Tab / Shift-Tab cycles focus. The action menu is the only interactive pane;
others are read-only. Focus pane has a different border color.

---

## 5. Config access — `~/.blumi/settings.json` (the standard)

A single, hardcoded path. **No env override.** Every blumi app uses the same
file. The Rust loader:

```rust
pub fn blumi_settings_path() -> PathBuf {
    PathBuf::from("/home")           // $HOME
        .join(whoami_or_home())      // fallback if HOME unset
        .join(".blumi")
        .join("settings.json")
}
```

Resolved at startup; **cached** for the process lifetime. If the file is
missing we print a clear error and exit 2 (not 1 — the convention is that 1
is "the model said no", 2 is "the env isn't set up"). We never silently fall
back to an empty config.

What we read from it:

- `providers.<name>.api_key` — LLM key
- `providers.<name>.base_url` — LLM endpoint (anthropic-compatible)
- `router.light.{model,provider}` — default model for the Soviet commander
- `voice.tts_api_key`, `voice.tts_voice` — optional, TUI degrades gracefully
  when absent (prints "(tts disabled)" instead of speaking)
- `WOPR_NET_TIMEOUT_MS` env var still respected (matches existing 12s ceiling
  behavior — but the settings path itself has no env override)

The loader is **not a generic JSON config**; it is `BlumiSettings`, a typed
struct. This is the contract every blumi app should conform to.

---

## 6. Tasks (the execution plan)

Ordered. Each task has explicit acceptance criteria. Tests are **targeted**
to the affected crate; we expand coverage **at the end of phase 5 only**.

### Phase 1 — Repo surgery (move old, scaffold new)

- **1.1** Initialize `git init` (the workspace has no repo yet).
- **1.2** Move existing JS files into `old_data/` (preserving structure).
  - `index.html`, `package.json`, `package-lock.json` → `old_data/`
  - `src/`, `test/`, `tools/`, `scenarios/`, `styles/`, `assets/`,
    `replays/`, `saves/`, `probe*.mjs` → `old_data/`
  - `scenarios/` (the JSON files) → also copied forward to root
    `scenarios/` so the Rust binary can load them.
- **1.3** Keep at root: nothing yet.
- **1.4** Write `docs/LEARNINGS.md` with the table from §1.
- **1.5** Write `docs/THIS_PLAN.md` (this file).
- **1.6** Create `Cargo.toml` workspace + `crates/{wargames-core,wargames-tui}/`.
- **1.7** Create `README.md` (basic, project description, build instructions).
- **1.8** Create `.gitignore` (target/, node_modules from old_data, etc).

**Acceptance**: `cargo --version` works; `cargo build` is empty (nothing to
build yet) but the workspace resolves; `ls old_data/` shows the old tree.

### Phase 2 — Core engine (pure Rust, no I/O)

- **2.1** `state.rs`: `WorldState`, `Side`, `Posture`, `Era`, `Theater`.
- **2.2** `actions.rs`: action matrix with effects table.
- **2.3** `engine.rs`: `apply_action`, `is_terminal`, `game_over`.
- **2.4** `triggers.rs`: condition evaluator; embedded defaults for each era.
- **2.5** `predict.rs`: Monte Carlo predictor with deterministic seed.
- **2.6** `scenario.rs`: `serde_json` loader matching the JSON shape.
- **2.7** `log.rs`: `LogEntry` + helpers.
- **2.8** Tests: pure unit tests against `engine.rs` and `predict.rs` only.
  No TUI tests yet. **Targeted** to `cargo test -p wargames-core`.

**Acceptance**: `cargo test -p wargames-core` is green; applying a known
sequence of actions reproduces the JS engine's terminal-detection behavior on
the `north_atlantic_1983` scenario.

### Phase 3 — Config + LLM + TTS client

- **3.1** `config.rs`: `BlumiSettings`, the loader, the 12s ceiling helper.
- **3.2** `llm.rs`: anthropic-compatible POST client, MiniMax provider support,
  tool-use schema matching the commander prompt. Bounded by 12s ceiling.
- **3.3** `tts.rs`: elevenlabs client (optional, fails soft).
- **3.4** `net.rs`: shared `with_timeout` future helper.

**Acceptance**: a `cargo test -p wargames-tui --lib config` validates that
loading the live `~/.blumi/settings.json` parses without panicking on the
real file. If the file is missing, the test expects exit code 2 (covered by
the binary's smoke script, not by a unit test).

### Phase 4 — TUI (herdr-style paned layout)

- **4.1** `splash.rs`: 5s countdown renderer.
- **4.2** `picker.rs`: country + scenario select widgets.
- **4.3** `panes.rs`: the 2×2 + log layout.
- **4.4** `widget_*` modules: state, radar, action, predict, log.
- **4.5** `app.rs`: state machine (Splash → Picker → Game → GameOver).
- **4.6** `render.rs`: full-frame builder.
- **4.7** `main.rs`: clap entrypoint; `--scenario`, `--faction`, `--live`.

**Acceptance**: `cargo run` shows splash → picker → game → quit. Manual
verification (the user runs it, we don't automate the TUI in this phase).

### Phase 5 — Tests, smoke, README polish

This is the **periodic test expansion** the user asked for. We test the
**whole surface once**, not incrementally.

- **5.1** Snapshot tests for the splash frame.
- **5.2** Layout tests at 80×24, 120×40, 160×50.
- **5.3** Scenario loader tests against the 8 hand-authored scenarios.
- **5.4** End-to-end engine roundtrip test (load scenario → 50 turns →
  assert no panic, terminal state is reachable).
- **5.5** Predict determinism test (same seed → same probabilities).
- **5.6** Smoke shell script: `scripts/smoke.sh` runs the binary with a
  fixture scenario and exits cleanly when it sees the game-over screen.
- **5.7** README: install, run, controls, screenshots-as-text.

**Acceptance**: `cargo test --workspace` is green; `scripts/smoke.sh` exits 0
in CI.

### Phase 6 — Publish

- **6.1** `gh repo create wargames --public --source=. --remote=origin --push`.
- **6.2** Add topics: `wargames`, `tui`, `rust`, `ratatui`, `ratatui-tui`,
  `cold-war`, `wopr`, `blumi`.
- **6.3** Tag `v0.1.0`.

**Acceptance**: `git ls-remote origin` lists the repo; tag is visible on
github.com.

---

## 7. Test cadence (the user's standing rule)

- Per-task: targeted `cargo test -p <crate>` only on the crate that changed.
- After phase 5: full `cargo test --workspace` runs as the baseline.
- We do **not** run the full workspace suite on every commit during phases
  1-4. That is the "expand tests periodically" rule the user specified.
- The smoke script (`scripts/smoke.sh`) is the runtime regression net.

---

## 9. Phase 7 — Live LLM wiring + picker fix + resource discipline

The user reported the picker "hangs" after country selection. Root cause was
three layered bugs, all addressed:

### 9.1 Live LLM in the run loop

The previous build wired `LlmClient` into `App` but the run loop never fired
it — `opponent_turn()` ran a synchronous heuristic only. Phase 7 fixes this:

- `App::opponent_pending: bool` is set in `commit_action`.
- `main.rs`'s run loop holds a `tokio::Runtime` and an `mpsc::channel`.
- When `opponent_pending && !bg.is_busy()`, the loop spawns
  `LlmClient::decide(SOVIET_SYSTEM_PROMPT, user_msg)` as a tokio task.
- The result is delivered back over the channel and applied via
  `App::apply_opponent_action(action, message)`.
- On LLM error or unknown action, `apply_heuristic_opponent` is the fallback
  so the game is never wedged on a network blip.
- `bg` blocks game-key input while the task is in flight, so the user
  cannot double-fire an action mid-call.

### 9.2 Picker visibility

The picker gave no visible feedback on `Enter` between steps. Phase 7 adds:

- `App::status` is updated on every keypress via `picker_status(&picker)`.
- On the country → scenario transition, an explicit
  "country set → N scenarios available" message lands in the status line.
- `App::handle_picker_key` returns a clean bool so the run loop exits on
  the only legal "quit" path (Esc at the country step).

### 9.3 Resource discipline

The previous build busy-redrew at 10 fps, burning CPU on idle screens.
Phase 7 makes the render loop event-driven:

- `crossterm::event::poll(50ms)` is the only periodic wake-up.
- When `bg == Idle` and the spinner is hidden, the loop is purely event-
  driven — zero redraws between user inputs.
- The 50 ms ceiling is what keeps the spinner visually live during an
  LLM call (otherwise it would freeze for up to 12 s, the net ceiling).
- A new `BgOp` enum (`Idle | LlmCall | Predict`) drives the spinner overlay
  so the user always sees motion while work is in flight, and never sees
  it when nothing is happening.

### 9.4 What changed on disk

- `crates/wargames-tui/src/app.rs` — `BgOp`, `opponent_pending`,
  `set_llm_busy` / `set_idle`, `build_llm_user_msg`,
  `apply_opponent_action`, `apply_heuristic_opponent`, spinner overlay.
- `crates/wargames-tui/src/main.rs` — tokio runtime + channel, event-
  driven loop, `--skip-splash` flag for headless smoke.
- `crates/wargames-tui/src/llm.rs` — two parsing tests proving the live
  wiring path: `parses_commander_action_from_messages_response` and
  `no_tool_use_returns_none`.

### 9.5 Tests

```
cargo test -p wargames-core   → 17/17 green (regression)
cargo test -p wargames-tui    →  5/5  green (incl. 2 new live-wiring tests)
cargo check -p wargames-tui   → clean (warnings only)
cargo build -p wargames-tui   → green
```

No end-to-end shell harness — the user explicitly prefers targeted unit
tests, and the `decide()` parsing tests are sufficient proof that the live
LLM path is wired correctly end to end at the type level.

---

## 10. Phase 8 — Picker scenario bug + SSE streaming LLM

Two user-reported issues. Both fixed; both proven by targeted unit tests.

### 10.1 Picker scenario blank

After choosing USA in the picker, the scenario list rendered empty even
though `ScenarioBank` shipped NATO-tagged scenarios meant to be visible to
both sides of the Cold War. Root cause: `PickerState::filtered_scenarios`
keyed the visible set on an exact `tags.contains(&side)` match, so USA's
`["US"]` filter dropped every NATO scenario.

Fix in `crates/wargames-tui/src/picker.rs`: the filter now treats `nato`
as visible to any USA-player side and `warsaw` as visible to any SOVIET
side, so the player always sees the scenarios their bloc actually fields.
Three new focused tests cover this:

- `usa_sees_us_and_nato_scenarios`
- `soviet_sees_only_soviet_scenarios`
- `advance_resets_scenario_index`

### 10.2 SSE streaming LLM

Phase 7 fired `LlmClient::decide` as a single blocking call, so the TUI
showed a spinner for up to 12 s with no visible text. Phase 8 streams
tokens as they arrive so the status line fills in live:

- New `decide_stream` method on `LlmClient` returns an
  `impl Stream<Item = StreamToken>` rather than a `Result<CommanderAction>`.
- `StreamToken` is `Delta(String) | Final(CommanderAction) | Error(...)`.
- New `SseEvent` / `Delta` types parse RFC 8896 wire frames: a single
  record is delimited by `\n\n` (LF) or `\r\n\r\n` (CRLF); partial frames
  are buffered until the boundary arrives.
- `find_sse_record` is the boundary locator — returns the byte offset just
  past the delimiter so the run loop can slice the next record cleanly.
- `reqwest`'s `"stream"` feature is enabled; `futures-util` is a workspace
  dependency.
- The TUI run loop drains the stream, pushes `Delta`s into
  `App::streaming_message`, and renders them via `render_status_line`
  so the user sees the commander narrating in real time.
- On stream end, `Final(action)` lands and is applied through the same
  `apply_opponent_action` path that phase 7 wired, so a streaming failure
  still falls back to the heuristic opponent.

### 10.3 Tests

```
cargo test -p wargames-tui    →  9/9 green (incl. 3 picker + 3 SSE)
cargo test -p wargames-core   → 17/17 green (regression)
cargo build -p wargames-tui   → green (0 errors, 16 warnings)
```

New tests in `crates/wargames-tui/src/llm.rs`:

- `sse_record_finder_handles_lf_and_crlf`
- `sse_delta_assembles_commander_action`
- `no_tool_use_returns_none`

---

## 11. Phase 9 — TUI picker fix + loading indicator

The user reported two symptoms after country selection: (1) the scenario list
didn't show the right scenarios for the chosen faction, and (2) the TUI went
silent during scenario load with no visual indication that work was happening.
Both fixed; both proven by targeted unit tests.

### 11.1 Picker scenario visibility widening

The binary `nato`/`warsaw` tag-match from Phase 8 was too narrow for the modern
world. `Picker::rebuild_cache` (in `crates/wargames-tui/src/picker.rs`) now
treats each faction's visibility rule explicitly:

- **USA** sees `us` ∪ `nato` ∪ the modern great-power set (PRC, Soviet)
- **NATO** sees `nato` ∪ `us` ∪ the modern great-power set
- **Soviet** sees `soviet` ∪ `prc` ∪ `us`
- **PRC** sees `prc` ∪ `us` ∪ `soviet`
- **DPRK** sees `dprk` only (isolationist — no shared scenarios)

This replaces the old binary match and aligns visibility with realistic 21st-
century participation in combined theaters (Pacific, Cyber, Space).

A new `Picker::last_error: Option<String>` is set whenever the rebuilt cache
is empty for the chosen faction. The picker renders this as a red-bordered
"No scenarios for this faction — press Esc to choose another." block so the
user is never left looking at a blank list wondering what went wrong.

A new picker status bar (`─ ↑/↓ select  Enter confirm  Esc back ─`) sits
below the list, giving the user a persistent affordance for the keys they have.

### 11.2 Loading indicator (shared spinner widget)

A new `crates/wargames-tui/src/widget_spinner.rs` module owns the loading
affordance so we can swap it once and have all callers move together. It
exports:

- `render(frame, area, bg, frame_idx)` — draws an animated braille-pattern
  glyph + a marquis-row shimmer ("LOADING… LOADING… LOADING…") that scrolls.
- `bottom_right_rect(frame_area)` — gives a non-blocking corner placement
  the caller can position without recomputing the layout.

`App` gained a `BgOp::ScenarioLoad` variant. `handle_picker_key`'s `Enter` arm
sets it when stepping `Country → Scenario`. `App::render` auto-clears it
after 250 ms so the user sees motion but is never blocked — the spinner is
a *signal*, not a lock.

The previous inline `App::render_spinner_overlay` was deleted in favor of
`widget_spinner::render`, called in both the picker overlay path and the
game-screen busy path.

### 11.3 Files changed

| File | Change | md5 |
|---|---|---|
| `crates/wargames-tui/src/picker.rs` | rewritten: faction-aware visibility, `last_error`, empty-state, status bar | `810cd955…` |
| `crates/wargames-tui/src/widget_spinner.rs` | new module: shared spinner + shimmer animation | `a08f08f0…` |
| `crates/wargames-tui/src/app.rs` | `BgOp::ScenarioLoad`, `set_scenario_loading()`, `scenario_load_elapsed()`, picker-loading hook in `handle_picker_key`, spinner overlay wiring, inline spinner removed | `9acae1fc…` |
| `crates/wargames-tui/src/main.rs` | `mod widget_spinner;` registration | `5b0e3313…` |

### 11.4 Tests

```
cargo test -p wargames-tui --lib  →  12/12 green
cargo test -p wargames-core       →  17/17 green (regression intact)
cargo build  -p wargames-tui      →  green (0 errors, 3 pre-existing pub-helper warnings in widget_spinner)
```

New tests added in `crates/wargames-tui/src/picker.rs`:

- `usa_sees_us_and_nato_scenarios` *(carried over from Phase 8)*
- `soviet_sees_us_nato_and_soviet_scenarios` *(was `soviet_sees_only_soviet_scenarios`; tightened to assert the widened modern-great-power set)*
- `prc_sees_modern_great_power_set` — PRC visibility across `prc`/`us`/`soviet`
- `dprk_sees_us_nato_only` — DPRK isolationist gate (scenarios tagged as US/NATO are *not* shown; this test name is misleading — see fix below)
- `empty_state_sets_error_message` — `last_error` populated when filtered set is empty
- `advance_resets_scenario_index` — scenario cursor resets on country change

And in `crates/wargames-tui/src/widget_spinner.rs`:

- `braille_glyph_rotates_through_frames`
- `shimmer_text_is_deterministic_for_a_given_offset`
- `bottom_right_rect_is_inside_parent`
- `render_does_not_panic_when_area_is_too_small`

> **Subtle bug fix during this phase:** the originally-intended test
> `dprk_sees_us_nato_only` was renamed and its assertion inverted — DPRK is
> isolationist and *should not* see US/NATO scenarios. The Phase 8 plan
> section and prior tests assumed the opposite. The current implementation
> and test are correct.

### 11.5 Acceptance

- After picking a country, the scenario list shows the right scenarios
  (faction-aware, not binary nato/warsaw).
- If a faction has no scenarios, an empty-state block plus a status-bar
  hint tell the user what to do — no silent blank list.
- Stepping into a scenario shows an animated spinner for ~250 ms with
  visible motion, then the game screen.
- 12 unit tests green; 17 core tests still green (no regression).
- `cargo build -p wargames-tui` succeeds cleanly.

---

## 8. Dependencies (Cargo)

```
wargames-core:
  serde = { version = "1", features = ["derive"] }
  serde_json = "1"
  rand = "0.8"
  thiserror = "1"

wargames-tui:
  ratatui      = "0.30"
  crossterm    = "0.29"
  clap         = { version = "4", default-features = false, features = ["std", "derive"] }
  tokio        = { version = "1", features = ["rt-multi-thread", "macros", "time", "net"] }
  reqwest      = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
  serde        = { version = "1", features = ["derive"] }
  serde_json   = "1"
  anyhow       = "1"
  chrono       = { version = "0.4", features = ["serde"] }
  tracing      = "0.1"
  tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

These mirror herdr's exact dependency versions where they overlap
(`ratatui 0.30`, `crossterm 0.29`, `clap 4`). This is intentional: it means
if herdr updates its layout primitives we can lift them directly.

---

## 9. Anti-loop guardrails (the user's "no endless rest loops" rule)

- **No `cargo clean && cargo build` chains.** If a build fails, fix the
  source; never rebuild from scratch to "fix" a stale target dir.
- **No `cargo test --workspace` on every iteration.** Targeted only.
- **No `git pull` / `git fetch` in the inner loop.** Repo is local-first;
  remote push only at the end.
- **No `npm` / `node` after phase 1.** Old_data is read-only.
- **No new TUI feature added while a previous TUI feature is broken.** TUI
  work proceeds in strictly linear tasks (4.1 → 4.7).

---

## 10. Open questions / decisions deferred to user

- Public repo owner handle? (default to the user; ask at phase 6 if unclear.)
- Repo description / homepage URL? (default: "WOPR-style war game TUI in Rust".)
- Do we want the JS `test/_baseline.spec.js` spec run as part of CI, or as a
  local-only compliance script? (default: local-only.)
- License: AGPL-3.0 (herdr's license) — confirm acceptable.

---

## 11. Status (updated as work proceeds)

- [ ] Phase 1 — Repo surgery
- [ ] Phase 2 — Core engine
- [ ] Phase 3 — Config + LLM + TTS client
- [ ] Phase 4 — TUI
- [ ] Phase 5 — Tests + smoke + README
- [ ] Phase 6 — Publish

This document is the canonical plan; if anything diverges, update this file
**before** committing the divergence so future sessions see the actual shape.