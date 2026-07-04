# Wargames

A WOPR-style strategic war game TUI written in Rust.

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

The relevant fields:

- `providers.<name>.{api_key, base_url}` — LLM credentials (anthropic-compatible).
- `router.light.{model, provider}` — default model for the Soviet commander.
- `voice.tts_api_key`, `voice.tts_voice` — optional, TTS is best-effort.

If `~/.blumi/settings.json` is missing, the binary exits with code 2.

## Layout

```
crates/
  wargames-core/   # pure rules: state, engine, triggers, predictions
  wargames-tui/    # the binary: splash, picker, paned UI
scenarios/         # scenario JSON (verbatim from the JS impl)
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