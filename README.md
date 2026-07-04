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

## Build

```
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
docs/              # plan + learnings
old_data/          # the previous JS implementation, archived
scripts/           # smoke test
```

## License

AGPL-3.0-or-later.