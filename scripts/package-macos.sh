#!/usr/bin/env sh
# Build rome-gui + rome (release) and package them as RomeGUI.app, with the
# CLI binary bundled alongside the GUI so rome-gui's self-install can find
# and install it. Produces dist/RomeGUI-macos.zip.
set -eu

cd "$(dirname "$0")/.."

echo "==> building release binaries"
cargo build --release -p rome -p rome-gui

APP="dist/RomeGUI.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

cp target/release/rome-gui "$APP/Contents/MacOS/rome-gui"
cp target/release/rome     "$APP/Contents/MacOS/rome"
chmod +x "$APP/Contents/MacOS/rome-gui" "$APP/Contents/MacOS/rome"

cat > "$APP/Contents/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>            <string>rome</string>
    <key>CFBundleDisplayName</key>     <string>rome</string>
    <key>CFBundleIdentifier</key>      <string>com.jackiscool.rome</string>
    <key>CFBundleVersion</key>         <string>0.3.0</string>
    <key>CFBundleShortVersionString</key> <string>0.3.0</string>
    <key>CFBundleExecutable</key>      <string>rome-gui</string>
    <key>CFBundlePackageType</key>     <string>APPL</string>
    <key>LSMinimumSystemVersion</key>  <string>11.0</string>
    <key>NSHighResolutionCapable</key> <true/>
</dict>
</plist>
EOF

echo "==> ad-hoc codesigning"
codesign --force --deep --sign - "$APP"

echo "==> zipping"
mkdir -p dist
(cd dist && rm -f RomeGUI-macos.zip && zip -qry RomeGUI-macos.zip RomeGUI.app)

echo "==> done: dist/RomeGUI-macos.zip"
