//! The only code that reads disk for a refresh. Library calls only.

use super::{
    rounds::{self, Loaded, LoadedDraft, LoadedPlan, Rounds},
    state::{RecoveryEntry, Snapshot},
    tree_index::TreeIndex,
};
use organizer::{
    llm::{self, ChatReport},
    migration::{MigrationPlan, journal},
    storage::SavedState,
    sync::{self, DetailedStatus},
    tree::parse_tree_with_base,
    workspace::{self, Artifact, ArtifactKind},
};
use std::{collections::HashSet, fs, io::Read, path::Path, time::Instant};

const READ_LIMIT: u64 = 64 * 1024 * 1024;

fn read(path: &Path) -> Result<String, String> {
    let mut bytes = vec![];
    fs::File::open(path)
        .and_then(|file| file.take(READ_LIMIT + 1).read_to_end(&mut bytes))
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > READ_LIMIT {
        return Err("文件过大".into());
    }
    String::from_utf8(bytes).map_err(|_| "不是 UTF-8 文本".into())
}

pub fn snapshot(root: &Path) -> Snapshot {
    let probe = workspace::probe(root).map_err(|e| e.to_string());
    let root = probe
        .as_ref()
        .map_or_else(|_| root.to_owned(), |p| p.root.clone());
    let mut recoveries = vec![];
    let mut status = None;
    let mut rounds = Rounds::default();
    let mut baseline = TreeIndex::default();
    let mut excluded = HashSet::new();
    let mut history = Ok(vec![]);
    if let Ok(probe) = &probe
        && probe.initialized
    {
        recoveries = probe
            .unfinished_runs()
            .map(|run| RecoveryEntry {
                run: run.clone(),
                report: journal::recover(&root, &run.run_id, false).map_err(|e| e.to_string()),
            })
            .collect();
        // status refuses these states; the probe already reports them.
        let blocked = !recoveries.is_empty()
            || probe.pending_marker.is_some()
            || !probe.recovery_files.is_empty();
        if !blocked {
            let detailed = sync::status_detailed(&root).map_err(|e| e.to_string());
            if let Ok(detailed) = &detailed {
                let saved = &detailed.saved;
                baseline = TreeIndex::new(&saved.tree);
                excluded = llm::excluded_ids(saved).into_iter().collect();
                rounds = rounds::build(&saved.tree, &saved.tree_text, &root, load(&root, detailed));
                history = sync::history(&root)
                    .map(|text| parse_history(&text))
                    .map_err(|e| e.to_string());
            }
            status = Some(detailed);
        }
    }
    Snapshot {
        probe,
        status,
        recoveries,
        rounds,
        baseline,
        excluded,
        history,
        scanned_at: Instant::now(),
    }
}

fn parse_history(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| line.split_once(' '))
        .map(|(hash, subject)| (hash.to_owned(), subject.to_owned()))
        .collect()
}

fn load(root: &Path, detailed: &DetailedStatus) -> Loaded {
    let mut loaded = Loaded::default();
    for artifact in &detailed.pending {
        let name = artifact.name.clone();
        if artifact.kind != ArtifactKind::File {
            let note = match artifact.kind {
                ArtifactKind::Directory => "目录",
                _ => "符号链接或特殊文件",
            };
            loaded.others.push((artifact.selector(), note.into()));
            continue;
        }
        match artifact.directory {
            "drafts" if name.ends_with(".review.json") => loaded.reviews.push(name),
            "drafts" => loaded.drafts.push(draft(root, &detailed.saved, artifact)),
            "plans" if name.ends_with(".sh") => loaded.scripts.push(name),
            _ => loaded.plans.push(LoadedPlan {
                plan: read(&artifact.path).and_then(|text| {
                    serde_json::from_str::<MigrationPlan>(&text)
                        .map_err(|_| "不是迁移计划 JSON".to_owned())
                }),
                path: artifact.path.clone(),
                name,
            }),
        }
    }
    loaded
}

/// Runs the same `llm::check_review` that `plan` runs, with and without
/// `--acknowledge-review`, so the TUI never re-implements the review gate.
fn draft(root: &Path, saved: &SavedState, artifact: &Artifact) -> LoadedDraft {
    let text = read(&artifact.path);
    let review = llm::review_path(&artifact.path).ok();
    let report = review
        .as_ref()
        .filter(|path| path.is_file())
        .and_then(|path| read(path).ok())
        .and_then(|text| serde_json::from_str::<ChatReport>(&text).ok());
    let generated = text.as_ref().is_ok_and(|t| llm::has_review_marker(t))
        || review.as_ref().is_some_and(|path| path.exists());
    let tree = text
        .clone()
        .and_then(|t| parse_tree_with_base(&t, &saved.tree).map_err(|e| e.to_string()));
    let check = |acknowledged| match (&text, &tree) {
        (Ok(source), Ok(new)) => llm::check_review(
            Some(root),
            &artifact.path,
            source,
            &saved.tree,
            new,
            acknowledged,
        )
        .map_err(|e| e.to_string()),
        _ => Err("草案无法解析".to_owned()),
    };
    LoadedDraft {
        name: artifact.name.clone(),
        path: artifact.path.clone(),
        strict: check(false),
        lenient: check(true),
        tree,
        generated,
        report,
    }
}
