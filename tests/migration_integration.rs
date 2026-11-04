#![cfg(target_os = "linux")]

use organizer::{
    migration::{self, MigrationPlan, Operation, PlanningContext, build_plan},
    scan::scan,
    storage,
    tree::{NodeKind, TreeNode, TreeSpec, parse_normalized_tree, relative_paths, render_tree},
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::SystemTime,
};
use tempfile::TempDir;

const PLAN: &str = ".organizer/plans/migration.json";

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_organizer"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}
fn success(root: &Path, args: &[&str]) -> Output {
    let output = run(root, args);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root.join(".organizer"))
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn fixture(files: &[&str]) -> TempDir {
    let temp = tempfile::tempdir().unwrap();
    for path in files {
        let path = temp.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"private bytes\0\xff\n").unwrap();
    }
    success(temp.path(), &["init"]);
    temp
}
fn tree(root: &Path) -> TreeSpec {
    parse_normalized_tree(&fs::read_to_string(root.join(".organizer/structure.tree")).unwrap())
        .unwrap()
}
fn id(tree: &TreeSpec, path: &str) -> String {
    relative_paths(tree)
        .unwrap()
        .into_iter()
        .find(|(_, value)| value == path)
        .unwrap()
        .0
}
fn node<'a>(tree: &'a mut TreeSpec, id: &str) -> &'a mut TreeNode {
    tree.nodes.iter_mut().find(|node| node.id == id).unwrap()
}
fn add_directory(tree: &mut TreeSpec, id: &str, name: &str, parent: Option<String>) {
    tree.nodes.push(TreeNode {
        id: id.into(),
        name: name.into(),
        parent_id: parent,
        kind: NodeKind::Directory,
        description: Some("reviewed target".into()),
    });
}
fn write_plan(root: &Path, target: &TreeSpec) -> MigrationPlan {
    fs::write(
        root.join(".organizer/drafts/new.tree"),
        render_tree(target).unwrap(),
    )
    .unwrap();
    success(
        root,
        &[
            "plan",
            "--from",
            ".organizer/structure.tree",
            "--to",
            ".organizer/drafts/new.tree",
            "--out",
            PLAN,
        ],
    );
    serde_json::from_slice(&fs::read(root.join(PLAN)).unwrap()).unwrap()
}
fn renamed_plan(root: &Path) -> MigrationPlan {
    let mut target = tree(root);
    let file = id(&target, "file");
    node(&mut target, &file).name = "renamed".into();
    write_plan(root, &target)
}

type DiskState = BTreeMap<PathBuf, (bool, u64, SystemTime, Vec<u8>)>;
fn disk_state(root: &Path) -> DiskState {
    let mut result = BTreeMap::new();
    let mut pending = vec![root.to_owned()];
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path).unwrap();
        let contents = if metadata.is_file() {
            fs::read(&path).unwrap()
        } else {
            vec![]
        };
        result.insert(
            path.strip_prefix(root).unwrap().into(),
            (
                metadata.is_dir(),
                metadata.len(),
                metadata.modified().unwrap(),
                contents,
            ),
        );
        if metadata.is_dir() {
            pending.extend(
                fs::read_dir(path)
                    .unwrap()
                    .map(|entry| entry.unwrap().path()),
            );
        }
    }
    result
}

#[test]
fn end_to_end_classification_dry_run_apply_and_structure_only_git() {
    let temp = fixture(&["Research/Papers/CRUSH.pdf", "Research/Papers/ASLR.pdf"]);
    let root = temp.path();
    let old = tree(root);
    let mut target = old.clone();
    let papers = id(&old, "Research/Papers");
    add_directory(&mut target, "c2rust", "C2Rust", Some(papers.clone()));
    add_directory(&mut target, "security", "Security", Some(papers));
    node(&mut target, &id(&old, "Research/Papers/CRUSH.pdf")).parent_id = Some("c2rust".into());
    node(&mut target, &id(&old, "Research/Papers/ASLR.pdf")).parent_id = Some("security".into());
    let plan = write_plan(root, &target);
    assert_eq!(
        plan.operations
            .iter()
            .filter(|op| matches!(op, Operation::Mkdir { .. }))
            .count(),
        2
    );
    assert_eq!(
        plan.operations
            .iter()
            .filter(|op| matches!(op, Operation::MoveFile { .. }))
            .count(),
        2
    );
    assert_eq!(
        plan.base_tree_digest,
        migration::plan::digest(&render_tree(&old).unwrap())
    );
    assert_eq!(
        plan.target_tree_digest,
        migration::plan::digest(&render_tree(&target).unwrap())
    );
    let before = disk_state(root);
    for args in [&["apply", PLAN][..], &["apply", PLAN, "--dry-run"]] {
        let report: Value = serde_json::from_slice(&success(root, args).stdout).unwrap();
        assert_eq!(report["dry_run"], true);
        assert_eq!(disk_state(root), before);
    }
    assert!(!root.join(".organizer/state/operations.sqlite").exists());
    assert!(
        !run(root, &["apply", PLAN, "--dry-run", "--yes"])
            .status
            .success()
    );
    let result: Value =
        serde_json::from_slice(&success(root, &["apply", PLAN, "--yes"]).stdout).unwrap();
    assert_eq!(result["completed_operations"], 4);
    for path in [
        "Research/Papers/C2Rust/CRUSH.pdf",
        "Research/Papers/Security/ASLR.pdf",
    ] {
        assert_eq!(fs::read(root.join(path)).unwrap(), b"private bytes\0\xff\n");
    }
    assert_eq!(
        render_tree(&tree(root)).unwrap(),
        render_tree(&target).unwrap()
    );
    assert_eq!(git(root, &["ls-files"]), "structure.tree");
    assert_eq!(git(root, &["rev-list", "--count", "HEAD"]), "2");
    assert!(git(root, &["log", "-1", "--format=%s"]).starts_with("refactor:"));
    assert!(!git(root, &["show", "HEAD:structure.tree"]).contains("private bytes"));
    let before = disk_state(root);
    let report: Value = serde_json::from_slice(
        &success(root, &["recover", result["run_id"].as_str().unwrap()]).stdout,
    )
    .unwrap();
    assert_eq!(report["status"], "complete");
    assert_eq!(disk_state(root), before);
    assert!(!run(root, &["apply", PLAN, "--yes"]).status.success());
    assert_eq!(git(root, &["rev-list", "--count", "HEAD"]), "2");
}

#[test]
fn directory_move_has_no_duplicate_child_moves() {
    let temp = fixture(&["A/sub/file", "A/other"]);
    let root = temp.path();
    let mut target = tree(root);
    let directory = id(&target, "A");
    node(&mut target, &directory).name = "B".into();
    let plan = write_plan(root, &target);
    assert_eq!(plan.operations.len(), 1);
    assert!(matches!(
        plan.operations[0],
        Operation::MoveDirectory { .. }
    ));
    success(root, &["apply", PLAN, "--yes"]);
    assert_eq!(
        relative_paths(&tree(root)).unwrap(),
        relative_paths(&target).unwrap()
    );
    assert_eq!(
        fs::read(root.join("B/sub/file")).unwrap(),
        b"private bytes\0\xff\n"
    );
}

#[test]
fn directory_move_orders_mixed_incoming_and_outgoing_children() {
    let temp = fixture(&["A/sub/stay", "A/out", "incoming"]);
    let root = temp.path();
    let mut target = tree(root);
    let old = target.clone();
    node(&mut target, &id(&old, "A")).name = "B".into();
    let outgoing = node(&mut target, &id(&old, "A/out"));
    outgoing.parent_id = None;
    outgoing.name = "z-out".into();
    node(&mut target, &id(&old, "incoming")).parent_id = Some(id(&old, "A/sub"));
    let plan = write_plan(root, &target);
    assert_eq!(plan.operations.len(), 3);
    success(root, &["apply", PLAN, "--yes"]);
    assert_eq!(
        relative_paths(&tree(root)).unwrap(),
        relative_paths(&target).unwrap()
    );
    for path in ["B/sub/stay", "B/sub/incoming", "z-out"] {
        assert_eq!(fs::read(root.join(path)).unwrap(), b"private bytes\0\xff\n");
    }
}

#[test]
fn vacated_destinations_are_ordered_but_cycles_are_unresolved() {
    for cycle in [false, true] {
        let temp = fixture(&["a", "b"]);
        let root = temp.path();
        let mut target = tree(root);
        let old = target.clone();
        node(&mut target, &id(&old, "a")).name = "b".into();
        node(&mut target, &id(&old, "b")).name = if cycle { "a" } else { "c" }.into();
        let plan = migration::plan(root, &old, &target).unwrap();
        if cycle {
            assert!(!plan.unresolved.is_empty());
            assert!(plan.operations.is_empty());
            assert!(migration::apply_plan(&plan, root).is_err());
        } else {
            assert_eq!(plan.operations[0].source(), Some("b"));
            migration::apply_plan(&plan, root).unwrap();
            assert_eq!(
                relative_paths(&tree(root)).unwrap(),
                relative_paths(&target).unwrap()
            );
        }
    }
}

#[test]
fn missing_files_new_files_and_omitted_directories_never_delete_or_create_files() {
    for scenario in 0..3 {
        let temp = fixture(&["directory/file"]);
        let root = temp.path();
        let old = tree(root);
        let mut target = old.clone();
        let file = id(&old, "directory/file");
        match scenario {
            0 => target.nodes.retain(|node| node.id != file),
            1 => node(&mut target, &file).id = "new-file-id".into(),
            _ => {
                node(&mut target, &file).parent_id = None;
                target.nodes.retain(|node| node.kind == NodeKind::File);
            }
        }
        fs::write(
            root.join(".organizer/drafts/new.tree"),
            render_tree(&target).unwrap(),
        )
        .unwrap();
        let output = run(
            root,
            &[
                "plan",
                "--from",
                ".organizer/structure.tree",
                "--to",
                ".organizer/drafts/new.tree",
                "--out",
                PLAN,
            ],
        );
        assert_eq!(output.status.code(), Some(2));
        let plan: MigrationPlan =
            serde_json::from_slice(&fs::read(root.join(PLAN)).unwrap()).unwrap();
        assert!(!plan.unresolved.is_empty());
        assert!(plan.operations.is_empty());
        assert!(!run(root, &["apply", PLAN, "--yes"]).status.success());
        assert!(root.join("directory/file").is_file());
        assert!(!root.join(".organizer/state/operations.sqlite").exists());
    }
}

#[test]
fn changed_or_replaced_source_and_occupied_target_invalidate_old_plan() {
    for scenario in 0..3 {
        let temp = fixture(&["file"]);
        let root = temp.path();
        renamed_plan(root);
        match scenario {
            0 => fs::write(root.join("file"), "edited after planning").unwrap(),
            1 => {
                fs::rename(root.join("file"), root.join("original")).unwrap();
                fs::write(root.join("file"), b"private bytes\0\xff\n").unwrap();
            }
            _ => fs::write(root.join("renamed"), "target must survive").unwrap(),
        }
        let before = disk_state(root);
        assert!(!run(root, &["apply", PLAN, "--yes"]).status.success());
        assert_eq!(disk_state(root), before);
    }
}

#[test]
fn tampered_operations_digests_schema_and_target_are_rejected() {
    for scenario in 0..6 {
        let temp = fixture(&["file"]);
        let root = temp.path();
        let plan = renamed_plan(root);
        let mut value = serde_json::to_value(&plan).unwrap();
        match scenario {
            0 => value["operations"][0]["to"] = "../escape".into(),
            1 => value["operations"][0]["op"] = "delete_file".into(),
            2 => value["target_tree_digest"] = "sha256:bogus".into(),
            3 => value["schema_version"] = 2.into(),
            4 => value["shell"] = "touch forbidden".into(),
            _ => value["operations"][0]["to"] = "/tmp/escape".into(),
        }
        fs::write(root.join(PLAN), serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(!run(root, &["apply", PLAN, "--yes"]).status.success());
        assert!(root.join("file").is_file());
        assert!(!root.join(".organizer/state/operations.sqlite").exists());
    }
}

#[test]
fn edited_operations_with_recomputed_digest_still_require_target_consistency() {
    let temp = fixture(&["file"]);
    let root = temp.path();
    let mut plan = renamed_plan(root);
    if let Operation::MoveFile { to, .. } = &mut plan.operations[0] {
        *to = "unreviewed".into();
    }
    plan.plan_id.clear();
    plan.plan_id = migration::plan::digest(&serde_json::to_string(&plan).unwrap());
    assert!(migration::validate_plan(&plan, root).is_err());
    assert!(root.join("file").is_file());
}

#[test]
fn apply_uses_bound_target_and_never_reloads_the_draft() {
    let temp = fixture(&["file"]);
    let root = temp.path();
    renamed_plan(root);
    fs::write(root.join(".organizer/drafts/new.tree"), "!tree 1\n").unwrap();
    success(root, &["apply", PLAN, "--yes"]);
    assert!(root.join("renamed").is_file());
}

#[test]
fn plan_cannot_switch_the_selected_root() {
    let a = fixture(&["file"]);
    let b = fixture(&["file"]);
    let plan = renamed_plan(a.path());
    assert!(migration::apply_plan(&plan, b.path()).is_err());
    assert!(
        !run(
            b.path(),
            &["apply", a.path().join(PLAN).to_str().unwrap(), "--yes"]
        )
        .status
        .success()
    );
    success(
        b.path(),
        &[
            "apply",
            a.path().join(PLAN).to_str().unwrap(),
            "--root",
            a.path().to_str().unwrap(),
            "--yes",
        ],
    );
    assert!(a.path().join("renamed").exists());
    assert!(b.path().join("file").exists());
}

#[test]
fn symlinks_cannot_redirect_source_destination_or_parent() {
    use std::os::unix::fs::symlink;
    for scenario in 0..3 {
        let temp = fixture(&["dir/file"]);
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret"), "outside bytes").unwrap();
        let root = temp.path();
        let mut target = tree(root);
        let file = id(&target, "dir/file");
        node(&mut target, &file).name = "renamed".into();
        write_plan(root, &target);
        match scenario {
            0 => {
                fs::remove_file(root.join("dir/file")).unwrap();
                symlink(outside.path().join("secret"), root.join("dir/file")).unwrap();
            }
            1 => symlink(outside.path().join("secret"), root.join("dir/renamed")).unwrap(),
            _ => {
                fs::rename(root.join("dir"), root.join("original-dir")).unwrap();
                symlink(outside.path(), root.join("dir")).unwrap();
            }
        }
        assert!(!run(root, &["apply", PLAN, "--yes"]).status.success());
        assert_eq!(
            fs::read_to_string(outside.path().join("secret")).unwrap(),
            "outside bytes"
        );
        assert!(!outside.path().join("renamed").exists());
    }
}

#[test]
fn protected_projects_nested_metadata_skipped_files_and_hard_links_block_subtree_moves() {
    use std::os::unix::fs::symlink;
    for scenario in 0..4 {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::create_dir(root.join("parent")).unwrap();
        fs::write(root.join("parent/file"), "bytes").unwrap();
        match scenario {
            0 => fs::create_dir_all(root.join("parent/project/.git")).unwrap(),
            1 => fs::create_dir(root.join("parent/.organizer")).unwrap(),
            2 => symlink("/nonexistent", root.join("parent/link")).unwrap(),
            _ => fs::hard_link(root.join("parent/file"), root.join("hardlink")).unwrap(),
        }
        success(root, &["init"]);
        let old = tree(root);
        let mut target = old.clone();
        node(&mut target, &id(&old, "parent")).name = "renamed".into();
        let plan = migration::plan(root, &old, &target).unwrap();
        assert!(!plan.unresolved.is_empty());
        assert!(migration::apply_plan(&plan, root).is_err());
        assert!(root.join("parent/file").exists());
    }
}

#[test]
fn deterministic_operations_and_plan_id_for_the_same_snapshot() {
    let temp = fixture(&["a/file", "incoming"]);
    let root = temp.path();
    let saved = storage::load(&root.join(".organizer/state/index.sqlite")).unwrap();
    let snapshot = scan(root).unwrap();
    let mut target = saved.tree.clone();
    node(&mut target, &id(&saved.tree, "a")).name = "b".into();
    node(&mut target, &id(&saved.tree, "incoming")).parent_id = Some(id(&saved.tree, "a"));
    let first = build_plan(&PlanningContext {
        saved: &saved,
        snapshot: &snapshot,
        target: &target,
    })
    .unwrap();
    target.nodes.reverse();
    let second = build_plan(&PlanningContext {
        saved: &saved,
        snapshot: &snapshot,
        target: &target,
    })
    .unwrap();
    assert_eq!(first, second);
    assert_eq!(
        migration::plan::digest("abc"),
        "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn cross_device_identity_is_unresolved_without_copy_fallback() {
    let temp = fixture(&["file"]);
    let root = temp.path();
    let mut saved = storage::load(&root.join(".organizer/state/index.sqlite")).unwrap();
    let mut snapshot = scan(root).unwrap();
    saved.records[0].entry.stamp.platform_file_key = Some("another-device:123".into());
    snapshot.entries[0].stamp = saved.records[0].entry.stamp.clone();
    let mut target = saved.tree.clone();
    target.nodes[0].name = "renamed".into();
    let plan = build_plan(&PlanningContext {
        saved: &saved,
        snapshot: &snapshot,
        target: &target,
    })
    .unwrap();
    assert!(
        plan.unresolved
            .iter()
            .any(|reason| reason.contains("filesystem boundary"))
    );
    assert!(plan.operations.is_empty());
    assert!(root.join("file").is_file());
}

#[test]
fn unicode_shell_punctuation_and_control_characters_are_only_path_data() {
    let temp = fixture(&["file"]);
    let root = temp.path();
    let mut target = tree(root);
    let file = id(&target, "file");
    add_directory(&mut target, "directory", "资料 ' $(touch INJECTED)", None);
    let file = node(&mut target, &file);
    file.parent_id = Some("directory".into());
    file.name = "--name # @ \"\n\t.txt".into();
    write_plan(root, &target);
    success(root, &["apply", PLAN, "--yes"]);
    assert_eq!(
        relative_paths(&tree(root)).unwrap(),
        relative_paths(&target).unwrap()
    );
    assert!(!root.join("INJECTED").exists());
    assert_eq!(
        fs::read(root.join("资料 ' $(touch INJECTED)/--name # @ \"\n\t.txt")).unwrap(),
        b"private bytes\0\xff\n"
    );
}

#[test]
fn git_failure_retries_metadata_only_without_duplicate_moves_or_commits() {
    for lock_name in ["refs/heads/main.lock", "index.lock"] {
        let temp = fixture(&["file"]);
        let root = temp.path();
        renamed_plan(root);
        let lock = root.join(".organizer/.git").join(lock_name);
        fs::write(&lock, "held").unwrap();
        let result = run(root, &["apply", PLAN, "--yes"]);
        assert!(!result.status.success());
        assert!(
            String::from_utf8_lossy(&result.stderr)
                .contains("file migration and metadata publication completed")
        );
        assert!(root.join("renamed").exists());
        assert_eq!(tree(root).nodes[0].name, "renamed");
        let db =
            rusqlite::Connection::open(root.join(".organizer/state/operations.sqlite")).unwrap();
        let run_id: String = db
            .query_row("SELECT run_id FROM runs", [], |row| row.get(0))
            .unwrap();
        drop(db);
        assert!(!run(root, &["apply", PLAN, "--yes"]).status.success());
        fs::remove_file(lock).unwrap();
        let report: Value =
            serde_json::from_slice(&success(root, &["recover", &run_id, "--finish-git"]).stdout)
                .unwrap();
        assert_eq!(report["status"], "complete");
        success(root, &["recover", &run_id, "--finish-git"]);
        assert_eq!(git(root, &["rev-list", "--count", "HEAD"]), "2");
        assert!(git(root, &["log", "-1", "--format=%s"]).starts_with("refactor:"));
        assert_eq!(
            fs::read(root.join("renamed")).unwrap(),
            b"private bytes\0\xff\n"
        );
    }
}

#[test]
fn description_only_plan_and_noop_do_not_move_files_or_duplicate_commits() {
    let temp = fixture(&["file"]);
    let root = temp.path();
    let mut target = tree(root);
    target.nodes[0].description = Some("annotated".into());
    let plan = write_plan(root, &target);
    assert!(plan.operations.is_empty());
    success(root, &["apply", PLAN, "--yes"]);
    assert_eq!(git(root, &["rev-list", "--count", "HEAD"]), "2");
    let target = tree(root);
    let plan = migration::plan(root, &target, &target).unwrap();
    migration::apply_plan(&plan, root).unwrap();
    assert!(migration::apply_plan(&plan, root).is_err());
    assert_eq!(git(root, &["rev-list", "--count", "HEAD"]), "2");
}

#[test]
fn inconsistent_journal_blocks_further_work_and_recovery() {
    let temp = fixture(&["file"]);
    let root = temp.path();
    renamed_plan(root);
    let result: Value =
        serde_json::from_slice(&success(root, &["apply", PLAN, "--yes"]).stdout).unwrap();
    let db = rusqlite::Connection::open(root.join(".organizer/state/operations.sqlite")).unwrap();
    db.execute("UPDATE runs SET completed=0", []).unwrap();
    drop(db);
    let failure = run(root, &["status"]);
    assert!(!failure.status.success());
    assert!(String::from_utf8_lossy(&failure.stderr).contains("inconsistent"));
    assert!(
        !run(root, &["recover", result["run_id"].as_str().unwrap()])
            .status
            .success()
    );
    assert_eq!(
        fs::read(root.join("renamed")).unwrap(),
        b"private bytes\0\xff\n"
    );
}

#[test]
fn exported_script_quotes_paths_and_only_previews_through_the_cli() {
    let temp = fixture(&["file"]);
    let root = temp.path();
    renamed_plan(root);
    let tricky = ".organizer/plans/quote' $(touch INJECTED); plan.json";
    fs::rename(root.join(PLAN), root.join(tricky)).unwrap();
    let script = ".organizer/plans/migrate.sh";
    success(root, &["export-script", tricky, "--out", script]);
    let text = fs::read_to_string(root.join(script)).unwrap();
    assert!(text.contains("'\\''"));
    assert!(!text.contains("--yes"));
    assert_eq!(text.lines().count(), 3);
    let binary_dir = Path::new(env!("CARGO_BIN_EXE_organizer")).parent().unwrap();
    let path = format!(
        "{}:{}",
        binary_dir.display(),
        std::env::var("PATH").unwrap()
    );
    let output = Command::new("bash")
        .current_dir(root)
        .arg(script)
        .env("PATH", path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!root.join("INJECTED").exists());
    assert!(root.join("file").exists());
    assert!(!root.join("renamed").exists());
    assert!(
        !run(root, &["export-script", tricky, "--out", script])
            .status
            .success()
    );
}
