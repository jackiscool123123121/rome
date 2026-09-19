# rome-android

Android companion app for the SP-1 stem player: battery status and song
upload over USB-OTG. Sibling to the desktop `rome-cli`/`rome-gui` in this
repo, but does **not** wrap `rome-core` — see "Architecture decision" below
for why.

## Modules

- `:romecore` — device layer: USB permission/transport (`usb/`), the wire
  protocol (`RomeProtocol.kt`), and the `RomeDevice` API the UI talks to.
  Has no Android-UI dependencies, only `android.hardware.usb.*` + coroutines.
- `:app` — Jetpack Compose UI (battery screen, upload screen, connect flow).

## Architecture decision: hand-ported protocol, not a Rust/UniFFI bridge

The original plan was to reuse `rome-core` directly: refactor it to accept a
`Transport` trait, cross-compile for Android with `cargo-ndk`, and bridge to
Kotlin via UniFFI callback interfaces. That's still the arguably-cleaner
long-term answer (one tested implementation instead of two) — it just wasn't
possible this session, because `/Volumes/LLMDATA` (holding the actual
`rome-core` source) was unplugged the whole time.

Instead, both BATTERY (read) and song upload (write) are hand-ported to
Kotlin against the *real* protocol:
- BATTERY's wire framing was confirmed directly against the physical SP-1
  (two live probes, see `RomeProtocol.kt`'s doc comment).
- Song upload's opcodes, payload layouts, and the IMA-ADPCM encoding format
  were extracted by disassembling the real, unstripped `rome` CLI binary
  (`~/.cargo/bin/rome`) — not reverse-engineered by guessing or probing the
  device blind. Full method, verified-vs-assumed breakdown, and cross-checks
  (including an independent Python round-trip test of the encoder: 36.9dB
  SNR) are in `romecore/BRIDGE_TODO.md`.

If the two implementations (this one and the real `rome-core`) ever need to
be reconciled, `BRIDGE_TODO.md` has the exact diff points to check first.

## Building

```
export JAVA_HOME=$(brew --prefix openjdk@17)   # AGP needs 17+, not the system default
export ANDROID_HOME=/opt/homebrew/share/android-commandlinetools
./gradlew :app:assembleDebug
```

## Running on the emulator

The Android emulator has **no USB-host passthrough** for arbitrary attached
devices (verified, not assumed, while building this) — it cannot see the
real SP-1 plugged into the host Mac. Use "Use Demo Device" on the connect
screen to exercise the full UI/navigation with simulated data. Real hardware
validation needs a physical Android phone with USB-OTG, connected directly
to the SP-1.

```
$ANDROID_HOME/emulator/emulator -avd rome_test &
adb install -r app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n com.jackiscool.rome/.MainActivity
```
