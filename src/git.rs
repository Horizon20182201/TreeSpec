//! Dedicated metadata repository. Git plumbing writes exactly one TreeSpec blob;
//! no recursive add, hooks, content filters, remote calls, or shell interpolation.
use crate::{
    error::{Result, fail},
    metadata,
    storage::SavedState,
};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

fn command(directory: &Path) -> Command {
    let mut command = Command::new("git");
    command.current_dir(directory);
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0");
    for setting in [
        "core.hooksPath=/dev/null",
        "core.fsmonitor=false",
        "commit.gpgSign=false",
        "core.autocrlf=false",
        "core.attributesFile=/dev/null",
        "user.name=TreeSpec Organizer",
        "user.email=organizer@localhost",
    ] {
        command.arg("-c").arg(setting);
    }
    command
}

fn execute(metadata: &Path, args: &[&str], input: Option<&str>) -> Result<Output> {
    let mut command = command(metadata);
    command
        .arg("--git-dir")
        .arg(metadata.join(".git"))
        .arg("--work-tree")
        .arg(metadata)
        .args(args);
    command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    if let Some(input) = input {
        let result = child
            .stdin
            .take()
            .ok_or_else(|| fail("missing Git stdin"))?
            .write_all(input.as_bytes());
        if let Err(error) = result {
            let _ = child.wait();
            return Err(error.into());
        }
    }
    Ok(child.wait_with_output()?)
}

fn run(metadata: &Path, args: &[&str], input: Option<&str>) -> Result<String> {
    let output = execute(metadata, args, input)?;
    if !output.status.success() {
        return Err(fail(format!(
            "git {} failed: {}",
            args[0],
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    String::from_utf8(output.stdout).map_err(|_| fail("Git returned non-UTF-8 metadata"))
}

pub(crate) fn init(metadata: &Path) -> Result<()> {
    if metadata::exists(&metadata.join(".git"))? {
        return Err(fail("metadata Git repository already exists"));
    }
    let output = command(metadata)
        .args([
            "init",
            "--quiet",
            "--template=",
            "--initial-branch=main",
            "--object-format=sha1",
            ".",
        ])
        .output()?;
    if !output.status.success() {
        return Err(fail(format!(
            "Git initialization failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(())
}

fn audit(metadata: &Path) -> Result<bool> {
    let repository = metadata.join(".git");
    metadata::plain_directory(&repository)?;
    // Prevent Git indirections from pointing outside the dedicated repository.
    for file in [
        "commondir",
        "gitdir",
        "objects/info/alternates",
        "info/grafts",
        "shallow",
    ] {
        if metadata::exists(&repository.join(file))? {
            return Err(fail(format!("unsupported Git indirection: {file}")));
        }
    }
    let mut pending = vec![repository];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            let path = entry?.path();
            let info = fs::symlink_metadata(&path)?;
            if info.is_dir() {
                pending.push(path);
            } else {
                metadata::plain_file(&path)?;
            }
        }
    }
    let names = run(metadata, &["ls-files", "--stage", "-z"], None)?;
    for entry in names.split('\0').filter(|s| !s.is_empty()) {
        let Some((header, path)) = entry.split_once('\t') else {
            return Err(fail("invalid Git index entry"));
        };
        if path != "structure.tree" || !header.starts_with("100644 ") || !header.ends_with(" 0") {
            return Err(fail(
                "metadata Git index contains unexpected paths or conflict stages",
            ));
        }
    }
    Ok(!names.is_empty())
}

fn head(metadata: &Path) -> Result<Option<String>> {
    let output = execute(
        metadata,
        &["rev-parse", "--verify", "--quiet", "HEAD"],
        None,
    )?;
    if output.status.success() {
        return Ok(Some(String::from_utf8_lossy(&output.stdout).trim().into()));
    }
    if output.status.code() == Some(1) {
        return Ok(None);
    }
    Err(fail(format!(
        "cannot read Git HEAD: {}",
        String::from_utf8_lossy(&output.stderr)
    )))
}

fn head_tree(metadata: &Path) -> Result<String> {
    let listing = run(metadata, &["ls-tree", "-r", "HEAD"], None)?;
    let entries: Vec<_> = listing.lines().collect();
    if entries.len() != 1
        || !entries[0].starts_with("100644 blob ")
        || !entries[0].ends_with("\tstructure.tree")
    {
        return Err(fail(
            "Git HEAD must contain only the regular structure.tree file",
        ));
    }
    run(metadata, &["show", "HEAD:structure.tree"], None)
}

fn is_pending_commit(metadata: &Path, state: &SavedState) -> Result<bool> {
    if head(metadata)?.is_none() {
        return Ok(false);
    }
    let parents = run(metadata, &["show", "-s", "--format=%P", "HEAD"], None)?;
    Ok(parents.trim() == state.git_head.as_deref().unwrap_or("")
        && head_tree(metadata)? == state.tree_text)
}

pub(crate) fn check(metadata: &Path, state: &SavedState) -> Result<()> {
    let indexed = audit(metadata)?;
    if indexed {
        let staged = run(metadata, &["show", ":structure.tree"], None)?;
        if staged != state.tree_text {
            let previous = match state.git_head.as_deref().filter(|_| state.pending_git) {
                Some(head) => Some(run(
                    metadata,
                    &["show", &format!("{head}:structure.tree")],
                    None,
                )?),
                None => None,
            };
            if previous.as_deref() != Some(staged.as_str()) {
                return Err(fail(
                    "staged structure.tree edits conflict with the saved or pending baseline",
                ));
            }
        }
    } else if !state.pending_git {
        return Err(fail(
            "structure.tree is missing from the metadata Git index",
        ));
    }
    if head(metadata)? != state.git_head {
        if state.pending_git && is_pending_commit(metadata, state)? {
            return Ok(());
        }
        return Err(fail(
            "metadata Git HEAD changed outside organizer; resolve the state conflict before syncing",
        ));
    }
    if !state.pending_git && head_tree(metadata)? != state.tree_text {
        return Err(fail("Git baseline and identity index disagree"));
    }
    Ok(())
}

/// Returns the committed HEAD. A retry can finish an already-published commit
/// only when its sole parent and exact Tree content match the pending state.
pub(crate) fn commit(metadata: &Path, state: &SavedState, message: &str) -> Result<String> {
    check(metadata, state)?;
    let current = head(metadata)?;
    let commit = if current != state.git_head
        || (current.is_some() && head_tree(metadata)? == state.tree_text)
    {
        current.ok_or_else(|| fail("missing pending Git commit"))?
    } else {
        let blob = run(
            metadata,
            &["hash-object", "-w", "--stdin", "--no-filters"],
            Some(&state.tree_text),
        )?;
        let tree = run(
            metadata,
            &["mktree"],
            Some(&format!("100644 blob {}\tstructure.tree\n", blob.trim())),
        )?;
        let mut args = vec!["commit-tree", tree.trim()];
        if let Some(parent) = current.as_deref() {
            args.extend(["-p", parent]);
        }
        args.extend(["-m", message]);
        let commit = run(metadata, &args, None)?.trim().to_owned();
        run(
            metadata,
            &[
                "update-ref",
                "HEAD",
                &commit,
                current
                    .as_deref()
                    .unwrap_or("0000000000000000000000000000000000000000"),
            ],
            None,
        )?;
        commit
    };
    run(metadata, &["read-tree", &commit], None)?;
    Ok(commit)
}

pub fn history(metadata: &Path, state: &SavedState) -> Result<String> {
    check(metadata, state)?;
    run(
        metadata,
        &["log", "--format=%h %s", "--", "structure.tree"],
        None,
    )
}
