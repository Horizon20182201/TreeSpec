//! New-file outputs shared by the CLI and the front ends (`normalize --out`,
//! `plan --out`, `export-script --out`, TUI/GUI plan files). Outputs are only
//! ever created, never overwritten. This is not the descriptor-relative
//! migration executor of M2.

use std::{
    fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
};

/// Reads a text input, naming the path in the error.
pub fn read_text(path: &Path) -> io::Result<String> {
    fs::read_to_string(path)
        .map_err(|e| io::Error::new(e.kind(), format!("cannot read {}: {e}", path.display())))
}

/// Writes `contents` to `path`, which must not exist yet. The parent must be a
/// plain directory reached without `..` or symlinked ancestors (macOS system
/// aliases such as `/var` excepted). The file is synced before returning.
pub fn write_new(path: &Path, contents: &str) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    check_directory(parent)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("cannot create {} (output must be new): {e}", path.display()),
            )
        })?;
    file.write_all(contents.as_bytes())?;
    file.sync_all()
}

/// Rejects existing symlink ancestors and lexical traversal for metadata writes.
pub fn check_directory(path: &Path) -> io::Result<()> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut current = PathBuf::new();
    for component in absolute.components() {
        if component == Component::ParentDir {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "metadata output paths must not contain ..",
            ));
        }
        current.push(component);
        let metadata = fs::symlink_metadata(&current)?;
        if metadata.file_type().is_symlink() && system_path_alias(&current) {
            continue;
        }
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("not a plain directory: {}", current.display()),
            ));
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn system_path_alias(path: &Path) -> bool {
    matches!(path.to_str(), Some("/var" | "/tmp" | "/etc"))
}

#[cfg(not(target_os = "macos"))]
fn system_path_alias(_: &Path) -> bool {
    false
}
