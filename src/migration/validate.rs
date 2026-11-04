use super::plan::{MigrationPlan, PlanningContext, build_plan, digest, plan_id};
use crate::{
    error::{Result, fail},
    metadata,
    scan::{ScanSnapshot, identity::reconcile, scan},
    storage::SavedState,
    sync,
    tree::{parse_normalized_tree, render_tree},
};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct ValidationReport {
    pub plan_id: String,
    pub operation_count: usize,
    pub dry_run: bool,
    pub operations: Vec<super::Operation>,
}

pub(crate) fn relative_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.contains(['\\', '\0'])
        || path
            .split('/')
            .any(|part| matches!(part, "" | "." | ".." | ".organizer" | ".git"))
    {
        return Err(fail(format!("unsafe migration path: {path:?}")));
    }
    Ok(())
}

pub(crate) fn document(plan: &MigrationPlan) -> Result<()> {
    if plan.schema_version != 1 || !plan.root.is_absolute() {
        return Err(fail("unsupported plan version or non-absolute root"));
    }
    let target = parse_normalized_tree(&plan.target_tree)?;
    if render_tree(&target)? != plan.target_tree
        || digest(&plan.target_tree) != plan.target_tree_digest
        || plan_id(plan)? != plan.plan_id
    {
        return Err(fail(
            "plan digest mismatch or noncanonical target Tree; regenerate and review the plan",
        ));
    }
    if !plan.scan_id.starts_with("scan_") || uuid::Uuid::parse_str(&plan.scan_id[5..]).is_err() {
        return Err(fail("invalid scan ID"));
    }
    for operation in &plan.operations {
        relative_path(operation.destination())?;
        if let Some(source) = operation.source() {
            relative_path(source)?;
        }
    }
    Ok(())
}

pub(crate) fn current(saved: &SavedState, snapshot: &ScanSnapshot) -> Result<()> {
    let report = reconcile(snapshot, saved)?;
    if saved.pending_git
        || !report.scan_complete
        || !report.conflicts.is_empty()
        || !report.added.is_empty()
        || !report.missing.is_empty()
        || !report.moved.is_empty()
        || !report.changed.is_empty()
        || !report.unknown.is_empty()
    {
        return Err(fail(
            "baseline is stale, conflicted, or scan incomplete; inspect status and sync before planning/applying",
        ));
    }
    Ok(())
}

pub(crate) fn context(
    root: &Path,
    write: bool,
) -> Result<(metadata::MetadataLock, SavedState, ScanSnapshot)> {
    let root = metadata::canonical_root(root)?;
    let directory = root.join(".organizer");
    metadata::layout(&directory)?;
    let lock = metadata::lock(&directory, write)?;
    metadata::layout(&directory)?;
    let saved = sync::load(&root)?;
    let conflicts = sync::baseline_conflicts(&directory, &saved)?;
    if !conflicts.is_empty() {
        return Err(fail(conflicts.join("; ")));
    }
    let snapshot = scan(&root)?;
    current(&saved, &snapshot)?;
    Ok((lock, saved, snapshot))
}

pub(crate) fn against(
    plan: &MigrationPlan,
    saved: &SavedState,
    snapshot: &ScanSnapshot,
) -> Result<()> {
    document(plan)?;
    if plan.root != saved.root {
        return Err(fail(
            "plan root differs from explicitly selected/current managed root",
        ));
    }
    if !plan.unresolved.is_empty() {
        return Err(fail(format!(
            "plan has unresolved items: {}",
            plan.unresolved.join("; ")
        )));
    }
    // Regenerate from the authoritative baseline and the actual disk, never
    // trust an edited list of operations or caller-provided preconditions.
    let mut snapshot = snapshot.clone();
    snapshot.scan_id.clone_from(&plan.scan_id);
    let target = parse_normalized_tree(&plan.target_tree)?;
    let rebuilt = build_plan(&PlanningContext {
        saved,
        snapshot: &snapshot,
        target: &target,
    })?;
    if &rebuilt != plan {
        return Err(fail(
            "plan is stale or altered: operations/preconditions do not match the current baseline and disk",
        ));
    }
    // Probe descriptor traversal, mount boundaries and all existing sources.
    let filesystem = super::filesystem::Filesystem::open(&saved.root, &saved.root_stamp)?;
    filesystem.preflight(plan)?;
    Ok(())
}

pub fn validate_plan(plan: &MigrationPlan, root: &Path) -> Result<ValidationReport> {
    let (_lock, saved, snapshot) = context(root, false)?;
    against(plan, &saved, &snapshot)?;
    Ok(ValidationReport {
        plan_id: plan.plan_id.clone(),
        operation_count: plan.operations.len(),
        dry_run: true,
        operations: plan.operations.clone(),
    })
}
