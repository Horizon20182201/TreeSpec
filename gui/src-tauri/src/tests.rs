//! Session tests on real temporary directories (design §13.1). The worker runs
//! for real; events are collected from the sink.

use crate::{
    action::GuiAction,
    dto::{GuiEvent, OutcomeView, ViewModel},
    secrets::Memory,
    session::{AppCore, RevealTarget, Session},
};
use std::{
    fs,
    sync::{Arc, mpsc},
    time::Duration,
};

struct Harness {
    _temp: tempfile::TempDir,
    root: std::path::PathBuf,
    session: Arc<Session>,
    events: mpsc::Receiver<GuiEvent>,
}

impl Harness {
    fn new(init: bool) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("Workspace");
        fs::create_dir_all(root.join("Inbox")).unwrap();
        fs::write(root.join("Inbox/note.txt"), "note").unwrap();
        fs::write(root.join(".env"), "SECRET=1").unwrap();
        if init {
            organizer::sync::init(&root).unwrap();
        }
        let root = organizer::sync::resolve_root(Some(&root)).unwrap();
        let core = AppCore::new(
            temp.path().join("settings.json"),
            Box::new(Memory::default()),
        );
        let (tx, events) = mpsc::channel();
        let tx = std::sync::Mutex::new(tx);
        let sink = Arc::new(move |e| {
            let _ = tx.lock().unwrap().send(e);
        });
        let session = Session::spawn(root.clone(), core, sink, None);
        let mut h = Self {
            _temp: temp,
            root,
            session,
            events,
        };
        h.idle();
        h
    }

    /// Events until the session is idle.
    fn idle(&mut self) -> Vec<GuiEvent> {
        let mut out = vec![];
        loop {
            let event = self
                .events
                .recv_timeout(Duration::from_secs(30))
                .expect("timed out");
            let idle = matches!(event, GuiEvent::Idle);
            out.push(event);
            if idle {
                return out;
            }
        }
    }

    fn view(&self) -> ViewModel {
        self.session.view().expect("view")
    }

    fn run(&mut self, action: GuiAction) -> crate::dto::ActionResult {
        self.session.run(action).unwrap();
        self.idle()
            .into_iter()
            .find_map(|e| match e {
                GuiEvent::Done { result } => Some(*result),
                _ => None,
            })
            .expect("done event")
    }
}

#[test]
fn full_round_from_init_to_archive() {
    let mut h = Harness::new(false);
    assert_eq!(h.view().state.code, "uninit");
    assert_eq!(h.view().next.intent.as_deref(), Some("init"));
    assert_eq!(h.run(GuiAction::Init).kind, "ok");
    let view = h.view();
    assert_eq!(view.state.code, "clean");
    // `.env` is never offered to the LLM.
    assert!(
        view.nodes
            .iter()
            .any(|n| n.name == ".env" && n.excluded.is_some())
    );
    assert!(
        view.nodes
            .iter()
            .any(|n| n.name == "Inbox" && n.excluded.is_none())
    );

    assert_eq!(
        h.run(GuiAction::CopyBaseline {
            name: "new.tree".into()
        })
        .kind,
        "ok"
    );
    let draft = h.session.draft_text("new.tree").unwrap();
    assert!(!draft.generated);
    let edited = format!("# 手写注释\n{}", draft.text.replace("Inbox/", "Archive/"));
    let check = h.session.check_draft("new.tree", &edited).unwrap();
    assert!(check.ok && check.renamed == 1 && check.missing_files.is_empty());
    let saved = h
        .session
        .save_draft("new.tree", &draft.digest, &edited)
        .unwrap();
    assert_eq!(saved.kind, "ok", "{saved:?}");
    h.idle();
    // A second save with the old digest is refused.
    let stale = h
        .session
        .save_draft("new.tree", &draft.digest, &edited)
        .unwrap();
    assert_eq!(stale.kind, "failed");
    assert_eq!(h.view().rounds[0].stage_code, "needs_plan");

    let shown = h
        .session
        .command(GuiAction::Plan {
            round: "new.tree".into(),
            acknowledge: false,
            out: "new.json".into(),
        })
        .unwrap();
    assert!(
        shown.starts_with("organizer plan --root ")
            && shown.ends_with("/.organizer/plans/new.json"),
        "{shown}"
    );
    let planned = h.run(GuiAction::Plan {
        round: "new.tree".into(),
        acknowledge: false,
        out: "new.json".into(),
    });
    assert!(
        matches!(
            planned.outcome,
            Some(OutcomeView::Planned {
                written: true,
                ops: 1,
                ..
            })
        ),
        "{planned:?}"
    );
    let detail = h.session.round_detail("new.tree").unwrap();
    assert_eq!(detail.plan.as_ref().unwrap().ops.len(), 1);
    assert!(detail.rows.iter().any(|r| r.mark.as_deref() == Some("~")));

    // Apply needs the token from a preview of this plan.
    let wrong = GuiAction::Apply {
        plan: "new.json".into(),
        token: "nope".into(),
    };
    assert!(h.session.run(wrong).is_err());
    let preview = h.run(GuiAction::DryRun {
        plan: "new.json".into(),
    });
    let Some(OutcomeView::Previewed { token, .. }) = preview.outcome else {
        panic!("{preview:?}")
    };
    let applied = h.run(GuiAction::Apply {
        plan: "new.json".into(),
        token: token.clone(),
    });
    assert!(
        matches!(applied.outcome, Some(OutcomeView::Applied { ops: 1, .. })),
        "{applied:?}"
    );
    assert_eq!(
        fs::read_to_string(h.root.join("Archive/note.txt")).unwrap(),
        "note"
    );
    // The token was one-shot.
    assert!(
        h.session
            .run(GuiAction::Apply {
                plan: "new.json".into(),
                token
            })
            .is_err()
    );
    assert_eq!(h.view().rounds[0].stage_code, "applied");

    let archived = h.run(GuiAction::Archive { selectors: vec![] });
    assert!(
        matches!(archived.outcome, Some(OutcomeView::Archived { .. })),
        "{archived:?}"
    );
    let view = h.view();
    assert!(view.rounds.is_empty() && view.state.code == "clean");
    assert_eq!(
        view.commits.first().map(|c| c.kind.as_str()),
        Some("refactor")
    );
}

#[test]
fn refresh_invalidates_tokens_and_whitelist_rejects_paths() {
    let mut h = Harness::new(true);
    h.run(GuiAction::CopyBaseline {
        name: "a.tree".into(),
    });
    let draft = h.session.draft_text("a.tree").unwrap();
    h.session
        .save_draft(
            "a.tree",
            &draft.digest,
            &draft.text.replace("Inbox/", "Box/"),
        )
        .unwrap();
    h.idle();
    h.run(GuiAction::Plan {
        round: "a.tree".into(),
        acknowledge: false,
        out: "a.json".into(),
    });
    let preview = h.run(GuiAction::DryRun {
        plan: "a.json".into(),
    });
    let Some(OutcomeView::Previewed { token, .. }) = preview.outcome else {
        panic!()
    };
    h.session.refresh();
    h.idle();
    let error = h
        .session
        .run(GuiAction::Apply {
            plan: "a.json".into(),
            token,
        })
        .unwrap_err();
    assert!(error.contains("预览已失效"), "{error}");

    for bad in [
        GuiAction::CopyBaseline {
            name: "../x.tree".into(),
        },
        GuiAction::Archive {
            selectors: vec!["../structure.tree".into()],
        },
        GuiAction::Plan {
            round: "missing.tree".into(),
            acknowledge: false,
            out: "m.json".into(),
        },
        GuiAction::Plan {
            round: "a.tree".into(),
            acknowledge: false,
            out: "a.json".into(),
        },
        GuiAction::Export {
            plan: "../../etc.json".into(),
        },
        GuiAction::MarkResolved {
            run: "run_x".into(),
        },
    ] {
        assert!(h.session.run(bad.clone()).is_err(), "{bad:?}");
    }
    let env = h
        .view()
        .nodes
        .iter()
        .find(|n| n.name == ".env")
        .unwrap()
        .id
        .clone();
    let chat = GuiAction::Chat {
        prompt: "x".into(),
        scope: vec![env],
        classify: false,
        read_content: false,
        out: "c.tree".into(),
    };
    assert!(h.session.run(chat).is_err());
    assert!(
        h.session
            .reveal_path(&RevealTarget::File {
                selector: "drafts/a.tree".into()
            })
            .is_ok()
    );
    assert!(
        h.session
            .reveal_path(&RevealTarget::File {
                selector: "drafts/../../x".into()
            })
            .is_err()
    );
    assert!(
        h.session
            .reveal_path(&RevealTarget::Node { id: "nope".into() })
            .is_err()
    );
    assert!(h.session.check_draft("missing.tree", "").is_err());
}

#[test]
fn changes_block_drafts_and_unconfigured_llm_explains_why() {
    let mut h = Harness::new(true);
    h.run(GuiAction::CopyBaseline {
        name: "a.tree".into(),
    });
    fs::write(h.root.join("added.txt"), "x").unwrap();
    h.session.refresh();
    h.idle();
    let view = h.view();
    assert_eq!(view.state.code, "changed");
    assert!(!view.gates.draft.ok && view.gates.draft.reason.as_deref() == Some("受阻：磁盘有变化"));
    assert!(!view.gates.sync.ok, "pending rounds block sync");
    assert_eq!(view.next.intent.as_deref(), Some("archive_all"));
    assert_eq!(
        view.rounds[0].blocked, None,
        "an unchanged copy is already done"
    );
    assert!(!view.llm.available && view.llm.reason.is_some());
    assert_eq!(view.overview.as_ref().unwrap().added.len(), 1);
    // The view model serializes for the WebView.
    serde_json::to_string(&view).unwrap();
}

#[test]
fn apply_refuses_a_plan_file_replaced_after_preview() {
    let mut h = Harness::new(true);
    h.run(GuiAction::CopyBaseline {
        name: "a.tree".into(),
    });
    let draft = h.session.draft_text("a.tree").unwrap();
    h.session
        .save_draft(
            "a.tree",
            &draft.digest,
            &draft.text.replace("Inbox/", "Box/"),
        )
        .unwrap();
    h.idle();
    h.run(GuiAction::Plan {
        round: "a.tree".into(),
        acknowledge: false,
        out: "a.json".into(),
    });
    let preview = h.run(GuiAction::DryRun {
        plan: "a.json".into(),
    });
    let Some(OutcomeView::Previewed { token, .. }) = preview.outcome else {
        panic!()
    };
    // Another process rewrites the plan file; no refresh happens in this window.
    let path = h.root.join(".organizer/plans/a.json");
    let mut plan: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    plan["plan_id"] = "sha256:other".into();
    fs::write(&path, plan.to_string()).unwrap();
    let error = h
        .session
        .run(GuiAction::Apply {
            plan: "a.json".into(),
            token,
        })
        .unwrap_err();
    assert!(error.contains("被替换"), "{error}");
    assert!(h.root.join("Inbox").is_dir(), "nothing moved");
}

#[test]
fn closing_a_window_frees_its_session() {
    let h = Harness::new(true);
    let weak = Arc::downgrade(&h.session);
    drop(h);
    for _ in 0..100 {
        if weak.strong_count() == 0 {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("the session is still alive after its window closed");
}
