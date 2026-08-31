//! Shared core for rome: device protocol, disk format, firmware flashing, and
//! the audio-loading/encoding/upload pipeline. Used by both the `rome` CLI and
//! the `rome-gui` desktop app so neither duplicates the other's logic (and a
//! fix to one applies to both automatically).

pub mod adpcm;
pub mod disk;
pub mod flash;
pub mod proto;

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

/// Load any audio file (WAV, FLAC, MP3, OGG…) and return (left, right) at 48 kHz.
/// Mono files are duplicated to stereo. Multi-channel files use ch 0 + ch 1.
/// `resampled_from` is set to the source sample rate if it wasn't already 48 kHz
/// (callers decide whether/how to report that; the CLI prints it, the GUI shows
/// it in the UI instead of stderr).
pub fn load_audio_stereo(path: &Path) -> Result<(Vec<i16>, Vec<i16>, Option<u32>)> {
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
        .unwrap_or(2);
    let track_id = track.id;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .with_context(|| format!("{}: unsupported codec", path.display()))?;

    let mut left_raw: Vec<i16> = Vec::new();
    let mut right_raw: Vec<i16> = Vec::new();

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

        let ch = n_channels.max(1);
        for frame in samples.chunks(ch) {
            left_raw.push(frame[0]);
            right_raw.push(if ch >= 2 { frame[1] } else { frame[0] });
        }
    }

    if left_raw.is_empty() {
        bail!("{}: decoded no samples", path.display());
    }

    let resampled_from = if sample_rate != 48000 { Some(sample_rate) } else { None };
    let left  = resample_to_48k(&left_raw, sample_rate);
    let right = resample_to_48k(&right_raw, sample_rate);
    Ok((left, right, resampled_from))
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
pub fn encode_song(stems: [&Path; 4]) -> Result<EncodedSong> {
    let mut channel_pcm: [Vec<i16>; adpcm::CHANNELS] = std::array::from_fn(|_| Vec::new());
    let mut notes: Vec<StemNote> = Vec::with_capacity(4);

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
