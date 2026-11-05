//! Live check of an unsaved draft for the GUI editor (GUI design §6.5): the
//! same parse and review checks the snapshot loader runs on a saved draft,
//! applied to text in memory. Writes nothing and does not scan the disk; the
//! review gate reads the draft's `NAME.review.json` if there is one.

use crate::{
    error::TreeError,
    llm,
    storage::SavedState,
    tree::{DiffReport, compare_trees, parse_tree_with_base},
};
use std::path::Path;

#[derive(Debug)]
pub struct DraftCheck {
    /// Parse or validation error, with the 1-based line when the parser has one.
    pub error: Option<(Option<usize>, String)>,
    /// Comparison with the baseline, when the text parses.
    pub diff: Option<DiffReport>,
    /// `llm::check_review` without `--acknowledge-review`.
    pub strict: Result<(), String>,
    /// `llm::check_review` with `--acknowledge-review`.
    pub lenient: Result<(), String>,
}

/// `saved` is the indexed baseline from the last snapshot; `path` is the
/// draft's file, used only to find its review report.
pub fn check_draft(root: &Path, saved: &SavedState, path: &Path, text: &str) -> DraftCheck {
    let tree = match parse_tree_with_base(text, &saved.tree) {
        Ok(tree) => tree,
        Err(error) => {
            let line = match &error {
                TreeError::Parse { line, .. } => Some(*line),
                _ => None,
            };
            let message = error.to_string();
            return DraftCheck {
                error: Some((line, message)),
                diff: None,
                strict: Err("草案无法解析".into()),
                lenient: Err("草案无法解析".into()),
            };
        }
    };
    let review = |acknowledged| {
        llm::check_review(Some(root), path, text, &saved.tree, &tree, acknowledged)
            .map_err(|e| e.to_string())
    };
    let (diff, error) = match compare_trees(&saved.tree, &tree) {
        Ok(diff) => (Some(diff), None),
        Err(error) => (None, Some((None, error.to_string()))),
    };
    DraftCheck {
        error,
        diff,
        strict: review(false),
        lenient: review(true),
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;
    use crate::{
        frontend::{load, next, state},
        tree::UnresolvedKind,
        workspace,
    };
    use std::fs;

    fn managed() -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("Inbox")).unwrap();
        fs::write(temp.path().join("Inbox/note.txt"), "note").unwrap();
        crate::sync::init(temp.path()).unwrap();
        temp
    }

    #[test]
    fn check_reports_lines_changes_and_missing_files() {
        let temp = managed();
        let path = workspace::copy_baseline_draft(temp.path(), "new.tree").unwrap();
        let saved = crate::sync::status_detailed(temp.path()).unwrap().saved;
        let text = fs::read_to_string(&path).unwrap();

        let renamed = text.replace("Inbox/", "Archive/");
        let ok = check_draft(temp.path(), &saved, &path, &renamed);
        assert!(ok.error.is_none() && ok.strict.is_ok() && ok.lenient.is_ok());
        assert_eq!(ok.diff.unwrap().changes.len(), 1);

        let bad = check_draft(temp.path(), &saved, &path, &format!("{text}   odd\n"));
        assert!(matches!(bad.error, Some((Some(_), _))), "{:?}", bad.error);

        let dropped: String = text
            .lines()
            .filter(|l| !l.contains("note.txt"))
            .map(|l| format!("{l}\n"))
            .collect();
        let missing = check_draft(temp.path(), &saved, &path, &dropped)
            .diff
            .unwrap();
        assert!(
            missing
                .unresolved
                .iter()
                .any(|u| u.reason == UnresolvedKind::UnresolvedMissingFile)
        );
        // Nothing was written.
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
    }

    #[test]
    fn next_step_follows_rounds() {
        let temp = managed();
        let snapshot = load::snapshot(temp.path());
        let state = snapshot.classify();
        assert_eq!(state, state::Workspace::Clean);
        assert_eq!(
            next::next_step(&state, Some(&snapshot)),
            next::NextStep::NewDraft
        );

        let path = workspace::copy_baseline_draft(temp.path(), "new.tree").unwrap();
        let unchanged = load::snapshot(temp.path());
        assert!(matches!(
            next::next_step(&unchanged.classify(), Some(&unchanged)),
            next::NextStep::Archive { .. }
        ));
        let text = fs::read_to_string(&path)
            .unwrap()
            .replace("Inbox/", "Archive/");
        fs::write(&path, text).unwrap();
        let edited = load::snapshot(temp.path());
        assert_eq!(
            next::next_step(&edited.classify(), Some(&edited)),
            next::NextStep::Plan {
                round: "new.tree".into()
            }
        );

        fs::write(temp.path().join("added.txt"), "x").unwrap();
        let changed = load::snapshot(temp.path());
        assert_eq!(
            next::next_step(&changed.classify(), Some(&changed)),
            next::NextStep::Sync {
                archive_first: true
            }
        );
        let fresh = tempfile::tempdir().unwrap();
        let uninit = load::snapshot(fresh.path());
        assert_eq!(
            next::next_step(&uninit.classify(), Some(&uninit)),
            next::NextStep::Init
        );
    }
}
