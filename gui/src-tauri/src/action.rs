//! The `run` command's whitelist (design §3.4, §8.2 item 4). The WebView sends
//! names and IDs, never paths; each is checked against the session's last
//! snapshot and turned into the shared [`Action`] here.

use organizer::{
    frontend::{
        action::Action,
        rounds::{PlanFile, Stage},
        state::Snapshot,
    },
    workspace::check_new_name,
};
use serde::Deserialize;
use std::path::Path;
use ts_rs::TS;

#[derive(Clone, Debug, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum GuiAction {
    Init,
    Sync {
        accept_unknown: bool,
    },
    Chat {
        prompt: String,
        scope: Vec<String>,
        classify: bool,
        read_content: bool,
        out: String,
    },
    CopyBaseline {
        name: String,
    },
    Plan {
        round: String,
        acknowledge: bool,
        out: String,
    },
    DryRun {
        plan: String,
    },
    Apply {
        plan: String,
        token: String,
    },
    Export {
        plan: String,
    },
    /// Empty `selectors` archives everything pending.
    Archive {
        selectors: Vec<String>,
    },
    FinishGit {
        run: String,
    },
    MarkResolved {
        run: String,
    },
}

impl GuiAction {
    pub fn is_apply(&self) -> bool {
        matches!(self, Self::Apply { .. })
    }
}

/// A plan file in the snapshot and whether it is paired with a round.
pub fn find_plan<'a>(snapshot: &'a Snapshot, name: &str) -> Option<(&'a PlanFile, bool)> {
    let rounds = &snapshot.rounds;
    rounds
        .rounds
        .iter()
        .flat_map(|r| r.plans.iter())
        .find(|p| p.name == name)
        .map(|p| (p, true))
        .or_else(|| {
            rounds
                .unpaired
                .iter()
                .find(|p| p.name == name)
                .map(|p| (p, false))
        })
}

fn exists(path: &Path) -> bool {
    path.symlink_metadata().is_ok()
}

/// Every `drafts/…` or `plans/…` selector currently pending.
pub fn pending_selectors(snapshot: &Snapshot) -> Vec<String> {
    snapshot
        .detailed()
        .map(|s| s.pending.iter().map(|a| a.selector()).collect())
        .unwrap_or_default()
}

/// Validates `action` against `snapshot` and builds the shared action.
/// `executable` is the extra check for `Apply` and `DryRun`; the library
/// checks everything again.
pub fn resolve(root: &Path, snapshot: &Snapshot, action: GuiAction) -> Result<Action, String> {
    let organizer = root.join(".organizer");
    let root = root.to_owned();
    let new_name =
        |name: &str, suffix: &str| check_new_name(name, suffix).map_err(|e| e.to_string());
    let plan = |name: &str| find_plan(snapshot, name).ok_or_else(|| format!("没有计划 {name}"));
    let run = |id: &str| {
        snapshot
            .recoveries
            .iter()
            .any(|r| r.run.run_id == id)
            .then_some(id.to_owned())
            .ok_or_else(|| format!("没有未完成的运行 {id}"))
    };
    Ok(match action {
        GuiAction::Init => Action::Init { root },
        GuiAction::Sync { accept_unknown } => Action::Sync {
            root,
            accept_unknown,
        },
        GuiAction::Chat {
            prompt,
            scope,
            classify,
            read_content,
            out,
        } => {
            new_name(&out, ".tree")?;
            let draft = organizer.join("drafts").join(&out);
            if exists(&draft)
                || exists(&organizer.join("drafts").join(format!("{out}.review.json")))
            {
                return Err(format!("drafts/{out} 已存在，换一个文件名"));
            }
            if scope.len() > 512 {
                return Err("范围过大".into());
            }
            for id in &scope {
                if !snapshot.baseline.is_dir(id) || snapshot.excluded.contains(id) {
                    return Err(format!("{id} 不能选为范围"));
                }
            }
            Action::Chat {
                root,
                prompt,
                scope_ids: scope,
                classify,
                read_content,
                out: draft,
            }
        }
        GuiAction::CopyBaseline { name } => {
            new_name(&name, ".tree")?;
            Action::CopyBaseline { root, name }
        }
        GuiAction::Plan {
            round,
            acknowledge,
            out,
        } => {
            let round = snapshot
                .rounds
                .rounds
                .iter()
                .find(|r| r.name == round)
                .ok_or_else(|| format!("没有轮次 {round}"))?;
            new_name(&out, ".json")?;
            let out = organizer.join("plans").join(out);
            if exists(&out) {
                return Err("计划文件已存在，换一个文件名".into());
            }
            Action::Plan {
                root,
                to: round.path.clone(),
                acknowledge_review: acknowledge,
                out,
            }
        }
        GuiAction::DryRun { plan: ref name } | GuiAction::Apply { plan: ref name, .. } => {
            let (file, paired) = plan(name)?;
            if !paired || file.stage != Stage::Executable {
                return Err(format!("{name} 现在不可执行（{}）", file.stage.label()));
            }
            let plan = file.path.clone();
            match action {
                GuiAction::DryRun { .. } => Action::DryRun { root, plan },
                _ => Action::Apply { root, plan },
            }
        }
        GuiAction::Export { plan: name } => {
            let (file, _) = plan(&name)?;
            let script = format!("{}.sh", name.strip_suffix(".json").unwrap_or(&name));
            new_name(&script, ".sh")?;
            let out = organizer.join("plans").join(script);
            if exists(&out) {
                return Err("脚本已存在".into());
            }
            Action::ExportScript {
                plan: file.path.clone(),
                out,
            }
        }
        GuiAction::Archive { selectors } => {
            let pending = pending_selectors(snapshot);
            if let Some(bad) = selectors.iter().find(|s| !pending.contains(s)) {
                return Err(format!("{bad} 不是待处理文件"));
            }
            Action::Archive {
                root,
                entries: selectors,
            }
        }
        GuiAction::FinishGit { run: id } => Action::FinishGit {
            root,
            run_id: run(&id)?,
        },
        GuiAction::MarkResolved { run: id } => Action::MarkResolved {
            root,
            run_id: run(&id)?,
        },
    })
}
