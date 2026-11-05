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

/// Largest draft [`read_draft`] and [`save_draft`] accept.
pub const MAX_DRAFT_BYTES: usize = 16 * 1024 * 1024;

/// A draft's text and its digest (`sha256:…`), for an editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftText {
    pub text: String,
    pub digest: String,
}

/// Reads `drafts/NAME` for editing. The digest is what [`save_draft`] expects.
pub fn read_draft(root: &Path, name: &str) -> Result<DraftText> {
    check_new_name(name, ".tree")?;
    let root = metadata::canonical_root(root)?;
    let directory = root.join(".organizer");
    metadata::layout(&directory)?;
    let _lock = metadata::lock(&directory, false)?;
    let text = read_plain_draft(&directory.join("drafts").join(name))?;
    Ok(DraftText {
        digest: crate::migration::plan::digest(&text),
        text,
    })
}

fn read_plain_draft(path: &Path) -> Result<String> {
    metadata::plain_file(path)?;
    let before = fs::symlink_metadata(path)?;
    let file = fs::File::open(path)?;
    // The opened file must be the plain file just checked, not a swapped-in link.
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let opened = file.metadata()?;
        if !opened.is_file() || opened.dev() != before.dev() || opened.ino() != before.ino() {
            return Err(fail("draft changed while it was opened"));
        }
    }
    #[cfg(not(unix))]
    let _ = before;
    let mut bytes = vec![];
    file.take(MAX_DRAFT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_DRAFT_BYTES {
        return Err(fail("draft exceeds 16 MiB"));
    }
    String::from_utf8(bytes).map_err(|_| fail("draft is not UTF-8 text"))
}

/// Replaces an existing `drafts/NAME` with `text`, the in-app editor's save
/// (GUI design §6.5): the README step “edit the draft in a text editor”.
///
/// Refuses unless the file on disk still has `expected_digest` (from
/// [`read_draft`] or the previous save), so changes made elsewhere are never
/// overwritten, and unless the LLM review-marker lines are exactly unchanged,
/// so a generated draft stays paired with its report and a manual draft cannot
/// gain a marker. The text is written as given (no normalization), through a
/// temporary file and an atomic replace. Returns the new digest.
pub fn save_draft(root: &Path, name: &str, expected_digest: &str, text: &str) -> Result<String> {
    check_new_name(name, ".tree")?;
    if text.len() > MAX_DRAFT_BYTES {
        return Err(fail("draft exceeds 16 MiB"));
    }
    let root = metadata::canonical_root(root)?;
    let directory = root.join(".organizer");
    metadata::layout(&directory)?;
    let _lock = metadata::lock(&directory, true)?;
    metadata::layout(&directory)?;
    let path = directory.join("drafts").join(name);
    let current = read_plain_draft(&path)?;
    if crate::migration::plan::digest(&current) != expected_digest {
        return Err(fail(
            "the draft changed on disk since it was loaded; reload it or save as a new draft",
        ));
    }
    if crate::llm::review_marker_lines(&current) != crate::llm::review_marker_lines(text) {
        return Err(fail(
            "the review marker line must stay unchanged; restore it before saving",
        ));
    }
    metadata::write_snapshot(&path, Some(&current), text)?;
    Ok(crate::migration::plan::digest(text))
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
