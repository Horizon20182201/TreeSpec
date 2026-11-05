use super::*;
use std::time::{Duration, UNIX_EPOCH};

fn managed() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("Inbox")).unwrap();
    fs::write(temp.path().join("Inbox/note.txt"), "note").unwrap();
    crate::sync::init(temp.path()).unwrap();
    temp
}

#[test]
fn utc_stamp_matches_reference_dates() {
    for (seconds, expected) in [
        (0, "19700101T000000Z"),
        (1_700_000_000, "20231114T221320Z"),
        (951_782_400, "20000229T000000Z"),
        (4_102_444_799, "20991231T235959Z"),
    ] {
        let time = UNIX_EPOCH + Duration::from_secs(seconds);
        assert_eq!(archive::utc_stamp(time), expected);
    }
}

#[test]
fn probe_reports_uninitialized_roots_and_interrupted_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let probe = probe(temp.path()).unwrap();
    assert!(!probe.initialized);
    let temp = managed();
    let clean = super::probe(temp.path()).unwrap();
    assert!(clean.initialized && clean.runs.is_empty() && clean.pending_marker.is_none());
    fs::write(
        temp.path().join(".organizer/state/pending.json"),
        "{\"operation\":\"sync\"}\n",
    )
    .unwrap();
    fs::write(
        temp.path().join(".organizer/state/index.sqlite-journal"),
        "",
    )
    .unwrap();
    let interrupted = super::probe(temp.path()).unwrap();
    assert_eq!(
        interrupted.pending_marker.as_deref(),
        Some("{\"operation\":\"sync\"}\n")
    );
    assert_eq!(interrupted.recovery_files, ["state/index.sqlite-journal"]);
    // status itself refuses this state; the probe is the only reporter.
    assert!(crate::sync::status(temp.path()).is_err());
}

#[test]
fn detailed_status_separates_pending_drafts_from_conflicts() {
    let temp = managed();
    let root = temp.path();
    copy_baseline_draft(root, "new.tree").unwrap();
    let detailed = crate::sync::status_detailed(root).unwrap();
    assert!(detailed.report.conflicts.is_empty());
    assert_eq!(detailed.pending.len(), 1);
    assert_eq!(detailed.pending[0].selector(), "drafts/new.tree");
    assert_eq!(
        detailed.saved.tree_text,
        fs::read_to_string(root.join(".organizer/structure.tree")).unwrap()
    );
    // The CLI report is unchanged: the pending draft is still a conflict.
    let plain = crate::sync::status(root).unwrap();
    assert_eq!(plain.conflicts.len(), 1);
    assert!(plain.conflicts[0].starts_with("pending drafts entry \"new.tree\""));
}

#[test]
fn copy_baseline_never_overwrites_and_validates_names() {
    let temp = managed();
    let root = temp.path();
    let draft = copy_baseline_draft(root, "new.tree").unwrap();
    assert_eq!(
        fs::read(&draft).unwrap(),
        fs::read(root.join(".organizer/structure.tree")).unwrap()
    );
    fs::write(&draft, "edited").unwrap();
    assert!(copy_baseline_draft(root, "new.tree").is_err());
    assert_eq!(fs::read_to_string(&draft).unwrap(), "edited");
    for bad in [
        "new",
        "../x.tree",
        ".hidden.tree",
        "a/b.tree",
        ".tree",
        "x.review.json",
    ] {
        assert!(copy_baseline_draft(root, bad).is_err(), "{bad}");
    }
    fs::write(root.join(".organizer/drafts/llm.tree.review.json"), "{}").unwrap();
    assert!(copy_baseline_draft(root, "llm.tree").is_err());
}

#[test]
fn archive_moves_pairs_without_overwriting_and_unblocks_sync() {
    let temp = managed();
    // macOS resolves /var to /private/var; compare against the canonical root.
    let root = &temp.path().canonicalize().unwrap();
    let drafts = root.join(".organizer/drafts");
    copy_baseline_draft(root, "keep.tree").unwrap();
    copy_baseline_draft(root, "llm.tree").unwrap();
    fs::write(drafts.join("llm.tree.review.json"), "{}").unwrap();
    fs::write(root.join(".organizer/plans/llm.json"), "{}").unwrap();
    assert!(archive(root, &["drafts/missing.tree".into()]).is_err());
    let report = archive(root, &["drafts/llm.tree".into(), "plans/llm.json".into()]).unwrap();
    assert_eq!(
        report.moved,
        [
            "drafts/llm.tree",
            "drafts/llm.tree.review.json",
            "plans/llm.json"
        ]
    );
    assert!(report.archive.starts_with(root.join(".organizer/archive")));
    assert!(
        report
            .archive
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .ends_with("-llm")
    );
    assert_eq!(
        fs::read_to_string(report.archive.join("drafts/llm.tree.review.json")).unwrap(),
        "{}"
    );
    assert!(drafts.join("keep.tree").exists() && !drafts.join("llm.tree").exists());
    assert!(crate::sync::sync(root, false).is_err());
    let all = archive(root, &[]).unwrap();
    assert_eq!(all.moved, ["drafts/keep.tree"]);
    assert_ne!(all.archive, report.archive);
    assert!(archive(root, &[]).is_err());
    // Archives live inside .organizer, so they are neither user files nor drafts.
    let synced = crate::sync::sync(root, false).unwrap();
    assert!(!synced.structure_changed && synced.report.added.is_empty());
}
