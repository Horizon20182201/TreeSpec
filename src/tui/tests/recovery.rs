//! Failure, interruption, conflict and display-safety paths.

use super::{Harness, ids, managed};
use crate::tui::{
    app::dialog::{Dialog, Message},
    state::Workspace,
};
use ratatui::crossterm::event::KeyCode;
use std::{fs, os::unix::fs::PermissionsExt};

#[test]
fn failed_apply_opens_recovery_and_closes_only_after_manual_review() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir(root.join("locked")).unwrap();
    fs::write(root.join("a.txt"), "keep").unwrap();
    fs::set_permissions(root.join("locked"), fs::Permissions::from_mode(0o555)).unwrap();
    if fs::write(root.join("locked/probe"), "").is_ok() {
        return; // Running as root: permissions cannot inject the failure.
    }
    organizer::sync::init(root).unwrap();
    let ids = ids(root);
    // Move a.txt into a new directory; mkdir locked/sub then fails with EACCES.
    let draft = format!(
        "!tree 1\nlocked/ @{}\n  sub/\nnew/\n  a.txt @{}\n",
        ids["locked"], ids["a.txt"]
    );
    fs::write(root.join(".organizer/drafts/fail.tree"), draft).unwrap();
    let mut h = Harness::new(root);
    h.keys("3p").key(KeyCode::Enter).keys("A");
    h.keys("apply").key(KeyCode::Enter);
    assert!(matches!(
        &h.app.dialog,
        Some(Dialog::Message(Message { error: true, .. }))
    ));
    assert_eq!(h.app.state, Workspace::Unfinished);
    assert_eq!(h.app.page, crate::tui::app::Page::Recovery);
    h.key(KeyCode::Esc);
    let screen = h.screen(100, 30);
    assert!(
        screen.contains("状态 failed") && screen.contains("已确认 1/3"),
        "{screen}"
    );
    assert!(
        screen.contains("在目标") && screen.contains("在源"),
        "{screen}"
    );
    h.keys("F");
    assert!(
        h.app.dialog.is_none(),
        "finish-git needs every step acknowledged"
    );
    h.keys("1s");
    assert!(
        h.app.dialog.is_none(),
        "sync is blocked by the unfinished run"
    );
    h.keys("5My");
    assert!(matches!(&h.app.dialog, Some(Dialog::Confirm(c)) if c.note.is_some()));
    h.keys(" y");
    assert_ne!(h.app.state, Workspace::Unfinished);
    let runs = organizer::migration::journal::list_runs(root).unwrap();
    assert_eq!(runs[0].status, "resolved");
    h.keys("3Zy1sy");
    assert_eq!(h.app.state, Workspace::Clean);
    assert!(
        h.app
            .snap()
            .unwrap()
            .baseline
            .paths
            .values()
            .any(|p| p == "new")
    );
    assert_eq!(
        fs::read(root.join("a.txt")).unwrap(),
        b"keep",
        "nothing was replayed"
    );
    fs::set_permissions(root.join("locked"), fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn interrupted_metadata_is_read_only() {
    let temp = managed();
    fs::write(
        temp.path().join(".organizer/state/pending.json"),
        "{\"operation\":\"sync\"}\n",
    )
    .unwrap();
    let mut h = Harness::new(temp.path());
    assert_eq!(h.app.state, Workspace::Interrupted);
    assert!(h.screen(100, 24).contains("先备份整个 .organizer"));
    h.keys("1s");
    assert!(h.app.dialog.is_none());
    h.keys("3n");
    assert!(h.app.dialog.is_none());
}

#[test]
fn baseline_conflict_can_save_the_edited_structure_as_a_draft() {
    let temp = managed();
    let baseline = temp.path().join(".organizer/structure.tree");
    let edited = fs::read_to_string(&baseline)
        .unwrap()
        .replace("Inbox/ @", "Mail/ @");
    fs::write(&baseline, &edited).unwrap();
    let mut h = Harness::new(temp.path());
    assert_eq!(h.app.state, Workspace::Conflict);
    h.keys("3n");
    assert!(matches!(&h.app.dialog, Some(Dialog::Choice(c)) if c.llm_blocked.is_some()));
    h.key(KeyCode::Enter).key(KeyCode::Enter);
    assert_eq!(
        fs::read_to_string(temp.path().join(".organizer/drafts/new.tree")).unwrap(),
        edited
    );
    assert_eq!(h.app.state, Workspace::Conflict);
}

#[test]
fn hostile_names_are_escaped_and_small_terminals_do_not_panic() {
    let temp = managed();
    fs::write(temp.path().join("evil\u{1b}[2J\u{202e}.txt"), "x").unwrap();
    organizer::sync::sync(temp.path(), false).unwrap();
    let mut h = Harness::new(temp.path());
    h.keys("2");
    let screen = h.screen(100, 30);
    assert!(!screen.contains('\u{1b}') && !screen.contains('\u{202e}'));
    assert!(screen.contains("\\u{1b}"), "{screen}");
    h.keys("3n");
    h.key(KeyCode::Esc);
    // Below 80 columns one pane is shown; Tab switches (design §9.4).
    h.keys("2");
    let narrow = h.screen(60, 20);
    assert!(
        narrow.contains("基线结构") && !narrow.contains("范围：未选择"),
        "{narrow}"
    );
    h.key(KeyCode::Tab);
    let narrow = h.screen(60, 20);
    assert!(
        !narrow.contains("基线结构") && narrow.contains("范围：未选择"),
        "{narrow}"
    );
    h.key(KeyCode::Tab);
    for page in "12345".chars() {
        h.keys(&page.to_string());
        for dialog in [None, Some(Dialog::Help(0)), Some(Dialog::Log(0))] {
            h.app.dialog = dialog;
            for (w, rows) in [(80, 24), (60, 16), (30, 8), (10, 3)] {
                h.screen(w, rows);
            }
        }
    }
}
