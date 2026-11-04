use super::{
    MigrationPlan, Operation,
    filesystem::Filesystem,
    journal::{Journal, RecoveryReport},
    plan::{parent, within},
    validate,
};
use crate::{
    error::{Result, fail},
    git, metadata,
    scan::{ScanEntry, ScanSnapshot, identity::compatible_identity, scan},
    storage::{self, IndexedNode, SavedState},
    sync,
    tree::{NodeKind, parse_normalized_tree, relative_paths},
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
};

#[derive(Debug, Serialize)]
pub struct ExecutionReport {
    pub run_id: String,
    pub plan_id: String,
    pub completed_operations: usize,
    pub git_head: Option<String>,
}

/// Call only after explicit user confirmation. The CLI defaults to validate_plan.
pub fn apply_plan(plan: &MigrationPlan, root: &Path) -> Result<ExecutionReport> {
    apply_with_hook(plan, root, |_| Ok(()))
}

fn apply_with_hook(
    plan: &MigrationPlan,
    root: &Path,
    mut after_step: impl FnMut(usize) -> Result<()>,
) -> Result<ExecutionReport> {
    let (_lock, saved, snapshot) = validate::context(root, true)?;
    validate::against(plan, &saved, &snapshot)?;
    let directory = saved.root.join(".organizer");
    let filesystem = Filesystem::open(&saved.root, &saved.root_stamp)?;
    let mut journal = Journal::begin(&directory, plan)?;
    let mut entries: BTreeMap<_, _> = plan
        .preconditions
        .iter()
        .map(|p| (p.id.clone(), p.entry.clone()))
        .collect();
    let mut completed = 0;
    let result = (|| {
        for (position, operation) in plan.operations.iter().enumerate() {
            exact_snapshot(&scan(&saved.root)?, &entries, &snapshot)?;
            let result = filesystem.execute(operation, &entries)?;
            let next = scan(&saved.root)?;
            advance(&mut entries, operation, result, &next, &snapshot)?;
            journal.done(position)?;
            completed += 1;
            after_step(completed)?;
        }
        let final_scan = scan(&saved.root)?;
        exact_snapshot(&final_scan, &entries, &snapshot)?;
        let target = parse_normalized_tree(&plan.target_tree)?;
        let paths = relative_paths(&target)?;
        if paths.len() != entries.len()
            || entries
                .iter()
                .any(|(id, entry)| paths.get(id) != Some(&entry.path))
        {
            return Err(fail(
                "actual final structure does not match the planned target",
            ));
        }
        let conflicts = sync::baseline_conflicts(&directory, &saved)?;
        if !conflicts.is_empty() {
            return Err(fail(conflicts.join("; ")));
        }
        // Also reject relocation/replacement of the managed root before publishing.
        sync::load(&saved.root)?;
        let records = entries
            .iter()
            .map(|(id, entry)| IndexedNode {
                logical_id: id.clone(),
                entry: entry.clone(),
                last_seen_scan: final_scan.scan_id.clone(),
            })
            .collect();
        let next = SavedState {
            tree: target,
            tree_text: plan.target_tree.clone(),
            records,
            generation: saved
                .generation
                .checked_add(1)
                .ok_or_else(|| fail("identity generation exhausted"))?,
            pending_git: saved.tree_text != plan.target_tree,
            ..saved.clone()
        };
        journal.status("publishing", None)?;
        let marker = serde_json::json!({"operation":"migration", "run_id":journal.run_id, "plan_id":plan.plan_id, "phase":"publishing_metadata", "base_tree":saved.tree_text, "target_tree":plan.target_tree});
        metadata::write_snapshot(
            &directory.join("state/pending.json"),
            None,
            &(marker.to_string() + "\n"),
        )?;
        if next.tree_text != saved.tree_text {
            metadata::write_snapshot(
                &directory.join("structure.tree"),
                Some(&saved.tree_text),
                &next.tree_text,
            )?;
        }
        storage::save(&directory.join("state/index.sqlite"), &next, false)?;
        sync::clear_pending(&directory)?;
        Ok(next)
    })();
    let mut next = match result {
        Ok(state) => state,
        Err(error) => {
            let message = format!(
                "migration {} stopped after {completed}/{} acknowledged operations: {error}. File changes may already exist; run organizer recover {}. No automatic replay.",
                journal.run_id,
                plan.operations.len(),
                journal.run_id
            );
            journal
                .status("failed", Some(&message))
                .map_err(|log_error| {
                    fail(format!(
                        "{message}; journal update also failed: {log_error}"
                    ))
                })?;
            return Err(fail(message));
        }
    };
    journal.status("git_pending", None).map_err(|error| fail(format!(
        "file migration and metadata publication completed, but journal acknowledgement failed: {error}; inspect organizer recover {} before retrying Git", journal.run_id
    )))?;
    finish_git(&directory, &mut next, &journal)?;
    Ok(ExecutionReport {
        run_id: journal.run_id,
        plan_id: plan.plan_id.clone(),
        completed_operations: completed,
        git_head: next.git_head,
    })
}

fn exact_snapshot(
    snapshot: &ScanSnapshot,
    entries: &BTreeMap<String, ScanEntry>,
    original: &ScanSnapshot,
) -> Result<()> {
    sync::require_complete(snapshot)?;
    let mut expected: Vec<_> = entries.values().cloned().collect();
    expected.sort_by(|a, b| a.path.cmp(&b.path));
    if snapshot.entries != expected || snapshot.warnings != original.warnings {
        return Err(fail(
            "filesystem changed during migration; source attributes, paths or scan boundaries differ",
        ));
    }
    Ok(())
}

/// Accept only metadata changes caused by this step. Parent-directory times,
/// sizes and link counts can change; unrelated files must remain byte-metadata
/// identical, and moved files retain identity, size, mtime and link count.
fn advance(
    entries: &mut BTreeMap<String, ScanEntry>,
    operation: &Operation,
    result: ScanEntry,
    snapshot: &ScanSnapshot,
    original: &ScanSnapshot,
) -> Result<()> {
    sync::require_complete(snapshot)?;
    if snapshot.warnings != original.warnings {
        return Err(fail("scan boundaries changed during migration"));
    }
    if let Some(source) = operation.source() {
        for entry in entries.values_mut() {
            if within(&entry.path, source) {
                entry.path = format!("{}{}", operation.destination(), &entry.path[source.len()..]);
            }
        }
    }
    entries.insert(operation.id().into(), result);
    let observed: HashMap<_, _> = snapshot
        .entries
        .iter()
        .map(|entry| (entry.path.as_str(), entry))
        .collect();
    if entries.len() != observed.len() {
        return Err(fail(
            "unexpected nodes appeared/disappeared during migration",
        ));
    }
    for expected in entries.values_mut() {
        let actual = observed
            .get(expected.path.as_str())
            .ok_or_else(|| fail("expected path missing after operation"))?;
        let parent_changed = expected.kind == NodeKind::Directory
            && (expected.path == parent(operation.destination())
                || operation
                    .source()
                    .is_some_and(|source| expected.path == parent(source)));
        if parent_changed {
            if expected.kind != actual.kind
                || expected.protected != actual.protected
                || expected.stamp.platform_file_key != actual.stamp.platform_file_key
                || expected.stamp.birthtime != actual.stamp.birthtime
            {
                return Err(fail("parent identity changed during migration"));
            }
        } else if expected != *actual {
            return Err(fail(format!(
                "unexpected attribute change at {:?}",
                expected.path
            )));
        }
        *expected = (*actual).clone();
    }
    Ok(())
}

fn finish_git(directory: &Path, next: &mut SavedState, journal: &Journal) -> Result<()> {
    let result = (|| {
        if next.pending_git {
            next.git_head = Some(git::commit(
                directory,
                next,
                "refactor: apply reviewed migration",
            )?);
            next.pending_git = false;
            storage::save(&directory.join("state/index.sqlite"), next, false)?;
        }
        journal.status("complete", None)?;
        Ok::<_, crate::error::OrganizerError>(())
    })();
    result.map_err(|error| fail(format!("file migration and metadata publication completed; Git recording/acknowledgement is pending: {error}. Use organizer recover {} --finish-git; do not reapply moves.", journal.run_id)))
}

pub(crate) fn finish_recovered_git(directory: &Path, report: &RecoveryReport) -> Result<()> {
    if report.status == "complete" {
        return Ok(());
    }
    if report.metadata_update_pending || report.completed_steps != report.plan.operations.len() {
        return Err(fail(
            "partial migration/publication requires manual recovery; --finish-git never replays file operations",
        ));
    }
    let mut saved = sync::load(&report.plan.root)?;
    if saved.tree_text != report.plan.target_tree
        || Some(saved.generation) != report.plan.base_generation.checked_add(1)
    {
        return Err(fail(
            "target metadata is not fully published; manual recovery required",
        ));
    }
    let conflicts = sync::baseline_conflicts(directory, &saved)?;
    if !conflicts.is_empty() {
        return Err(fail(conflicts.join("; ")));
    }
    // Use the published index, including identities of newly created directories.
    let entries = saved
        .records
        .iter()
        .map(|record| (record.logical_id.clone(), record.entry.clone()))
        .collect();
    let mut original = report.current_scan.clone();
    original.warnings = report.plan.boundaries.clone();
    exact_snapshot(&report.current_scan, &entries, &original)?;
    for condition in &report.plan.preconditions {
        let record = saved
            .records
            .iter()
            .find(|r| r.logical_id == condition.id)
            .ok_or_else(|| fail("published index lost a source ID"))?;
        if !compatible_identity(&condition.entry.stamp, &record.entry.stamp) {
            return Err(fail("published source identity differs from plan"));
        }
    }
    let journal = Journal::resume_metadata(directory, &report.run_id)?;
    finish_git(directory, &mut saved, &journal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{TreeNode, render_tree};
    use std::fs;

    #[test]
    fn interrupted_run_is_logged_and_never_replayed() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::write(root.join("file"), "unchanged").unwrap();
        sync::init(root).unwrap();
        let old = storage::load(&root.join(".organizer/state/index.sqlite"))
            .unwrap()
            .tree;
        let mut target = old.clone();
        target.nodes[0].parent_id = Some("directory".into());
        target.nodes.push(TreeNode {
            id: "directory".into(),
            parent_id: None,
            name: "new".into(),
            kind: NodeKind::Directory,
            description: None,
        });
        let plan = crate::migration::plan(root, &old, &target).unwrap();
        let error =
            apply_with_hook(&plan, root, |_| Err(fail("injected interruption"))).unwrap_err();
        assert!(error.to_string().contains("1/2 acknowledged"));
        assert!(root.join("new").is_dir());
        assert_eq!(fs::read(root.join("file")).unwrap(), b"unchanged");
        assert_eq!(
            fs::read_to_string(root.join(".organizer/structure.tree")).unwrap(),
            render_tree(&old).unwrap()
        );
        assert!(
            apply_plan(&plan, root)
                .unwrap_err()
                .to_string()
                .contains("unfinished migration")
        );
        assert!(sync::sync(root, false).is_err());
        let db =
            rusqlite::Connection::open(root.join(".organizer/state/operations.sqlite")).unwrap();
        let run_id: String = db
            .query_row("SELECT run_id FROM runs", [], |row| row.get(0))
            .unwrap();
        let report = crate::migration::journal::recover(root, &run_id, false).unwrap();
        assert_eq!(report.status, "failed");
        assert_eq!(report.completed_steps, 1);
        assert_eq!(report.steps[0].status, "done");
        assert_eq!(report.steps[1].status, "pending");
        assert!(crate::migration::journal::recover(root, &run_id, true).is_err());
    }

    #[test]
    fn crash_after_rename_before_acknowledgement_is_not_replayed() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::write(root.join("file"), "keep").unwrap();
        sync::init(root).unwrap();
        let saved = storage::load(&root.join(".organizer/state/index.sqlite")).unwrap();
        let mut target = saved.tree.clone();
        target.nodes[0].name = "renamed".into();
        let plan = crate::migration::plan(root, &saved.tree, &target).unwrap();
        let journal = Journal::begin(&root.join(".organizer"), &plan).unwrap();
        let run_id = journal.run_id.clone();
        let entries = plan
            .preconditions
            .iter()
            .map(|p| (p.id.clone(), p.entry.clone()))
            .collect();
        Filesystem::open(root, &saved.root_stamp)
            .unwrap()
            .execute(&plan.operations[0], &entries)
            .unwrap();
        // Simulate a process exit before journal.done, despite a durable rename.
        drop(journal);
        let report = crate::migration::journal::recover(root, &run_id, false).unwrap();
        assert_eq!(report.completed_steps, 0);
        assert_eq!(report.steps[0].status, "pending");
        assert!(
            report
                .current_scan
                .entries
                .iter()
                .any(|entry| entry.path == "renamed")
        );
        assert!(apply_plan(&plan, root).is_err());
        assert!(crate::migration::journal::recover(root, &run_id, true).is_err());
        assert_eq!(fs::read(root.join("renamed")).unwrap(), b"keep");
    }
}
