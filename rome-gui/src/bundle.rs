// A "bundle" packs multiple songs -- in a fixed order -- into one .zip for
// sharing with someone else's rome install. Each song's 4 stereo stems are
// stored as WAV files, regenerated from whatever the source actually was:
// the original local files, a combined multi-stem WAV split into 4, or (for
// a song already sitting on the device) audio read back and ADPCM-decoded.
// A manifest.json records the song order and names so importing rebuilds
// the transfer queue exactly as it was assembled.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use eframe::egui;

use crate::jobs::{JobMsg, SongInput};

/// Where a bundle entry's audio comes from at build time -- resolved lazily,
/// only when the bundle .zip is actually written (device readback in
/// particular is slow, so it must not happen just from adding to the list).
#[derive(Clone)]
pub enum BundleSource {
    Local(SongInput),
    Device { block_start: u32, block_count: u32 },
}

impl BundleSource {
    pub fn label(&self) -> &'static str {
        match self {
            BundleSource::Local(SongInput::FourStems(_)) => "4 stems",
            BundleSource::Local(SongInput::CombinedWav(_)) => "combined WAV",
            BundleSource::Device { .. } => "on device",
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct BundleManifest {
    songs: Vec<BundleSongManifest>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct BundleSongManifest {
    name: String,
    stems: [String; 4],
}

/// Build a multi-song bundle .zip, in order. Runs on a background thread;
/// reports log/progress messages as it goes since device readback and WAV
/// writing can both take a while for a full song.
pub fn save_bundle(
    dest: &Path,
    items: &[(String, BundleSource)],
    tx: &mpsc::Sender<JobMsg>,
    ctx: &egui::Context,
) -> anyhow::Result<()> {
    let send = |m: JobMsg| { let _ = tx.send(m); ctx.request_repaint(); };

    let file = std::fs::File::create(dest)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts: zip::write::FileOptions<()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let tmp = std::env::temp_dir().join(format!("rome-bundle-build-{}", std::process::id()));
    std::fs::create_dir_all(&tmp)?;

    let mut manifest = BundleManifest { songs: Vec::with_capacity(items.len()) };

    for (i, (name, source)) in items.iter().enumerate() {
        send(JobMsg::Progress { frac: 0.0, eta_secs: None });
        send(JobMsg::Log(format!("[{}/{}] adding \"{}\" to bundle...", i + 1, items.len(), name)));
        let folder = format!("song_{i:03}");

        let stem_paths: [PathBuf; 4] = match source {
            BundleSource::Local(SongInput::FourStems(paths)) => {
                // Copy originals byte-for-byte -- no re-encode needed, format preserved.
                let dests: [PathBuf; 4] = std::array::from_fn(|s| {
                    let ext = paths[s].extension().and_then(|e| e.to_str()).unwrap_or("wav");
                    tmp.join(format!("{folder}_stem{}.{}", s + 1, ext))
                });
                for (src, dst) in paths.iter().zip(dests.iter()) {
                    std::fs::copy(src, dst)?;
                }
                dests
            }
            BundleSource::Local(SongInput::CombinedWav(path)) => {
                let (stems, _) = rome_core::load_combined_stem_wav(path)?;
                write_stem_wavs(&tmp, &folder, &stems)?
            }
            BundleSource::Device { block_start, block_count } => {
                let mut dev = rome_core::open_dev(None)?;
                let tx2 = tx.clone();
                let ctx2 = ctx.clone();
                let stems = rome_core::read_song_stems(
                    &mut dev, *block_start, *block_count,
                    move |done, total| {
                        let _ = tx2.send(JobMsg::Progress {
                            frac: done as f32 / total.max(1) as f32,
                            eta_secs: None,
                        });
                        ctx2.request_repaint();
                    },
                )?;
                write_stem_wavs(&tmp, &folder, &stems)?
            }
        };

        let filenames: [String; 4] = std::array::from_fn(|s| {
            stem_paths[s].file_name().unwrap().to_string_lossy().to_string()
        });
        for (path, filename) in stem_paths.iter().zip(filenames.iter()) {
            zip.start_file(filename, opts)?;
            let mut f = std::fs::File::open(path)?;
            std::io::copy(&mut f, &mut zip)?;
        }
        manifest.songs.push(BundleSongManifest { name: name.clone(), stems: filenames });
    }

    zip.start_file("manifest.json", opts)?;
    zip.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;
    zip.finish()?;

    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}

fn write_stem_wavs(
    tmp: &Path,
    folder: &str,
    stems: &[(Vec<i16>, Vec<i16>); 4],
) -> anyhow::Result<[PathBuf; 4]> {
    let dests: [PathBuf; 4] = std::array::from_fn(|s| tmp.join(format!("{folder}_stem{}.wav", s + 1)));
    for (s, (left, right)) in stems.iter().enumerate() {
        rome_core::wav::write_stereo_i16(&dests[s], left, right, 48000)?;
    }
    Ok(dests)
}

/// Unzip a bundle and return its songs in order, each as (name, 4 stem
/// paths) ready to push onto the transfer queue.
pub fn load_bundle(src: &Path) -> anyhow::Result<Vec<(String, [PathBuf; 4])>> {
    let file = std::fs::File::open(src)?;
    let mut zip = zip::ZipArchive::new(file)?;

    let mut manifest_str = String::new();
    zip.by_name("manifest.json")?.read_to_string(&mut manifest_str)?;
    let manifest: BundleManifest = serde_json::from_str(&manifest_str)?;

    let tmp = std::env::temp_dir().join(format!("rome-bundle-{}", std::process::id()));
    std::fs::create_dir_all(&tmp)?;

    let mut out = Vec::with_capacity(manifest.songs.len());
    for song in &manifest.songs {
        let mut out_paths: Vec<PathBuf> = Vec::with_capacity(4);
        for filename in &song.stems {
            let mut entry = zip.by_name(filename)?;
            let out_path = tmp.join(filename);
            let mut f = std::fs::File::create(&out_path)?;
            std::io::copy(&mut entry, &mut f)?;
            out_paths.push(out_path);
        }
        let stems: [PathBuf; 4] = out_paths.try_into().map_err(|_| {
            anyhow::anyhow!("bundle manifest song \"{}\" did not list exactly 4 stems", song.name)
        })?;
        out.push((song.name.clone(), stems));
    }
    Ok(out)
}
