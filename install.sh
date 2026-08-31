#!/usr/bin/env sh
# rome — one-line installer (macOS / Linux / WSL)
# usage:  curl -sSL https://raw.githubusercontent.com/jackiscool123123121/rome/main/install.sh | sh
#
# installs Rust (if missing), then builds and installs the `rome` binary.
# libusb is compiled from source (vendored), so no system dependency is needed.
# afterwards run `rome format` before loading music.

set -eu

VERSION="0.2.0"
REPO="jackiscool123123121/rome"

say() { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m==>\033[0m %s\n' "$*"; }

# ── Rust toolchain ────────────────────────────────────────────────────────────
if ! command -v cargo >/dev/null 2>&1; then
    say "cargo not found — installing Rust via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
    . "$HOME/.cargo/env"
else
    say "cargo already installed ($(cargo --version | awk '{print $2}'))"
fi

# ── install rome ──────────────────────────────────────────────────────────────
say "building & installing rome v$VERSION..."
cargo install --git "https://github.com/$REPO" --tag "v$VERSION" --locked

say "done. verifying..."
if command -v rome >/dev/null 2>&1; then
    say "rome installed: $(command -v rome)"
else
    warn "cargo installed rome but it isn't on PATH — you may need to add ~/.cargo/bin to PATH and start a new shell."
fi

cat <<'EOF'

  ─────────────────────────────────────────────────────────────
   NEXT STEPS
   ───────────
   1. Plug in the SP-1 (running marisko).
   2. rome format          ← REQUIRED first time: initializes the
                             disk. do this BEFORE loading any music.
   3. rome song add "track" a.wav b.wav c.wav d.wav
   ─────────────────────────────────────────────────────────────
EOF
