# Wargames

> A WOPR-style strategic war game TUI written in Rust.

```
   ╔══════════════════════════════════════════════════════════════════════╗
   ║  ▓▓▓  W A R   G A M E S    ::    WOPR   JOSHUA   ::    v 1.0          ║
   ║  ─────────────────────────────────────────────────────────────────    ║
   ║   > PLAYER:    UNITED STATES                              ▮ DEFCON 3   ║
   ║   > OPPONENT:  USSR                                        ▮ TENSION 4 ║
   ║   > SCENARIO:  NORTH_ATLANTIC_2024                                   ║
   ║   > MODEL:     MiniMax-M3     ── streaming ── →  keeper, 720p, ⅓ height ║
   ║   > STATE:     ◤ ━━━━━━━━━━━━ STRIKE ━━━━━━━━━━━━ ◢                         ║
   ╚══════════════════════════════════════════════════════════════════════╝
```

Modeled after the [herdr](https://github.com/ogulcancelik/herdr) terminal
workspace manager (ratatui 0.30 + crossterm 0.29 + clap 4).

On launch:

1. Splash screen (`WAR GAMES OG`) for 5 seconds.
2. Country picker.
3. Scenario picker (scenarios are derived from real-world events and the
   simulator produces turn-by-turn predictions of strike / disarm / defect /
   negotiate probabilities).
4. Gameplay loop in a herdr-style panned TUI: state, prediction, radar,
   action menu, and event log.

## Install (one command)

```
curl -fsSL https://raw.githubusercontent.com/ankurCES/wargames/main/scripts/install.sh | bash
```

The installer:

1. Checks for a Rust toolchain (`rustc >= 1.80`) and installs via `rustup` if
   missing.
2. Clones this repo into a scratch directory under `$TMPDIR`.
3. Runs `cargo build --release` on `wargames-tui`.
4. Installs the binary to `~/.cargo/bin/wargames`.
5. Cleans up the scratch directory — nothing is left behind.
6. Prints launch instructions.

The scratch dir is preserved only on failure (for debugging); on success the
`EXIT` trap removes it. There are no retries and no `cargo clean` — if the
build fails the source is preserved for inspection, otherwise the install is
one-shot.

Override the install location with `WARGAMES_INSTALL_DIR=/some/path bash …`.

## Build from source

```
git clone https://github.com/ankurCES/wargames.git
cd wargames
cargo build --release
./target/release/wargames
```

## Configuration

The binary reads `~/.blumi/settings.json` (the shared, device-wide config
used by every blumi app on this host). The path is hardcoded — there is no
environment-variable override — so every app on the device agrees on a
single config file.

If `~/.blumi/settings.json` is missing, the binary exits with code 2.

### Sample

A complete, copiable starting point lives at
[`examples/settings.sample.json`](examples/settings.sample.json). Copy it to
`~/.blumi/settings.json` and fill in the placeholder API keys:

```
cp examples/settings.sample.json ~/.blumi/settings.json
$EDITOR ~/.blumi/settings.json
```

The file is template-only — every `api_key` is a literal `PLACEHOLDER_*`
string and is never read from the developer's actual config. The JSON file
is also round-tripped by `cargo test -p wargames-tui --lib config::` so a
schema drift will fail CI before any user copy-pastes a broken version.

### Fields wargames consumes

wargames reads a **strict subset** of the file (the rest is consumed by
other blumi apps on the same device and is ignored here):

- `providers.<name>.{api_key, base_url, kind, models}` — LLM credentials.
  `base_url` is the Anthropic-compatible root; wargames appends
  `/v1/messages` for both REST and SSE. `kind` defaults to `"anthropic"`.
  Three providers ship in the sample (`minimax`, `azure-foundry`,
  `anthropic-direct`); pick whichever you have a key for.
- `router.light.{model, provider}` — the default model for the Soviet
  commander. `provider` must name a key under `providers`; `model` should
  appear in that provider's `models` list.
- `voice.*` — optional, TTS is best-effort and wargames ignores it.

`router.heavy` and `router.judge` are read by other blumi apps (cyberdeck,
etc.) and ignored by wargames.

### How wargames uses it

At startup, `BlumiSettings::from_path` deserializes the file into the
typed `BlumiSettings { providers, router, voice }` struct (see
`crates/wargames-tui/src/config.rs`). The Soviet commander's LLM client is
then built by resolving `router.light.provider` → `providers[name]` and
reading `api_key`, `base_url`, and `model`. All HTTP calls go through one
`reqwest::Client`; the streaming path uses Anthropic-compatible SSE
(`/v1/messages?stream=true`) and falls back to a deterministic heuristic
opponent on timeout or 4xx.

## Layout

```
crates/
  wargames-core/   # pure rules: state, engine, triggers, predictions
  wargames-tui/    # the binary: splash, picker, paned UI
scenarios/         # scenario JSON (verbatim from the JS impl)
examples/          # settings.sample.json (copiable starting config)
scripts/           # install.sh (curl|bash) + smoke.sh
docs/              # plan + learnings
old_data/          # the previous JS implementation, archived
```

## Tests

Targeted unit tests live alongside the code:

```
cargo test -p wargames-core
```

`scripts/smoke.sh` is the runtime regression net — it verifies the binary
launches, the config path resolves to `~/.blumi/settings.json`, and the
`scenarios/` directory loads. Workspace-wide runs happen periodically, not on
every commit.

## License

AGPL-3.0-or-later.