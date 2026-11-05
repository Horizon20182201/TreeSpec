//! Design §9.1: each TUI action's command line parses with the real CLI parser
//! into the same parameters the TUI passes to the library.

use crate::{
    Cli, Command,
    tui::action::{Action, quote},
};
use clap::Parser;
use std::path::PathBuf;

fn parse(action: &Action) -> Command {
    let argv = std::iter::once("organizer".to_owned()).chain(action.argv());
    Cli::try_parse_from(argv)
        .unwrap_or_else(|e| panic!("{action:?}: {e}"))
        .command
}

#[test]
fn every_organizer_action_round_trips_through_the_cli_parser() {
    let root = PathBuf::from("/w/My Root");
    let plan = root.join(".organizer/plans/new.json");
    let tree = root.join(".organizer/drafts/new.tree");
    let r = || Some(root.clone());
    match parse(&Action::Init { root: root.clone() }) {
        Command::Init { root: got } => assert_eq!(got, root),
        _ => panic!(),
    }
    for accept in [false, true] {
        match parse(&Action::Sync {
            root: root.clone(),
            accept_unknown: accept,
        }) {
            Command::Sync {
                root: got,
                accept_unknown,
            } => assert_eq!((got, accept_unknown), (r(), accept)),
            _ => panic!(),
        }
    }
    // A prompt starting with "-" must stay a prompt.
    let chat = Action::Chat {
        root: root.clone(),
        prompt: "--classify 按主题'整理'".into(),
        scope_ids: vec!["d_a".into(), "d_b".into()],
        classify: true,
        read_content: true,
        out: tree.clone(),
    };
    match parse(&chat) {
        Command::Chat {
            prompt,
            root: got,
            scope_ids,
            classify,
            read_content,
            out,
        } => {
            assert_eq!(prompt.as_deref(), Some("--classify 按主题'整理'"));
            assert_eq!(
                (got, scope_ids, classify, read_content, out),
                (
                    r(),
                    vec!["d_a".into(), "d_b".into()],
                    true,
                    true,
                    Some(tree.clone())
                )
            );
        }
        _ => panic!(),
    }
    for ack in [false, true] {
        match parse(&Action::Plan {
            root: root.clone(),
            to: tree.clone(),
            acknowledge_review: ack,
            out: plan.clone(),
        }) {
            Command::Plan {
                root: got,
                structural_only,
                acknowledge_review,
                from,
                to,
                out,
            } => {
                assert_eq!(
                    (got, structural_only, acknowledge_review),
                    (r(), false, ack)
                );
                assert_eq!(
                    (from, to, out),
                    (
                        root.join(".organizer/structure.tree"),
                        tree.clone(),
                        Some(plan.clone())
                    )
                );
            }
            _ => panic!(),
        }
    }
    match parse(&Action::DryRun {
        root: root.clone(),
        plan: plan.clone(),
    }) {
        Command::Apply {
            plan: p,
            root: got,
            dry_run,
            yes,
        } => assert_eq!((p, got, dry_run, yes), (plan.clone(), r(), true, false)),
        _ => panic!(),
    }
    match parse(&Action::Apply {
        root: root.clone(),
        plan: plan.clone(),
    }) {
        Command::Apply {
            plan: p,
            root: got,
            dry_run,
            yes,
        } => assert_eq!((p, got, dry_run, yes), (plan.clone(), r(), false, true)),
        _ => panic!(),
    }
    let script = root.join(".organizer/plans/new.sh");
    match parse(&Action::ExportScript {
        plan: plan.clone(),
        out: script.clone(),
    }) {
        Command::ExportScript { plan: p, out } => assert_eq!((p, out), (plan.clone(), script)),
        _ => panic!(),
    }
    match parse(&Action::Archive {
        root: root.clone(),
        entries: vec!["drafts/new.tree".into()],
    }) {
        Command::Archive {
            root: got,
            entries,
            all,
        } => assert_eq!(
            (got, entries, all),
            (r(), vec!["drafts/new.tree".into()], false)
        ),
        _ => panic!(),
    }
    match parse(&Action::Archive {
        root: root.clone(),
        entries: vec![],
    }) {
        Command::Archive { entries, all, .. } => assert!(entries.is_empty() && all),
        _ => panic!(),
    }
    match parse(&Action::FinishGit {
        root: root.clone(),
        run_id: "run_x".into(),
    }) {
        Command::Recover {
            run_id,
            root: got,
            finish_git,
            mark_resolved,
        } => assert_eq!(
            (run_id.as_str(), got, finish_git, mark_resolved),
            ("run_x", r(), true, false)
        ),
        _ => panic!(),
    }
    match parse(&Action::MarkResolved {
        root: root.clone(),
        run_id: "run_x".into(),
    }) {
        Command::Recover {
            finish_git,
            mark_resolved,
            ..
        } => assert_eq!((finish_git, mark_resolved), (false, true)),
        _ => panic!(),
    }
}

#[test]
fn displayed_commands_survive_the_shell() {
    let value = "a b'c\"$HOME;`x`\\ 论文";
    let quoted = quote(value);
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("printf %s {quoted}"))
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(output.stdout).unwrap(), value);
    assert_eq!(quote("plain-path/x.tree"), "plain-path/x.tree");
    let copy = Action::CopyBaseline {
        root: "/w".into(),
        name: "new.tree".into(),
    };
    assert_eq!(
        copy.command(),
        "cp -n /w/.organizer/structure.tree /w/.organizer/drafts/new.tree"
    );
}
