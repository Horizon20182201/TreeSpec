use super::{FileStamp, ScanEntry, ScanIssue, ScanIssueKind, ScanSnapshot};
use crate::{
    error::{Result, fail},
    storage::{IndexedNode, SavedState},
    tree::{NodeKind, TreeNode, TreeSpec, validate_tree},
};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize)]
pub struct ObservedChange {
    pub id: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct UnknownIdentity {
    pub path: String,
    pub candidate_ids: Vec<String>,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReconcileReport {
    pub scan_complete: bool,
    pub added: Vec<ObservedChange>,
    pub missing: Vec<ObservedChange>,
    pub moved: Vec<ObservedChange>,
    pub changed: Vec<ObservedChange>,
    pub unknown: Vec<UnknownIdentity>,
    pub conflicts: Vec<String>,
    pub warnings: Vec<ScanIssue>,
    pub pending_git: bool,
    #[serde(skip)]
    pub tree: TreeSpec,
    #[serde(skip)]
    pub records: Vec<IndexedNode>,
}

pub fn compatible_identity(old: &FileStamp, new: &FileStamp) -> bool {
    !matches!((&old.platform_file_key, &new.platform_file_key), (Some(a),Some(b)) if a != b)
        && !matches!((&old.birthtime, &new.birthtime), (Some(a),Some(b)) if a != b)
}

fn safe_move(old: &ScanEntry, new: &ScanEntry) -> bool {
    old.kind == new.kind
        && old.stamp.platform_file_key.is_some()
        && old.stamp.platform_file_key == new.stamp.platform_file_key
        && compatible_identity(&old.stamp, &new.stamp)
        && (old.kind == NodeKind::Directory
            || (old.stamp.links == 1
                && new.stamp.links == 1
                && old.stamp.size == new.stamp.size
                && old.stamp.mtime == new.stamp.mtime))
}

/// Ambiguous candidates receive new provisional IDs, never inferred renames.
pub fn reconcile(snapshot: &ScanSnapshot, saved: &SavedState) -> Result<ReconcileReport> {
    if snapshot.root != saved.root {
        return Err(fail(
            "scan and saved state refer to different management roots",
        ));
    }
    let mut report = ReconcileReport {
        scan_complete: snapshot.complete,
        added: vec![],
        missing: vec![],
        moved: vec![],
        changed: vec![],
        unknown: vec![],
        conflicts: snapshot
            .errors
            .iter()
            .map(|issue| format!("incomplete scan at {:?}: {}", issue.path, issue.reason))
            .collect(),
        warnings: snapshot.warnings.clone(),
        pending_git: saved.pending_git,
        tree: saved.tree.clone(),
        records: saved.records.clone(),
    };
    if !snapshot.complete {
        if report.conflicts.is_empty() {
            report
                .conflicts
                .push("incomplete scan; no removals inferred".into());
        }
        return Ok(report);
    }
    let old_paths: HashMap<_, _> = saved
        .records
        .iter()
        .enumerate()
        .map(|(i, r)| (r.entry.path.as_str(), i))
        .collect();
    let mut old_keys: HashMap<&str, Vec<usize>> = HashMap::new();
    let mut new_keys: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, record) in saved.records.iter().enumerate() {
        if let Some(key) = &record.entry.stamp.platform_file_key {
            old_keys.entry(key).or_default().push(i);
        }
    }
    for (i, entry) in snapshot.entries.iter().enumerate() {
        if let Some(key) = &entry.stamp.platform_file_key {
            new_keys.entry(key).or_default().push(i);
        }
    }
    let mut matches = vec![None; snapshot.entries.len()];
    let mut used = HashSet::new();
    for (i, entry) in snapshot.entries.iter().enumerate() {
        if let Some(&old) = old_paths.get(entry.path.as_str()) {
            let previous = &saved.records[old].entry;
            if previous.kind == entry.kind && compatible_identity(&previous.stamp, &entry.stamp) {
                matches[i] = Some(old);
                used.insert(old);
            }
        }
    }
    for (i, entry) in snapshot.entries.iter().enumerate() {
        if matches[i].is_some() {
            continue;
        }
        let Some(key) = entry.stamp.platform_file_key.as_deref() else {
            continue;
        };
        let candidates: Vec<_> = old_keys
            .get(key)
            .into_iter()
            .flatten()
            .copied()
            .filter(|old| {
                !used.contains(old)
                    && compatible_identity(&saved.records[*old].entry.stamp, &entry.stamp)
            })
            .collect();
        if candidates.len() == 1
            && old_keys[key].len() == 1
            && new_keys[key].len() == 1
            && safe_move(&saved.records[candidates[0]].entry, entry)
        {
            matches[i] = Some(candidates[0]);
            used.insert(candidates[0]);
        } else if !candidates.is_empty() {
            report.unknown.push(UnknownIdentity {
                path: entry.path.clone(),
                candidate_ids: candidates
                    .iter()
                    .map(|&old| saved.records[old].logical_id.clone())
                    .collect(),
                reason: "file-key match is ambiguous or metadata changed; no rename inferred"
                    .into(),
            });
        }
    }
    let old_nodes: HashMap<_, _> = saved
        .tree
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let namespace =
        Uuid::parse_str(&saved.namespace).map_err(|_| fail("invalid identity namespace"))?;
    let mut reserved: HashSet<_> = saved.records.iter().map(|r| r.logical_id.clone()).collect();
    let mut records = Vec::with_capacity(snapshot.entries.len());
    for (i, entry) in snapshot.entries.iter().enumerate() {
        let id = if let Some(old) = matches[i] {
            let previous = &saved.records[old];
            if previous.entry.protected != entry.protected {
                report.conflicts.push(format!(
                    "protected Git boundary changed at {:?}",
                    entry.path
                ));
            }
            if previous.entry.path != entry.path {
                report.moved.push(ObservedChange {
                    id: previous.logical_id.clone(),
                    path: entry.path.clone(),
                    old_path: Some(previous.entry.path.clone()),
                });
            }
            if entry.kind == NodeKind::File
                && (previous.entry.stamp.size != entry.stamp.size
                    || previous.entry.stamp.mtime != entry.stamp.mtime
                    || (previous.entry.path == entry.path
                        && previous.entry.stamp.ctime != entry.stamp.ctime))
            {
                report
                    .changed
                    .push(change(&previous.logical_id, &entry.path));
            }
            previous.logical_id.clone()
        } else {
            let prefix = if entry.kind == NodeKind::Directory {
                "d"
            } else {
                "f"
            };
            let mut salt = 0u64;
            let id = loop {
                let seed = format!(
                    "{}:{prefix}:{salt}:{}:{:?}:{:?}",
                    saved.generation,
                    entry.path,
                    entry.stamp.platform_file_key,
                    entry.stamp.birthtime
                );
                let id = format!("{prefix}_{}", Uuid::new_v5(&namespace, seed.as_bytes()));
                if reserved.insert(id.clone()) {
                    break id;
                }
                salt += 1;
            };
            report.added.push(change(&id, &entry.path));
            id
        };
        records.push(IndexedNode {
            logical_id: id,
            entry: entry.clone(),
            last_seen_scan: snapshot.scan_id.clone(),
        });
    }
    let protected: HashSet<_> = snapshot
        .entries
        .iter()
        .filter(|entry| entry.protected)
        .map(|entry| entry.path.as_str())
        .collect();
    let skipped: HashSet<_> = snapshot
        .warnings
        .iter()
        .filter(|issue| issue.kind == ScanIssueKind::Skipped)
        .map(|issue| issue.path.as_str())
        .collect();
    for (i, record) in saved.records.iter().enumerate() {
        if !used.contains(&i) {
            report
                .missing
                .push(change(&record.logical_id, &record.entry.path));
            let mut hidden = skipped.contains(record.entry.path.as_str());
            let mut path = record.entry.path.as_str();
            while let Some((parent, _)) = path.rsplit_once('/') {
                hidden |= protected.contains(parent) || skipped.contains(parent);
                path = parent;
            }
            if hidden {
                report.conflicts.push(format!(
                    "previously tracked path {:?} is now hidden by a protected or skipped entry",
                    record.entry.path
                ));
            }
        }
    }
    let by_path: HashMap<_, _> = records
        .iter()
        .map(|record| (record.entry.path.as_str(), record.logical_id.as_str()))
        .collect();
    let mut tree = TreeSpec::default();
    for record in &records {
        let (parent_id, name) = match record.entry.path.rsplit_once('/') {
            Some((parent, name)) => (
                Some(
                    by_path
                        .get(parent)
                        .ok_or_else(|| fail("scan has a missing parent"))?
                        .to_string(),
                ),
                name,
            ),
            None => (None, record.entry.path.as_str()),
        };
        tree.nodes.push(TreeNode {
            id: record.logical_id.clone(),
            parent_id,
            name: name.into(),
            kind: record.entry.kind,
            description: old_nodes
                .get(record.logical_id.as_str())
                .and_then(|node| node.description.clone()),
        });
    }
    validate_tree(&tree)?;
    report.tree = tree;
    report.records = records;
    Ok(report)
}

fn change(id: &str, path: &str) -> ObservedChange {
    ObservedChange {
        id: id.into(),
        path: path.into(),
        old_path: None,
    }
}
