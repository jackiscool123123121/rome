#!/usr/bin/env sh
# rome — one-line installer (macOS / Linux / WSL)
# usage:  curl -sSL https://raw.githubusercontent.com/jackiscool123123121/rome/main/install.sh | sh
#
# installs Rust (if missing), then builds and installs the `rome` binary.
# libusb is compiled from source (vendored), so no system dependency is needed.
# afterwards run `rome format` before loading music.

set -eu

REPO="jackiscool123123121/rome"
BANNER=' _ __ ___  _ __ ___   ___
| '"'"'__/ _ \| '"'"'_ ` _ \ / _ \
| | | (_) | | | | | |  __/
|_|  \___/|_| |_| |_|\___|'

say()   { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
warn()  { printf '\033[1;33m==>\033[0m %s\n' "$*"; }
dim()   { printf '\033[2m%s\033[0m\n' "$*"; }
cyan()  { printf '\033[1;36m%s\033[0m\n' "$*"; }
step_no=0
step()  { step_no=$((step_no+1)); printf '\033[1;34mSTEP %s/%s\033[0m  %s\n' "$step_no" "$NSTEPS" "$1"; }

NSTEPS=3

# ── banner ───────────────────────────────────────────────────────────────────
clear 2>/dev/null || true
printf '\033[1;96m%s\033[0m\n' "$BANNER"
dim "  teenage engineering sp-1 stem player — companion cli"
printf '\n'

# ── STEP 1: Rust toolchain ───────────────────────────────────────────────────
step "Rust toolchain"
if ! command -v cargo >/dev/null 2>&1; then
    say "cargo not found — installing Rust via rustup (minimal profile)..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
    . "$HOME/.cargo/env"
else
    cyan "cargo already installed ($(cargo --version | awk '{print $2}'))"
fi

# ── STEP 2: build & install rome ─────────────────────────────────────────────
step "Build & install rome"
say "compiling rome (libusb vendored — first build takes a bit)..."
printf '\033[2m'
cargo install --git "https://github.com/$REPO" --branch main --locked --force rome
printf '\033[0m'
cyan "→ rome installed"

# ── STEP 3: verify ───────────────────────────────────────────────────────────
step "Verify"
if command -v rome >/dev/null 2>&1; then
    cyan "✓ rome ready: $(command -v rome)"
else
    warn "rome not on PATH — add ~/.cargo/bin to PATH and start a new shell."
fi

printf '\n'
cyan "$BANNER"
cat <<'EOF'

  ────────────────────────────────────────────────────────────
   NEXT STEPS
   ───────────
   1. Plug in the SP-1 (running marisko).
   2. rome format        ← REQUIRED first time: initializes the
                            disk. do this BEFORE loading any music.
   3. rome song add "track" a.wav b.wav c.wav d.wav

   marisko docs: https://github.com/jackiscool123123121/marisko
  ────────────────────────────────────────────────────────────
EOF
