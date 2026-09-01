use std::path::PathBuf;
use std::sync::mpsc;

use eframe::egui;
use rome_core::disk::{DiskHeader, SongEntry};

use crate::bundle::BundleSource;
use crate::jobs::{self, AddMode, Job, JobMsg, SongInput};

#[derive(PartialEq)]
pub enum Tab { Songs, Bundle, Device, Flash, Diagnostics }

pub struct RomeApp {
    pub(crate) tab: Tab,
    tx: mpsc::Sender<JobMsg>,
    rx: mpsc::Receiver<JobMsg>,
    pub(crate) busy: bool,
    pub(crate) progress: f32,
    pub(crate) eta_secs: Option<f32>,
    pub(crate) log: String,
    pub(crate) header: Option<DiskHeader>,
    pub(crate) songs: Vec<SongEntry>,
    pub(crate) diagnostics_text: String,

    // Add-song form (Songs tab)
    pub(crate) add_mode: AddMode,
    pub(crate) new_name: String,
    pub(crate) new_stems: [Option<PathBuf>; 4],
    pub(crate) new_combined: Option<PathBuf>,
    pub(crate) queue: Vec<(String, SongInput)>,

    // Bundle tab
    pub(crate) bundle_add_mode: AddMode,
    pub(crate) bundle_new_name: String,
    pub(crate) bundle_new_stems: [Option<PathBuf>; 4],
    pub(crate) bundle_new_combined: Option<PathBuf>,
    pub(crate) bundle_items: Vec<(String, BundleSource)>,

    // Flash form
    pub(crate) flash_port: String,
    pub(crate) flash_path: Option<PathBuf>,
    pub(crate) ports: Vec<String>,

    pub(crate) format_confirm: bool,
}

impl RomeApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = mpsc::channel();
        let mut app = Self {
            tab: Tab::Songs,
            tx,
            rx,
            busy: false,
            progress: 0.0,
            eta_secs: None,
            log: String::new(),
            header: None,
            songs: Vec::new(),
            diagnostics_text: String::new(),
            add_mode: AddMode::FourStems,
            new_name: String::new(),
            new_stems: Default::default(),
            new_combined: None,
            queue: Vec::new(),
            bundle_add_mode: AddMode::FourStems,
            bundle_new_name: String::new(),
            bundle_new_stems: Default::default(),
            bundle_new_combined: None,
            bundle_items: Vec::new(),
            flash_port: String::new(),
            flash_path: None,
            ports: Vec::new(),
            format_confirm: false,
        };
        app.ports = rome_core::list_serial_ports().unwrap_or_default();
        if let Some(msg) = crate::install::self_install_cli() {
            app.log = msg;
        }
        app
    }

    pub(crate) fn spawn(&mut self, job: Job, ctx: &egui::Context) {
        self.busy = true;
        self.progress = 0.0;
        jobs::run_job(job, self.tx.clone(), ctx.clone());
    }

    fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                JobMsg::Info { header, songs } => {
                    self.header = Some(header);
                    self.songs = songs;
                }
                JobMsg::Progress { frac, eta_secs } => {
                    self.progress = frac;
                    self.eta_secs = eta_secs;
                }
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
                ui.selectable_value(&mut self.tab, Tab::Bundle, "Bundle");
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
                let text = match self.eta_secs {
                    Some(eta) => format!("{:.0}% • eta {}", self.progress * 100.0, jobs::format_eta(eta)),
                    None => format!("{:.0}%", self.progress * 100.0),
                };
                ui.add(egui::ProgressBar::new(self.progress).text(text));
            }
            ui.label(&self.log);
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Songs => self.songs_tab(ui, ctx),
            Tab::Bundle => self.bundle_tab(ui, ctx),
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
