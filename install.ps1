# rome — one-line installer (Windows PowerShell)
# usage:  irm https://raw.githubusercontent.com/jackiscool123123121/rome/main/install.ps1 | iex
#
# installs Rust (if missing), then builds and installs the `rome` binary.
# libusb is compiled from source (vendored), so no system dependency is needed.
# afterwards run `rome format` before loading music.

$ErrorActionPreference = "Stop"
$VERSION = "0.2.0"
$REPO = "jackiscool123123121/rome"

function Info  { Write-Host "==> $args" -ForegroundColor Green }
function Warn  { Write-Host "==> $args" -ForegroundColor Yellow }

# ── Rust toolchain ───────────────────────────────────────────────────────────
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Info "cargo not found - installing Rust via rustup..."
    Invoke-WebRequest https://sh.rustup.rs -UseBasicParsing -OutFile "$env:TEMP\rustup-init.exe"
    & "$env:TEMP\rustup-init.exe" -y --profile minimal
    # add cargo to this session's PATH
    $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
} else {
    Info "cargo already installed"
}

# ── install rome ─────────────────────────────────────────────────────────────
Info "building & installing rome v$VERSION..."
cargo install --git "https://github.com/$REPO" --tag "v$VERSION" --locked

Info "done. verifying..."
$rome = Join-Path $env:USERPROFILE ".cargo\bin\rome.exe"
if (Test-Path $rome) {
    Write-Host "==> rome installed: $rome" -ForegroundColor Green
} else {
    Warn "cargo installed rome but it isn't on PATH yet - start a new shell."
}

Write-Host @"

  -------------------------------------------------------------
   NEXT STEPS
   -----------
   1. Plug in the SP-1 (running marisko).
   2. rome format          <-- REQUIRED first time: initializes the
                             disk. do this BEFORE loading any music.
   3. rome song add "track" a.wav b.wav c.wav d.wav
  -------------------------------------------------------------
"@
