#!/usr/bin/env bash
# wargames — one-shot installer
#
# Single curl|bash command:
#
#   curl -fsSL https://raw.githubusercontent.com/ankurCES/wargames/main/scripts/install.sh | bash
#
# What it does:
#   1. Verifies a Rust toolchain is present (rustc 1.80+). If not, installs it
#      via rustup (interactive prompts are auto-accepted). This is the only
#      "host dependency" we ask for beyond the standard build toolchain.
#   2. Clones the public `ankurCES/wargames` repo into a scratch directory
#      under $TMPDIR (default /tmp).
#   3. Runs `cargo build --release` from inside the clone.
#   4. Installs the resulting binary to ~/.cargo/bin/wargames.
#   5. Cleans up the scratch clone (the source tree is preserved only in the
#      user's mind; we don't leave a working copy behind).
#   6. Prints how to launch the game.
#
# On any failure we exit non-zero with a clear error and the scratch dir is
# preserved for inspection (delete it manually if you don't want it).
#
# Designed to NOT loop: no retries, no rebuild-from-scratch, no cargo clean.
#
# Visual style: matches the in-game TUI splash (WOPR / War Games). Green
# phosphor CRT borders, yellow accents, gray body. Animation only fires when
# stdout is a TTY — when piped (`curl | bash`) we fall back to a plain log
# stream so the user can still see what's happening, but the screen doesn't
# glitch out from escape codes racing over a non-tty pipe.

set -euo pipefail

REPO="https://github.com/ankurCES/wargames.git"
BIN_NAME="wargames"
INSTALL_DIR="${WARGAMES_INSTALL_DIR:-$HOME/.cargo/bin}"
SCRATCH_DIR="$(mktemp -d -t wargames-install-XXXXXX)"
REQUIRED_RUST_VERSION="1.80"

# ---------------------------------------------------------------- ANSI palette
# Mirrors the in-game TUI splash palette so the installer reads as the same
# CRT-phosphor world. Disabled automatically when stdout is not a TTY (the
# `curl | bash` path) so we don't spam escape codes over a pipe.

if [[ -t 1 ]]; then
    C_RESET=$'\033[0m'
    C_BOLD=$'\033[1m'
    C_DIM=$'\033[2m'
    C_GREEN=$'\033[32m'
    C_BRIGHT_GREEN=$'\033[1;32m'
    C_CYAN=$'\033[36m'
    C_YELLOW=$'\033[1;33m'
    C_RED=$'\033[1;31m'
    C_GRAY=$'\033[90m'
    C_BG_DARK=$'\033[40m'
    C_CLR=$'\033[2K'
    C_CR=$'\r'
else
    C_RESET=""
    C_BOLD=""
    C_DIM=""
    C_GREEN=""
    C_BRIGHT_GREEN=""
    C_CYAN=""
    C_YELLOW=""
    C_RED=""
    C_GRAY=""
    C_BG_DARK=""
    C_CLR=""
    C_CR=""
fi

# ---------------------------------------------------------------- log helpers
# All output goes to stderr so the installer's stdout can stay clean for any
# future caller that wants to capture it. The visual style mirrors the
# in-game status bar: `[ OK ]` / `[FAIL]` / `[..]`.

log()      { printf '%b[wargames-install]%b %s\n' "$C_DIM" "$C_RESET" "$*" >&2; }
ok()       { printf '%b[ %bOK%b ]%b %s\n'   "$C_GREEN" "$C_BRIGHT_GREEN" "$C_GREEN" "$C_RESET" "$*" >&2; }
fail()     { printf '%b[%bFAIL%b]%b %s\n'   "$C_RED" "$C_BOLD" "$C_RED" "$C_RESET" "$*" >&2; exit 1; }
phase()    { printf '%b[ %b..%b ]%b %b%s%b\n' "$C_CYAN" "$C_BOLD" "$C_CYAN" "$C_RESET" "$C_YELLOW" "$*" "$C_RESET" >&2; }

# Print the WOPR-style header banner. Only shown once at the top of the run.
banner() {
    if [[ ! -t 1 ]]; then
        return
    fi
    # The same ASCII art the in-game splash renders, with a green border.
    # We deliberately keep it short so the banner fits in a typical 80-col
    # terminal even when piped through `curl | bash` with line-buffering.
    printf '%b' "$C_BRIGHT_GREEN" >&2
    cat >&2 <<'BANNER'
+---------------------------------------------------------------+
|   _    _    ___     __ ______ ____   ___  ____   ____        |
|  | |  / \  / _ \   / /| ____|  _ \ / _ \|  _ \ / ___|       |
|  | | / _ \| | | | / /_|  _| | |_) | | | | |_) | |  _        |
|  | |/ ___ \ |_| |/ ___ | |___|  _ <| |_| |  _ <| |_| |       |
|  |__/_/   \_\___/_/   |_____|_| \_\\___/|_| \_\\____|       |
+---------------------------------------------------------------+
BANNER
    printf '%b' "$C_RESET" >&2
    printf '%b[ wargames installer ]%b  %bStrategic Defense Initiative Online%b\n' \
        "$C_BRIGHT_GREEN" "$C_RESET" "$C_YELLOW" "$C_RESET" >&2
    printf '\n' >&2
}

# Animated DEFCON counter that ticks during long-running steps. We don't
# actually drive cargo's output through it — we run cargo with `--quiet` and
# show our own ticker alongside it. The ticker yields to a real terminal so
# the user sees motion; on a non-tty pipe it just sits still, which is fine.
#
# Usage:
#   defcon_ticker "DEFCON 5 — FETCHING REPO" 0.15 &
#   ... long step ...
#   kill %1 2>/dev/null || true
#   wait 2>/dev/null || true
defcon_ticker() {
    local label="$1"
    local delay="${2:-0.2}"
    # Only animate when stdout is a TTY. Without a TTY the carriage-return
    # dance produces no visible output anyway, but we still avoid the
    # background-process overhead.
    if [[ ! -t 1 ]]; then
        return
    fi
    local phases=("SCANNING" "FETCHING" "COMPILING" "LINKING" "INSTALLING" "VERIFYING")
    local i=0
    # Hide the cursor for the duration of the ticker so the in-place
    # updates don't leave a blinking block on top of the bar.
    printf '\033[?25l' >&2
    while true; do
        local phase="${phases[$((i % ${#phases[@]}))]}"
        local dots=""
        local n=$((i % 4))
        case "$n" in
            0) dots="" ;;
            1) dots="." ;;
            2) dots=".." ;;
            3) dots="..." ;;
        esac
        printf '%b%s%b %b%s%s%b' \
            "$C_YELLOW" "$label" "$C_RESET" \
            "$C_DIM" "$phase$dots" "$C_RESET" >&2
        sleep "$delay" 2>/dev/null || true
        # Carriage return + clear-to-EOL so we overwrite the previous line
        # in place rather than scrolling.
        printf '\r\033[2K' >&2
        i=$((i + 1))
    done
}

cleanup() {
    # Always restore the cursor, even on error paths — otherwise a Ctrl-C
    # mid-animation would leave the user's terminal without a cursor.
    if [[ -t 1 ]]; then
        printf '\033[?25h' >&2
    fi
    # Kill any background ticker that escaped.
    jobs -p 2>/dev/null | xargs -r kill 2>/dev/null || true
    if [[ -d "$SCRATCH_DIR" ]]; then
        rm -rf "$SCRATCH_DIR"
    fi
}
trap cleanup EXIT

# ---------------------------------------------------------------- 1. rust check
ensure_rust() {
    phase "DEFCON 5 — checking rust toolchain"
    if command -v cargo >/dev/null 2>&1 && command -v rustc >/dev/null 2>&1; then
        local current
        current="$(rustc --version | awk '{print $2}')"
        if [[ "$(printf '%s\n%s\n' "$REQUIRED_RUST_VERSION" "$current" | sort -V | head -n1)" == "$REQUIRED_RUST_VERSION" ]]; then
            ok "rust $current present"
            return
        fi
        log "rust $current found, but >= $REQUIRED_RUST_VERSION required; upgrading via rustup"
    else
        log "rust not found; installing via rustup"
    fi

    if ! command -v rustup >/dev/null 2>&1; then
        log "fetching rustup-init"
        local installer="$SCRATCH_DIR/rustup-init.sh"
        curl -fsSL https://sh.rustup.rs -o "$installer"
        sh "$installer" -y --default-toolchain stable --profile minimal
    else
        rustup update stable
    fi

    # Make cargo available in this shell.
    # shellcheck disable=SC1091
    source "$HOME/.cargo/env" || true
    if ! command -v cargo >/dev/null 2>&1; then
        fail "rust install finished but cargo is not on PATH; check $HOME/.cargo/env"
    fi
    ok "rust toolchain ready"
}

# ---------------------------------------------------------------- 2. clone
do_clone() {
    phase "DEFCON 4 — cloning repository"
    log "cloning $REPO into scratch dir $SCRATCH_DIR/src"
    mkdir -p "$SCRATCH_DIR/src"
    if ! git clone --depth=1 "$REPO" "$SCRATCH_DIR/src"; then
        fail "git clone failed; check network and that $REPO is reachable"
    fi
    ok "repository cloned"
}

# ---------------------------------------------------------------- 3. build
do_build() {
    phase "DEFCON 3 — building release binary"
    log "cargo build --release (this may take several minutes on first run)"

    # Run the cargo build in the foreground with a background ticker for
    # visual feedback. The ticker yields a progress dot-stream on a TTY; on
    # a pipe it just no-ops. We never lose cargo's own error output because
    # we don't redirect it — `--quiet` only suppresses cargo's progress bar,
    # not its compile errors.
    defcon_ticker "DEFCON 3" 0.2 &
    local ticker_pid=$!
    local build_ok=0
    if ( cd "$SCRATCH_DIR/src" && cargo build --release -p wargames-tui --quiet ); then
        build_ok=1
    fi
    # Stop the ticker cleanly before we print the success/failure line.
    kill "$ticker_pid" 2>/dev/null || true
    wait "$ticker_pid" 2>/dev/null || true
    # Carriage return + clear-to-EOL — only meaningful on a TTY where the
    # ticker was overwriting the previous line in place. On a pipe the
    # ticker was a no-op so there's nothing to clear.
    if [[ -t 1 ]]; then
        printf '\r\033[2K' >&2
    fi

    if [[ "$build_ok" -ne 1 ]]; then
        fail "cargo build failed; rerun without --quiet to see compile errors"
    fi

    if [[ ! -x "$SCRATCH_DIR/src/target/release/$BIN_NAME" ]]; then
        fail "build did not produce target/release/$BIN_NAME"
    fi
    ok "release binary built"
}

# ---------------------------------------------------------------- 4. install
do_install() {
    phase "DEFCON 2 — installing"
    mkdir -p "$INSTALL_DIR"
    install -m 0755 "$SCRATCH_DIR/src/target/release/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"
    ok "installed $INSTALL_DIR/$BIN_NAME"

    # Ship the bundled scenarios alongside the binary so the installed
    # user can run without pointing --scenarios-dir somewhere. Mirror the
    # logic in `default_scenarios_dir()` (main.rs): $XDG_DATA_HOME first,
    # then $HOME/.local/share, then /usr/local/share as a last resort.
    local data_base="${WARGAMES_DATA_DIR:-}"
    if [[ -z "$data_base" ]]; then
        if [[ -n "${XDG_DATA_HOME:-}" ]]; then
            data_base="$XDG_DATA_HOME"
        elif [[ -n "${HOME:-}" ]]; then
            data_base="$HOME/.local/share"
        else
            data_base="/usr/local/share"
        fi
    fi
    local data_dir="$data_base/wargames/scenarios"
    if [[ -d "$SCRATCH_DIR/src/scenarios" ]]; then
        mkdir -p "$data_dir"
        # Idempotent install — only copy if source is newer or dest missing.
        install -m 0644 "$SCRATCH_DIR/src/scenarios/"*.json "$data_dir/" 2>/dev/null || \
            cp "$SCRATCH_DIR/src/scenarios/"*.json "$data_dir/"
        ok "installed scenarios to $data_dir"
    else
        log "warning: scenarios dir not found at $SCRATCH_DIR/src/scenarios — TUI may fail to load"
    fi
}

# ---------------------------------------------------------------- 5. config check
config_check() {
    phase "DEFCON 1 — final checks"
    local cfg="$HOME/.blumi/settings.json"
    if [[ -f "$cfg" ]]; then
        ok "config: $cfg present"
    else
        log "config: $cfg NOT present — the game will exit 2 on first run."
        log "create it (or symlink it) before launching, or the LLM-driven"
        log "Soviet commander will not have credentials."
    fi
}

# ---------------------------------------------------------------- outro
outro() {
    printf '\n' >&2
    if [[ -t 1 ]]; then
        printf '%b' "$C_BRIGHT_GREEN" >&2
        cat >&2 <<'EOF'
+---------------------------------------------------------------+
|  GREETINGS, PROFESSOR FALKEN                                |
|                                                              |
|  THE ONLY WINNING MOVE IS NOT TO PLAY.                       |
+---------------------------------------------------------------+
EOF
        printf '%b' "$C_RESET" >&2
    else
        cat <<EOF

[wargames-install] GREETINGS, PROFESSOR FALKEN.
[wargames-install] THE ONLY WINNING MOVE IS NOT TO PLAY.
EOF
    fi

    cat <<EOF >&2

To play:
    $INSTALL_DIR/$BIN_NAME

Or, if $INSTALL_DIR is on your PATH:
    $BIN_NAME

The first launch will:
  1. Show "WAR GAMES" splash for 5 seconds
  2. Pick a mode (Human vs AI / AI vs AI)
  3. Pick a country (USA / USSR / NATO / PRC / DPRK)
  4. Pick a scenario (8 hand-authored, derived from real-world events)
  5. Play in a CRT-phosphor TUI with per-turn Monte Carlo predictions

EOF
}

# ---------------------------------------------------------------- main
banner
ensure_rust
do_clone
do_build
do_install
config_check
outro