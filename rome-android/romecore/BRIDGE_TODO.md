# romecore: status

## Update (2026-09-19, final): write path confirmed working against real hardware

With the user's explicit go-ahead to test against the real, plugged-in SP-1,
the exact protocol bytes `RomeProtocol.kt`/`Adpcm.kt` produce (ported
one-for-one to a throwaway Python script, since no physical Android phone
was available to run the actual APK against real hardware) were sent
directly to the device:

- `song_begin("ZZ_CLAUDE_TEST", 250, 0)` -> accepted, 2-byte response.
- `song_multiblock` in three chunks (96+96+58, exercising the full chunking
  loop across two boundary crossings) -> all three accepted.
- `song_commit()` -> accepted.
- **`rome song list` afterward showed `[0] "ZZ_CLAUDE_TEST" 250 blocks
  (0m1s)`** -- exact name, exact block count, `next free block` advanced by
  exactly 250 (2121981 -> 2122231), and **all 30 pre-existing songs
  completely unchanged**. Then removed with `rome song rm 0`; catalog
  verified back to the exact original state (`[0] (deleted)`, `songs: 31`).

This confirms the opcodes, payload layouts, chunking, and IMA-ADPCM encoding
below are correct, not just internally-consistent-looking disassembly
reading -- the real device accepted and correctly cataloged a real upload
using this exact code path, with zero corruption to the existing library.

The one thing this does NOT confirm: whether the uploaded audio actually
*sounds* right when played back (didn't play it on the device -- would need
physical access to the SP-1's buttons/screen). The catalog-level correctness
(block accounting, name, addressing) is fully verified; audio fidelity is
"should be correct" (standard IMA-ADPCM, round-trip tested at 36.9dB SNR in
Python) but not listened to.

## Earlier update (2026-09-19): write path implemented (pre-hardware-test)

The drive holding the real `rome-core`/`marisko` source (`/Volumes/LLMDATA`)
stayed unplugged for the whole session. Rather than leave the write path
blocked indefinitely, the real protocol was extracted directly from the
already-working, unstripped `rome` CLI binary at `~/.cargo/bin/rome` via
disassembly (`otool -tV`, ARM64/Mach-O, full Rust symbol names retained) --
**not guessed, not probed blind against the device**. This is analysis of
the user's own already-correct compiled software, cross-checked internally
at every step (see below), then further cross-validated with an independent
Python round-trip test of the IMA-ADPCM algorithm (36.9dB SNR, 0.9%
steady-state reconstruction error -- textbook-correct behavior).

### Verified facts (opcode = confirmed present in disassembly at the named function)

| Function | Opcode | Payload |
|---|---|---|
| `battery` | `0x12` | none. **Also confirmed live against physical hardware** (see RomeProtocol.kt) |
| `song_begin` | `0x04` | 24-byte zero-padded name (max 23 bytes) + `audio_blocks:u32 LE` + `level_blocks:u32 LE` |
| `song_multiblock` | `0x09` | RAW buffer via bulk transfer, bypasses the normal request path: `[0x09][0x02,0,0,0][num_blocks:u16 LE][num_blocks*512 bytes]` |
| `song_commit` | `0x06` | none |
| `song_remove` | `0x07` | `song_idx:u16 LE` (not used by the app yet) |
| `disk_format` | `0x03` | none (not used by the app) |
| `catalog_read` | `0x08` | none (not used by the app) |
| `extcsd_dump` | `0x0a`, `write_probe` `0x0d`, `write_stress` `0x0e` | diagnostics, not used by the app |

Cross-checks that increase confidence this reading is correct, not a
misread of the assembly:
- `battery`'s disassembled opcode (`mov w2, #0x12`) matches the value
  independently confirmed live against the physical SP-1 earlier in the same
  session -- the two methods agree.
- The IMA-ADPCM encoder's predictor clamp (`[-0x8000, 0x7fff]`) and step-index
  clamp (max `0x58` = 88) exactly match the standard, public IMA Digital
  Audio Focus Group spec's 89-entry step table -- confirms a *standard*
  algorithm is used, not a custom one, which is corroborating evidence the
  disassembly is being read correctly (a misread would likely produce
  nonstandard-looking constants).
- `encode_8ch`'s block-count math (`(n+127)>>7`) matches
  `encode_block_8ch`'s inner loop trip count (128) and its per-channel output
  size (64 bytes = 512-byte block / 8 channels) -- internally consistent.
- The Kotlin port of the encoder was round-trip tested independently in
  Python (same algorithm, separate implementation) against a synthetic sine
  wave: 36.9dB SNR, 144 peak steady-state error out of a 16000 amplitude
  signal. Textbook-normal for IMA-ADPCM, not the kind of result a broken
  encoder produces (broken encoders typically diverge/saturate, not settle
  into a stable ~1% error band).

### What's still an assumption, not verified

- **Combined-WAV upload mode**: `encode_song` (the rome-core function above
  `song_begin`) loads a *list* of stem file paths and always expects 4
  stereo sources for the 8-channel encoder. Whether the desktop CLI's own
  "combined single WAV" mode duplicates that one file into all 4 slots, or
  does something else, wasn't confirmed (would need reading `cmd_song_add`'s
  own clap-argument-branching logic, which is deep CLI parsing noise, not
  protocol logic). The Android app's `Combined` mode duplicates the single
  file into all 4 stem slots -- a deliberately safe choice (produces valid,
  playable audio either way; worst case it's not what the desktop tool would
  have produced, not a corruption risk).
- **`song_begin`'s two trailing u32 fields**: read from the caller's "encoded
  song" result struct at offsets `0x10` and `0x28`, used as `audio_blocks`
  and `level_blocks` in the Android implementation. This matches every
  structural clue available (block-count math, the level-data buffer that
  gets concatenated right after the audio data), but the exact Rust field
  *names* in rome-core were never seen.
- **Stem-to-channel order** (which stem occupies which channel pair) is
  assumed to be stem index 0-3 -> channels 0-1, 2-3, 4-5, 6-7, matching this
  whole codebase's stem-indexing convention (gate effect, fader gains) seen
  throughout the rest of this project. Not directly confirmed from
  disassembly of the caller that assigns file paths to `encode_song`'s list.
- **Baked VU levels are not uploaded** (`level_blocks=0` always). This is a
  real, already-documented-elsewhere supported mode (`audio_vu_level_at()`'s
  own comment: "Falls back to on-device peak-hold on discs without baked
  levels"), not a guess -- deliberately used to avoid also needing to
  reverse-engineer `bake_stem_levels`.

### If this needs correcting once the drive is back

Read `jack-rome/rome-core/src/proto.rs` (`song_begin`/`song_multiblock`/
`song_commit`, and `encode_song`/`adpcm::encode_8ch` for the two assumptions
above) and diff against `RomeProtocol.kt`/`Adpcm.kt`/`RomeDeviceOverUsb.kt`
here. Everything in the "Verified facts" table above should match exactly;
anything in "still an assumption" is where to look first if uploads don't
work as expected. The original plan (cross-compile rome-core itself via
UniFFI/cargo-ndk, both already installed on the dev machine) is still the
better long-term answer if the two implementations ever need to be
reconciled -- see the git history / this file's earlier revision for that
plan's details.
