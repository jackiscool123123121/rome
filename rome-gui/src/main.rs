// rome-gui: desktop companion app for the SP-1 stem player.
//
// All device I/O runs on a short-lived background thread per job (never on
// the UI thread), reporting back over an mpsc channel that the UI polls each
// frame. rome_core::proto::DeviceConn binds directly to the device by USB
// VID/PID (port name is only meaningful for the separate bootloader-mode
// serial protocol used by Flash), so most jobs need no port argument at all.

mod app;
mod bundle;
mod jobs;
mod tabs;

use app::RomeApp;
use eframe::egui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([620.0, 760.0])
            .with_min_inner_size([480.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        "rome",
        options,
        Box::new(|cc| Ok(Box::new(RomeApp::new(cc)))),
    )
}
