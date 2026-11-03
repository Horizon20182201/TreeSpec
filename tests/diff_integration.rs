use organizer::{error::TreeError, tree::*};

fn compare(old: &str, new: &str) -> DiffReport {
    compare_trees(&parse_tree(old).unwrap(), &parse_tree(new).unwrap()).unwrap()
}

#[test]
fn directory_rename_does_not_rename_descendants() {
    let report = compare(
        "!tree 1\na/ @d\n  sub/ @s\n    file @f\n",
        "!tree 1\nb/ @d\n  sub/ @s\n    file @f\n",
    );
    assert_eq!(report.changes.len(), 1);
    assert_eq!(report.changes[0].change, ChangeKind::RenameNode);
    assert_eq!(report.changes[0].id, "d");
    assert_eq!(report.effective_path_changes.len(), 3);
    let file = report
        .effective_path_changes
        .iter()
        .find(|change| change.id == "f")
        .unwrap();
    assert_eq!(
        (
            file.old_path.as_str(),
            file.new_path.as_str(),
            file.inherited
        ),
        ("a/sub/file", "b/sub/file", true)
    );
    assert!(report.unresolved.is_empty());
}

#[test]
fn directory_reparent_is_single_change_with_inherited_child_paths() {
    let report = compare(
        "!tree 1\na/ @a\nb/ @b\n  x @x\n",
        "!tree 1\na/ @a\n  b/ @b\n    x @x\n",
    );
    assert_eq!(report.changes.len(), 1);
    assert_eq!(report.changes[0].change, ChangeKind::ReparentNode);
    assert_eq!(report.changes[0].new_path.as_deref(), Some("a/b"));
    assert!(
        report
            .effective_path_changes
            .iter()
            .any(|change| change.id == "x" && change.inherited)
    );
}

#[test]
fn rename_reparent_and_description_can_all_change_together() {
    let report = compare(
        "!tree 1\na/ @a\n  old @f # before\nb/ @b\n",
        "!tree 1\na/ @a\nb/ @b\n  new @f # after\n",
    );
    assert_eq!(
        report.changes.iter().map(|c| c.change).collect::<Vec<_>>(),
        [
            ChangeKind::RenameNode,
            ChangeKind::ReparentNode,
            ChangeKind::UpdateDescription
        ]
    );
    for change in &report.changes {
        assert_eq!(change.old_path.as_deref(), Some("a/old"));
        assert_eq!(change.new_path.as_deref(), Some("b/new"));
        assert_eq!(change.old_description.as_deref(), Some("before"));
        assert_eq!(change.new_description.as_deref(), Some("after"));
    }
}

#[test]
fn moving_file_to_root_is_reparenting() {
    let report = compare("!tree 1\na/ @a\n  x @x\n", "!tree 1\na/ @a\nx @x\n");
    assert_eq!(report.changes[0].change, ChangeKind::ReparentNode);
    assert_eq!(report.changes[0].new_path.as_deref(), Some("x"));
}

#[test]
fn missing_and_new_files_remain_unresolved_even_at_same_path() {
    let report = compare("!tree 1\nx @old\n", "!tree 1\nx @new\n");
    assert_eq!(report.changes.len(), 2);
    assert_eq!(report.unresolved.len(), 2);
    assert!(
        report
            .unresolved
            .iter()
            .any(|item| item.reason == UnresolvedKind::UnresolvedMissingFile)
    );
    assert!(
        report
            .unresolved
            .iter()
            .any(|item| item.reason == UnresolvedKind::UnresolvedNewFile)
    );
    assert!(report.effective_path_changes.is_empty());
    let json = serde_json::to_value(report).unwrap();
    assert_eq!(json["report_type"], "tree_diff");
    assert!(json.get("operations").is_none());
}

#[test]
fn missing_subtree_reports_every_old_file() {
    let report = compare("!tree 1\na/ @a\n  x @x\n  sub/ @b\n    y @y\n", "!tree 1\n");
    assert_eq!(report.changes.len(), 4);
    assert_eq!(report.unresolved.len(), 2);
    assert!(
        report
            .unresolved
            .iter()
            .all(|item| item.reason == UnresolvedKind::UnresolvedMissingFile)
    );
}

#[test]
fn added_empty_directory_does_not_relocate_files() {
    let report = compare("!tree 1\nx @x\n", "!tree 1\nx @x\nnew/ @d\n");
    assert_eq!(report.changes.len(), 1);
    assert_eq!(report.changes[0].change, ChangeKind::AddNode);
    assert_eq!(report.changes[0].node_kind, NodeKind::Directory);
    assert!(report.unresolved.is_empty());
    assert!(report.effective_path_changes.is_empty());
}

#[test]
fn rejects_kind_changes_for_stable_ids() {
    let old = parse_tree("!tree 1\nx @id\n").unwrap();
    let new = parse_tree("!tree 1\nx/ @id\n").unwrap();
    assert!(matches!(diff(&old, &new), Err(TreeError::Conflict { .. })));
    assert!(diff(&new, &old).is_err());
}

#[test]
fn rejects_self_nested_target_even_with_acyclic_new_ids() {
    let old = parse_tree("!tree 1\na/ @a\n").unwrap();
    let new = parse_tree("!tree 1\na/ @wrapper\n  nested/ @a\n").unwrap();
    assert!(matches!(diff(&old, &new), Err(TreeError::Conflict { .. })));
    let sibling = parse_tree("!tree 1\nabc/ @a\n").unwrap();
    assert!(diff(&old, &sibling).is_ok());
}

#[test]
fn identity_and_reordering_have_empty_diffs() {
    let report = compare("!tree 1\nx @x\na/ @a\n", "!tree 1\na/ @a\nx @x\n");
    assert!(report.changes.is_empty());
    assert!(report.effective_path_changes.is_empty());
    assert!(report.unresolved.is_empty());
}

#[test]
fn description_only_changes_do_not_change_paths() {
    let report = compare("!tree 1\na/ @a # old\n", "!tree 1\na/ @a\n");
    assert_eq!(report.changes.len(), 1);
    assert_eq!(report.changes[0].change, ChangeKind::UpdateDescription);
    assert_eq!(report.changes[0].new_description, None);
    assert!(report.effective_path_changes.is_empty());
}

#[test]
fn example_report_is_stable_and_complete() {
    let old = parse_normalized_tree(include_str!("../examples/old.tree")).unwrap();
    let mut new = parse_normalized_tree(include_str!("../examples/new.tree")).unwrap();
    let expected = compare_trees(&old, &new).unwrap();
    assert_eq!(expected.changes.len(), 6);
    assert_eq!(
        expected
            .changes
            .iter()
            .filter(|c| c.change == ChangeKind::AddNode)
            .count(),
        2
    );
    assert_eq!(
        expected
            .changes
            .iter()
            .filter(|c| c.change == ChangeKind::ReparentNode)
            .count(),
        2
    );
    assert!(expected.unresolved.is_empty());
    new.nodes.reverse();
    assert_eq!(expected, compare_trees(&old, &new).unwrap());
}
