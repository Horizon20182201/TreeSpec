//! Organizing rounds (design §3.2): drafts, review reports and plans paired by
//! content. Pure: `load.rs` reads the files and runs the library's review check.

use super::tree_index::TreeIndex;
use organizer::{
    llm::ChatReport,
    migration::{MigrationPlan, plan::digest},
    tree::{DiffReport, TreeSpec, compare_trees, render_tree},
};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stage {
    Invalid(String),
    Stale,
    NeedsPlan,
    NoChanges,
    Blocked,
    PlanStale,
    Executable,
    NothingToApply,
    Applied,
}

impl Stage {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Invalid(_) => "草案无效",
            Self::Stale => "草案过期",
            Self::NeedsPlan => "待计划",
            Self::NoChanges => "无变化",
            Self::Blocked => "计划不可执行",
            Self::PlanStale => "计划已过期",
            Self::Executable => "可执行",
            Self::NothingToApply => "无需执行",
            Self::Applied => "已执行",
        }
    }
    /// Finished or outdated rounds whose only remaining step is archiving.
    pub fn done(&self) -> bool {
        matches!(
            self,
            Self::Stale | Self::PlanStale | Self::NoChanges | Self::NothingToApply | Self::Applied
        )
    }
}

/// A draft as read by `load.rs`.
pub struct LoadedDraft {
    pub name: String,
    pub path: PathBuf,
    pub tree: Result<TreeSpec, String>,
    /// Has a review marker or a `NAME.review.json` report.
    pub generated: bool,
    /// `llm::check_review` without and with `--acknowledge-review`.
    pub strict: Result<(), String>,
    pub lenient: Result<(), String>,
    pub report: Option<ChatReport>,
}

pub struct LoadedPlan {
    pub name: String,
    pub path: PathBuf,
    pub plan: Result<MigrationPlan, String>,
}

#[derive(Default)]
pub struct Loaded {
    pub drafts: Vec<LoadedDraft>,
    pub plans: Vec<LoadedPlan>,
    /// Names in drafts/ ending in `.review.json`.
    pub reviews: Vec<String>,
    /// Names in plans/ ending in `.sh`.
    pub scripts: Vec<String>,
    /// (selector, note) for anything else.
    pub others: Vec<(String, String)>,
}

pub struct Draft {
    pub index: TreeIndex,
    pub digest: String,
    pub generated: bool,
    pub diff: Result<DiffReport, String>,
}

#[derive(Clone)]
pub struct PlanFile {
    pub name: String,
    pub path: PathBuf,
    pub plan: MigrationPlan,
    pub script: Option<String>,
    pub stage: Stage,
}

pub struct Round {
    pub name: String,
    pub path: PathBuf,
    pub draft: Result<Draft, String>,
    pub report: Option<ChatReport>,
    pub plans: Vec<PlanFile>,
    pub active: Option<usize>,
    pub stage: Stage,
    /// Why `--acknowledge-review` is required, if it is.
    pub needs_ack: Option<String>,
    /// Every file `z` archives with this round.
    pub selectors: Vec<String>,
}

impl Round {
    pub fn active_plan(&self) -> Option<&PlanFile> {
        self.active.map(|i| &self.plans[i])
    }

    /// Show “需确认审核” only while it still blocks the next step: a paired plan
    /// for this exact draft can only have been made with review confirmation.
    pub fn ack_badge(&self) -> Option<&str> {
        self.needs_ack
            .as_deref()
            .filter(|_| self.active.is_none() && !self.stage.done())
    }
}

#[derive(Default)]
pub struct Rounds {
    pub rounds: Vec<Round>,
    pub unpaired: Vec<PlanFile>,
    pub orphans: Vec<String>,
    pub others: Vec<(String, String)>,
}

impl Rounds {
    pub fn is_empty(&self) -> bool {
        self.rounds.is_empty()
            && self.unpaired.is_empty()
            && self.orphans.is_empty()
            && self.others.is_empty()
    }
}

pub fn plan_stage(plan: &MigrationPlan, current: &str) -> Stage {
    let (base, target) = (
        plan.base_tree_digest == current,
        plan.target_tree_digest == current,
    );
    match () {
        _ if !plan.unresolved.is_empty() => Stage::Blocked,
        _ if base && target => Stage::NothingToApply,
        _ if target => Stage::Applied,
        _ if base => Stage::Executable,
        _ => Stage::PlanStale,
    }
}

fn rank(stage: &Stage) -> u8 {
    match stage {
        Stage::Executable => 0,
        Stage::Applied => 1,
        Stage::NothingToApply => 2,
        Stage::Blocked => 3,
        _ => 4,
    }
}

fn ack_reason(report: Option<&ChatReport>, draft: &Draft) -> String {
    let mut reasons = vec![];
    if let Some(report) = report {
        if !report.unresolved.is_empty() {
            reasons.push(format!("有 {} 个待审核项", report.unresolved.len()));
        }
        if report.target_tree_digest != draft.digest {
            reasons.push("草案在生成后被编辑过".to_owned());
        }
    }
    if reasons.is_empty() {
        reasons.push("需要确认审核".to_owned());
    }
    reasons.join("；")
}

/// Pairs plans with drafts by `target_tree_digest` and computes stages.
pub fn build(baseline: &TreeSpec, baseline_text: &str, root: &Path, loaded: Loaded) -> Rounds {
    let current = digest(baseline_text);
    let Loaded {
        drafts,
        plans,
        reviews,
        scripts,
        others,
    } = loaded;
    let mut rounds = Rounds {
        others,
        ..Rounds::default()
    };
    let mut files = vec![];
    for loaded in plans {
        match loaded.plan {
            Ok(plan) => {
                let stem = loaded.name.strip_suffix(".json").unwrap_or(&loaded.name);
                let script = format!("{stem}.sh");
                files.push(PlanFile {
                    stage: plan_stage(&plan, &current),
                    script: scripts.contains(&script).then_some(script),
                    name: loaded.name,
                    path: loaded.path,
                    plan,
                });
            }
            Err(error) => rounds
                .others
                .push((format!("plans/{}", loaded.name), error)),
        }
    }
    for script in &scripts {
        if !files.iter().any(|f| f.script.as_ref() == Some(script)) {
            rounds
                .others
                .push((format!("plans/{script}"), "导出脚本，没有对应计划".into()));
        }
    }
    let mut taken = vec![false; files.len()];
    let mut used_reviews = vec![];
    for loaded in drafts {
        let LoadedDraft {
            name,
            path,
            tree,
            generated,
            strict,
            lenient,
            report,
        } = loaded;
        let draft = tree.and_then(|tree| {
            let text = render_tree(&tree).map_err(|e| e.to_string())?;
            Ok(Draft {
                digest: digest(&text),
                diff: compare_trees(baseline, &tree).map_err(|e| e.to_string()),
                index: TreeIndex::new(&tree),
                generated,
            })
        });
        let mut paired = vec![];
        if let Ok(draft) = &draft {
            for (i, file) in files.iter().enumerate() {
                if !taken[i]
                    && file.plan.target_tree_digest == draft.digest
                    && file.plan.root == root
                {
                    taken[i] = true;
                    paired.push(file.clone());
                }
            }
        }
        let active = paired
            .iter()
            .enumerate()
            .min_by_key(|(_, p)| (rank(&p.stage), std::cmp::Reverse(p.name.clone())))
            .map(|(i, _)| i);
        let active_stage = active.map(|i| paired[i].stage.clone());
        let (stage, needs_ack) = match (&draft, active_stage) {
            (Err(error), _) => (Stage::Invalid(error.clone()), None),
            // Applying moved the baseline, so the review check no longer applies.
            (Ok(_), Some(stage @ (Stage::Applied | Stage::NothingToApply))) => (stage, None),
            (Ok(d), plan_stage) => match &lenient {
                Err(error) => {
                    let stale = report
                        .as_ref()
                        .is_some_and(|r| r.base_tree_digest != current);
                    (
                        if stale {
                            Stage::Stale
                        } else {
                            Stage::Invalid(error.clone())
                        },
                        None,
                    )
                }
                Ok(()) => {
                    let ack = strict.is_err().then(|| ack_reason(report.as_ref(), d));
                    let stage = match plan_stage {
                        Some(stage) => stage,
                        None if d.digest == current => Stage::NoChanges,
                        None => Stage::NeedsPlan,
                    };
                    (stage, ack)
                }
            },
        };
        let mut selectors = vec![format!("drafts/{name}")];
        let review = format!("{name}.review.json");
        if reviews.contains(&review) {
            selectors.push(format!("drafts/{review}"));
            used_reviews.push(review);
        }
        for plan in &paired {
            selectors.push(format!("plans/{}", plan.name));
            if let Some(script) = &plan.script {
                selectors.push(format!("plans/{script}"));
            }
        }
        rounds.rounds.push(Round {
            name,
            path,
            draft,
            report,
            plans: paired,
            active,
            stage,
            needs_ack,
            selectors,
        });
    }
    rounds.orphans = reviews
        .into_iter()
        .filter(|r| !used_reviews.contains(r))
        .collect();
    rounds.unpaired = files
        .into_iter()
        .zip(taken)
        .filter(|(_, t)| !t)
        .map(|(f, _)| f)
        .collect();
    rounds
}

/// Next-step suggestion when the workspace is clean (design §2 bottom bar).
pub fn suggestion(rounds: &Rounds) -> String {
    let find = |f: &dyn Fn(&Round) -> bool| {
        let r = rounds.rounds.iter().find(|r| f(r))?;
        Some((
            super::text::middle(&super::text::clean(&r.name), 24),
            r.stage.label(),
        ))
    };
    if let Some((name, _)) = find(&|r| r.stage == Stage::Executable) {
        return format!("{name}：计划可执行。在整理页按 d 预览，A 执行");
    }
    if let Some((name, _)) = find(&|r| r.stage == Stage::NeedsPlan && r.needs_ack.is_some()) {
        return format!("{name}：先在审核视图检查，再按 p 确认审核并生成计划");
    }
    if let Some((name, _)) = find(&|r| r.stage == Stage::NeedsPlan) {
        return format!("{name}：检查目标结构后按 p 生成计划");
    }
    if let Some((name, stage)) = find(&|r| matches!(r.stage, Stage::Blocked | Stage::Invalid(_))) {
        return format!("{name}：{stage}，按 e 编辑草案或 z 归档");
    }
    if let Some((name, stage)) = find(&|r| r.stage.done()) {
        return format!("{name}：{stage}，按 z 归档本轮");
    }
    if !rounds.is_empty() {
        return "有未关联的计划或孤立文件，按 Z 全部归档".into();
    }
    "整理页按 n 新建草案（LLM 或复制当前结构）".into()
}
