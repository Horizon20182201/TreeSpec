//! Read-only probes and `.organizer`-internal helpers for front ends.
//!
//! Nothing here moves managed user files. [`archive`] and
//! [`copy_baseline_draft`] only create or move files inside
//! `.organizer/drafts`, `.organizer/plans` and `.organizer/archive`: the steps
//! the README otherwise asks users to perform by hand.

mod archive;
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests;

pub use archive::{ArchiveReport, archive};

use crate::{
    error::{Result, fail},
    metadata,
    migration::journal::{self, RunSummary},
};
use serde::Serialize;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const MARKER_LIMIT: u64 = 64 * 1024;

/// State that `status` itself refuses to report, gathered read-only.
#[derive(Clone, Debug, Serialize)]
pub struct Probe {
    pub root: PathBuf,
    /// `.organizer` exists below `root`.
    pub initialized: bool,
    /// SQLite recovery files next to the index or the operation journal.
    pub recovery_files: Vec<String>,
    /// Contents of `state/pending.json`: an interrupted metadata update.
    pub pending_marker: Option<String>,
    /// All journaled runs, oldest first; empty if the journal needs recovery.
    pub runs: Vec<RunSummary>,
}

impl Probe {
    pub fn unfinished_runs(&self) -> impl Iterator<Item = &RunSummary> {
        self.runs.iter().filter(|run| !run.is_finished())
    }
}

/// Never fails for an uninitialized root. Otherwise it holds the shared
/// metadata lock, so an update in progress is reported as busy, not as a crash.
pub fn probe(root: &Path) -> Result<Probe> {
    let root = metadata::canonical_root(root)?;
    let directory = root.join(".organizer");
    if !metadata::exists(&directory)? {
        return Ok(Probe {
            root,
            initialized: false,
            recovery_files: vec![],
            pending_marker: None,
            runs: vec![],
        });
    }
    metadata::plain_directory(&directory)?;
    let _lock = metadata::lock(&directory, false)?;
    let mut recovery_files = vec![];
    for database in ["index.sqlite", "operations.sqlite"] {
        for suffix in ["-journal", "-wal", "-shm"] {
            let name = format!("state/{database}{suffix}");
            if metadata::exists(&directory.join(&name))? {
                recovery_files.push(name);
            }
        }
    }
    let marker = directory.join("state/pending.json");
    let pending_marker = if metadata::exists(&marker)? {
        metadata::plain_file(&marker)?;
        let mut bytes = vec![];
        fs::File::open(&marker)?
            .take(MARKER_LIMIT)
            .read_to_end(&mut bytes)?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    } else {
        None
    };
    let journal_blocked = recovery_files
        .iter()
        .any(|name| name.starts_with("state/operations"));
    let runs = if journal_blocked {
        vec![]
    } else {
        journal::summaries(&directory)?
    };
    Ok(Probe {
        root,
        initialized: true,
        recovery_files,
        pending_marker,
        runs,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    File,
    Directory,
    Other,
}

/// A pending entry in `.organizer/drafts` or `.organizer/plans`; these are the
/// entries that make `sync` stop.
#[derive(Clone, Debug, Serialize)]
pub struct Artifact {
    /// `"drafts"` or `"plans"`.
    pub directory: &'static str,
    /// Display name (lossy for non-UTF-8 names); `path` is exact.
    pub name: String,
    pub path: PathBuf,
    pub kind: ArtifactKind,
}

impl Artifact {
    /// `drafts/NAME` or `plans/NAME`, the form [`archive`] accepts.
    pub fn selector(&self) -> String {
        format!("{}/{}", self.directory, self.name)
    }
}

pub(crate) fn artifacts(directory: &Path) -> Result<Vec<Artifact>> {
    let mut list: Vec<_> = metadata::pending_entries(directory)?
        .into_iter()
        .map(|(parent, name)| {
            let path = directory.join(parent).join(&name);
            let kind = match fs::symlink_metadata(&path) {
                Ok(meta) if meta.file_type().is_file() => ArtifactKind::File,
                Ok(meta) if meta.file_type().is_dir() => ArtifactKind::Directory,
                _ => ArtifactKind::Other,
            };
            Artifact {
                directory: parent,
                name: name.to_string_lossy().into_owned(),
                path,
                kind,
            }
        })
        .collect();
    list.sort_by(|a, b| (a.directory, &a.name).cmp(&(b.directory, &b.name)));
    Ok(list)
}

/// Creates `drafts/NAME` with the current `structure.tree` text, like the
/// README's `cp -n`. Never overwrites a draft or an existing review report.
/// Allowed while the baseline is conflicted, so a manually edited
/// `structure.tree` can be preserved as a draft before restoring the baseline.
pub fn copy_baseline_draft(root: &Path, name: &str) -> Result<PathBuf> {
    check_new_name(name, ".tree")?;
    let root = metadata::canonical_root(root)?;
    let directory = root.join(".organizer");
    metadata::layout(&directory)?;
    let _lock = metadata::lock(&directory, true)?;
    metadata::layout(&directory)?;
    let source = directory.join("structure.tree");
    metadata::plain_file(&source)?;
    let text = fs::read_to_string(&source)?;
    let draft = directory.join("drafts").join(name);
    if metadata::exists(&crate::llm::review_path(&draft)?)? {
        return Err(fail(
            "a review report with this draft name already exists; choose another name",
        ));
    }
    metadata::write_snapshot(&draft, None, &text)?;
    Ok(draft)
}

/// A single new file name inside `drafts/` or `plans/`.
pub fn check_new_name(name: &str, suffix: &str) -> Result<()> {
    let stem = name.strip_suffix(suffix).unwrap_or("");
    if stem.is_empty()
        || name.len() > 200
        || name.starts_with('.')
        || name.ends_with(".review.json")
        || name.contains(['/', '\\', '\0'])
        || name.chars().any(char::is_control)
    {
        return Err(fail(format!(
            "invalid file name {name:?}: use a single name ending in {suffix}, without / or a leading dot"
        )));
    }
    Ok(())
}
