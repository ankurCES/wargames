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

set -euo pipefail

REPO="https://github.com/ankurCES/wargames.git"
BIN_NAME="wargames"
INSTALL_DIR="${WARGAMES_INSTALL_DIR:-$HOME/.cargo/bin}"
SCRATCH_DIR="$(mktemp -d -t wargames-install-XXXXXX)"
REQUIRED_RUST_VERSION="1.80"

log() { printf '[wargames-install] %s\n' "$*" >&2; }
err() { printf '[wargames-install][error] %s\n' "$*" >&2; exit 1; }
cleanup() {
    if [[ -d "$SCRATCH_DIR" ]]; then
        rm -rf "$SCRATCH_DIR"
    fi
}
trap cleanup EXIT

# ---------------------------------------------------------------- 1. rust check
ensure_rust() {
    if command -v cargo >/dev/null 2>&1 && command -v rustc >/dev/null 2>&1; then
        local current
        current="$(rustc --version | awk '{print $2}')"
        if [[ "$(printf '%s\n%s\n' "$REQUIRED_RUST_VERSION" "$current" | sort -V | head -n1)" == "$REQUIRED_RUST_VERSION" ]]; then
            log "rust $current present"
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
        err "rust install finished but cargo is not on PATH; check $HOME/.cargo/env"
    fi
}

# ---------------------------------------------------------------- 2. clone
do_clone() {
    log "cloning $REPO into scratch dir $SCRATCH_DIR/src"
    mkdir -p "$SCRATCH_DIR/src"
    if ! git clone --depth=1 "$REPO" "$SCRATCH_DIR/src"; then
        err "git clone failed; check network and that $REPO is reachable"
    fi
}

# ---------------------------------------------------------------- 3. build
do_build() {
    log "cargo build --release (this may take several minutes on first run)"
    ( cd "$SCRATCH_DIR/src" && cargo build --release -p wargames-tui --quiet )
    if [[ ! -x "$SCRATCH_DIR/src/target/release/$BIN_NAME" ]]; then
        err "build did not produce target/release/$BIN_NAME"
    fi
}

# ---------------------------------------------------------------- 4. install
do_install() {
    mkdir -p "$INSTALL_DIR"
    install -m 0755 "$SCRATCH_DIR/src/target/release/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"
    log "installed $INSTALL_DIR/$BIN_NAME"

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
        log "installed scenarios to $data_dir"
    else
        log "warning: scenarios dir not found at $SCRATCH_DIR/src/scenarios — TUI may fail to load"
    fi
}

# ---------------------------------------------------------------- 5. config check
config_check() {
    local cfg="$HOME/.blumi/settings.json"
    if [[ -f "$cfg" ]]; then
        log "config: $cfg present (good — all blumi apps on this device share it)"
    else
        log "config: $cfg NOT present — the game will exit 2 on first run."
        log "create it (or symlink it) before launching, or the LLM-driven"
        log "Soviet commander will not have credentials."
    fi
}

# ---------------------------------------------------------------- main
ensure_rust
do_clone
do_build
do_install
config_check

cat <<EOF

[wargames-install] done.

To play:
    $INSTALL_DIR/$BIN_NAME

Or, if $INSTALL_DIR is on your PATH:
    $BIN_NAME

The first launch will:
  1. Show "WAR GAMES OG" for 5 seconds
  2. Prompt you to pick a country
  3. Pick a scenario (8 hand-authored, derived from real-world events)
  4. Play in a herdr-style panned TUI with per-turn Monte Carlo predictions

EOF