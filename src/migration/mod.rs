//! Deterministic plans and a fail-closed, journaled Linux/macOS executor.
pub mod execute;
mod filesystem;
pub mod journal;
pub mod plan;
pub mod validate;

pub use execute::{ExecutionReport, apply_plan, apply_plan_with_progress};
pub use plan::{MigrationPlan, Operation, PlanningContext, build_plan};
pub use validate::{ValidationReport, validate_plan};

use crate::{
    error::{Result, fail},
    tree::TreeSpec,
};
use std::path::Path;

pub fn plan(root: &Path, old: &TreeSpec, target: &TreeSpec) -> Result<MigrationPlan> {
    let (_lock, saved, snapshot) = validate::context(root, false)?;
    if crate::tree::render_tree(old)? != saved.tree_text {
        return Err(fail("--from is not the recorded baseline for this root"));
    }
    let plan = build_plan(&PlanningContext {
        saved: &saved,
        snapshot: &snapshot,
        target,
    })?;
    if plan.unresolved.is_empty() {
        filesystem::Filesystem::open(&saved.root, &saved.root_stamp)?.preflight(&plan)?;
    }
    Ok(plan)
}

/// The wrapper has no user-supplied executable fragments; apply still checks
/// the selected root, plan, disk state and explicit --yes confirmation.
pub fn export_script(path: &Path) -> Result<String> {
    let plan: MigrationPlan = serde_json::from_str(&std::fs::read_to_string(path)?)
        .map_err(|error| fail(format!("invalid migration plan: {error}")))?;
    validate::document(&plan)?;
    let absolute = path.canonicalize()?;
    let quote = |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
    let path = absolute
        .to_str()
        .ok_or_else(|| fail("script plan path must be UTF-8"))?;
    // No --yes: a generated script is a preview until the user opts in.
    Ok(format!(
        "#!/usr/bin/env bash\nset -euo pipefail\norganizer apply {}\n",
        quote(path)
    ))
}
