#!/usr/bin/env sh
# Build rome-gui + rome (release) and package them together as a folder,
# with the CLI binary bundled alongside the GUI so rome-gui's self-install
# can find and install it. Produces dist/RomeGUI-windows.zip. Run from
# git-bash (present on GitHub's windows-latest runners).
set -eu

cd "$(dirname "$0")/.."

echo "==> building release binaries"
cargo build --release -p rome -p rome-gui

OUT="dist/RomeGUI-windows"
rm -rf "$OUT"
mkdir -p "$OUT"

cp target/release/rome-gui.exe "$OUT/rome-gui.exe"
cp target/release/rome.exe     "$OUT/rome.exe"

echo "==> zipping"
rm -f dist/RomeGUI-windows.zip
powershell.exe -NoProfile -Command \
    "Compress-Archive -Path 'dist/RomeGUI-windows/*' -DestinationPath 'dist/RomeGUI-windows.zip'"

echo "==> done: dist/RomeGUI-windows.zip"
