# rome — one-line installer (Windows PowerShell)
# usage:  irm https://raw.githubusercontent.com/jackiscool123123121/rome/main/install.ps1 | iex
#
# installs Rust (if missing), then builds and installs the `rome` binary.
# libusb is compiled from source (vendored), so no system dependency is needed.
# afterwards run `rome format` before loading music.

$ErrorActionPreference = "Stop"
$VERSION = "0.2.0"
$REPO = "jackiscool123123121/rome"

$BANNER = @'
 _ __ ___  _ __ ___   ___
 | '__/ _ \| '_ ` _ \ / _ \
 | | | (_) | | | | | |  __/
 |_|  \___/|_| |_| |_|\___|
'@

function Info    { Write-Host "==> $args" -ForegroundColor Green }
function Warn    { Write-Host "==> $args" -ForegroundColor Yellow }
function Step    { Write-Host "STEP $script:stepNo/$script:NSTEPS  $args" -ForegroundColor DarkCyan }
function Banner  { Write-Host $BANNER -ForegroundColor Cyan }

$script:NSTEPS = 3
$script:stepNo = 0
Clear-Host
Banner
Write-Host "  teenage engineering sp-1 stem player - companion cli" -ForegroundColor DarkGray
Write-Host "  version $VERSION" -ForegroundColor DarkGray
Write-Host ""

# ── STEP 1: Rust toolchain ───────────────────────────────────────────────────
$script:stepNo++
Step "Rust toolchain"
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Info "cargo not found - installing Rust via rustup (minimal profile)..."
    Invoke-WebRequest https://sh.rustup.rs -UseBasicParsing -OutFile "$env:TEMP\rustup-init.exe"
    & "$env:TEMP\rustup-init.exe" -y --profile minimal
    # add cargo to this session's PATH
    $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
} else {
    Write-Host "cargo already installed" -ForegroundColor Cyan
}

# ── STEP 2: build & install rome ─────────────────────────────────────────────
$script:stepNo++
Step "Build & install rome"
Info "compiling rome v$VERSION (libusb vendored - first build takes a bit)..."
cargo install --git "https://github.com/$REPO" --tag "v$VERSION" --locked
Write-Host "-> rome installed" -ForegroundColor Cyan

# ── STEP 3: verify ───────────────────────────────────────────────────────────
$script:stepNo++
Step "Verify"
$rome = Join-Path $env:USERPROFILE ".cargo\bin\rome.exe"
if (Test-Path $rome) {
    Write-Host "OK rome ready: $rome" -ForegroundColor Green
} else {
    Warn "rome not on PATH - restart your shell or add ~/.cargo\bin."
}

Write-Host ""
Banner
Write-Host @"

  -------------------------------------------------------------
   NEXT STEPS
   -----------
   1. Plug in the SP-1 (running marisko).
   2. rome format        <-- REQUIRED first time: initializes the
                            disk. do this BEFORE loading any music.
   3. rome song add "track" a.wav b.wav c.wav d.wav

   marisko docs: https://github.com/jackiscool123123121/marisko
  -------------------------------------------------------------
"@
