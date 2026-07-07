# Receiving Popup — Design Spec

**Status:** Awaiting user review
**Date:** 2026-07-07
**Owner:** wargames-tui

## 1. Problem

When the player commits an action with Enter, there is a window between
"action applied to world" and "opponent response applied to world". On
heuristic runs that window is microseconds (the spinner never appears).
On real LLM runs it can be hundreds of ms to several seconds. Today the
only feedback during that window is the small bottom-right corner
spinner on `BgOp::LlmCall` — which doesn't cover the heuristic gap and
is easy to miss in the corner.

The user asked for a small **popup** that explicitly says we're waiting
for the opponent's response, with a braille animation, that lives until
the response arrives.

## 2. Behavior

### 2.1 Visibility

| Condition                                                | Popup state |
| -------------------------------------------------------- | ----------- |
| `opponent_pending == true`                               | Visible     |
| `opponent_pending == false` AND no fade timer set        | Hidden      |
| `opponent_pending == false` AND fade timer set AND not yet expired | Visible |
| `opponent_pending == false` AND fade timer expired       | Hidden      |
| Frame too narrow (< 36 cells) or too short (< 4 rows)    | No-op (hidden, no errors) |
| Screen ≠ `Screen::Game`                                  | Hidden      |

### 2.2 Show trigger

`commit_action` (`app.rs:1166`) flips `opponent_pending = true` at line
1181. The popup's visibility derivation reads that field directly; no
new show-call is needed. The existing `render()` loop paints the popup
on the next frame.

### 2.3 Hide trigger + linger

When the opponent response completes, two existing code paths clear
`opponent_pending`:

- `apply_opponent_action_heuristic` (`app.rs:1291`)
- `apply_opponent_action` after the LLM/SSE task drains (`app.rs:1331`)

A new per-frame tick step in the run loop watches for the
`true → false` transition and, when it sees one, sets
`receiving_popup_fade_at = Some(Instant::now() + 300ms)`. The popup
stays visible until that instant passes, then a second tick step
clears the field.

The 300 ms linger covers the visual gap between "opp action applied"
and "log entry + status line reflect it" so the popup doesn't blink
off awkwardly.

### 2.4 Restart

If the player commits another action while the fade timer is still
pending, `commit_action` clears `receiving_popup_fade_at = None` and
the popup snaps back to fully visible (`opponent_pending == true`).

### 2.5 Rendering constraints

- Popup does **not** block input. Player can still scroll the log,
  cycle tabs, press Esc to quit, etc.
- Popup is **not** shown on Login, Splash, Picker, GameOver, or
  Settings screens.
- Popup is drawn **after** the main game widgets so it paints on top.
- Popup reuses `App::spinner_frame` (already incremented in the run
  loop) — no new counter.

## 3. State additions

One new field on `App`:

```rust
/// Wall-clock instant after which the receiving popup should disappear.
/// `None` while the popup is either hidden or actively visible
/// (no linger). Set 300 ms after `opponent_pending` flips false.
pub receiving_popup_fade_at: Option<Instant>,
```

Visibility derivation (used in `render()`):

```rust
let popup_visible = self.opponent_pending
    || self.receiving_popup_fade_at.is_some_and(|t| Instant::now() < t);
```

## 4. New module — `crates/wargames-tui/src/widget_receiving_popup.rs`

### 4.1 Public API

```rust
/// Minimum frame width to attempt rendering. Set to the popup's
/// own width (34 cells for the standard label + 2-cell margin)
/// so the no-op triggers exactly when the popup would be cropped.
pub const MIN_POPUP_WIDTH: u16 = 36;
/// Minimum frame height to attempt rendering.
pub const MIN_POPUP_HEIGHT: u16 = 4;

/// Compute the centered rect for the popup inside `frame_area`.
/// Returns `Rect::default()` (zero area) when the frame is below the
/// minimum dimensions — callers treat that as "don't render".
pub fn centered_rect(frame_area: Rect) -> Rect;

/// Render the popup on top of whatever's already in `frame`.
/// No-op when `centered_rect` returned a degenerate area.
/// Uses `braille_at(frame_idx)` from `widget_spinner` for the glyph.
pub fn render(frame: &mut Frame, frame_area: Rect, frame_idx: usize);
```

### 4.2 Visual layout

Single-row block, dim border, padded 1 cell on each side.

```
╭ ⠹ RECEIVING OPPONENT RESPONSE… ╮
```

- Border style: `theme.accent_warn` (yellow)
- Text style: `theme.status_text`
- Padding background: `theme.pane_bg`
- Glyph: `widget_spinner::braille_at(frame_idx)` (10-frame cycle)
- Label: literal `"RECEIVING OPPONENT RESPONSE…"`
- Popup interior width: `label.len() + 2` (braille + space + label)
- Total popup width including borders + padding: `label.len() + 6`
- Popup height: `1` row of content + `2` border rows = `3` rows total

For `RECEIVING OPPONENT RESPONSE…` (28 chars) that's 34-cell wide,
3-row tall.

### 4.3 Anchor

- `x = (frame_area.width - popup_width) / 2`
- `y = frame_area.height - 4` (4 rows above the bottom — clears the
  status line, doesn't overlap the action menu which lives higher in
  the `game_layout`)

## 5. Render wiring

### 5.1 App::render (game branch)

In the `Screen::Game` branch, AFTER the game widgets render
(`render_compact_game`, `render_grid_game`, or `render_too_small`):

```rust
let popup_visible = self.opponent_pending
    || self.receiving_popup_fade_at.is_some_and(|t| Instant::now() < t);
if popup_visible {
    widget_receiving_popup::render(frame, frame.area(), self.spinner_frame);
}
```

### 5.2 App tick hook (run loop)

The existing run-loop tick (find via `grep "spinner_frame +=\|tick\|on_tick"`)
already runs once per frame. Add two transitions there:

1. **Entering fade:** if `opponent_pending` was `true` on the previous
   tick and is `false` now, set `receiving_popup_fade_at =
   Some(Instant::now() + Duration::from_millis(300))`.
2. **Exiting fade:** if `receiving_popup_fade_at` is `Some(t)` and
   `Instant::now() >= t`, clear it.

To implement "previous tick", introduce a small shadow field
`prev_opponent_pending: bool` initialised to `false` and updated at
the end of every tick. This is the smallest, most testable approach
(no event channel needed).

### 5.3 commit_action restart

At the top of `commit_action`:

```rust
self.receiving_popup_fade_at = None; // cancel any in-flight fade
```

Placed *before* the `opponent_pending = true` assignment at line 1181
so the next render shows the popup fully visible.

## 6. Tests

### 6.1 `widget_receiving_popup::tests`

- `centered_rect_sits_at_expected_x_y` — 80×24 frame, popup center is
  `((80-34)/2, 24-4) = (23, 20)`.
- `centered_rect_is_zero_when_too_narrow` — 30×24 frame, returns
  `Rect::default()`.
- `centered_rect_is_zero_when_too_short` — 80×3 frame, returns
  `Rect::default()`.
- `render_paints_braille_glyph_and_label` — TestBackend 80×24, paint
  with `frame_idx = 0`, assert buffer contains the braille glyph and
  `"RECEIVING OPPONENT RESPONSE…"`.
- `render_is_noop_on_subminimum_frame` — TestBackend 30×3, paint,
  assert no popup content appears.

### 6.2 `app::playable_flow_tests`

- `commit_action_sets_opponent_pending_and_clears_fade_at` — call
  `commit_action`, assert `opponent_pending == true` and
  `receiving_popup_fade_at == None`.
- `apply_opponent_action_heuristic_sets_fade_at` — drive the run
  loop past the heuristic path, assert `receiving_popup_fade_at` is
  `Some` with a 300 ms delta.
- `fade_clears_after_window_passes` — `#[ignore]`'d test (real wall
  clock), `std::thread::sleep(350)`, assert
  `receiving_popup_fade_at.is_none()` after the next tick. The
  non-ignored form uses an injected clock seam — see §7.

### 6.3 Targeted run

```
cargo test -p wargames-tui --lib -- \
  widget_receiving_popup \
  app::playable_flow_tests::commit_action_sets_opponent_pending_and_clears_fade_at \
  app::playable_flow_tests::apply_opponent_action_heuristic_sets_fade_at \
  app::playable_flow_tests::fade_clears_after_window_passes
```

## 7. Testability seam — `Instant` clock

To avoid `#[ignore]` on the fade-clears test, extract a tiny clock
trait or `now() -> Instant` function pointer on `App`:

```rust
pub type ClockFn = fn() -> Instant;
impl App {
    pub fn now(&self) -> Instant {
        (self.clock)(/* self */)
    }
}
```

The default `clock` returns `Instant::now()`. Tests override it with
a function that returns a `Mutex<Instant>` they control. This is a
seam, not a refactor — `App::new` continues to use `Instant::now()`
by default.

If the seam proves invasive, fall back to the `#[ignore]` test and
accept the slower targeted run.

## 8. Non-goals

- No new keyboard handling. Popup is read-only.
- No new theme tokens. Reuses `accent_warn`, `status_text`, `pane_bg`.
- No new dependencies. Ratatui only.
- No animation beyond reusing the existing 10-frame braille cycle.
- No fade-in/fade-out alpha animation. The 300 ms linger IS the
  disappearance — popup appears instantly, holds, then vanishes.

## 9. Risk & mitigations

| Risk                                                  | Mitigation |
| ----------------------------------------------------- | ---------- |
| Popup covers the action menu or log                   | Anchor at `height - 4` clears the bottom status line; popup is only 3 rows tall; the layout puts action menu and log well above that. |
| Two spinners on screen during LLM turns               | Acceptable — corner spinner covers LLM latency, popup covers semantic "waiting for opp". They serve different questions. Heuristic path (smoke test, CI default) shows only the popup. |
| Popup leaks across screen transitions                 | Visibility derivation only triggers in `Screen::Game` branch — natural containment. |
| Fade timer never clears if run loop stalls            | The clear step runs in the tick hook on every frame; even a stalled frame eventually catches up. |
| `prev_opponent_pending` shadow drifts on restart      | `commit_action` writes the canonical state synchronously; the tick hook reads the value as of the *previous* frame. Restart case: `opponent_pending` flips `false → true` mid-fade; the tick hook sees `prev=true, now=true` → no fade entry. The fade is cleared by `commit_action` itself. |

## 10. Out of scope (explicit YAGNI)

- Centering on the **y** axis (e.g. true vertical middle of the frame).
- A countdown showing seconds remaining.
- A cancel button that aborts the LLM call.
- A separate "receiving log entry" placed inside the log.
- Configurable linger duration.
- Different popup per game mode (multiplayer, tutorial, replay).