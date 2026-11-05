//! End-to-end flows through the App, against real temporary roots.

use super::{Harness, ids, managed};
use crate::tui::{app::dialog::Dialog, rounds::Stage, state::Workspace};
use ratatui::crossterm::event::KeyCode;
use std::fs;

fn stage(h: &Harness) -> Stage {
    h.app
        .current_round()
        .expect("a selected round")
        .stage
        .clone()
}

#[test]
fn initializes_only_after_confirmation() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("note.txt"), "x").unwrap();
    let mut h = Harness::new(temp.path());
    assert_eq!(h.app.state, Workspace::Uninitialized);
    assert!(h.screen(80, 24).contains("按 i 初始化"));
    h.keys("i");
    assert!(
        matches!(&h.app.dialog, Some(Dialog::Confirm(c)) if c.command.as_deref().unwrap().starts_with("organizer init --root"))
    );
    h.key(KeyCode::Enter); // focus starts on 取消
    assert!(h.app.dialog.is_none() && !temp.path().join(".organizer").exists());
    h.keys("iy");
    assert_eq!(h.app.state, Workspace::Clean);
    assert!(temp.path().join(".organizer/structure.tree").is_file());
}

#[test]
fn manual_round_plans_previews_applies_and_archives() {
    let temp = managed();
    let root = temp.path();
    let mut h = Harness::new(root);
    assert_eq!(h.app.state, Workspace::Clean);
    h.keys("3n");
    assert!(
        matches!(&h.app.dialog, Some(Dialog::Choice(c)) if c.focus == 1 && c.llm_blocked.is_some())
    );
    h.key(KeyCode::Enter);
    assert!(matches!(&h.app.dialog, Some(Dialog::Input(i)) if i.value == "new.tree"));
    h.key(KeyCode::Enter);
    let draft = root.join(".organizer/drafts/new.tree");
    assert_eq!(
        fs::read(&draft).unwrap(),
        fs::read(root.join(".organizer/structure.tree")).unwrap()
    );
    assert_eq!(stage(&h), Stage::NoChanges);
    // What the user does in the external editor.
    let text = fs::read_to_string(&draft)
        .unwrap()
        .replace("Inbox/ @", "Archive/ @");
    fs::write(&draft, text).unwrap();
    h.keys("R");
    assert_eq!(stage(&h), Stage::NeedsPlan);
    assert!(h.screen(80, 24).contains("原名 Inbox"));
    h.keys("p");
    assert!(matches!(&h.app.dialog, Some(Dialog::Input(i)) if i.value == "new.json"));
    h.key(KeyCode::Enter);
    assert!(root.join(".organizer/plans/new.json").is_file());
    assert_eq!(stage(&h), Stage::Executable);
    h.keys("d");
    assert!(h.app.flash.as_ref().unwrap().0.starts_with("预览通过"));
    h.keys("A");
    assert!(
        matches!(&h.app.dialog, Some(Dialog::Apply(_))),
        "strong confirmation expected"
    );
    h.keys("appl").key(KeyCode::Enter);
    assert!(
        root.join("Inbox").exists(),
        "an incomplete confirmation must not apply"
    );
    h.keys("y").key(KeyCode::Enter);
    assert!(root.join("Archive/note.txt").is_file() && !root.join("Inbox").exists());
    assert_eq!(
        fs::read(root.join("Archive/note.txt")).unwrap(),
        b"user bytes"
    );
    assert_eq!(stage(&h), Stage::Applied);
    assert_eq!(h.app.state, Workspace::Clean);
    h.keys("1s");
    assert!(
        h.app.dialog.is_none(),
        "sync is blocked while the round is pending"
    );
    h.keys("3zy");
    assert!(h.app.snap().unwrap().rounds.is_empty());
    assert_eq!(
        fs::read_dir(root.join(".organizer/archive"))
            .unwrap()
            .count(),
        1
    );
    assert!(
        h.app
            .log
            .iter()
            .any(|e| e.command.contains(" apply ") && e.command.ends_with("--yes"))
    );
}

#[test]
fn outside_changes_block_new_drafts_until_synced() {
    let temp = managed();
    let root = temp.path();
    fs::write(root.join("Inbox/new.txt"), "x").unwrap();
    let mut h = Harness::new(root);
    assert_eq!(h.app.state, Workspace::Changed);
    h.keys("3n");
    assert!(
        h.app.dialog.is_none(),
        "no new drafts while the disk has changes"
    );
    h.keys("1s");
    let Some(Dialog::Confirm(c)) = &h.app.dialog else {
        panic!("sync confirmation expected")
    };
    assert!(
        c.check.is_none()
            && c.command
                .as_deref()
                .unwrap()
                .starts_with("organizer sync --root")
    );
    h.keys("y");
    assert_eq!(h.app.state, Workspace::Clean);
}

#[test]
fn edited_llm_draft_requires_review_confirmation() {
    let temp = managed();
    let root = temp.path();
    let ids = ids(root);
    let reply = serde_json::json!({"schema_version":1,"edits":[{"op":"rename_node","id":ids["Papers"],"name":"Research"}],"migration_intent":{"kind":"structure_only","scope_dir_ids":[]}});
    let mut h = Harness::with_llm(root, vec![reply]);
    h.keys("3n").key(KeyCode::Enter);
    let Some(Dialog::Chat(form)) = &h.app.dialog else {
        panic!("chat form expected")
    };
    assert!(form.preview.is_ok());
    h.keys("把 Papers 改名为 Research");
    for _ in 0..5 {
        h.key(KeyCode::Tab);
    }
    h.key(KeyCode::Enter);
    let round = h.app.current_round().unwrap();
    assert_eq!(round.stage, Stage::NeedsPlan);
    assert!(round.needs_ack.is_none() && round.report.is_some());
    let draft = round.path.clone();
    let inbox = format!("Inbox/ @{}", ids["Inbox"]);
    let edited = fs::read_to_string(&draft)
        .unwrap()
        .replace(&inbox, &format!("{inbox} # 收件箱"));
    fs::write(&draft, edited).unwrap();
    h.keys("R");
    let reason = h
        .app
        .current_round()
        .unwrap()
        .needs_ack
        .clone()
        .expect("review confirmation required");
    assert!(reason.contains("编辑"));
    let from = root
        .canonicalize()
        .unwrap()
        .join(".organizer/structure.tree");
    assert!(
        crate::plan_documents(Some(&h.app.root), &from, &draft, false).is_err(),
        "the library gate agrees"
    );
    h.keys("py");
    assert!(
        matches!(&h.app.dialog, Some(Dialog::Confirm(c)) if c.note.is_some()),
        "must tick the statement first"
    );
    h.keys(" y");
    assert!(matches!(&h.app.dialog, Some(Dialog::Input(i)) if i.value.ends_with(".reviewed.json")));
    h.key(KeyCode::Enter);
    assert_eq!(stage(&h), Stage::Executable);
}

#[test]
fn quitting_during_apply_needs_two_ctrl_c() {
    use crate::tui::app::Busy;
    use ratatui::crossterm::event::{KeyEvent, KeyModifiers};
    let temp = managed();
    let mut h = Harness::new(temp.path());
    let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    h.app.busy = Some(Busy {
        label: "执行迁移",
        started: std::time::Instant::now(),
        progress: Some((1, 3)),
        applying: true,
    });
    h.keys("q");
    assert!(!h.app.quit, "q is ignored while files move");
    h.app.key(ctrl_c);
    assert!(!h.app.quit && h.app.flash.as_ref().unwrap().0.contains("人工恢复"));
    h.app.key(ctrl_c);
    assert!(h.app.quit);
    // Outside a migration a single Ctrl-C quits.
    let mut h = Harness::new(temp.path());
    h.app.key(ctrl_c);
    assert!(h.app.quit);
}
