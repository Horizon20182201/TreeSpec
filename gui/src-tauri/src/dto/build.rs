//! Snapshot → view model. Pure apart from reading the bounded `sync.log` tail.

use super::*;
use organizer::{
    frontend::{
        check::DraftCheck,
        llm_info::LlmInfo,
        next::{NextStep, next_step},
        recovery::disk,
        review,
        rounds::{PlanFile, Round, Stage},
        state::{Snapshot, Workspace},
        text::{clean, hint},
        tree_index::TreeIndex,
    },
    llm::{ScopePreview, file_mapper::Assignment},
    migration::{MigrationPlan, Operation},
    scan::ScanIssueKind,
    tree::{ChangeKind, NodeKind, UnresolvedKind},
};
use std::{collections::HashSet, io::Read};

const LOG_TAIL: u64 = 8 * 1024;

fn n(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn gate(ok: bool, reason: Option<&str>) -> Gate {
    Gate {
        ok,
        reason: (!ok).then(|| reason.unwrap_or("现在不可用").to_owned()),
    }
}

pub fn state_view(state: &Workspace) -> StateView {
    let (code, tone) = match state {
        Workspace::Failed(_) => ("failed", "bad"),
        Workspace::Uninitialized => ("uninit", "mute"),
        Workspace::Interrupted => ("interrupted", "bad"),
        Workspace::Unfinished => ("unfinished", "bad"),
        Workspace::Conflict => ("conflict", "bad"),
        Workspace::ScanIncomplete => ("scan_incomplete", "warn"),
        Workspace::GitPending => ("git_pending", "warn"),
        Workspace::Changed => ("changed", "warn"),
        Workspace::Clean => ("clean", "ok"),
    };
    StateView {
        code: code.into(),
        label: state.label().into(),
        tone: tone.into(),
    }
}

fn next_view(state: &Workspace, snapshot: &Snapshot, changes: usize) -> NextView {
    let v = |tone: &str,
             text: String,
             button: Option<&str>,
             intent: Option<&str>,
             round: Option<&String>| NextView {
        text,
        tone: tone.into(),
        button: button.map(Into::into),
        intent: intent.map(Into::into),
        round: round.map(|r| clean(r)),
    };
    match next_step(state, Some(snapshot)) {
        NextStep::Failed => v(
            "bad",
            "无法读取状态：查看概览中的错误，修正后重新扫描。".into(),
            Some("重新扫描"),
            Some("refresh"),
            None,
        ),
        NextStep::Init => v(
            "",
            "初始化这个文件夹：扫描目录并创建 .organizer/，不移动任何文件。".into(),
            Some("初始化…"),
            Some("init"),
            None,
        ),
        NextStep::Inspect => v(
            "bad",
            "元数据更新曾中断：先备份 .organizer，再人工检查。".into(),
            Some("在 Finder 中显示 .organizer"),
            Some("reveal_meta"),
            None,
        ),
        NextStep::Recover => v(
            "bad",
            "迁移未完成：在恢复页检查，其他页面的写操作已禁用。".into(),
            Some("前往恢复"),
            Some("recovery"),
            None,
        ),
        NextStep::SaveBaseline => v(
            "bad",
            "基线冲突：另存当前 structure.tree 为草案，再人工恢复基线。".into(),
            Some("另存为草案…"),
            Some("save_baseline"),
            None,
        ),
        NextStep::Rescan => v(
            "warn",
            "扫描不完整：修正出错路径的权限后重新扫描。".into(),
            Some("重新扫描"),
            Some("refresh"),
            None,
        ),
        NextStep::RetryGit {
            archive_first: true,
        } => v(
            "warn",
            "Git 待补记：先归档进行中的轮次，再同步重试。".into(),
            Some("全部归档…"),
            Some("archive_all"),
            None,
        ),
        NextStep::RetryGit { .. } => v(
            "warn",
            "Git 待补记：同步以重试 Git 记录。".into(),
            Some("同步…"),
            Some("sync"),
            None,
        ),
        NextStep::Sync {
            archive_first: true,
        } => v(
            "warn",
            format!("磁盘有 {changes} 项变化：先归档进行中的轮次，再同步。"),
            Some("全部归档…"),
            Some("archive_all"),
            None,
        ),
        NextStep::Sync { .. } => v(
            "warn",
            format!("磁盘有 {changes} 项变化：同步以记录这些变化，之后才能新建草案。"),
            Some("同步…"),
            Some("sync"),
            None,
        ),
        NextStep::Apply { round } => v(
            "",
            format!("{} 可执行：预览后执行迁移。", clean(&round)),
            Some("执行迁移…"),
            Some("apply"),
            Some(&round),
        ),
        NextStep::AcknowledgeAndPlan { round } => v(
            "",
            format!(
                "{} 待计划：先检查审核视图，再确认审核并生成计划。",
                clean(&round)
            ),
            Some("确认并生成计划…"),
            Some("plan"),
            Some(&round),
        ),
        NextStep::Plan { round } => v(
            "",
            format!("{} 待计划：检查目标结构后生成计划。", clean(&round)),
            Some("生成计划…"),
            Some("plan"),
            Some(&round),
        ),
        NextStep::Fix { round, stage } => v(
            "warn",
            format!("{}：{stage}。编辑草案，或归档这一轮。", clean(&round)),
            Some("编辑草案"),
            Some("edit"),
            Some(&round),
        ),
        NextStep::Archive { round, stage } => v(
            "",
            format!("{}：{stage}。归档用完的草案和计划。", clean(&round)),
            Some("归档本轮…"),
            Some("archive"),
            Some(&round),
        ),
        NextStep::ArchiveAll => v(
            "",
            "有未关联的计划或孤立文件：全部归档。".into(),
            Some("全部归档…"),
            Some("archive_all"),
            None,
        ),
        NextStep::NewDraft => v(
            "",
            "已同步：可以开始新一轮整理。".into(),
            Some("新建草案…"),
            Some("new_draft"),
            None,
        ),
    }
}

fn env_set(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| v == "1")
}

pub fn llm_view(info: &LlmInfo, base_url: &str, model: &str, key_saved: bool) -> LlmView {
    let quote = |s: &str| organizer::frontend::action::quote(s);
    let mut prefix = vec![];
    if !base_url.trim().is_empty() {
        prefix.push(format!("ORGANIZER_LLM_BASE_URL={}", quote(base_url.trim())));
    }
    if !model.trim().is_empty() {
        prefix.push(format!("ORGANIZER_LLM_MODEL={}", quote(model.trim())));
    }
    if key_saved {
        prefix.push("ORGANIZER_LLM_API_KEY=<钥匙串中的密钥>".into());
    }
    LlmView {
        available: info.available,
        label: info.label.clone(),
        host: info.host.as_deref().map(clean),
        remote: info.remote,
        no_content: info.no_content || env_set("ORGANIZER_LLM_NO_CONTENT"),
        reason: info
            .reason
            .as_deref()
            .map(|r| hint(r).map_or_else(|| clean(r), str::to_owned)),
        env_prefix: prefix.join(" "),
    }
}

fn change(c: &organizer::scan::identity::ObservedChange) -> ChangeView {
    ChangeView {
        path: clean(&c.path),
        old_path: c.old_path.as_deref().map(clean),
        note: None,
    }
}

fn issue(i: &organizer::scan::ScanIssue) -> IssueView {
    let (tag, note) = match i.kind {
        ScanIssueKind::Protected => ("P", "嵌套 Git 项目，受保护"),
        ScanIssueKind::Skipped => ("S", "符号链接或特殊文件，已跳过"),
        ScanIssueKind::Excluded => ("E", "元数据，已排除"),
        ScanIssueKind::Error => ("!", "扫描出错"),
    };
    let note = if i.kind == ScanIssueKind::Error {
        format!("{note}：{}", clean(&i.reason))
    } else {
        note.to_owned()
    };
    IssueView {
        tag: tag.into(),
        path: clean(&i.path),
        note,
    }
}

fn tail(path: &std::path::Path) -> String {
    let Ok(mut file) = std::fs::File::open(path) else {
        return String::new();
    };
    let len = file.metadata().map_or(0, |m| m.len());
    let _ = std::io::Seek::seek(
        &mut file,
        std::io::SeekFrom::Start(len.saturating_sub(LOG_TAIL)),
    );
    let mut bytes = vec![];
    let _ = file.take(LOG_TAIL).read_to_end(&mut bytes);
    clean(&String::from_utf8_lossy(&bytes)).replace("\\u{a}", "\n")
}

fn op_view(op: &Operation) -> OpView {
    match op {
        Operation::Mkdir { path, .. } => OpView {
            kind: "mkdir".into(),
            from: None,
            to: clean(path),
        },
        Operation::MoveFile { from, to, .. } => OpView {
            kind: "move_file".into(),
            from: Some(clean(from)),
            to: clean(to),
        },
        Operation::MoveDirectory { from, to, .. } => OpView {
            kind: "move_directory".into(),
            from: Some(clean(from)),
            to: clean(to),
        },
    }
}

fn summary(plan: &MigrationPlan) -> String {
    let mut kinds: Vec<(&str, usize)> = vec![];
    for op in &plan.operations {
        let kind = op_view(op).kind;
        let kind = match kind.as_str() {
            "mkdir" => "mkdir",
            "move_file" => "move_file",
            _ => "move_directory",
        };
        match kinds.iter_mut().find(|(k, _)| *k == kind) {
            Some((_, count)) => *count += 1,
            None => kinds.push((kind, 1)),
        }
    }
    kinds
        .iter()
        .map(|(k, c)| format!("{c} {k}"))
        .collect::<Vec<_>>()
        .join("，")
}

pub fn stage_code(stage: &Stage) -> &'static str {
    match stage {
        Stage::Invalid(_) => "invalid",
        Stage::Stale => "stale",
        Stage::NeedsPlan => "needs_plan",
        Stage::NoChanges => "no_changes",
        Stage::Blocked => "blocked",
        Stage::PlanStale => "plan_stale",
        Stage::Executable => "executable",
        Stage::NothingToApply => "nothing_to_apply",
        Stage::Applied => "applied",
    }
}

fn short(digest: &str) -> String {
    let hex = digest.strip_prefix("sha256:").unwrap_or(digest);
    format!("sha256:{}", &hex[..hex.len().min(8)])
}

fn plan_view(p: &PlanFile) -> PlanView {
    PlanView {
        name: clean(&p.name),
        sha: short(&p.plan.plan_id),
        stage: p.stage.label().into(),
        stage_code: stage_code(&p.stage).into(),
        ops: n(p.plan.operations.len()),
        summary: summary(&p.plan),
        script: p.script.as_deref().map(clean),
    }
}
fn round_view(r: &Round, blocked: Option<&str>) -> RoundView {
    RoundView {
        name: clean(&r.name),
        generated: r.draft.as_ref().is_ok_and(|d| d.generated),
        stage: r.stage.label().into(),
        stage_code: stage_code(&r.stage).into(),
        blocked: blocked.filter(|_| !r.stage.done()).map(Into::into),
        ack: r.ack_badge().map(clean),
        done: r.stage.done(),
        invalid: match &r.stage {
            Stage::Invalid(e) => Some(clean(e)),
            _ => None,
        },
        plans: r.plans.iter().map(plan_view).collect(),
        active: r.active_plan().map(|p| clean(&p.name)),
        selectors: r.selectors.iter().map(|s| clean(s)).collect(),
    }
}

fn excluded_note(id: &str, path: &str, snapshot: &Snapshot) -> Option<String> {
    if !snapshot.excluded.contains(id) {
        return None;
    }
    let protected = snapshot.detailed().is_some_and(|s| {
        s.report
            .warnings
            .iter()
            .any(|w| w.kind == ScanIssueKind::Protected && w.path == path)
    });
    Some(if protected {
        "受保护的嵌套 Git 项目：不发送给 LLM，不能选为范围".into()
    } else {
        "敏感名称或受保护项：不发送给 LLM，不能选为范围".into()
    })
}

fn nodes(index: &TreeIndex, snapshot: &Snapshot) -> Vec<NodeView> {
    index
        .rows(&HashSet::new(), None)
        .into_iter()
        .filter_map(|(id, depth)| {
            let node = index.nodes.get(&id)?;
            let path = index.paths.get(&id).cloned().unwrap_or_default();
            let kids = index.kids(Some(&id));
            let dirs = kids.iter().filter(|k| index.is_dir(k)).count();
            Some(NodeView {
                excluded: excluded_note(&id, &path, snapshot),
                parent: node.parent_id.clone(),
                depth: n(depth),
                name: clean(&node.name),
                dir: node.kind == NodeKind::Directory,
                description: node.description.as_deref().map(clean).unwrap_or_default(),
                path: clean(&path),
                child_dirs: n(dirs),
                child_files: n(kids.len() - dirs),
                id,
            })
        })
        .collect()
}

fn recovery(entry: &organizer::frontend::state::RecoveryEntry) -> RecoveryView {
    let run = &entry.run;
    let mut view = RecoveryView {
        run_id: clean(&run.run_id),
        status: clean(&run.status),
        completed: n(run.completed_steps),
        total: n(run.total_steps),
        plan_id: short(&run.plan_id),
        error: run.error.as_deref().map(clean),
        hint: run.error.as_deref().and_then(hint).map(Into::into),
        guidance: String::new(),
        steps: vec![],
        finish_git: gate(false, Some("无法检查该运行")),
        mark_resolved: gate(false, Some("无法检查该运行")),
        report_error: None,
    };
    match &entry.report {
        Ok(report) => {
            let all_done = report.completed_steps == report.steps.len();
            view.guidance = clean(&report.guidance);
            view.steps = report
                .steps
                .iter()
                .map(|s| StepView {
                    position: n(s.position),
                    status: clean(&s.status),
                    op: op_view(&s.operation),
                    disk: disk(report, &s.operation).into(),
                })
                .collect();
            view.finish_git = gate(
                !report.metadata_update_pending && all_done,
                Some("只有全部步骤已确认、且没有发布标记时才能补记 Git"),
            );
            view.mark_resolved = if report.status == "git_pending" {
                gate(false, Some("该运行只差 Git 记录：用“补记 Git”"))
            } else {
                gate(
                    !report.metadata_update_pending,
                    Some("元数据发布曾中断：先备份 .organizer 再人工检查"),
                )
            };
        }
        Err(e) => view.report_error = Some(clean(e)),
    }
    view
}

/// `llm` is the LLM summary for the current settings.
pub fn view_model(
    root: &std::path::Path,
    snapshot: &Snapshot,
    llm: LlmView,
    busy: Option<String>,
    scanned_ms: f64,
) -> ViewModel {
    let state = snapshot.classify();
    let blocked = state.blocked_reason();
    let detailed = snapshot.detailed();
    let changes = detailed.map_or(0, |s| {
        let r = &s.report;
        r.added.len() + r.missing.len() + r.moved.len() + r.changed.len() + r.unknown.len()
    });
    let pending = snapshot.has_pending();
    let probe = snapshot.probe.as_ref().ok();
    let root = probe.map_or_else(|| root.to_owned(), |p| p.root.clone());
    let error = match &state {
        Workspace::Failed(e) => Some(ErrorView {
            message: clean(e),
            hint: hint(e).map(Into::into),
        }),
        _ => None,
    };
    let interrupted = (state == Workspace::Interrupted).then(|| InterruptedView {
        recovery_files: probe
            .map(|p| p.recovery_files.iter().map(|f| clean(f)).collect())
            .unwrap_or_default(),
        pending_marker: probe.and_then(|p| p.pending_marker.as_deref()).map(clean),
        sync_log_tail: tail(&root.join(".organizer/sync.log")),
    });
    let overview = detailed.map(|s| {
        let r = &s.report;
        OverviewView {
            added: r.added.iter().map(change).collect(),
            missing: r.missing.iter().map(change).collect(),
            moved: r.moved.iter().map(change).collect(),
            changed: r.changed.iter().map(change).collect(),
            unknown: r
                .unknown
                .iter()
                .map(|u| ChangeView {
                    path: clean(&u.path),
                    old_path: None,
                    note: Some(format!(
                        "候选 {}：{}",
                        clean(&u.candidate_ids.join(", ")),
                        clean(&u.reason)
                    )),
                })
                .collect(),
            conflicts: s
                .baseline_conflicts
                .iter()
                .chain(r.conflicts.iter())
                .map(|c| clean(c))
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect(),
            boundaries: r
                .warnings
                .iter()
                .filter(|w| w.kind != ScanIssueKind::Error)
                .take(200)
                .map(issue)
                .collect(),
            errors: r
                .warnings
                .iter()
                .filter(|w| w.kind == ScanIssueKind::Error)
                .take(200)
                .map(issue)
                .collect(),
            node_count: n(s.saved.tree.nodes.len()),
            git_head: s
                .saved
                .git_head
                .as_deref()
                .map(|h| clean(&h[..h.len().min(12)])),
        }
    });
    let (commits, history_error) = match &snapshot.history {
        Ok(lines) => (
            lines
                .iter()
                .map(|(hash, subject)| CommitView {
                    hash: clean(hash),
                    kind: ["init", "reconcile", "refactor"]
                        .into_iter()
                        .find(|k| subject.starts_with(&format!("{k}:")))
                        .unwrap_or("other")
                        .into(),
                    subject: clean(subject),
                })
                .collect(),
            None,
        ),
        Err(e) => (vec![], Some(clean(e))),
    };
    let rounds = &snapshot.rounds;
    let mut others: Vec<OtherView> = rounds
        .orphans
        .iter()
        .map(|o| OtherView {
            selector: clean(&format!("drafts/{o}")),
            note: "孤立报告".into(),
        })
        .collect();
    others.extend(rounds.others.iter().map(|(s, note)| OtherView {
        selector: clean(s),
        note: clean(note),
    }));
    ViewModel {
        name: root.file_name().map_or_else(
            || root.display().to_string(),
            |n| clean(&n.to_string_lossy()),
        ),
        root: clean(&root.display().to_string()),
        next: next_view(&state, snapshot, changes),
        gates: Gates {
            draft: gate(state.can_draft(), blocked),
            plan: gate(state.can_plan(), blocked),
            sync: gate(
                state.can_sync(pending),
                Some(if pending {
                    "先归档进行中的整理轮次"
                } else {
                    blocked.unwrap_or("现在不能同步")
                }),
            ),
            archive: gate(
                state.can_archive() && !rounds.is_empty(),
                Some(if rounds.is_empty() {
                    "没有待归档的文件"
                } else {
                    "现在不能归档"
                }),
            ),
            copy_baseline: gate(state.can_copy_baseline(), blocked),
        },
        state: state_view(&state),
        llm,
        scanned_ms,
        busy,
        init_warning: (state == Workspace::Uninitialized)
            .then(|| crate::open::init_warning(&root))
            .flatten(),
        error,
        interrupted,
        overview,
        nodes: nodes(&snapshot.baseline, snapshot),
        rounds: rounds
            .rounds
            .iter()
            .map(|r| round_view(r, blocked))
            .collect(),
        unpaired: rounds.unpaired.iter().map(plan_view).collect(),
        others,
        commits,
        history_error,
        runs: probe
            .map(|p| {
                p.runs
                    .iter()
                    .rev()
                    .map(|r| RunView {
                        run_id: clean(&r.run_id),
                        plan_id: short(&r.plan_id),
                        status: clean(&r.status),
                        completed: n(r.completed_steps),
                        total: n(r.total_steps),
                        error: r.error.as_deref().map(clean),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        recoveries: snapshot.recoveries.iter().map(recovery).collect(),
    }
}
fn assignment(a: &Assignment, draft: &TreeIndex, base: &TreeIndex) -> AssignView {
    let path = |id: &str| {
        draft
            .paths
            .get(id)
            .or_else(|| base.paths.get(id))
            .map_or_else(|| clean(id), |p| clean(p))
    };
    let (file, decision, target, reason) = match a {
        Assignment::Place {
            file_id,
            target_dir_id,
            reason,
        } => (file_id, "place", Some(path(target_dir_id)), reason),
        Assignment::Keep { file_id, reason } => (file_id, "keep", None, reason),
        Assignment::Review { file_id, reason } => (file_id, "review", None, reason),
    };
    AssignView {
        file: path(file),
        decision: decision.into(),
        target,
        reason: clean(reason),
    }
}

/// The annotated target tree, plan and report of one round.
pub fn round_detail(snapshot: &Snapshot, name: &str) -> Option<RoundDetail> {
    let round = snapshot.rounds.rounds.iter().find(|r| r.name == name)?;
    let mut detail = RoundDetail {
        name: clean(&round.name),
        generated: round.draft.as_ref().is_ok_and(|d| d.generated),
        stage: round.stage.label().into(),
        stage_code: stage_code(&round.stage).into(),
        ack: round.ack_badge().map(clean),
        invalid: None,
        rows: vec![],
        missing: vec![],
        counts: vec![],
        report: None,
        plan: None,
    };
    match &round.draft {
        Err(e) => detail.invalid = Some(clean(e)),
        Ok(draft) => {
            if let Err(e) = &draft.diff {
                detail.invalid = Some(clean(e));
            }
            let marks = review::marks(round, draft);
            for (id, depth) in draft.index.rows(&HashSet::new(), None) {
                let Some(node) = draft.index.nodes.get(&id) else {
                    continue;
                };
                let mark = marks.get(&id);
                detail.rows.push(TreeRowView {
                    depth: n(depth),
                    name: clean(&node.name),
                    dir: node.kind == NodeKind::Directory,
                    description: node.description.as_deref().map(clean).unwrap_or_default(),
                    path: clean(draft.index.paths.get(&id).map_or("", String::as_str)),
                    mark: mark.map(|m| m.symbol.to_string()),
                    note: mark.map(|m| m.text.clone()),
                    excluded: snapshot.excluded.contains(&id),
                    detail: if mark.is_some() {
                        review::detail(round, draft, &id)
                    } else {
                        vec![]
                    },
                    id,
                });
            }
            detail.missing = review::missing(draft);
            let mut counts: Vec<MarkCount> = vec![];
            for m in marks.values() {
                let mark = m.symbol.to_string();
                match counts.iter_mut().find(|c| c.mark == mark) {
                    Some(c) => c.count += 1,
                    None => counts.push(MarkCount { mark, count: 1 }),
                }
            }
            if !detail.missing.is_empty() {
                counts.push(MarkCount {
                    mark: "!".into(),
                    count: n(detail.missing.len()),
                });
            }
            counts.sort_by_key(|c| "+~>^#?!".find(&c.mark).unwrap_or(9));
            detail.counts = counts;
            detail.report = round.report.as_ref().map(|r| ReportView {
                edit_count: n(r.edit_count),
                bytes_read: n(r.context_bytes_read),
                base_digest: short(&r.base_tree_digest),
                target_digest: short(&r.target_tree_digest),
                assignments: r
                    .assignments
                    .iter()
                    .map(|a| assignment(a, &draft.index, &snapshot.baseline))
                    .collect(),
                unresolved: r.unresolved.iter().map(|u| clean(&u.reason)).collect(),
                warnings: r.warnings.iter().map(|w| clean(w)).collect(),
            });
        }
    }
    detail.plan = round.active_plan().map(|p| PlanDetail {
        name: clean(&p.name),
        sha: short(&p.plan.plan_id),
        stage: p.stage.label().into(),
        stage_code: stage_code(&p.stage).into(),
        script: p.script.as_deref().map(clean),
        ops: p.plan.operations.iter().map(op_view).collect(),
        unresolved: p.plan.unresolved.iter().map(|u| clean(u)).collect(),
        boundaries: p.plan.boundaries.iter().map(issue).collect(),
        warnings: p.plan.warnings.iter().map(|w| clean(w)).collect(),
    });
    Some(detail)
}

pub fn check_view(check: &DraftCheck) -> DraftCheckView {
    let mut view = DraftCheckView {
        ok: check.error.is_none(),
        line: check.error.as_ref().and_then(|(l, _)| l.map(n)),
        error: check.error.as_ref().map(|(_, e)| clean(e)),
        hint: check
            .error
            .as_ref()
            .and_then(|(_, e)| hint(e))
            .map(Into::into),
        added: 0,
        moved: 0,
        renamed: 0,
        described: 0,
        missing_files: vec![],
        new_files: vec![],
        needs_ack: check.error.is_none() && check.lenient.is_ok() && check.strict.is_err(),
        review_error: if check.error.is_none() {
            check.lenient.as_ref().err().map(|e| clean(e))
        } else {
            None
        },
    };
    if let Some(diff) = &check.diff {
        for c in &diff.changes {
            match c.change {
                ChangeKind::AddNode => view.added += 1,
                ChangeKind::ReparentNode => view.moved += 1,
                ChangeKind::RenameNode => view.renamed += 1,
                ChangeKind::UpdateDescription => view.described += 1,
                ChangeKind::RemoveNode => {}
            }
        }
        for u in &diff.unresolved {
            match u.reason {
                UnresolvedKind::UnresolvedMissingFile => view.missing_files.push(clean(&u.path)),
                UnresolvedKind::UnresolvedNewFile => view.new_files.push(clean(&u.path)),
            }
        }
    }
    view
}

pub fn scope_view(preview: &organizer::error::Result<ScopePreview>) -> ScopeView {
    match preview {
        Ok(p) => ScopeView {
            ok: p.visible_nodes <= organizer::llm::MAX_SCOPE_NODES,
            visible_nodes: n(p.visible_nodes),
            editable_files: n(p.editable_files),
            excluded: n(p.excluded_ids.len()),
            error: (p.visible_nodes > organizer::llm::MAX_SCOPE_NODES)
                .then(|| "范围超过 512 个节点：选择更小的目录".into()),
            warn_files: p.editable_files > organizer::llm::MAX_CLASSIFY_FILES,
        },
        Err(e) => {
            let e = e.to_string();
            ScopeView {
                ok: false,
                visible_nodes: 0,
                editable_files: 0,
                excluded: 0,
                error: Some(hint(&e).map_or_else(|| clean(&e), str::to_owned)),
                warn_files: false,
            }
        }
    }
}
