use organizer::tree::{parse_normalized_tree, relative_paths};
use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;

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
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
fn ids(root: &Path) -> std::collections::HashMap<String, String> {
    let tree =
        parse_normalized_tree(&fs::read_to_string(root.join(".organizer/structure.tree")).unwrap())
            .unwrap();
    relative_paths(&tree)
        .unwrap()
        .into_iter()
        .map(|(id, path)| (path, id))
        .collect()
}

#[test]
fn init_tracks_only_structure_and_status_is_read_only() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::create_dir(root.join("资料")).unwrap();
    fs::write(root.join("资料/秘密.txt"), "private content").unwrap();
    success(root, &["init"]);
    assert_eq!(git(root, &["ls-files"]), "structure.tree");
    assert!(!root.join(".git").exists());
    assert_eq!(ids(root).len(), 2);
    let tree = fs::read(root.join(".organizer/structure.tree")).unwrap();
    let db = fs::read(root.join(".organizer/state/index.sqlite")).unwrap();
    let head = git(root, &["rev-parse", "HEAD"]);
    let output = success(root, &["status"]);
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(report["added"].as_array().unwrap().is_empty());
    assert_eq!(
        tree,
        fs::read(root.join(".organizer/structure.tree")).unwrap()
    );
    assert_eq!(
        db,
        fs::read(root.join(".organizer/state/index.sqlite")).unwrap()
    );
    assert_eq!(head, git(root, &["rev-parse", "HEAD"]));
    assert!(!git(root, &["show", "HEAD:structure.tree"]).contains("private content"));
}

#[test]
fn additions_moves_copies_deletions_and_content_changes_sync() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::create_dir(root.join("a")).unwrap();
    fs::write(root.join("a/file"), "original").unwrap();
    success(root, &["init"]);
    let old = ids(root);
    fs::rename(root.join("a"), root.join("b")).unwrap();
    success(root, &["sync"]);
    let moved = ids(root);
    assert_eq!(old["a"], moved["b"]);
    assert_eq!(old["a/file"], moved["b/file"]);
    fs::copy(root.join("b/file"), root.join("copy")).unwrap();
    fs::write(root.join("new"), "new data").unwrap();
    let report: Value = serde_json::from_slice(&success(root, &["status"]).stdout).unwrap();
    assert_eq!(report["added"].as_array().unwrap().len(), 2);
    success(root, &["sync"]);
    assert_ne!(ids(root)["copy"], old["a/file"]);
    let head = git(root, &["rev-parse", "HEAD"]);
    let tree = fs::read(root.join(".organizer/structure.tree")).unwrap();
    fs::write(
        root.join("b/file"),
        "modified contents with a different size",
    )
    .unwrap();
    let report: Value = serde_json::from_slice(&success(root, &["status"]).stdout).unwrap();
    assert_eq!(report["changed"].as_array().unwrap().len(), 1);
    success(root, &["sync"]);
    assert_eq!(head, git(root, &["rev-parse", "HEAD"]));
    assert_eq!(
        tree,
        fs::read(root.join(".organizer/structure.tree")).unwrap()
    );
    fs::remove_file(root.join("copy")).unwrap();
    success(root, &["sync"]);
    assert!(!ids(root).contains_key("copy"));
    assert_eq!(
        fs::read_to_string(root.join("b/file")).unwrap(),
        "modified contents with a different size"
    );
}

#[test]
fn dirty_baseline_and_drafts_block_sync() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::write(root.join("file"), "keep").unwrap();
    success(root, &["init"]);
    let baseline = root.join(".organizer/structure.tree");
    let original = fs::read(&baseline).unwrap();
    fs::write(&baseline, "!tree 1\n").unwrap();
    assert!(!run(root, &["sync"]).status.success());
    assert_eq!(fs::read_to_string(&baseline).unwrap(), "!tree 1\n");
    fs::write(&baseline, original).unwrap();
    fs::write(root.join(".organizer/drafts/new.tree"), "!tree 1\n").unwrap();
    fs::write(root.join("new"), "keep too").unwrap();
    assert!(!run(root, &["sync"]).status.success());
    assert!(!ids(root).contains_key("new"));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn skips_symlinks_special_files_and_nested_repositories() {
    use std::os::unix::fs::symlink;
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::create_dir(root.join("project")).unwrap();
    fs::create_dir(root.join("project/.git")).unwrap();
    fs::write(root.join("project/secret"), "not inventoried").unwrap();
    symlink(root.join("project"), root.join("link")).unwrap();
    assert!(
        Command::new("mkfifo")
            .arg(root.join("pipe"))
            .status()
            .unwrap()
            .success()
    );
    success(root, &["init"]);
    let tree = ids(root);
    assert_eq!(tree.len(), 1);
    assert!(tree.contains_key("project"));
    let status: Value = serde_json::from_slice(&success(root, &["status"]).stdout).unwrap();
    assert!(status["warnings"].as_array().unwrap().len() >= 3);
}

#[test]
fn protected_boundary_changes_do_not_silently_drop_old_files() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::create_dir(root.join("project")).unwrap();
    fs::write(root.join("project/file"), "keep").unwrap();
    success(root, &["init"]);
    let before = fs::read(root.join(".organizer/structure.tree")).unwrap();
    fs::create_dir(root.join("project/.git")).unwrap();
    assert!(!run(root, &["sync"]).status.success());
    assert_eq!(
        before,
        fs::read(root.join(".organizer/structure.tree")).unwrap()
    );
}

#[test]
fn plan_with_root_rejects_stale_baseline() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::write(root.join("file"), "keep").unwrap();
    success(root, &["init"]);
    // Draft outside the managed tree does not itself make the baseline stale.
    let draft = temp
        .path()
        .parent()
        .unwrap()
        .join(format!("treespec-draft-{}.tree", uuid::Uuid::new_v4()));
    fs::copy(root.join(".organizer/structure.tree"), &draft).unwrap();
    let args = [
        "plan",
        "--root",
        ".",
        "--from",
        ".organizer/structure.tree",
        "--to",
        draft.to_str().unwrap(),
    ];
    success(root, &args);
    fs::write(root.join("new"), "new").unwrap();
    assert!(!run(root, &args).status.success());
    fs::remove_file(draft).unwrap();
}

#[test]
fn git_failures_before_and_after_ref_update_can_retry_without_duplicate_commits() {
    for lock in ["refs/heads/main.lock", "index.lock"] {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::write(root.join("old"), "unchanged").unwrap();
        success(root, &["init"]);
        let lock = root.join(".organizer/.git").join(lock);
        fs::write(&lock, "held by test").unwrap();
        fs::write(root.join("new"), "new content").unwrap();
        let failure = run(root, &["sync"]);
        assert!(!failure.status.success());
        assert!(String::from_utf8_lossy(&failure.stderr).contains("Git recording is pending"));
        assert!(ids(root).contains_key("new"));
        let status = run(root, &["status"]);
        assert_eq!(status.status.code(), Some(2));
        let report: Value = serde_json::from_slice(&status.stdout).unwrap();
        assert_eq!(report["pending_git"], true);
        fs::remove_file(lock).unwrap();
        let output = success(root, &["sync"]);
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["retried_git_commit"], true);
        assert_eq!(git(root, &["rev-list", "--count", "HEAD"]), "2");
        success(root, &["sync"]);
        assert_eq!(git(root, &["rev-list", "--count", "HEAD"]), "2");
        assert_eq!(fs::read_to_string(root.join("old")).unwrap(), "unchanged");
    }
}

#[test]
fn ambiguous_move_and_edit_requires_explicit_new_identity_acceptance() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::write(root.join("old"), "before").unwrap();
    success(root, &["init"]);
    let id = ids(root)["old"].clone();
    fs::rename(root.join("old"), root.join("new")).unwrap();
    fs::write(root.join("new"), "changed and moved").unwrap();
    let output = run(root, &["status"]);
    assert_eq!(output.status.code(), Some(2));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["unknown"].as_array().unwrap().len(), 1);
    assert!(!run(root, &["sync"]).status.success());
    assert_eq!(ids(root)["old"], id);
    success(root, &["sync", "--accept-unknown"]);
    assert_ne!(ids(root)["new"], id);
    assert_eq!(
        fs::read_to_string(root.join("new")).unwrap(),
        "changed and moved"
    );
}

#[test]
fn metadata_lock_and_interrupted_publish_block_writes() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    success(root, &["init"]);
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(root.join(".organizer/state/lock"))
        .unwrap();
    lock.try_lock().unwrap();
    assert!(!run(root, &["status"]).status.success());
    assert!(!run(root, &["sync"]).status.success());
    drop(lock);
    success(root, &["status"]);
    fs::write(
        root.join(".organizer/state/pending.json"),
        "{\"phase\":\"publishing_metadata\"}",
    )
    .unwrap();
    let before = fs::read(root.join(".organizer/state/index.sqlite")).unwrap();
    assert!(!run(root, &["sync"]).status.success());
    assert_eq!(
        before,
        fs::read(root.join(".organizer/state/index.sqlite")).unwrap()
    );
}

#[test]
fn staged_foreign_files_block_sync_instead_of_entering_history() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    success(root, &["init"]);
    fs::write(root.join(".organizer/state/secret"), "test secret").unwrap();
    git(root, &["add", "--force", "--", "state/secret"]);
    fs::write(root.join("new"), "new").unwrap();
    assert!(!run(root, &["sync"]).status.success());
    assert_eq!(
        git(root, &["ls-tree", "--name-only", "HEAD"]),
        "structure.tree"
    );
    assert_eq!(git(root, &["rev-list", "--count", "HEAD"]), "1");
    assert!(!ids(root).contains_key("new"));
}

#[test]
fn git_environment_hooks_and_content_filters_cannot_redirect_recording() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::write(root.join("file"), "data").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_organizer"))
        .current_dir(root)
        .args(["init"])
        .env("GIT_DIR", root.join("outside.git"))
        .env("GIT_WORK_TREE", root.join("outside"))
        .env("GIT_INDEX_FILE", root.join("outside.index"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!root.join("outside.git").exists());
    assert!(!root.join("outside.index").exists());
    fs::write(root.join(".organizer/.gitattributes"), "* filter=reject\n").unwrap();
    git(root, &["config", "filter.reject.clean", "false"]);
    git(root, &["config", "filter.reject.required", "true"]);
    git(root, &["config", "core.hooksPath", "/nonexistent/hooks"]);
    fs::write(root.join("new"), "private").unwrap();
    success(root, &["sync"]);
    assert_eq!(git(root, &["ls-files"]), "structure.tree");
}

#[test]
fn upgrades_only_recognized_m0_layout() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let metadata = root.join(".organizer");
    for directory in ["drafts", "plans", "state", "cache"] {
        fs::create_dir_all(metadata.join(directory)).unwrap();
    }
    fs::write(
        metadata.join(".gitignore"),
        "drafts/\nplans/\nstate/\ncache/\n*.log\n.env\n",
    )
    .unwrap();
    fs::write(metadata.join("init.log"), "M0 metadata initialization started; no scan, baseline, or Git repository\ncompleted: .gitignore and drafts/initial.tree created\n").unwrap();
    fs::write(metadata.join("drafts/initial.tree"), "!tree 1\n").unwrap();
    fs::write(root.join("existing"), "keep").unwrap();
    success(root, &["init"]);
    assert!(ids(root).contains_key("existing"));
    assert!(metadata.join("drafts/initial.tree").exists());
    success(root, &["status"]);
}

#[test]
fn root_discovery_history_and_namespace_isolation() {
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    for root in [a.path(), b.path()] {
        fs::create_dir(root.join("sub")).unwrap();
        fs::write(root.join("same"), "same").unwrap();
        success(root, &["init"]);
        success(&root.join("sub"), &["status"]);
        let output = success(&root.join("sub"), &["history"]);
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("init: record filesystem structure")
        );
    }
    assert_ne!(ids(a.path())["same"], ids(b.path())["same"]);
}

#[cfg(unix)]
#[test]
fn incomplete_scan_does_not_infer_deletions_or_update_baseline() {
    use std::os::unix::fs::PermissionsExt;
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::create_dir(root.join("locked")).unwrap();
    fs::write(root.join("locked/file"), "keep").unwrap();
    success(root, &["init"]);
    let before = fs::read(root.join(".organizer/structure.tree")).unwrap();
    fs::set_permissions(root.join("locked"), fs::Permissions::from_mode(0o000)).unwrap();
    let status = run(root, &["status"]);
    let sync = run(root, &["sync"]);
    fs::set_permissions(root.join("locked"), fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(status.status.code(), Some(2));
    let report: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(report["scan_complete"], false);
    assert!(report["missing"].as_array().unwrap().is_empty());
    assert!(!sync.status.success());
    assert_eq!(
        before,
        fs::read(root.join(".organizer/structure.tree")).unwrap()
    );
}

#[cfg(unix)]
#[test]
fn unrepresentable_names_and_replaced_symlinks_fail_closed() {
    use std::os::unix::fs::symlink;
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::write(root.join("file"), "keep").unwrap();
    success(root, &["init"]);
    // APFS rejects non-UTF-8 names at the syscall layer, so this half only
    // applies to filesystems that can store such names (for example ext4).
    #[cfg(not(target_os = "macos"))]
    {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};
        let invalid = root.join(OsString::from_vec(vec![0xff]));
        fs::write(&invalid, "invalid name").unwrap();
        let output = run(root, &["status"]);
        assert_eq!(output.status.code(), Some(2));
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["scan_complete"], false);
        assert!(report["missing"].as_array().unwrap().is_empty());
        assert!(!run(root, &["sync"]).status.success());
        fs::remove_file(invalid).unwrap();
    }
    fs::remove_file(root.join("file")).unwrap();
    symlink("/nonexistent", root.join("file")).unwrap();
    assert!(!run(root, &["sync"]).status.success());
    assert!(ids(root).contains_key("file"));
}

#[test]
fn pending_git_retry_preserves_conflicting_manual_staged_edits() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    success(root, &["init"]);
    let lock = root.join(".organizer/.git/refs/heads/main.lock");
    fs::write(&lock, "held").unwrap();
    fs::write(root.join("new"), "keep").unwrap();
    assert!(!run(root, &["sync"]).status.success());
    fs::remove_file(lock).unwrap();
    let path = root.join(".organizer/structure.tree");
    let pending = fs::read(&path).unwrap();
    fs::write(&path, "!tree 1\nmanual/ @manual\n").unwrap();
    git(root, &["add", "--", "structure.tree"]);
    fs::write(&path, pending).unwrap();
    assert!(!run(root, &["sync"]).status.success());
    assert!(git(root, &["show", ":structure.tree"]).contains("manual"));
    git(root, &["read-tree", "HEAD"]);
    success(root, &["sync"]);
    assert_eq!(git(root, &["rev-list", "--count", "HEAD"]), "2");
}

#[test]
fn swapping_existing_file_paths_preserves_both_ids() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::write(root.join("a"), "a").unwrap();
    fs::write(root.join("b"), "b").unwrap();
    success(root, &["init"]);
    let old = ids(root);
    fs::rename(root.join("a"), root.join("temporary")).unwrap();
    fs::rename(root.join("b"), root.join("a")).unwrap();
    fs::rename(root.join("temporary"), root.join("b")).unwrap();
    success(root, &["sync"]);
    assert_eq!(ids(root)["a"], old["b"]);
    assert_eq!(ids(root)["b"], old["a"]);
}
