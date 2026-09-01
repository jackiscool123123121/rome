//! First-run self-setup for the packaged app: install the bundled `rome` CLI
//! binary onto PATH (once, for a new user -- re-running `rome update` is how
//! an existing install gets refreshed later), and (macOS only) offer to copy
//! the .app into /Applications. Both are best-effort -- a dev build run
//! straight out of `target/debug` has no sibling CLI binary or .app bundle
//! to work with, so everything here quietly does nothing in that case.

use std::path::{Path, PathBuf};

/// True if this looks like a first run: no marker file recording a prior
/// CLI install exists yet.
pub fn is_new_user() -> bool {
    match marker_path() {
        Some(p) => !p.is_file(),
        None => false,
    }
}

/// Copy the `rome` CLI binary sitting next to this GUI executable (as the
/// packaged .app/.exe lays them out) to a per-user bin directory, so `rome`
/// also works from a terminal, then record that it's done so future launches
/// don't repeat it. Returns a status line for the GUI log.
pub fn install_cli_for_new_user() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let cli_name = if cfg!(windows) { "rome.exe" } else { "rome" };
    let src = dir.join(cli_name);
    if !src.is_file() {
        return None; // dev build without a sibling CLI binary
    }

    let dest = cli_install_path()?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    if dest.is_file() {
        std::fs::remove_file(&dest).ok()?;
    }
    std::fs::copy(&src, &dest).ok()?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&dest) {
            let mut perm = meta.permissions();
            perm.set_mode(0o755);
            let _ = std::fs::set_permissions(&dest, perm);
        }
    }

    if let Some(marker) = marker_path() {
        if let Some(parent) = marker.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&marker, b"");
    }

    Some(format!(
        "installed rome CLI to {} -- add that folder to PATH to use `rome` in a terminal \
         (run `rome update` later to get the newest version)",
        dest.display()
    ))
}

#[cfg(unix)]
fn cli_install_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".local").join("bin").join("rome"))
}

#[cfg(windows)]
fn cli_install_path() -> Option<PathBuf> {
    let local = std::env::var("LOCALAPPDATA").ok()?;
    Some(PathBuf::from(local).join("rome").join("bin").join("rome.exe"))
}

#[cfg(unix)]
fn marker_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".local").join("share").join("rome").join("cli-installed"))
}

#[cfg(windows)]
fn marker_path() -> Option<PathBuf> {
    let local = std::env::var("LOCALAPPDATA").ok()?;
    Some(PathBuf::from(local).join("rome").join("cli-installed"))
}

/// macOS only: if running from somewhere other than /Applications (i.e. a
/// freshly unzipped download), offer to copy the .app bundle there. Native
/// modal dialogs are unreliable if shown before the app's event loop is
/// actually pumping frames, so callers must only invoke this from inside a
/// running eframe update() -- never from main() before run_native.
#[cfg(target_os = "macos")]
pub fn maybe_offer_move_to_applications() {
    let Ok(exe) = std::env::current_exe() else { return };
    if exe.starts_with("/Applications/") {
        return;
    }
    let Some(app_root) = find_app_bundle_root(&exe) else { return };
    let Some(app_name) = app_root.file_name() else { return };
    let dest = Path::new("/Applications").join(app_name);
    if dest.exists() {
        return;
    }

    let confirmed = rfd::MessageDialog::new()
        .set_title("Move to Applications?")
        .set_description("Copy rome to your Applications folder so it's easy to find later?")
        .set_buttons(rfd::MessageButtons::YesNo)
        .show();
    if confirmed != rfd::MessageDialogResult::Yes {
        return;
    }

    if copy_dir_recursive(&app_root, &dest).is_ok() {
        rfd::MessageDialog::new()
            .set_title("Done")
            .set_description(format!("Copied to {}", dest.display()))
            .show();
    }
}

#[cfg(not(target_os = "macos"))]
pub fn maybe_offer_move_to_applications() {}

#[cfg(target_os = "macos")]
fn find_app_bundle_root(exe: &Path) -> Option<PathBuf> {
    // .../RomeGUI.app/Contents/MacOS/rome-gui -> .../RomeGUI.app
    let app_dir = exe.parent()?.parent()?.parent()?;
    if app_dir.extension().is_some_and(|e| e == "app") {
        Some(app_dir.to_path_buf())
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
fn copy_dir_recursive(src: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let dest_path = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else {
            std::fs::copy(entry.path(), &dest_path)?;
        }
    }
    Ok(())
}
