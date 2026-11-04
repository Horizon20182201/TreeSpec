use crate::{
    error::{Result, fail},
    git, metadata,
    scan::{
        FileStamp, ScanSnapshot,
        identity::{ReconcileReport, compatible_identity, reconcile},
        scan,
    },
    storage::{self, SavedState},
    tree::{TreeSpec, render_tree},
};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
};
use uuid::Uuid;

#[derive(Debug, Serialize)]
pub struct SyncReport {
    pub report: ReconcileReport,
    pub structure_changed: bool,
    pub git_head: Option<String>,
    pub retried_git_commit: bool,
}

pub fn discover_root() -> Result<Option<PathBuf>> {
    for directory in std::env::current_dir()?.ancestors() {
        if metadata::exists(&directory.join(".organizer"))? {
            metadata::plain_directory(&directory.join(".organizer"))?;
            return Ok(Some(metadata::canonical_root(directory)?));
        }
    }
    Ok(None)
}

pub fn resolve_root(root: Option<&Path>) -> Result<PathBuf> {
    match root {
        Some(root) => metadata::canonical_root(root),
        None => discover_root()?
            .ok_or_else(|| fail("no managed root found; run organizer init --root PATH")),
    }
}

pub(crate) fn load(root: &Path) -> Result<SavedState> {
    let saved = storage::load(&root.join(".organizer/state/index.sqlite"))?;
    if root != saved.root
        || !compatible_identity(
            &saved.root_stamp,
            &FileStamp::from_metadata(&fs::metadata(root)?),
        )
    {
        return Err(fail(
            "management root path or identity changed; refusing to switch roots",
        ));
    }
    Ok(saved)
}

pub(crate) fn baseline_conflicts(metadata: &Path, saved: &SavedState) -> Result<Vec<String>> {
    let mut conflicts = Vec::new();
    if fs::read_to_string(metadata.join("structure.tree"))? != saved.tree_text {
        conflicts.push("structure.tree has unapplied manual edits; preserve them as a draft and restore the indexed baseline before sync (Git HEAD only if no commit is pending)".into());
    }
    if let Err(error) = git::check(metadata, saved) {
        conflicts.push(error.to_string());
    }
    Ok(conflicts)
}

pub fn status(root: &Path) -> Result<ReconcileReport> {
    let root = metadata::canonical_root(root)?;
    let directory = root.join(".organizer");
    metadata::layout(&directory)?;
    let _lock = metadata::lock(&directory, false)?;
    metadata::layout(&directory)?;
    let saved = load(&root)?;
    let mut report = reconcile(&scan(&root)?, &saved)?;
    report
        .conflicts
        .extend(baseline_conflicts(&directory, &saved)?);
    report
        .conflicts
        .extend(metadata::draft_conflicts(&directory)?);
    Ok(report)
}

pub fn init(root: &Path) -> Result<SyncReport> {
    let root = metadata::canonical_root(root)?;
    let directory = root.join(".organizer");
    let upgrade = metadata::exists(&directory)?;
    if upgrade {
        check_m0(&directory)?;
    }
    let first = scan(&root)?;
    require_complete(&first)?;
    if !upgrade {
        metadata::create_directory(&directory)?;
        for name in ["drafts", "plans", "state", "cache"] {
            metadata::create_directory(&directory.join(name))?;
        }
        metadata::write_snapshot(&directory.join(".gitignore"), None, metadata::IGNORE)?;
    }
    metadata::write_snapshot(&directory.join("state/lock"), None, "")?;
    let _lock = metadata::lock(&directory, true)?;
    metadata::log(&directory, "init: metadata initialization started")?;
    let snapshot = scan(&root)?;
    require_stable(&first, &snapshot)?;
    let empty = SavedState {
        root: root.clone(),
        root_stamp: FileStamp::from_metadata(&fs::metadata(&root)?),
        namespace: Uuid::new_v4().to_string(),
        generation: 0,
        tree: TreeSpec::default(),
        tree_text: "!tree 1\n".into(),
        records: vec![],
        git_head: None,
        pending_git: true,
    };
    let mut report = reconcile(&snapshot, &empty)?;
    require_resolved(&report, false)?;
    let mut state = SavedState {
        tree_text: render_tree(&report.tree)?,
        tree: report.tree.clone(),
        records: report.records.clone(),
        generation: 1,
        ..empty
    };
    metadata::write_snapshot(
        &directory.join("state/pending.json"),
        None,
        "{\"operation\":\"init\",\"phase\":\"creating_repository\"}\n",
    )?;
    git::init(&directory)?;
    metadata::write_snapshot(&directory.join("structure.tree"), None, &state.tree_text)?;
    storage::save(&directory.join("state/index.sqlite"), &state, true)?;
    clear_pending(&directory)?;
    finish_git(&directory, &mut state, "init: record filesystem structure")?;
    report.pending_git = false;
    metadata::log(&directory, "init: completed")?;
    Ok(SyncReport {
        report,
        structure_changed: true,
        git_head: state.git_head,
        retried_git_commit: false,
    })
}

fn check_m0(directory: &Path) -> Result<()> {
    metadata::plain_directory(directory)?;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if ![
            ".gitignore",
            "init.log",
            "drafts",
            "plans",
            "state",
            "cache",
        ]
        .iter()
        .any(|name| entry.file_name() == *name)
        {
            return Err(fail(
                "existing metadata is not an untouched M0 layout; use status/sync or inspect the incomplete initialization",
            ));
        }
    }
    for name in ["drafts", "plans", "state", "cache"] {
        metadata::plain_directory(&directory.join(name))?;
    }
    for name in [".gitignore", "init.log", "drafts/initial.tree"] {
        metadata::plain_file(&directory.join(name))?;
    }
    let log = fs::read_to_string(directory.join("init.log"))?;
    if !log.starts_with("M0 metadata initialization started;")
        || !log.contains("completed:")
        || fs::read_to_string(directory.join(".gitignore"))? != metadata::IGNORE
        || fs::read_dir(directory.join("state"))?.next().is_some()
        || !metadata::draft_conflicts(directory)?.is_empty()
    {
        return Err(fail(
            "M0 metadata contains edits, pending drafts, or incomplete state; review it before upgrading",
        ));
    }
    Ok(())
}

pub fn sync(root: &Path, accept_unknown: bool) -> Result<SyncReport> {
    let root = metadata::canonical_root(root)?;
    let directory = root.join(".organizer");
    metadata::layout(&directory)?;
    let _lock = metadata::lock(&directory, true)?;
    metadata::layout(&directory)?;
    let mut saved = load(&root)?;
    let conflicts = baseline_conflicts(&directory, &saved)?;
    if !conflicts.is_empty() {
        return Err(fail(conflicts.join("\n")));
    }
    let retried = saved.pending_git;
    if saved.pending_git {
        finish_git(
            &directory,
            &mut saved,
            "reconcile: finish pending structure snapshot",
        )?;
    }
    let snapshot = scan(&root)?;
    let mut report = reconcile(&snapshot, &saved)?;
    report
        .conflicts
        .extend(metadata::draft_conflicts(&directory)?);
    require_resolved(&report, accept_unknown)?;
    let second = scan(&root)?;
    require_stable(&snapshot, &second)?;
    // Recheck the editable baseline immediately before publishing metadata.
    let mut conflicts = baseline_conflicts(&directory, &saved)?;
    conflicts.extend(metadata::draft_conflicts(&directory)?);
    if !conflicts.is_empty() {
        return Err(fail(conflicts.join("\n")));
    }
    let tree_text = render_tree(&report.tree)?;
    let structure_changed = tree_text != saved.tree_text;
    let mut next = SavedState {
        tree: report.tree.clone(),
        tree_text,
        records: report.records.clone(),
        generation: saved
            .generation
            .checked_add(1)
            .ok_or_else(|| fail("identity generation exhausted"))?,
        pending_git: structure_changed,
        ..saved.clone()
    };
    let journal = serde_json::json!({"operation":"sync", "scan_id":snapshot.scan_id, "base_tree":saved.tree_text, "target_tree":next.tree_text, "phase":"publishing_metadata"});
    metadata::write_snapshot(
        &directory.join("state/pending.json"),
        None,
        &(journal.to_string() + "\n"),
    )?;
    metadata::log(
        &directory,
        &serde_json::to_string(&report).map_err(|error| fail(error.to_string()))?,
    )?;
    if structure_changed {
        metadata::write_snapshot(
            &directory.join("structure.tree"),
            Some(&saved.tree_text),
            &next.tree_text,
        )?;
    }
    storage::save(&directory.join("state/index.sqlite"), &next, false)?;
    clear_pending(&directory)?;
    if structure_changed {
        finish_git(
            &directory,
            &mut next,
            "reconcile: update filesystem structure",
        )?;
    }
    report.pending_git = false;
    metadata::log(&directory, "sync: completed")?;
    Ok(SyncReport {
        report,
        structure_changed,
        git_head: next.git_head,
        retried_git_commit: retried,
    })
}

pub(crate) fn clear_pending(directory: &Path) -> Result<()> {
    fs::remove_file(directory.join("state/pending.json"))?;
    fs::File::open(directory.join("state"))?.sync_all()?;
    Ok(())
}

fn finish_git(directory: &Path, state: &mut SavedState, message: &str) -> Result<()> {
    let result = git::commit(directory, state, message);
    match result {
        Ok(head) => {
            state.git_head = Some(head);
            state.pending_git = false;
            storage::save(&directory.join("state/index.sqlite"), state, false).map_err(|error| fail(format!("Git commit was recorded but its SQLite acknowledgement failed: {error}; inspect status and retry sync")))?;
            Ok(())
        }
        Err(error) => Err(fail(format!(
            "structure.tree and SQLite are saved, but Git recording is pending: {error}. Resolve the Git error and run organizer sync to retry; no managed files were moved."
        ))),
    }
}

pub(crate) fn require_complete(snapshot: &ScanSnapshot) -> Result<()> {
    if !snapshot.complete {
        return Err(fail(format!(
            "scan incomplete; no snapshot published: {:?}",
            snapshot.errors
        )));
    }
    Ok(())
}

fn require_stable(first: &ScanSnapshot, second: &ScanSnapshot) -> Result<()> {
    require_complete(first)?;
    require_complete(second)?;
    if first.entries != second.entries || first.warnings != second.warnings {
        return Err(fail(
            "filesystem changed between verification scans; no snapshot published, retry when stable",
        ));
    }
    Ok(())
}

fn require_resolved(report: &ReconcileReport, accept_unknown: bool) -> Result<()> {
    if !report.scan_complete || !report.conflicts.is_empty() {
        return Err(fail(format!(
            "sync blocked: {}",
            report.conflicts.join("; ")
        )));
    }
    if !accept_unknown && !report.unknown.is_empty() {
        return Err(fail(
            "identity matches are unresolved; inspect organizer status. Use sync --accept-unknown only to record these as missing + new identities, without inferring moves",
        ));
    }
    Ok(())
}

/// Read-only preflight for structural plan. Draft files are expected here.
pub fn ensure_current(root: &Path, old: &TreeSpec) -> Result<()> {
    let root = metadata::canonical_root(root)?;
    let directory = root.join(".organizer");
    metadata::layout(&directory)?;
    let _lock = metadata::lock(&directory, false)?;
    metadata::layout(&directory)?;
    let saved = load(&root)?;
    let conflicts = baseline_conflicts(&directory, &saved)?;
    if !conflicts.is_empty() || saved.pending_git {
        return Err(fail(
            "baseline metadata is conflicted or Git recording is pending; resolve it before plan",
        ));
    }
    if render_tree(old)? != saved.tree_text {
        return Err(fail("--from is not the recorded baseline for this root"));
    }
    let report = reconcile(&scan(&root)?, &saved)?;
    if !report.scan_complete
        || !report.conflicts.is_empty()
        || !report.added.is_empty()
        || !report.missing.is_empty()
        || !report.moved.is_empty()
        || !report.changed.is_empty()
        || !report.unknown.is_empty()
    {
        return Err(fail(
            "baseline is stale or scan is incomplete; inspect status and sync before plan",
        ));
    }
    Ok(())
}

pub fn history(root: &Path) -> Result<String> {
    let root = metadata::canonical_root(root)?;
    let directory = root.join(".organizer");
    metadata::layout(&directory)?;
    let _lock = metadata::lock(&directory, false)?;
    git::history(&directory, &load(&root)?)
}
