use crate::error::{Result, fail};
use std::{
    fs::{self, File},
    io::Write,
    path::{Component, Path, PathBuf},
};

pub(crate) const IGNORE: &str = "drafts/\nplans/\nstate/\ncache/\n*.log\n.env\n";

pub(crate) fn canonical_root(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut current = PathBuf::new();
    for component in absolute.components() {
        if component == Component::ParentDir {
            return Err(fail("root and metadata paths must not contain .."));
        }
        current.push(component);
        plain_directory(&current)?;
    }
    let canonical = absolute.canonicalize()?;
    if canonical.to_str().is_none() {
        return Err(fail("management root must be UTF-8"));
    }
    Ok(canonical)
}

pub(crate) fn plain_directory(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err(fail(format!("not a plain directory: {}", path.display())));
    }
    Ok(())
}

pub(crate) fn plain_file(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err(fail(format!("not a plain file: {}", path.display())));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.nlink() != 1 {
            return Err(fail(format!(
                "hard-linked metadata is not supported: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

pub(crate) fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn create_directory(path: &Path) -> Result<()> {
    let builder = fs::DirBuilder::new();
    #[cfg(unix)]
    let builder = {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = builder;
        builder.mode(0o700);
        builder
    };
    builder.create(path)?;
    Ok(())
}

/// Replaces only an expected metadata baseline, never managed user content.
pub(crate) fn write_snapshot(path: &Path, expected: Option<&str>, contents: &str) -> Result<()> {
    let parent = path.parent().ok_or_else(|| fail("missing output parent"))?;
    plain_directory(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(contents.as_bytes())?;
    temporary.as_file().sync_all()?;
    if let Some(expected) = expected {
        plain_file(path)?;
        if fs::read_to_string(path)? != expected {
            return Err(fail(
                "metadata changed during sync; refusing to overwrite it",
            ));
        }
        temporary.persist(path).map_err(|error| error.error)?;
    } else {
        temporary
            .persist_noclobber(path)
            .map_err(|error| error.error)?;
    }
    File::open(parent)?.sync_all()?;
    Ok(())
}

pub(crate) fn lock(metadata: &Path, write: bool) -> Result<File> {
    let path = metadata.join("state/lock");
    plain_file(&path)?;
    let file = fs::OpenOptions::new().read(true).write(write).open(path)?;
    let result = if write {
        file.try_lock()
    } else {
        file.try_lock_shared()
    };
    result.map_err(|error| {
        fail(format!(
            "organizer metadata is busy or cannot be locked: {error}"
        ))
    })?;
    Ok(file)
}

pub(crate) fn layout(metadata: &Path) -> Result<()> {
    layout_files(metadata)?;
    if exists(&metadata.join("state/pending.json"))? {
        return Err(fail(
            "incomplete metadata update: inspect .organizer/state/pending.json and sync.log; automatic replay is disabled",
        ));
    }
    crate::migration::journal::ensure_idle(metadata)?;
    Ok(())
}

pub(crate) fn layout_files(metadata: &Path) -> Result<()> {
    plain_directory(metadata)?;
    for directory in ["drafts", "plans", "state", "cache", ".git"] {
        plain_directory(&metadata.join(directory))?;
    }
    for file in [
        "structure.tree",
        ".gitignore",
        "state/index.sqlite",
        "state/lock",
    ] {
        plain_file(&metadata.join(file))?;
    }
    for suffix in ["-journal", "-wal", "-shm"] {
        if exists(&metadata.join(format!("state/index.sqlite{suffix}")))? {
            return Err(fail(
                "SQLite recovery files exist; stop and inspect the interrupted metadata update",
            ));
        }
    }
    Ok(())
}

pub(crate) fn draft_conflicts(metadata: &Path) -> Result<Vec<String>> {
    let mut conflicts = Vec::new();
    for directory in ["drafts", "plans"] {
        for entry in fs::read_dir(metadata.join(directory))? {
            let entry = entry?;
            // The unchanged M0 bootstrap is not an outstanding user draft.
            if directory == "drafts" && entry.file_name() == "initial.tree" {
                plain_file(&entry.path())?;
                if fs::read_to_string(entry.path())? == "!tree 1\n" {
                    continue;
                }
            }
            conflicts.push(format!("pending {directory} entry {:?}; review and archive it outside drafts/plans before sync", entry.file_name()));
        }
    }
    conflicts.sort();
    Ok(conflicts)
}

pub(crate) fn log(metadata: &Path, message: &str) -> Result<()> {
    let path = metadata.join("sync.log");
    if exists(&path)? {
        plain_file(&path)?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{message}")?;
    file.sync_all()?;
    Ok(())
}
