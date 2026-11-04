use crate::{
    error::{Result, fail},
    scan::{FileStamp, ScanEntry, ScanIssue, ScanSnapshot},
    storage::SavedState,
    tree::{NodeKind, TreeSpec, compare_trees, relative_paths, render_tree},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Mkdir {
        directory_id: String,
        path: String,
    },
    MoveFile {
        file_id: String,
        from: String,
        to: String,
    },
    MoveDirectory {
        directory_id: String,
        from: String,
        to: String,
    },
}

impl Operation {
    pub fn id(&self) -> &str {
        match self {
            Self::Mkdir { directory_id, .. } | Self::MoveDirectory { directory_id, .. } => {
                directory_id
            }
            Self::MoveFile { file_id, .. } => file_id,
        }
    }
    pub fn source(&self) -> Option<&str> {
        match self {
            Self::Mkdir { .. } => None,
            Self::MoveFile { from, .. } | Self::MoveDirectory { from, .. } => Some(from),
        }
    }
    pub fn destination(&self) -> &str {
        match self {
            Self::Mkdir { path, .. } => path,
            Self::MoveFile { to, .. } | Self::MoveDirectory { to, .. } => to,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Precondition {
    pub id: String,
    pub entry: ScanEntry,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationPlan {
    pub schema_version: u32,
    pub plan_id: String,
    pub root: PathBuf,
    pub root_identity: FileStamp,
    pub base_generation: u64,
    pub base_git_head: Option<String>,
    pub base_tree_digest: String,
    pub target_tree_digest: String,
    pub target_tree: String,
    pub scan_id: String,
    pub preconditions: Vec<Precondition>,
    pub boundaries: Vec<ScanIssue>,
    pub operations: Vec<Operation>,
    pub unresolved: Vec<String>,
    pub warnings: Vec<String>,
}

pub struct PlanningContext<'a> {
    pub saved: &'a SavedState,
    pub snapshot: &'a ScanSnapshot,
    pub target: &'a TreeSpec,
}

pub fn digest(text: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(text.as_bytes()))
}

pub(crate) fn plan_id(plan: &MigrationPlan) -> Result<String> {
    let mut unsigned = plan.clone();
    unsigned.plan_id.clear();
    Ok(digest(
        &serde_json::to_string(&unsigned).map_err(|e| fail(e.to_string()))?,
    ))
}

pub(crate) fn within(path: &str, ancestor: &str) -> bool {
    path == ancestor
        || path
            .strip_prefix(ancestor)
            .is_some_and(|tail| tail.starts_with('/'))
}

pub(crate) fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

pub fn build_plan(ctx: &PlanningContext<'_>) -> Result<MigrationPlan> {
    super::validate::current(ctx.saved, ctx.snapshot)?;
    compare_trees(&ctx.saved.tree, ctx.target)?;
    let target_text = render_tree(ctx.target)?;
    let old_paths = relative_paths(&ctx.saved.tree)?;
    let target_paths = relative_paths(ctx.target)?;
    let by_path: HashMap<_, _> = ctx
        .snapshot
        .entries
        .iter()
        .map(|entry| (entry.path.as_str(), entry))
        .collect();
    let mut preconditions: Vec<_> = ctx
        .saved
        .records
        .iter()
        .map(|record| Precondition {
            id: record.logical_id.clone(),
            entry: by_path[record.entry.path.as_str()].clone(),
        })
        .collect();
    preconditions.sort_by(|a, b| a.id.cmp(&b.id));
    let mut plan = MigrationPlan {
        schema_version: 1,
        plan_id: String::new(),
        root: ctx.saved.root.clone(),
        root_identity: ctx.saved.root_stamp.clone(),
        base_generation: ctx.saved.generation,
        base_git_head: ctx.saved.git_head.clone(),
        base_tree_digest: digest(&ctx.saved.tree_text),
        target_tree_digest: digest(&target_text),
        target_tree: target_text,
        scan_id: ctx.snapshot.scan_id.clone(),
        preconditions,
        boundaries: ctx.snapshot.warnings.clone(),
        operations: vec![],
        unresolved: vec![],
        warnings: vec![],
    };
    for node in &ctx.saved.tree.nodes {
        if !target_paths.contains_key(&node.id) {
            plan.unresolved.push(format!(
                "missing {} at {:?}: deletion and empty-directory removal are not supported",
                node.id, old_paths[&node.id]
            ));
        }
    }
    for node in &ctx.target.nodes {
        let path = &target_paths[&node.id];
        super::validate::relative_path(path)?;
        if node.kind == NodeKind::File && !old_paths.contains_key(&node.id) {
            plan.unresolved
                .push(format!("new file {} at {path:?} has no source", node.id));
        }
        if plan
            .boundaries
            .iter()
            .any(|boundary| within(path, &boundary.path))
            && old_paths.get(&node.id) != Some(path)
        {
            plan.unresolved.push(format!(
                "target {path:?} touches an excluded, protected or skipped path"
            ));
        }
    }
    // A subtree move must not implicitly move unscanned contents, protected
    // projects, hard-linked files, or another filesystem's mount point.
    let root_device = plan
        .root_identity
        .platform_file_key
        .as_deref()
        .and_then(|key| key.split_once(':'))
        .map(|(dev, _)| dev);
    for condition in &plan.preconditions {
        let source = &condition.entry.path;
        if target_paths.get(&condition.id) == Some(source) {
            continue;
        }
        let affected: Vec<_> = plan
            .preconditions
            .iter()
            .filter(|p| within(&p.entry.path, source))
            .collect();
        if affected.iter().any(|p| {
            p.entry.protected || (p.entry.kind == NodeKind::File && p.entry.stamp.links != 1)
        }) || plan
            .boundaries
            .iter()
            .any(|boundary| within(&boundary.path, source))
        {
            plan.unresolved.push(format!(
                "source {source:?} contains protected/skipped metadata or hard links"
            ));
        }
        if affected.iter().any(|p| {
            p.entry
                .stamp
                .platform_file_key
                .as_deref()
                .and_then(|key| key.split_once(':'))
                .map(|(dev, _)| dev)
                != root_device
        }) {
            plan.unresolved
                .push(format!("source {source:?} crosses a filesystem boundary"));
        }
    }
    if plan.unresolved.is_empty() {
        schedule(ctx.target, &old_paths, &target_paths, &mut plan)?;
    }
    plan.unresolved.sort();
    plan.unresolved.dedup();
    if !plan.unresolved.is_empty() {
        plan.operations.clear();
    }
    plan.plan_id = plan_id(&plan)?;
    Ok(plan)
}

/// Simulate the namespace after each operation. Ancestors settle before their
/// children; children carried by a directory move require no extra operation.
fn schedule(
    target: &TreeSpec,
    old: &HashMap<String, String>,
    paths: &HashMap<String, String>,
    plan: &mut MigrationPlan,
) -> Result<()> {
    let mut current: BTreeMap<String, String> = old
        .iter()
        .map(|(id, path)| (id.clone(), path.clone()))
        .collect();
    let mut nodes: Vec<_> = target.nodes.iter().collect();
    nodes.sort_by_key(|node| {
        (
            paths[&node.id].matches('/').count(),
            &paths[&node.id],
            &node.id,
        )
    });
    loop {
        let mut progressed = false;
        for node in &nodes {
            let to = &paths[&node.id];
            if current.get(&node.id) == Some(to) {
                continue;
            }
            if node
                .parent_id
                .as_ref()
                .is_some_and(|id| current.get(id) != paths.get(id))
            {
                continue;
            }
            let occupied: HashSet<_> = current.values().collect();
            if occupied.contains(to) {
                continue;
            }
            let operation = match current.get(&node.id).cloned() {
                None => Operation::Mkdir {
                    directory_id: node.id.clone(),
                    path: to.clone(),
                },
                Some(from) => {
                    if node.kind == NodeKind::Directory && within(to, &from) {
                        continue;
                    }
                    if node.kind == NodeKind::Directory {
                        for path in current.values_mut() {
                            if within(path, &from) {
                                *path = format!("{to}{}", &path[from.len()..]);
                            }
                        }
                        Operation::MoveDirectory {
                            directory_id: node.id.clone(),
                            from,
                            to: to.clone(),
                        }
                    } else {
                        Operation::MoveFile {
                            file_id: node.id.clone(),
                            from,
                            to: to.clone(),
                        }
                    }
                }
            };
            current.insert(node.id.clone(), to.clone());
            plan.operations.push(operation);
            progressed = true;
        }
        if current.len() == paths.len()
            && current.iter().all(|(id, path)| paths.get(id) == Some(path))
        {
            break;
        }
        if !progressed {
            plan.unresolved.push("occupied destination or cyclic path dependency; temporary staging/path exchanges are unsupported".into());
            break;
        }
    }
    Ok(())
}
