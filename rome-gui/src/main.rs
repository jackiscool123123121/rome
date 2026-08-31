// rome-gui: desktop companion app for the SP-1 stem player.
//
// All device I/O runs on a short-lived background thread per job (never on
// the UI thread), reporting back over an mpsc channel that the UI polls each
// frame. rome_core::proto::DeviceConn binds directly to the device by USB
// VID/PID (port name is only meaningful for the separate bootloader-mode
// serial protocol used by Flash), so most jobs need no port argument at all.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use eframe::egui;
use rome_core::disk::{DiskHeader, SongEntry};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([620.0, 720.0])
            .with_min_inner_size([480.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        "rome",
        options,
        Box::new(|cc| Ok(Box::new(RomeApp::new(cc)))),
    )
}

// ── Bundle format (for "send to others") ────────────────────────────────────
//
// A bundle is a .zip containing the 4 original stem files plus manifest.json.
// This does NOT read audio back off the device -- it's produced at upload
// time, from the same local files the user is about to send to their own
// device, so sharing it with someone else costs nothing extra and needs no
// new firmware support. A song already sitting on someone's device with the
// original files long gone can't be bundled this way; that would need a
// bulk device-readback command that doesn't exist yet.

#[derive(serde::Serialize, serde::Deserialize)]
struct BundleManifest {
    name: String,
    stems: [String; 4],
}

fn save_bundle(dest: &Path, name: &str, stems: &[PathBuf; 4]) -> anyhow::Result<()> {
    let file = std::fs::File::create(dest)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts: zip::write::FileOptions<()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let filenames: [String; 4] = std::array::from_fn(|i| {
        stems[i]
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| format!("stem{}.wav", i + 1))
    });
    let manifest = BundleManifest { name: name.to_string(), stems: filenames.clone() };
    zip.start_file("manifest.json", opts)?;
    zip.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;

    for (path, filename) in stems.iter().zip(filenames.iter()) {
        zip.start_file(filename, opts)?;
        let mut f = std::fs::File::open(path)?;
        std::io::copy(&mut f, &mut zip)?;
    }
    zip.finish()?;
    Ok(())
}

/// Unzip a bundle into a fresh temp dir and return (name, 4 stem paths).
fn load_bundle(src: &Path) -> anyhow::Result<(String, [PathBuf; 4])> {
    let file = std::fs::File::open(src)?;
    let mut zip = zip::ZipArchive::new(file)?;

    let mut manifest_str = String::new();
    zip.by_name("manifest.json")?.read_to_string(&mut manifest_str)?;
    let manifest: BundleManifest = serde_json::from_str(&manifest_str)?;

    let tmp = std::env::temp_dir().join(format!("rome-bundle-{}", std::process::id()));
    std::fs::create_dir_all(&tmp)?;

    let mut out_paths: Vec<PathBuf> = Vec::with_capacity(4);
    for filename in &manifest.stems {
        let mut entry = zip.by_name(filename)?;
        let out_path = tmp.join(filename);
        let mut out = std::fs::File::create(&out_path)?;
        std::io::copy(&mut entry, &mut out)?;
        out_paths.push(out_path);
    }
    let stems: [PathBuf; 4] = out_paths
        .try_into()
        .map_err(|_| anyhow::anyhow!("bundle manifest did not list exactly 4 stems"))?;
    Ok((manifest.name, stems))
}

// ── Background jobs ──────────────────────────────────────────────────────────

enum Job {
    Refresh,
    AddSong { name: String, stems: [PathBuf; 4] },
    RemoveSong { idx: u16 },
    SwapSongs { idx_a: u16, idx_b: u16 },
    Format,
    Bootloader,
    Flash { port: String, firmware: PathBuf },
    Diagnostics,
}

enum JobMsg {
    Info { header: DiskHeader, songs: Vec<SongEntry> },
    Progress(f32),
    Log(String),
    Diagnostics(String),
    Done,
    Error(String),
}

fn run_job(job: Job, tx: mpsc::Sender<JobMsg>, ctx: egui::Context) {
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
            Job::AddSong { name, stems } => {
                send(JobMsg::Log(format!("loading stems for \"{name}\"...")));
                let stem_paths: [&Path; 4] = std::array::from_fn(|i| stems[i].as_path());
                let song = match rome_core::encode_song(stem_paths) {
                    Ok(s) => s,
                    Err(e) => { send(JobMsg::Error(format!("{e:#}"))); return; }
                };
                send(JobMsg::Log(format!(
                    "encoded {} blocks, uploading...", song.blocks.len()
                )));
                let mut dev = match rome_core::open_dev(None) {
                    Ok(d) => d,
                    Err(e) => { send(JobMsg::Error(format!("{e:#}"))); return; }
                };
                let total = (song.blocks.len() + song.level_blocks.len()) as f32;
                let tx2 = tx.clone();
                let ctx2 = ctx.clone();
                let result = rome_core::upload_encoded_song(&mut dev, &name, &song, |p| {
                    let _ = tx2.send(JobMsg::Progress(p.blocks_sent as f32 / total));
                    ctx2.request_repaint();
                });
                match result {
                    Ok(idx) => {
                        send(JobMsg::Log(format!("upload complete — catalog index {idx}")));
                        match fetch_info() {
                            Ok((header, songs)) => send(JobMsg::Info { header, songs }),
                            Err(e) => send(JobMsg::Error(format!("{e:#}"))),
                        }
                    }
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
                        "device powering off — press function to wake it into the bootloader"
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
                    Ok(()) => send(JobMsg::Log("flash complete — device will restart".into())),
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

fn fetch_info() -> anyhow::Result<(DiskHeader, Vec<SongEntry>)> {
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

// ── App state ────────────────────────────────────────────────────────────────

#[derive(PartialEq)]
enum Tab { Songs, Device, Flash, Diagnostics }

struct RomeApp {
    tab: Tab,
    tx: mpsc::Sender<JobMsg>,
    rx: mpsc::Receiver<JobMsg>,
    busy: bool,
    progress: f32,
    log: String,
    header: Option<DiskHeader>,
    songs: Vec<SongEntry>,
    diagnostics_text: String,

    // Add-song form
    new_name: String,
    new_stems: [Option<PathBuf>; 4],

    // Flash form
    flash_port: String,
    flash_path: Option<PathBuf>,
    ports: Vec<String>,

    format_confirm: bool,
}

impl RomeApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = mpsc::channel();
        let mut app = Self {
            tab: Tab::Songs,
            tx,
            rx,
            busy: false,
            progress: 0.0,
            log: String::new(),
            header: None,
            songs: Vec::new(),
            diagnostics_text: String::new(),
            new_name: String::new(),
            new_stems: Default::default(),
            flash_port: String::new(),
            flash_path: None,
            ports: Vec::new(),
            format_confirm: false,
        };
        app.ports = rome_core::list_serial_ports().unwrap_or_default();
        app
    }

    fn spawn(&mut self, job: Job, ctx: &egui::Context) {
        self.busy = true;
        self.progress = 0.0;
        run_job(job, self.tx.clone(), ctx.clone());
    }

    fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                JobMsg::Info { header, songs } => {
                    self.header = Some(header);
                    self.songs = songs;
                }
                JobMsg::Progress(p) => self.progress = p,
                JobMsg::Log(s) => {
                    self.log = s;
                }
                JobMsg::Diagnostics(s) => self.diagnostics_text = s,
                JobMsg::Error(e) => {
                    self.log = format!("error: {e}");
                    self.busy = false;
                }
                JobMsg::Done => self.busy = false,
            }
        }
    }
}

impl eframe::App for RomeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();

        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Songs, "Songs");
                ui.selectable_value(&mut self.tab, Tab::Device, "Device");
                ui.selectable_value(&mut self.tab, Tab::Flash, "Flash");
                ui.selectable_value(&mut self.tab, Tab::Diagnostics, "Diagnostics");
                ui.add_space(16.0);
                if ui.add_enabled(!self.busy, egui::Button::new("⟳ Refresh")).clicked() {
                    self.spawn(Job::Refresh, ctx);
                }
                if self.busy {
                    ui.add(egui::Spinner::new());
                }
            });
        });

        egui::TopBottomPanel::bottom("log").show(ctx, |ui| {
            if self.busy && self.progress > 0.0 {
                ui.add(egui::ProgressBar::new(self.progress).show_percentage());
            }
            ui.label(&self.log);
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Songs => self.songs_tab(ui, ctx),
            Tab::Device => self.device_tab(ui, ctx),
            Tab::Flash => self.flash_tab(ui, ctx),
            Tab::Diagnostics => self.diagnostics_tab(ui, ctx),
        });

        if self.header.is_none() && !self.busy {
            // First paint: kick off an initial refresh automatically.
            self.spawn(Job::Refresh, ctx);
        }
    }
}

impl RomeApp {
    fn songs_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        match &self.header {
            None => { ui.label("connecting..."); return; }
            Some(h) if !h.is_valid() => {
                ui.colored_label(egui::Color32::from_rgb(220, 120, 30), "Disk not formatted.");
                ui.label("Use the Device tab to format before adding songs.");
                return;
            }
            Some(h) => {
                ui.label(format!(
                    "v{}  •  {} songs  •  next free block {}",
                    h.version, h.song_count, h.next_free_block
                ));
            }
        }

        ui.separator();
        let live: Vec<(u16, SongEntry)> = self.songs.iter().enumerate()
            .filter(|(_, e)| !e.is_free())
            .map(|(i, e)| (i as u16, e.clone()))
            .collect();

        egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
            let mut swap_req: Option<(u16, u16)> = None;
            let mut remove_req: Option<u16> = None;
            for (row, (idx, entry)) in live.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("[{idx:>3}]"));
                    ui.label(entry.name_str());
                    let secs = entry.block_count as f64 * 128.0 / 48000.0;
                    ui.weak(format!("{:.0}m{:02.0}s", (secs / 60.0).floor(), secs % 60.0));
                    ui.add_space(8.0);
                    if row > 0 && ui.small_button("▲").clicked() {
                        swap_req = Some((*idx, live[row - 1].0));
                    }
                    if row + 1 < live.len() && ui.small_button("▼").clicked() {
                        swap_req = Some((*idx, live[row + 1].0));
                    }
                    if ui.small_button("Remove").clicked() {
                        remove_req = Some(*idx);
                    }
                });
            }
            if let Some((a, b)) = swap_req {
                if !self.busy { self.spawn(Job::SwapSongs { idx_a: a, idx_b: b }, ctx); }
            }
            if let Some(idx) = remove_req {
                if !self.busy { self.spawn(Job::RemoveSong { idx }, ctx); }
            }
        });

        ui.separator();
        ui.heading("Add song");
        ui.horizontal(|ui| {
            ui.label("Name:");
            ui.text_edit_singleline(&mut self.new_name);
        });
        let labels = ["Stem 1", "Stem 2", "Stem 3", "Stem 4"];
        for i in 0..4 {
            ui.horizontal(|ui| {
                ui.label(labels[i]);
                let shown = self.new_stems[i]
                    .as_ref()
                    .map(|p| p.file_name().unwrap_or_default().to_string_lossy().to_string())
                    .unwrap_or_else(|| "(none)".to_string());
                ui.label(shown);
                if ui.button("Browse...").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("audio", &["wav", "flac", "mp3", "ogg"])
                        .pick_file()
                    {
                        self.new_stems[i] = Some(path);
                    }
                }
            });
        }
        let ready = !self.new_name.trim().is_empty()
            && self.new_stems.iter().all(|s| s.is_some())
            && !self.busy;
        ui.horizontal(|ui| {
            if ui.add_enabled(ready, egui::Button::new("Upload")).clicked() {
                let stems: [PathBuf; 4] = std::array::from_fn(|i| self.new_stems[i].clone().unwrap());
                self.spawn(Job::AddSong { name: self.new_name.clone(), stems }, ctx);
            }
            if ui.add_enabled(ready, egui::Button::new("Save as bundle...")).clicked() {
                if let Some(dest) = rfd::FileDialog::new()
                    .set_file_name(&format!("{}.rsp1", self.new_name.trim()))
                    .add_filter("SP-1 stem bundle", &["rsp1"])
                    .save_file()
                {
                    let stems: [PathBuf; 4] = std::array::from_fn(|i| self.new_stems[i].clone().unwrap());
                    match save_bundle(&dest, &self.new_name, &stems) {
                        Ok(()) => self.log = format!("saved bundle: {}", dest.display()),
                        Err(e) => self.log = format!("bundle save failed: {e:#}"),
                    }
                }
            }
            if ui.add_enabled(!self.busy, egui::Button::new("Import bundle...")).clicked() {
                if let Some(src) = rfd::FileDialog::new()
                    .add_filter("SP-1 stem bundle", &["rsp1", "zip"])
                    .pick_file()
                {
                    match load_bundle(&src) {
                        Ok((name, stems)) => self.spawn(Job::AddSong { name, stems }, ctx),
                        Err(e) => self.log = format!("bundle load failed: {e:#}"),
                    }
                }
            }
        });
        ui.weak("A bundle packs the 4 stem files + song name into one file so you \
                  can send a song to someone else before it's uploaded -- it does \
                  not read audio back off the device.");
    }

    fn device_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Format");
        ui.label("Erases the entire song catalog. This cannot be undone.");
        ui.checkbox(&mut self.format_confirm, "I understand this erases all songs");
        if ui.add_enabled(self.format_confirm && !self.busy, egui::Button::new("Format disk"))
            .clicked()
        {
            self.format_confirm = false;
            self.spawn(Job::Format, ctx);
        }

        ui.separator();
        ui.heading("Bootloader");
        ui.label("Powers the device off (SYSTEM_OFF). Press function afterward to \
                   wake it into the bootloader for flashing.");
        if ui.add_enabled(!self.busy, egui::Button::new("Enter bootloader")).clicked() {
            self.spawn(Job::Bootloader, ctx);
        }
    }

    fn flash_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Flash firmware");
        ui.label("Device must already be in the bootloader (see the Device tab).");
        ui.horizontal(|ui| {
            ui.label("Port:");
            egui::ComboBox::from_id_salt("flash_port")
                .selected_text(if self.flash_port.is_empty() { "(select)" } else { &self.flash_port })
                .show_ui(ui, |ui| {
                    for p in &self.ports {
                        ui.selectable_value(&mut self.flash_port, p.clone(), p);
                    }
                });
            if ui.button("⟳").clicked() {
                self.ports = rome_core::list_serial_ports().unwrap_or_default();
            }
        });
        ui.horizontal(|ui| {
            ui.label("Firmware:");
            let shown = self.flash_path.as_ref()
                .map(|p| p.file_name().unwrap_or_default().to_string_lossy().to_string())
                .unwrap_or_else(|| "(none)".to_string());
            ui.label(shown);
            if ui.button("Browse...").clicked() {
                if let Some(path) = rfd::FileDialog::new().add_filter("firmware", &["bin"]).pick_file() {
                    self.flash_path = Some(path);
                }
            }
        });
        let ready = !self.flash_port.is_empty() && self.flash_path.is_some() && !self.busy;
        if ui.add_enabled(ready, egui::Button::new("Flash")).clicked() {
            self.spawn(
                Job::Flash { port: self.flash_port.clone(), firmware: self.flash_path.clone().unwrap() },
                ctx,
            );
        }
    }

    fn diagnostics_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if ui.add_enabled(!self.busy, egui::Button::new("Read diagnostics")).clicked() {
            self.spawn(Job::Diagnostics, ctx);
        }
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.label(egui::RichText::new(&self.diagnostics_text).monospace());
        });
    }
}
