//! The next-step suggestion shown on every page (TUI design §2), as data. Each
//! front end renders it in its own vocabulary: the TUI names keys, the GUI
//! shows a button.

use super::{
    rounds::{Round, Rounds, Stage},
    state::{Snapshot, Workspace},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NextStep {
    /// The state could not be read; show the error and rescan.
    Failed,
    Init,
    /// Interrupted metadata update: back up `.organizer`, inspect manually.
    Inspect,
    /// Unfinished migration: open the recovery page.
    Recover,
    /// Baseline conflict: save `structure.tree` as a draft, restore manually.
    SaveBaseline,
    /// Incomplete scan: fix permissions, then rescan.
    Rescan,
    /// Retry the pending Git record (`sync`); archive pending rounds first.
    RetryGit {
        archive_first: bool,
    },
    /// Record external changes (`sync`); archive pending rounds first.
    Sync {
        archive_first: bool,
    },
    Apply {
        round: String,
    },
    /// Plan, with `--acknowledge-review` required first.
    AcknowledgeAndPlan {
        round: String,
    },
    Plan {
        round: String,
    },
    /// Blocked plan or invalid draft: edit the draft or archive the round.
    Fix {
        round: String,
        stage: &'static str,
    },
    /// Finished or outdated round: archive it.
    Archive {
        round: String,
        stage: &'static str,
    },
    /// Only unpaired plans or stray files remain: archive everything.
    ArchiveAll,
    NewDraft,
}

/// First match wins, in the order of TUI design §3.1 and the round stages.
pub fn next_step(state: &Workspace, snapshot: Option<&Snapshot>) -> NextStep {
    let pending = snapshot.is_some_and(Snapshot::has_pending);
    match state {
        Workspace::Failed(_) => NextStep::Failed,
        Workspace::Uninitialized => NextStep::Init,
        Workspace::Interrupted => NextStep::Inspect,
        Workspace::Unfinished => NextStep::Recover,
        Workspace::Conflict => NextStep::SaveBaseline,
        Workspace::ScanIncomplete => NextStep::Rescan,
        Workspace::GitPending => NextStep::RetryGit {
            archive_first: pending,
        },
        Workspace::Changed => NextStep::Sync {
            archive_first: pending,
        },
        Workspace::Clean => snapshot.map_or(NextStep::NewDraft, |s| for_rounds(&s.rounds)),
    }
}

fn for_rounds(rounds: &Rounds) -> NextStep {
    let find = |f: &dyn Fn(&Round) -> bool| rounds.rounds.iter().find(|r| f(r));
    if let Some(r) = find(&|r| r.stage == Stage::Executable) {
        return NextStep::Apply {
            round: r.name.clone(),
        };
    }
    if let Some(r) = find(&|r| r.stage == Stage::NeedsPlan && r.needs_ack.is_some()) {
        return NextStep::AcknowledgeAndPlan {
            round: r.name.clone(),
        };
    }
    if let Some(r) = find(&|r| r.stage == Stage::NeedsPlan) {
        return NextStep::Plan {
            round: r.name.clone(),
        };
    }
    if let Some(r) = find(&|r| matches!(r.stage, Stage::Blocked | Stage::Invalid(_))) {
        return NextStep::Fix {
            round: r.name.clone(),
            stage: r.stage.label(),
        };
    }
    if let Some(r) = find(&|r| r.stage.done()) {
        return NextStep::Archive {
            round: r.name.clone(),
            stage: r.stage.label(),
        };
    }
    if !rounds.is_empty() {
        return NextStep::ArchiveAll;
    }
    NextStep::NewDraft
}
