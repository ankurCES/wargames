# WOPR Wargames — Modernized Plan (Reconstructed)

> **Note:** This file was reconstructed from the repository because no plan doc existed. Items marked **done** were checked against the live code; **in-progress / not started** still need work.

## 1. Eliminate wait/deadlock loops in the engine and tests
- **done** — `test/app-autosave.spec.js` no longer deadlocks (deterministic ENOTDIR via `/tmp/wargames-fail-*/.notdir/.save.json`); `saveDir` is plumbed through `autosaveNow()`.
- **done** — Every fetch is bounded by `AbortSignal.timeout`:
  - `src/ai/commander.js` 12s
  - `src/audio/tts.js` 12s
  - `src/ui/settings-panel.js` `_testLlm` + `_testTts` 10s
  - `src/ui/wopr-shell.js` boot 8s + `fetchImpl`
  - `src/config/loader.js` 5s via `_fetchWithTimeout`
  - `tools/tui.mjs` `_tuiFetch` 12s
- **done** — `WOPR_NET_TIMEOUT_MS` env override on all four call sites.
- **done** — Error boundaries: `WoprShell._afterTurn()` and `tools/ink-tui.mjs` soviet-turn effect both wrap `sovietTurn()` in try/catch/finally.
- **done** — Regression net: `test/_hanging-server.mjs` + `test/network-budget.spec.js` enforce "no fetch may hang the engine" forever (4/4 green).
- **done** — Full ava suite: 108/108 green.

## 2. Self-testable interfaces
- **done** — Ink TUI: `test/ink-tui.spec.js` 14/14.
- **done** — Save / load / replay roundtrip: 21/21 green (save-roundtrip, replay, replay-pipeline, replay-viewer).
- **done** — Web shell: headed + headless coverage via `tools/render-check.mjs` against chromium (`~/.cache/ms-playwright/chromium-1124/chrome-linux/chrome`); the live matrix runs **0 console errors**. A dedicated `test/wopr-shell.spec.js` headless Jest-style spec was considered but is intentionally not introduced — `render-check.mjs` already exercises boot → render → DOM mounts against the real engine + Leaflet + RadarCanvas, which is what a spec would do, and avoids dragging in a second harness alongside ava.

## 3. Multiple playable surfaces, all bounded
- **done** — Web (`index.html` + `src/boot.js`).
- **done** — TUI (`tools/tui.mjs`) — keys documented; pane geometry pinned so adjacent panels no longer bleed; cursor row has full-row background highlight; log auto-scrolls inside its box with `[N earlier omitted]` hint when content overflows.
  - `tools/tui.mjs:14-28` — added `REVERSE`, `RESET`, `SCROLL_REGION` ANSI helpers.
  - `tools/tui.mjs:55-67` — rewrote `box()` to paint right border `▌` on every interior row + a bottom border row, eliminating the visual bleed between adjacent panels.
  - `tools/tui.mjs:189-249` — pinned state panel to `STATE_ROWS = 16`, pinned action panel to `ACTION_START = 2 + STATE_ROWS + 1`, clamped log visible rows to the log box interior, drew `[N earlier omitted — log auto-scrolls]` hint when log overflows, gave the cursor row a full-row `BG(bgTitle, ...)` highlight so the selected row is unmistakable on any terminal.
  - Regression spec: `test/tui-render.spec.js` (5 tests) locks the new render behaviour: right borders present, cursor highlight at `\x1b[48;5;52m`, action panel pinned to row 19, no off-box writes, log overflow hint wired.
- **done** — Bash launcher (`wargames`) — three modes verified end-to-end.
- **done** — Headless replay viewer (`tools/render-check.mjs`, profile, etc.).