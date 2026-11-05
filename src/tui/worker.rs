//! Background execution. One worker thread runs jobs serially; the UI thread
//! only draws and handles keys (design §9.2). Each library call takes and
//! releases the metadata lock itself, exactly like the CLI.

use super::{action::Action, load, state::Snapshot};
use organizer::{
    llm::{
        self, ChatOptions, ChatReport,
        provider::{LlmError, LlmProvider, LlmRequest},
    },
    migration::{self, ExecutionReport, MigrationPlan, ValidationReport, journal},
    sync::{self, SyncReport},
    workspace::{self, ArchiveReport},
};
use std::{fs, path::PathBuf, sync::mpsc};

pub enum Job {
    Refresh,
    Run(Action),
}

pub enum Event {
    Snapshot(Box<Snapshot>),
    Progress(usize, usize),
    Done(Box<(Action, Result<Outcome, String>)>),
    /// The job and its follow-up refresh are complete.
    Finished,
}

pub enum Outcome {
    Init(SyncReport),
    Sync(SyncReport),
    Chat(Box<ChatReport>),
    Plan {
        plan: Box<MigrationPlan>,
        written: bool,
    },
    DryRun(ValidationReport),
    Apply(ExecutionReport),
    Script(PathBuf),
    Archive(ArchiveReport),
    Recover(Box<journal::RecoveryReport>),
    Draft(PathBuf),
}

pub type ProviderFactory = dyn Fn() -> Result<Box<dyn LlmProvider>, String> + Send + Sync;

pub struct Context {
    pub root: PathBuf,
    pub provider: Box<ProviderFactory>,
}

/// Runs one job, emitting events in order: Done, then a refresh Snapshot for
/// writing actions, then Finished.
pub fn execute(ctx: &Context, job: Job, emit: &mut dyn FnMut(Event)) {
    match job {
        Job::Refresh => emit(Event::Snapshot(Box::new(load::snapshot(&ctx.root)))),
        Job::Run(action) => {
            let result = run(ctx, &action, emit);
            let writes = action.writes();
            emit(Event::Done(Box::new((action, result))));
            if writes {
                emit(Event::Snapshot(Box::new(load::snapshot(&ctx.root))));
            }
        }
    }
    emit(Event::Finished);
}

pub fn spawn(ctx: Context) -> (mpsc::Sender<Job>, mpsc::Receiver<Event>) {
    let (jobs, inbox) = mpsc::channel::<Job>();
    let (events, outbox) = mpsc::channel();
    std::thread::Builder::new()
        .name("organizer-worker".into())
        .spawn(move || {
            for job in inbox {
                execute(&ctx, job, &mut |event| {
                    let _ = events.send(event);
                });
            }
        })
        .expect("cannot start the worker thread");
    (jobs, outbox)
}

struct Shared<'a>(&'a dyn LlmProvider);

impl LlmProvider for Shared<'_> {
    fn complete_json(&self, request: &LlmRequest) -> Result<serde_json::Value, LlmError> {
        self.0.complete_json(request)
    }
}

fn text<E: std::fmt::Display>(error: E) -> String {
    error.to_string()
}

fn read_plan(path: &std::path::Path) -> Result<MigrationPlan, String> {
    let source =
        fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    serde_json::from_str(&source).map_err(text)
}

fn run(ctx: &Context, action: &Action, emit: &mut dyn FnMut(Event)) -> Result<Outcome, String> {
    match action {
        Action::Init { root } => sync::init(root).map(Outcome::Init).map_err(text),
        Action::Sync {
            root,
            accept_unknown,
        } => sync::sync(root, *accept_unknown)
            .map(Outcome::Sync)
            .map_err(text),
        Action::Chat {
            root,
            prompt,
            scope_ids,
            classify,
            read_content,
            out,
        } => {
            let provider = (ctx.provider)()?;
            let options = ChatOptions {
                prompt: prompt.clone(),
                scope_ids: scope_ids.clone(),
                classify: *classify,
                read_content: *read_content,
            };
            llm::chat_with_provider(root, &Shared(provider.as_ref()), &options, Some(out))
                .map(|report| Outcome::Chat(Box::new(report)))
                .map_err(text)
        }
        Action::Plan {
            root,
            to,
            acknowledge_review,
            out,
        } => {
            let from = root.join(".organizer/structure.tree");
            match crate::plan_documents(Some(root), &from, to, *acknowledge_review).map_err(text)? {
                crate::PlanOutcome::Migration(plan) => {
                    // Design §4.5: a plan with unresolved items is shown, not written.
                    let written = plan.unresolved.is_empty();
                    if written {
                        let json = serde_json::to_string_pretty(&plan).map_err(text)? + "\n";
                        crate::output(&json, Some(out)).map_err(text)?;
                    }
                    Ok(Outcome::Plan { plan, written })
                }
                crate::PlanOutcome::Diff(_) => Err("not a managed root".into()),
            }
        }
        Action::DryRun { root, plan } => migration::validate_plan(&read_plan(plan)?, root)
            .map(Outcome::DryRun)
            .map_err(text),
        Action::Apply { root, plan } => {
            let plan = read_plan(plan)?;
            migration::apply_plan_with_progress(&plan, root, |done, total| {
                emit(Event::Progress(done, total));
            })
            .map(Outcome::Apply)
            .map_err(text)
        }
        Action::ExportScript { plan, out } => {
            let script = migration::export_script(plan).map_err(text)?;
            crate::output(&script, Some(out)).map_err(text)?;
            Ok(Outcome::Script(out.clone()))
        }
        Action::Archive { root, entries } => workspace::archive(root, entries)
            .map(Outcome::Archive)
            .map_err(text),
        Action::FinishGit { root, run_id } => journal::recover(root, run_id, true)
            .map(|report| Outcome::Recover(Box::new(report)))
            .map_err(text),
        Action::MarkResolved { root, run_id } => journal::mark_resolved(root, run_id)
            .map(|report| Outcome::Recover(Box::new(report)))
            .map_err(text),
        Action::CopyBaseline { root, name } => workspace::copy_baseline_draft(root, name)
            .map(Outcome::Draft)
            .map_err(text),
    }
}
