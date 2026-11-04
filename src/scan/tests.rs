use super::{identity::reconcile, *};
use crate::{
    storage::{IndexedNode, SavedState},
    tree::{NodeKind, parse_normalized_tree, render_tree},
};

fn entry(path: &str, key: &str) -> ScanEntry {
    ScanEntry {
        path: path.into(),
        kind: NodeKind::File,
        protected: false,
        stamp: FileStamp {
            platform_file_key: Some(key.into()),
            size: 10,
            mtime: Some("10:0".into()),
            birthtime: Some("1:0".into()),
            ctime: Some("10:0".into()),
            links: 1,
        },
    }
}

fn saved() -> SavedState {
    let entry = entry("file", "dev:inode");
    let text = "!tree 1\nfile @f # description\n";
    SavedState {
        root: "/managed".into(),
        root_stamp: entry.stamp.clone(),
        namespace: uuid::Uuid::nil().to_string(),
        generation: 1,
        tree: parse_normalized_tree(text).unwrap(),
        tree_text: text.into(),
        records: vec![IndexedNode {
            logical_id: "f".into(),
            entry,
            last_seen_scan: "old".into(),
        }],
        git_head: None,
        pending_git: false,
    }
}
fn snapshot(entries: Vec<ScanEntry>) -> ScanSnapshot {
    ScanSnapshot {
        scan_id: "new".into(),
        root: "/managed".into(),
        complete: true,
        entries,
        warnings: vec![],
        errors: vec![],
    }
}

#[test]
fn reused_inode_with_different_birthtime_gets_a_new_identity() {
    let saved = saved();
    let mut replacement = saved.records[0].entry.clone();
    replacement.stamp.birthtime = Some("2:0".into());
    let report = reconcile(&snapshot(vec![replacement]), &saved).unwrap();
    assert_eq!(report.added.len(), 1);
    assert_eq!(report.missing.len(), 1);
    assert!(report.unknown.is_empty());
    assert!(report.moved.is_empty());
    assert_ne!(report.tree.nodes[0].id, "f");
    assert_eq!(report.tree.nodes[0].description, None);
}

#[test]
fn changed_same_path_keeps_identity_and_description_but_copy_does_not() {
    let saved = saved();
    let mut changed = saved.records[0].entry.clone();
    changed.stamp.ctime = Some("20:0".into());
    let report = reconcile(&snapshot(vec![changed.clone()]), &saved).unwrap();
    assert_eq!(report.changed.len(), 1);
    assert_eq!(render_tree(&report.tree).unwrap(), saved.tree_text);
    let report = reconcile(
        &snapshot(vec![entry("copy", "other:inode"), changed]),
        &saved,
    )
    .unwrap();
    assert_eq!(report.added.len(), 1);
    assert!(report.missing.is_empty());
    assert_ne!(report.added[0].id, "f");
    assert!(report.moved.is_empty());
}

#[test]
fn matching_name_and_metadata_without_matching_file_key_is_not_a_move() {
    let saved = saved();
    let report = reconcile(&snapshot(vec![entry("similar-file", "new:key")]), &saved).unwrap();
    assert_eq!(report.missing.len(), 1);
    assert_eq!(report.added.len(), 1);
    assert!(report.moved.is_empty());
}

#[test]
fn hardlink_move_candidates_are_never_arbitrarily_paired() {
    let mut saved = saved();
    saved.tree = parse_normalized_tree("!tree 1\nfile @f\nother @g\n").unwrap();
    saved.records[0].entry.stamp.links = 2;
    let mut second = saved.records[0].clone();
    second.logical_id = "g".into();
    second.entry.path = "other".into();
    saved.records.push(second);
    let mut a = entry("moved-a", "dev:inode");
    a.stamp.links = 2;
    let mut b = a.clone();
    b.path = "moved-b".into();
    let report = reconcile(&snapshot(vec![a, b]), &saved).unwrap();
    assert_eq!(report.unknown.len(), 2);
    assert_eq!(report.missing.len(), 2);
    assert_eq!(report.added.len(), 2);
    assert!(report.moved.is_empty());
}

#[test]
fn incomplete_scan_keeps_old_tree_and_never_reports_missing_nodes() {
    let saved = saved();
    let mut scan = snapshot(vec![]);
    scan.complete = false;
    let report = reconcile(&scan, &saved).unwrap();
    assert!(report.missing.is_empty());
    assert_eq!(report.tree, saved.tree);
    assert!(!report.conflicts.is_empty());
    scan.root = "/different".into();
    assert!(reconcile(&scan, &saved).is_err());
}

#[test]
fn skipped_entry_conflicts_use_structured_kind_not_warning_text() {
    let saved = saved();
    let mut scan = snapshot(vec![]);
    scan.warnings.push(ScanIssue {
        path: "file".into(),
        kind: ScanIssueKind::Skipped,
        reason: "localized diagnostic".into(),
    });
    assert!(!reconcile(&scan, &saved).unwrap().conflicts.is_empty());
}
