use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

use serde_json::Value;
use tempfile::TempDir;

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_organizer"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

fn write_pair(root: &Path, old: &str, new: &str) {
    fs::write(root.join("old.tree"), old).unwrap();
    fs::write(root.join("new.tree"), new).unwrap();
}

#[test]
fn help_and_version_are_usable() {
    let temp = TempDir::new().unwrap();
    let help = run(temp.path(), &["--help"]);
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    for command in ["init", "validate", "normalize", "plan"] {
        assert!(help.contains(command));
    }
    assert!(run(temp.path(), &["--version"]).status.success());
    assert!(
        !run(temp.path(), &["apply", "anything.json"])
            .status
            .success()
    );
}

#[test]
fn init_creates_only_metadata_and_refuses_reinitialization() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::write(root.join("user.txt"), b"user content\0\xff").unwrap();
    let output = run(root, &["init", "--root", "."]);
    assert!(output.status.success(), "{:?}", output);
    let metadata = root.join(".organizer");
    assert_eq!(
        fs::read_to_string(metadata.join("drafts/initial.tree")).unwrap(),
        "!tree 1\n"
    );
    assert!(
        fs::read_to_string(metadata.join("init.log"))
            .unwrap()
            .contains("completed")
    );
    assert_eq!(
        fs::read_to_string(metadata.join(".gitignore")).unwrap(),
        "drafts/\nplans/\nstate/\ncache/\n*.log\n.env\n"
    );
    for dir in ["drafts", "plans", "state", "cache"] {
        assert!(metadata.join(dir).is_dir());
    }
    assert!(!metadata.join("structure.tree").exists());
    assert!(!metadata.join(".git").exists());
    assert!(!root.join(".git").exists());
    assert!(!run(root, &["init"]).status.success());
    assert_eq!(
        fs::read(root.join("user.txt")).unwrap(),
        b"user content\0\xff"
    );
}

#[test]
fn init_requires_an_existing_directory() {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("file"), "keep").unwrap();
    for root in ["missing", "file"] {
        assert!(!run(temp.path(), &["init", "--root", root]).status.success());
    }
    assert!(!temp.path().join("missing").exists());
}

#[test]
fn normalize_persists_ids_and_validate_is_read_only() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::write(root.join("input.tree"), "!tree 1\na/\n  file\n").unwrap();
    let before = fs::read(root.join("input.tree")).unwrap();
    assert!(run(root, &["validate", "input.tree"]).status.success());
    assert_eq!(fs::read(root.join("input.tree")).unwrap(), before);
    assert!(
        run(root, &["normalize", "input.tree", "--out", "base.tree"])
            .status
            .success()
    );
    let base = fs::read_to_string(root.join("base.tree")).unwrap();
    assert!(base.contains(" @d_"));
    assert!(base.contains(" @f_"));
    let output = run(root, &["normalize", "input.tree", "--base", "base.tree"]);
    assert!(output.status.success());
    assert_eq!(output.stdout, base.as_bytes());
    assert_eq!(fs::read(root.join("input.tree")).unwrap(), before);
    assert_eq!(fs::read_dir(root).unwrap().count(), 2);
}

#[test]
fn plan_prints_stable_structural_report_without_filesystem_changes() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    write_pair(
        root,
        include_str!("../examples/old.tree"),
        include_str!("../examples/new.tree"),
    );
    let before = (
        fs::read(root.join("old.tree")).unwrap(),
        fs::read(root.join("new.tree")).unwrap(),
    );
    let output = run(root, &["plan", "--from", "old.tree", "--to", "new.tree"]);
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["report_type"], "tree_diff");
    assert_eq!(report["changes"].as_array().unwrap().len(), 6);
    assert!(report["unresolved"].as_array().unwrap().is_empty());
    assert!(report.get("operations").is_none());
    assert_eq!(
        run(root, &["plan", "--from", "old.tree", "--to", "new.tree"]).stdout,
        output.stdout
    );
    assert_eq!(fs::read_dir(root).unwrap().count(), 2);
    assert_eq!(fs::read(root.join("old.tree")).unwrap(), before.0);
    assert_eq!(fs::read(root.join("new.tree")).unwrap(), before.1);
}

#[test]
fn plan_normalizes_missing_target_ids_and_new_directories_deterministically() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    write_pair(
        root,
        "!tree 1\na/ @d\n  file @f\n",
        "!tree 1\na/\n  file\nnew/\n",
    );
    let first = run(root, &["plan", "--from", "old.tree", "--to", "new.tree"]);
    assert!(first.status.success());
    let report: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(report["changes"].as_array().unwrap().len(), 1);
    assert_eq!(report["changes"][0]["change"], "add_node");
    assert_eq!(
        first.stdout,
        run(root, &["plan", "--from", "old.tree", "--to", "new.tree"]).stdout
    );
}

#[test]
fn unresolved_files_are_reported_and_return_exit_two() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    write_pair(root, "!tree 1\nold @old\n", "!tree 1\nnew @new\n");
    let output = run(
        root,
        &[
            "plan",
            "--from",
            "old.tree",
            "--to",
            "new.tree",
            "--out",
            "report.json",
        ],
    );
    assert_eq!(output.status.code(), Some(2));
    let report: Value =
        serde_json::from_slice(&fs::read(root.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["unresolved"].as_array().unwrap().len(), 2);
    assert!(output.stdout.is_empty());
}

#[test]
fn errors_produce_no_report_and_leave_existing_files_intact() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    for (old, new) in [
        ("!tree 1\nx\n", "!tree 1\nx @x\n"),
        ("!tree 1\nx @x\n", "!tree 1\nx/ @x\n"),
        ("!tree 1\nx @x\n", "!tree 1\nx @x\nx @y\n"),
        ("!tree 1\nx @x\n", "!tree 1\n../ @x\n"),
    ] {
        write_pair(root, old, new);
        let output = run(
            root,
            &[
                "plan",
                "--from",
                "old.tree",
                "--to",
                "new.tree",
                "--out",
                "report.json",
            ],
        );
        assert_eq!(output.status.code(), Some(1));
        assert!(!output.stderr.is_empty());
        assert!(!root.join("report.json").exists());
    }
    fs::write(root.join("bad.tree"), [0xff]).unwrap();
    assert!(!run(root, &["validate", "bad.tree"]).status.success());
    assert!(!run(root, &["validate", "missing.tree"]).status.success());
}

#[test]
fn metadata_output_never_overwrites_existing_files_or_inputs() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    write_pair(root, "!tree 1\na @a\n", "!tree 1\nb @a\n");
    fs::write(root.join("existing"), "do not overwrite").unwrap();
    for args in [
        vec![
            "plan", "--from", "old.tree", "--to", "new.tree", "--out", "existing",
        ],
        vec!["normalize", "old.tree", "--out", "existing"],
        vec!["normalize", "old.tree", "--out", "old.tree"],
    ] {
        assert!(!run(root, &args).status.success());
    }
    assert_eq!(
        fs::read_to_string(root.join("existing")).unwrap(),
        "do not overwrite"
    );
    assert_eq!(
        fs::read_to_string(root.join("old.tree")).unwrap(),
        "!tree 1\na @a\n"
    );
}

#[test]
fn metadata_output_rejects_parent_traversal() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::create_dir(root.join("child")).unwrap();
    fs::write(root.join("input.tree"), "!tree 1\n").unwrap();
    assert!(
        !run(
            root,
            &["normalize", "input.tree", "--out", "child/../escaped.tree"]
        )
        .status
        .success()
    );
    assert!(!root.join("escaped.tree").exists());
}

#[cfg(unix)]
#[test]
fn metadata_writes_reject_symlinks_including_dangling_links() {
    use std::os::unix::fs::symlink;
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::create_dir(root.join("real")).unwrap();
    fs::write(root.join("input.tree"), "!tree 1\n").unwrap();
    symlink(root.join("real"), root.join("link")).unwrap();
    assert!(!run(root, &["init", "--root", "link"]).status.success());
    assert!(
        !run(root, &["normalize", "input.tree", "--out", "link/out.tree"])
            .status
            .success()
    );
    symlink(root.join("not-created"), root.join("dangling")).unwrap();
    assert!(
        !run(root, &["normalize", "input.tree", "--out", "dangling"])
            .status
            .success()
    );
    symlink(root.join("real"), root.join(".organizer")).unwrap();
    assert!(!run(root, &["init"]).status.success());
    assert_eq!(fs::read_dir(root.join("real")).unwrap().count(), 0);
    assert!(!root.join("not-created").exists());
}
