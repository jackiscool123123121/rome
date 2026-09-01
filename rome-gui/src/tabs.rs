use std::path::PathBuf;

use eframe::egui;
use rome_core::disk::SongEntry;

use crate::app::{RomeApp, Tab};
use crate::bundle::{self, BundleSource};
use crate::jobs::{AddMode, Job, SongInput};

/// Draws the shared "pick 4 stems, or 1 combined WAV" input form used by both
/// the Songs-tab add form and the Bundle-tab add form. Returns true if the
/// form currently has everything needed to build a SongInput.
fn input_mode_form(
    ui: &mut egui::Ui,
    id_salt: &str,
    mode: &mut AddMode,
    stems: &mut [Option<PathBuf>; 4],
    combined: &mut Option<PathBuf>,
) -> bool {
    ui.horizontal(|ui| {
        ui.label("Input:");
        egui::ComboBox::from_id_salt(id_salt)
            .selected_text(match mode {
                AddMode::FourStems => "4 separate stem files",
                AddMode::CombinedWav => "1 combined multi-stem WAV",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(mode, AddMode::FourStems, "4 separate stem files");
                ui.selectable_value(mode, AddMode::CombinedWav, "1 combined multi-stem WAV");
            });
    });

    match mode {
        AddMode::FourStems => {
            let labels = ["Stem 1", "Stem 2", "Stem 3", "Stem 4"];
            for i in 0..4 {
                ui.horizontal(|ui| {
                    ui.label(labels[i]);
                    let shown = stems[i]
                        .as_ref()
                        .map(|p| p.file_name().unwrap_or_default().to_string_lossy().to_string())
                        .unwrap_or_else(|| "(none)".to_string());
                    ui.label(shown);
                    if ui.button("Browse...").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("audio", &["wav", "flac", "mp3", "ogg"])
                            .pick_file()
                        {
                            stems[i] = Some(path);
                        }
                    }
                });
            }
            stems.iter().all(|s| s.is_some())
        }
        AddMode::CombinedWav => {
            ui.horizontal(|ui| {
                ui.label("Combined WAV");
                let shown = combined
                    .as_ref()
                    .map(|p| p.file_name().unwrap_or_default().to_string_lossy().to_string())
                    .unwrap_or_else(|| "(none)".to_string());
                ui.label(shown);
                if ui.button("Browse...").clicked() {
                    if let Some(path) = rfd::FileDialog::new().add_filter("audio", &["wav"]).pick_file() {
                        *combined = Some(path);
                    }
                }
            });
            ui.weak("8-channel WAV: ch1-2/3-4/5-6/7-8 = stems 1-4 (TE / solderless \
                      stem-loader format). Missing channels are silent.");
            combined.is_some()
        }
    }
}

impl RomeApp {
    pub(crate) fn songs_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
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

        egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
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
        let form_ready = input_mode_form(
            ui, "add_mode", &mut self.add_mode, &mut self.new_stems, &mut self.new_combined,
        ) && !self.new_name.trim().is_empty();

        if ui.add_enabled(form_ready, egui::Button::new("Add to queue")).clicked() {
            let input = match self.add_mode {
                AddMode::FourStems => {
                    let stems: [PathBuf; 4] =
                        std::array::from_fn(|i| self.new_stems[i].clone().unwrap());
                    SongInput::FourStems(stems)
                }
                AddMode::CombinedWav => SongInput::CombinedWav(self.new_combined.clone().unwrap()),
            };
            self.queue.push((self.new_name.clone(), input));
            self.new_name.clear();
            self.new_stems = Default::default();
            self.new_combined = None;
        }

        ui.separator();
        ui.heading(format!("Transfer queue ({})", self.queue.len()));
        let mut remove_queue: Option<usize> = None;
        egui::ScrollArea::vertical().max_height(140.0).id_salt("queue_scroll").show(ui, |ui| {
            for (i, (name, input)) in self.queue.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("{}.", i + 1));
                    ui.label(name);
                    ui.weak(format!("({})", input.mode_label()));
                    if ui.add_enabled(!self.busy, egui::Button::new("Remove")).clicked() {
                        remove_queue = Some(i);
                    }
                });
            }
            if self.queue.is_empty() {
                ui.weak("(empty -- add songs above, then Transfer All)");
            }
        });
        if let Some(i) = remove_queue {
            self.queue.remove(i);
        }
        if ui.add_enabled(!self.queue.is_empty() && !self.busy,
            egui::Button::new(format!("Transfer All ({})", self.queue.len()))).clicked()
        {
            let items = std::mem::take(&mut self.queue);
            self.spawn(Job::TransferQueue { items }, ctx);
        }
    }

    pub(crate) fn bundle_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Add a local song");
        ui.horizontal(|ui| {
            ui.label("Name:");
            ui.text_edit_singleline(&mut self.bundle_new_name);
        });
        let form_ready = input_mode_form(
            ui, "bundle_add_mode", &mut self.bundle_add_mode,
            &mut self.bundle_new_stems, &mut self.bundle_new_combined,
        ) && !self.bundle_new_name.trim().is_empty();
        if ui.add_enabled(form_ready, egui::Button::new("Add to bundle")).clicked() {
            let input = match self.bundle_add_mode {
                AddMode::FourStems => {
                    let stems: [PathBuf; 4] =
                        std::array::from_fn(|i| self.bundle_new_stems[i].clone().unwrap());
                    SongInput::FourStems(stems)
                }
                AddMode::CombinedWav => SongInput::CombinedWav(self.bundle_new_combined.clone().unwrap()),
            };
            self.bundle_items.push((self.bundle_new_name.clone(), BundleSource::Local(input)));
            self.bundle_new_name.clear();
            self.bundle_new_stems = Default::default();
            self.bundle_new_combined = None;
        }

        ui.separator();
        ui.heading("Add an already-uploaded song");
        ui.weak("Pulls the song's audio off the device (from its stems) when the bundle is built.");
        let live: Vec<(u16, SongEntry)> = self.songs.iter().enumerate()
            .filter(|(_, e)| !e.is_free())
            .map(|(i, e)| (i as u16, e.clone()))
            .collect();
        egui::ScrollArea::vertical().max_height(140.0).id_salt("device_songs_scroll").show(ui, |ui| {
            if live.is_empty() {
                ui.weak("(no songs on device -- Refresh, or check the Songs tab)");
            }
            for (idx, entry) in &live {
                ui.horizontal(|ui| {
                    ui.label(format!("[{idx:>3}]"));
                    ui.label(entry.name_str());
                    if ui.small_button("Add to bundle").clicked() {
                        self.bundle_items.push((
                            entry.name_str().to_string(),
                            BundleSource::Device {
                                block_start: entry.block_start,
                                block_count: entry.block_count,
                            },
                        ));
                    }
                });
            }
        });

        ui.separator();
        ui.heading(format!("Bundle contents ({})", self.bundle_items.len()));
        let mut swap_req: Option<(usize, usize)> = None;
        let mut remove_req: Option<usize> = None;
        egui::ScrollArea::vertical().max_height(160.0).id_salt("bundle_items_scroll").show(ui, |ui| {
            for (i, (name, source)) in self.bundle_items.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("{}.", i + 1));
                    ui.label(name);
                    ui.weak(format!("({})", source.label()));
                    ui.add_space(8.0);
                    if i > 0 && ui.small_button("▲").clicked() {
                        swap_req = Some((i, i - 1));
                    }
                    if i + 1 < self.bundle_items.len() && ui.small_button("▼").clicked() {
                        swap_req = Some((i, i + 1));
                    }
                    if ui.small_button("Remove").clicked() {
                        remove_req = Some(i);
                    }
                });
            }
            if self.bundle_items.is_empty() {
                ui.weak("(empty -- add songs above)");
            }
        });
        if let Some((a, b)) = swap_req {
            self.bundle_items.swap(a, b);
        }
        if let Some(i) = remove_req {
            self.bundle_items.remove(i);
        }

        ui.horizontal(|ui| {
            if ui.add_enabled(!self.bundle_items.is_empty() && !self.busy,
                egui::Button::new("Save bundle as .zip...")).clicked()
            {
                if let Some(dest) = rfd::FileDialog::new()
                    .set_file_name("bundle.rsp1")
                    .add_filter("SP-1 song bundle", &["rsp1", "zip"])
                    .save_file()
                {
                    let items = std::mem::take(&mut self.bundle_items);
                    self.spawn(Job::SaveBundle { items, dest }, ctx);
                }
            }
            if ui.add_enabled(!self.busy, egui::Button::new("Import bundle...")).clicked() {
                if let Some(src) = rfd::FileDialog::new()
                    .add_filter("SP-1 song bundle", &["rsp1", "zip"])
                    .pick_file()
                {
                    match bundle::load_bundle(&src) {
                        Ok(songs) => {
                            let n = songs.len();
                            for (name, stems) in songs {
                                self.queue.push((name, SongInput::FourStems(stems)));
                            }
                            self.log = format!("imported {n} song(s) from bundle into the transfer queue");
                            self.tab = Tab::Songs;
                        }
                        Err(e) => self.log = format!("bundle load failed: {e:#}"),
                    }
                }
            }
        });
        ui.weak("A bundle packs multiple songs, in order, with their stems, into one file \
                  you can send to someone else -- import it on their machine to queue up \
                  the same songs for transfer.");
    }

    pub(crate) fn device_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
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

    pub(crate) fn flash_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
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

    pub(crate) fn diagnostics_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if ui.add_enabled(!self.busy, egui::Button::new("Read diagnostics")).clicked() {
            self.spawn(Job::Diagnostics, ctx);
        }
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.label(egui::RichText::new(&self.diagnostics_text).monospace());
        });
    }
}
