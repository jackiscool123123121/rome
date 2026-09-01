// Background jobs: all device I/O runs on a short-lived thread per job
// (never the UI thread), reporting back over an mpsc channel the UI polls
// each frame. rome_core::proto::DeviceConn binds directly to the device by
// USB VID/PID (port name only matters for the separate bootloader-mode
// serial protocol used by Flash), so most jobs need no port argument at all.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use eframe::egui;
use rome_core::disk::{DiskHeader, SongEntry};

use crate::bundle::{self, BundleSource};

// ── Song input (owned paths; borrowed into rome_core::SongSource at encode
// time) -- lets the queue hold either input mode uniformly. ─────────────────

#[derive(Clone, Copy, PartialEq)]
pub enum AddMode { FourStems, CombinedWav }

#[derive(Clone)]
pub enum SongInput {
    FourStems([PathBuf; 4]),
    CombinedWav(PathBuf),
}

impl SongInput {
    pub fn as_source(&self) -> rome_core::SongSource<'_> {
        match self {
            SongInput::FourStems(paths) => {
                let refs: [&Path; 4] = std::array::from_fn(|i| paths[i].as_path());
                rome_core::SongSource::FourStems(refs)
            }
            SongInput::CombinedWav(path) => rome_core::SongSource::CombinedWav(path.as_path()),
        }
    }

    pub fn mode_label(&self) -> &'static str {
        match self {
            SongInput::FourStems(_) => "4 stems",
            SongInput::CombinedWav(_) => "combined WAV",
        }
    }
}

pub enum Job {
    Refresh,
    TransferQueue { items: Vec<(String, SongInput)> },
    SaveBundle { items: Vec<(String, BundleSource)>, dest: PathBuf },
    RemoveSong { idx: u16 },
    SwapSongs { idx_a: u16, idx_b: u16 },
    Format,
    Bootloader,
    Flash { port: String, firmware: PathBuf },
    Diagnostics,
}

pub enum JobMsg {
    Info { header: DiskHeader, songs: Vec<SongEntry> },
    Progress { frac: f32, eta_secs: Option<f32> },
    Log(String),
    Diagnostics(String),
    Done,
    Error(String),
}

/// Encode + upload a single song, reporting log/progress messages as it goes.
/// Used both for a lone queue entry and for each item of a TransferQueue run.
fn upload_one(
    name: &str,
    input: &SongInput,
    tx: &mpsc::Sender<JobMsg>,
    ctx: &egui::Context,
) -> anyhow::Result<u16> {
    let send = |m: JobMsg| { let _ = tx.send(m); ctx.request_repaint(); };

    send(JobMsg::Log(format!("loading \"{name}\" ({})...", input.mode_label())));
    let song = rome_core::encode_song(input.as_source())?;
    send(JobMsg::Log(format!("encoded {} blocks, uploading \"{name}\"...", song.blocks.len())));

    let mut dev = rome_core::open_dev(None)?;
    let start = std::time::Instant::now();
    let idx = rome_core::upload_encoded_song(&mut dev, name, &song, |p| {
        let frac = p.blocks_sent as f32 / p.blocks_total.max(1) as f32;
        let elapsed = start.elapsed().as_secs_f32();
        let eta_secs = if p.blocks_sent > 0 && elapsed > 0.0 {
            let rate = p.blocks_sent as f32 / elapsed; // blocks/sec
            Some(((p.blocks_total - p.blocks_sent) as f32 / rate).max(0.0))
        } else {
            None
        };
        let _ = tx.send(JobMsg::Progress { frac, eta_secs });
        ctx.request_repaint();
    })?;
    send(JobMsg::Log(format!("\"{name}\" uploaded -- catalog index {idx}")));
    Ok(idx)
}

pub fn format_eta(secs: f32) -> String {
    let secs = secs.round() as u32;
    if secs >= 60 {
        format!("{}m{:02}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    }
}

pub fn run_job(job: Job, tx: mpsc::Sender<JobMsg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let send = |m: JobMsg| {
            let _ = tx.send(m);
            ctx.request_repaint();
        };
        match job {
            Job::Refresh => match fetch_info() {
                Ok((header, songs)) => send(JobMsg::Info { header, songs }),
                Err(e) => send(JobMsg::Error(format!("{e:#}"))),
            },
            Job::TransferQueue { items } => {
                let total = items.len();
                let mut had_error = false;
                for (i, (name, input)) in items.iter().enumerate() {
                    send(JobMsg::Progress { frac: 0.0, eta_secs: None });
                    send(JobMsg::Log(format!("[{}/{}] uploading \"{}\"...", i + 1, total, name)));
                    if let Err(e) = upload_one(name, input, &tx, &ctx) {
                        had_error = true;
                        send(JobMsg::Log(format!(
                            "[{}/{}] \"{}\" FAILED: {e:#}", i + 1, total, name
                        )));
                    }
                }
                match fetch_info() {
                    Ok((header, songs)) => send(JobMsg::Info { header, songs }),
                    Err(e) => send(JobMsg::Error(format!("{e:#}"))),
                }
                send(JobMsg::Log(if had_error {
                    "queue transfer finished with errors -- see log above".to_string()
                } else {
                    format!("queue transfer complete -- {total} song(s) uploaded")
                }));
            }
            Job::SaveBundle { items, dest } => {
                match bundle::save_bundle(&dest, &items, &tx, &ctx) {
                    Ok(()) => send(JobMsg::Log(format!("bundle saved: {}", dest.display()))),
                    Err(e) => send(JobMsg::Error(format!("{e:#}"))),
                }
            }
            Job::RemoveSong { idx } => {
                let result = (|| -> anyhow::Result<()> {
                    let mut dev = rome_core::open_dev(None)?;
                    dev.song_remove(idx)
                })();
                match result {
                    Ok(()) => match fetch_info() {
                        Ok((header, songs)) => send(JobMsg::Info { header, songs }),
                        Err(e) => send(JobMsg::Error(format!("{e:#}"))),
                    },
                    Err(e) => send(JobMsg::Error(format!("{e:#}"))),
                }
            }
            Job::SwapSongs { idx_a, idx_b } => {
                let result = (|| -> anyhow::Result<()> {
                    let mut dev = rome_core::open_dev(None)?;
                    dev.song_swap(idx_a, idx_b)
                })();
                match result {
                    Ok(()) => match fetch_info() {
                        Ok((header, songs)) => send(JobMsg::Info { header, songs }),
                        Err(e) => send(JobMsg::Error(format!("{e:#}"))),
                    },
                    Err(e) => send(JobMsg::Error(format!("{e:#}"))),
                }
            }
            Job::Format => {
                let result = (|| -> anyhow::Result<()> {
                    let mut dev = rome_core::open_dev(None)?;
                    dev.disk_format()
                })();
                match result {
                    Ok(()) => {
                        send(JobMsg::Log("disk formatted".into()));
                        match fetch_info() {
                            Ok((header, songs)) => send(JobMsg::Info { header, songs }),
                            Err(e) => send(JobMsg::Error(format!("{e:#}"))),
                        }
                    }
                    Err(e) => send(JobMsg::Error(format!("{e:#}"))),
                }
            }
            Job::Bootloader => {
                let result = (|| -> anyhow::Result<()> {
                    let mut dev = rome_core::open_dev(None)?;
                    dev.power_off()
                })();
                match result {
                    Ok(()) => send(JobMsg::Log(
                        "device powering off -- press function to wake it into the bootloader"
                            .into(),
                    )),
                    Err(e) => send(JobMsg::Error(format!("{e:#}"))),
                }
            }
            Job::Flash { port, firmware } => {
                // flash::run() prints its own progress via an indicatif bar to
                // this process's stderr (invisible in a windowed app) and does
                // real, safety-relevant bootloader protocol work -- reusing it
                // as-is rather than reimplementing avoids touching tested flash
                // logic just for a GUI progress bar. The GUI shows a spinner
                // instead of a precise percentage for this one operation.
                match rome_core::flash::run(Some(&port), Some(&firmware), false, false) {
                    Ok(()) => send(JobMsg::Log("flash complete -- device will restart".into())),
                    Err(e) => send(JobMsg::Error(format!("{e:#}"))),
                }
            }
            Job::Diagnostics => {
                let result = (|| -> anyhow::Result<String> {
                    let mut dev = rome_core::open_dev(None)?;
                    dev.ping()?;
                    let diag = dev.audio_diag()?;
                    let codec = dev.codec_diag()?;
                    Ok(format!(
                        "Feed thread:\n  recoveries={} write_fails={} max_read_us={} \
                         crc_errors={} blocks_fed={}\n  ain0={} ain1={}\n\n\
                         Codec:\n  init_ok={} i2c_errors={}\n  CS42L42 PLL lock={:#04x} \
                         HP_CTL={:#04x}\n",
                        diag[0], diag[1], diag[2], diag[6], diag[5], diag[7], diag[8],
                        codec[0], codec[12], codec[14], codec[24],
                    ))
                })();
                match result {
                    Ok(text) => send(JobMsg::Diagnostics(text)),
                    Err(e) => send(JobMsg::Error(format!("{e:#}"))),
                }
            }
        }
        send(JobMsg::Done);
    });
}

pub fn fetch_info() -> anyhow::Result<(DiskHeader, Vec<SongEntry>)> {
    let mut dev = rome_core::open_dev(None)?;
    dev.ping()?;
    let raw = dev.disk_info()?;
    let header = DiskHeader::from_block(&raw);
    if !header.is_valid() {
        return Ok((header, Vec::new()));
    }
    if header.song_count == 0 {
        return Ok((header, Vec::new()));
    }
    let catalog = dev.catalog_read()?;
    Ok((header, rome_core::disk::parse_catalog(&catalog)))
}
