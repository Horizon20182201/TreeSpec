//! Small macOS integrations that only start fixed system programs with
//! separate arguments, never through a shell (design §8.3, §9).

use std::{
    path::Path,
    process::{Child, Command, Stdio},
};

/// Keeps the system from idle-sleeping while a migration runs:
/// `/usr/bin/caffeinate -i -w PID`, stopped when dropped.
pub struct Caffeinate(Option<Child>);

impl Caffeinate {
    pub fn start() -> Self {
        let child = cfg!(target_os = "macos")
            .then(|| {
                Command::new("/usr/bin/caffeinate")
                    .args(["-i", "-w", &std::process::id().to_string()])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .ok()
            })
            .flatten();
        Self(child)
    }
}

impl Drop for Caffeinate {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Whether the Xcode command line tools are installed. Without them
/// `/usr/bin/git` only opens the system installer (design §9.2).
pub fn command_line_tools() -> bool {
    if !cfg!(target_os = "macos") {
        return true;
    }
    Command::new("/usr/bin/xcode-select")
        .arg("-p")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn open(args: &[&std::ffi::OsStr]) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("只在 macOS 上可用".into());
    }
    let status = Command::new("/usr/bin/open")
        .args(args)
        .stdin(Stdio::null())
        .status()
        .map_err(|e| e.to_string())?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| format!("open 退出状态 {status}"))
}

/// `sw_vers -productVersion`, for diagnostics.
pub fn os_version() -> String {
    Command::new("/usr/bin/sw_vers")
        .arg("-productVersion")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map_or_else(|| "?".into(), |v| v.trim().to_owned())
}

/// Shows `path` selected in Finder.
pub fn reveal(path: &Path) -> Result<(), String> {
    open(&["-R".as_ref(), path.as_os_str()])
}

/// Opens `path` with the given application.
pub fn open_with(app: &str, path: &Path) -> Result<(), String> {
    open(&["-a".as_ref(), app.as_ref(), path.as_os_str()])
}

/// Opens a fixed URL (help pages, System Settings panes).
pub fn open_url(url: &str) -> Result<(), String> {
    open(&[url.as_ref()])
}

pub const FILES_AND_FOLDERS: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_FilesAndFolders";
pub const HELP_URL: &str = "https://github.com/Horizon20182201/TreeSpec#readme";

/// Starts the system installer for the command line tools.
pub fn install_command_line_tools() -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("只在 macOS 上可用".into());
    }
    Command::new("/usr/bin/xcode-select")
        .arg("--install")
        .stdin(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}
