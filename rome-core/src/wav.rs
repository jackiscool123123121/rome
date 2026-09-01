//! Minimal PCM16 stereo WAV writer (no external crate needed for the one
//! format rome ever needs to produce: 16-bit, N-channel, little-endian PCM).

use std::io::Write;
use std::path::Path;

use anyhow::Result;

/// Write interleaved 16-bit stereo PCM to a standard RIFF/WAVE file.
pub fn write_stereo_i16(path: &Path, left: &[i16], right: &[i16], sample_rate: u32) -> Result<()> {
    let frames = left.len().min(right.len());
    let data_len = frames * 2 * 2; // frames * channels * bytes_per_sample
    let byte_rate = sample_rate * 2 * 2;

    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    f.write_all(b"RIFF")?;
    f.write_all(&((36 + data_len) as u32).to_le_bytes())?;
    f.write_all(b"WAVE")?;

    f.write_all(b"fmt ")?;
    f.write_all(&16u32.to_le_bytes())?;       // fmt chunk size
    f.write_all(&1u16.to_le_bytes())?;        // PCM
    f.write_all(&2u16.to_le_bytes())?;        // channels
    f.write_all(&sample_rate.to_le_bytes())?;
    f.write_all(&byte_rate.to_le_bytes())?;
    f.write_all(&4u16.to_le_bytes())?;        // block align (channels * bytes_per_sample)
    f.write_all(&16u16.to_le_bytes())?;       // bits per sample

    f.write_all(b"data")?;
    f.write_all(&(data_len as u32).to_le_bytes())?;
    for i in 0..frames {
        f.write_all(&left[i].to_le_bytes())?;
        f.write_all(&right[i].to_le_bytes())?;
    }
    f.flush()?;
    Ok(())
}
