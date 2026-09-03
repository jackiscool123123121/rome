//! Shared core for rome: device protocol, disk format, firmware flashing, and
//! the audio-loading/encoding/upload pipeline. Used by both the `rome` CLI and
//! the `rome-gui` desktop app so neither duplicates the other's logic (and a
//! fix to one applies to both automatically).

pub mod adpcm;
pub mod disk;
pub mod flash;
pub mod proto;
pub mod wav;

use std::path::Path;

use anyhow::{bail, Context, Result};

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Linear interpolation resample from src_rate to 48000 Hz.
pub fn resample_to_48k(samples: &[i16], src_rate: u32) -> Vec<i16> {
    const DST_RATE: u32 = 48000;
    if src_rate == DST_RATE { return samples.to_vec(); }
    let ratio = DST_RATE as f64 / src_rate as f64;
    let dst_len = (samples.len() as f64 * ratio).ceil() as usize;
    let mut out = Vec::with_capacity(dst_len);
    for i in 0..dst_len {
        let src_pos = i as f64 / ratio;
        let idx = src_pos as usize;
        let frac = src_pos - idx as f64;
        let s0 = samples.get(idx).copied().unwrap_or(0) as f64;
        let s1 = samples.get(idx + 1).copied().unwrap_or(0) as f64;
        let v = (s0 + frac * (s1 - s0)).round() as i32;
        out.push(v.clamp(-32768, 32767) as i16);
    }
    out
}

/// Decode every channel of an audio file at its native sample rate (no
/// resampling yet -- callers resample per-channel after this, since the
/// combined-8ch path needs to know the channel count before deciding how much
/// silence to pad missing stems with). Returns (channels, sample_rate).
fn decode_all_channels(path: &Path) -> Result<(Vec<Vec<i16>>, u32)> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("cannot open {}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .with_context(|| format!("cannot probe {}", path.display()))?;

    let mut format = probed.format;
    let track = format.default_track()
        .ok_or_else(|| anyhow::anyhow!("{}: no audio track", path.display()))?;

    let sample_rate = track.codec_params.sample_rate
        .ok_or_else(|| anyhow::anyhow!("{}: unknown sample rate", path.display()))?;
    let n_channels = track.codec_params.channels
        .map(|c| c.count())
        .unwrap_or(2)
        .max(1);
    let track_id = track.id;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .with_context(|| format!("{}: unsupported codec", path.display()))?;

    let mut channels: Vec<Vec<i16>> = vec![Vec::new(); n_channels];

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymphoniaError::ResetRequired) => continue,
            Err(e) => return Err(e).context("decode error"),
        };
        if packet.track_id() != track_id { continue; }

        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SymphoniaError::IoError(_)) => break,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(e).context("decode error"),
        };

        let spec = *decoded.spec();
        let mut buf: SampleBuffer<i16> = SampleBuffer::new(decoded.capacity() as u64, spec);
        buf.copy_interleaved_ref(decoded);
        let samples = buf.samples();

        for frame in samples.chunks(n_channels) {
            for (ch, &s) in frame.iter().enumerate() {
                channels[ch].push(s);
            }
        }
    }

    if channels[0].is_empty() {
        bail!("{}: decoded no samples", path.display());
    }
    Ok((channels, sample_rate))
}

/// Load any audio file (WAV, FLAC, MP3, OGG…) and return (left, right) at 48 kHz.
/// Mono files are duplicated to stereo. Multi-channel files use ch 0 + ch 1 --
/// for a combined 8-channel SP-1-style file, use `load_combined_stem_wav`
/// instead. `resampled_from` is set to the source sample rate if it wasn't
/// already 48 kHz (callers decide whether/how to report that; the CLI prints
/// it, the GUI shows it in the UI instead of stderr).
pub fn load_audio_stereo(path: &Path) -> Result<(Vec<i16>, Vec<i16>, Option<u32>)> {
    let (channels, sample_rate) = decode_all_channels(path)?;
    let resampled_from = if sample_rate != 48000 { Some(sample_rate) } else { None };
    let left  = resample_to_48k(&channels[0], sample_rate);
    let right = resample_to_48k(channels.get(1).unwrap_or(&channels[0]), sample_rate);
    Ok((left, right, resampled_from))
}

/// Load a single combined multi-stem WAV per the SP-1 / solderless stem-loader
/// convention (see https://solderless.engineering/stemloader/help/#preparing-wav-files):
/// up to 8 channels mapped as ch1-2 = stem1 L/R, ch3-4 = stem2, ch5-6 = stem3,
/// ch7-8 = stem4. Fewer than 8 channels leaves the remaining stems silent
/// (matching the documented behavior: "the corresponding stems will be
/// empty"); more than 8 are ignored. Returns 4 (left, right) pairs at 48 kHz,
/// each padded to the same length, plus the source sample rate if resampled.
pub fn load_combined_stem_wav(path: &Path) -> Result<([(Vec<i16>, Vec<i16>); 4], Option<u32>)> {
    let (channels, sample_rate) = decode_all_channels(path)?;
    let resampled_from = if sample_rate != 48000 { Some(sample_rate) } else { None };
    let frame_count = channels[0].len();
    let silence = vec![0i16; frame_count];

    let get = |idx: usize| -> Vec<i16> {
        resample_to_48k(channels.get(idx).unwrap_or(&silence), sample_rate)
    };
    let stems: [(Vec<i16>, Vec<i16>); 4] = std::array::from_fn(|i| {
        (get(i * 2), get(i * 2 + 1))
    });
    Ok((stems, resampled_from))
}

// ── Device helpers ────────────────────────────────────────────────────────────

pub fn open_dev(port: Option<&str>) -> Result<proto::DeviceConn> {
    match port {
        Some(p) => proto::DeviceConn::open(p),
        None    => proto::DeviceConn::open_auto(),
    }
}

/// Serial port names, for the flash/bootloader picker (bootloader mode is a
/// plain serial protocol, unlike the running app's raw-USB CDC bind).
pub fn list_serial_ports() -> Result<Vec<String>> {
    Ok(serialport::available_ports()
        .context("failed to enumerate serial ports")?
        .into_iter()
        .map(|p| p.port_name)
        .collect())
}

// ── Encode + upload pipeline ────────────────────────────────────────────────
//
// Split in two so a caller (GUI or CLI) can show "encoding..." and
// "uploading..." as distinct phases, and so encoding (pure CPU, no device
// needed) can happen before/without a device connection at all.

/// One stem's source path, with whatever resampling note came back from
/// loading it (for UI display -- the CLI prints this to stderr, the GUI shows
/// it next to the file picker).
pub struct StemNote {
    pub path: std::path::PathBuf,
    pub frames: usize,
    pub resampled_from: Option<u32>,
}

pub struct EncodedSong {
    pub blocks: Vec<[u8; 512]>,
    pub level_blocks: Vec<[u8; 512]>,
    pub frames: usize,
    pub stems: [StemNote; 4],
}

/// Load, resample, and IMA-ADPCM-encode 4 stereo stems into the on-disk block
/// format (audio blocks + baked VU-level blocks), without touching a device.
/// Where a song's audio comes from: 4 separate stereo files (the original
/// rome workflow), or one combined multi-channel WAV per the SP-1 /
/// solderless stem-loader convention (see `load_combined_stem_wav`).
pub enum SongSource<'a> {
    FourStems([&'a Path; 4]),
    CombinedWav(&'a Path),
}

pub fn encode_song(source: SongSource) -> Result<EncodedSong> {
    let mut channel_pcm: [Vec<i16>; adpcm::CHANNELS] = std::array::from_fn(|_| Vec::new());
    let mut notes: Vec<StemNote> = Vec::with_capacity(4);

    match source {
        SongSource::FourStems(stems) => {
            for (stem_idx, path) in stems.iter().enumerate() {
                let (left, right, resampled_from) = load_audio_stereo(path)?;
                let n = left.len();
                let target = channel_pcm[0].len().max(n);
                for ch in channel_pcm.iter_mut() {
                    ch.resize(target, 0);
                }
                let ch_l = stem_idx * 2;
                let ch_r = stem_idx * 2 + 1;
                channel_pcm[ch_l] = left;
                channel_pcm[ch_l].resize(target, 0);
                channel_pcm[ch_r] = right;
                channel_pcm[ch_r].resize(target, 0);
                notes.push(StemNote { path: path.to_path_buf(), frames: n, resampled_from });
            }
        }
        SongSource::CombinedWav(path) => {
            let (stems, resampled_from) = load_combined_stem_wav(path)?;
            let max_len = stems.iter().map(|(l, _)| l.len()).max().unwrap_or(0);
            for (stem_idx, (left, right)) in stems.into_iter().enumerate() {
                let n = left.len();
                let ch_l = stem_idx * 2;
                let ch_r = stem_idx * 2 + 1;
                channel_pcm[ch_l] = left;
                channel_pcm[ch_l].resize(max_len, 0);
                channel_pcm[ch_r] = right;
                channel_pcm[ch_r].resize(max_len, 0);
                // All 4 "stems" share the one source file -- honestly reflect
                // that in the per-stem notes rather than inventing 4 fake paths.
                notes.push(StemNote { path: path.to_path_buf(), frames: n, resampled_from });
            }
        }
    }

    let max_len = channel_pcm.iter().map(|c| c.len()).max().unwrap_or(0);
    for ch in channel_pcm.iter_mut() {
        ch.resize(max_len, 0);
    }

    let blocks = adpcm::encode_8ch(&channel_pcm);
    let levels = adpcm::bake_stem_levels(&blocks);
    let level_blocks = adpcm::pack_levels(&levels);

    let stems_arr: [StemNote; 4] = notes.try_into().ok()
        .expect("exactly 4 stems were pushed above");

    Ok(EncodedSong { blocks, level_blocks, frames: max_len, stems: stems_arr })
}

/// Upload progress: blocks sent so far and the total (audio + level blocks).
#[derive(Clone, Copy)]
pub struct UploadProgress {
    pub blocks_sent: usize,
    pub blocks_total: usize,
}

/// Upload an already-encoded song to the device, calling `progress` after each
/// batch. Returns the catalog index the song landed at.
pub fn upload_encoded_song(
    dev: &mut proto::DeviceConn,
    name: &str,
    song: &EncodedSong,
    mut progress: impl FnMut(UploadProgress),
) -> Result<u16> {
    if name.len() > 23 {
        bail!("song name too long (max 23 chars)");
    }
    dev.ping().context("ping failed")?;

    let mut name_bytes = [0u8; 24];
    let nb = name.as_bytes();
    name_bytes[..nb.len().min(23)].copy_from_slice(&nb[..nb.len().min(23)]);

    let song_idx = dev.song_begin(
        &name_bytes,
        song.blocks.len() as u32,
        song.level_blocks.len() as u32,
    )?;

    let mut stream: Vec<[u8; 512]> = Vec::with_capacity(song.blocks.len() + song.level_blocks.len());
    stream.extend_from_slice(&song.blocks);
    stream.extend_from_slice(&song.level_blocks);

    const BATCH: usize = 96;
    let total = stream.len();
    let mut sent = 0usize;
    for chunk in stream.chunks(BATCH) {
        if let Err(e) = dev.song_multiblock(chunk) {
            return Err(e).with_context(|| {
                format!("upload failed after {sent} of {total} blocks ok")
            });
        }
        sent += chunk.len();
        progress(UploadProgress { blocks_sent: sent, blocks_total: total });
    }

    dev.song_commit()?;
    Ok(song_idx)
}

/// Read a song's raw audio blocks off the device (one CMD_READ_BLOCK per
/// block -- the firmware has no batch-read command) and decode them back to
/// four stereo PCM stems at 48 kHz, the rate they were stored at. Used to
/// pull an already-uploaded song's audio into a bundle for sharing.
pub fn read_song_stems(
    dev: &mut proto::DeviceConn,
    block_start: u32,
    block_count: u32,
    mut progress: impl FnMut(u32, u32),
) -> Result<[(Vec<i16>, Vec<i16>); 4]> {
    let mut blocks: Vec<[u8; 512]> = Vec::with_capacity(block_count as usize);
    for i in 0..block_count {
        blocks.push(dev.read_block(block_start + i)?);
        progress(i + 1, block_count);
    }
    let channels = adpcm::decode_8ch(&blocks);
    Ok(std::array::from_fn(|stem| {
        (channels[stem * 2].clone(), channels[stem * 2 + 1].clone())
    }))
}

// NOTE: there used to be a relaunch_elevated_macos() here that tried to fix
// a stuck USB claim by relaunching the GUI under `osascript ... with
// administrator privileges`. Removed: macOS's WindowServer refuses a root
// process a connection to the logged-in user's display session, so a
// relaunch-as-root GUI app can never show a window -- it silently launched
// an invisible root process and did nothing visible, every time. The actual
// fix (retrying the transient claim, since Apple's CDC driver just needs a
// moment to let go) lives in proto.rs's open_dev(). See that function's
// comment for what still requires a Terminal + sudo (the CLI has no window
// to lose, so elevation genuinely works there).
