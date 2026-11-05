//! View models sent to the WebView (design §3.3 item 3). Every string from
//! disk, the LLM or an error has passed through `frontend::text::clean`; state
//! names, stage names and “why unavailable” reasons come from the shared
//! front-end layer, so the GUI shows the same words as the TUI. The types are
//! exported to `gui/src/ipc/bindings/` by `cargo test` (ts-rs).

mod build;
mod outcome;

pub use build::{check_view, llm_view, round_detail, scope_view, view_model};
pub use outcome::{action_result, display_command};

use serde::Serialize;
use ts_rs::TS;

#[derive(Clone, Debug, Serialize, TS)]
#[ts(export)]
pub struct ViewModel {
    pub root: String,
    pub name: String,
    pub state: StateView,
    pub next: NextView,
    pub gates: Gates,
    pub llm: LlmView,
    /// Wall-clock time of this snapshot, milliseconds since the epoch.
    pub scanned_ms: f64,
    pub busy: Option<String>,
    pub error: Option<ErrorView>,
    pub init_warning: Option<String>,
    pub interrupted: Option<InterruptedView>,
    pub overview: Option<OverviewView>,
    /// Baseline nodes in display order (directories first, then by name).
    pub nodes: Vec<NodeView>,
    pub rounds: Vec<RoundView>,
    pub unpaired: Vec<PlanView>,
    /// Orphan review reports and other stray files: `(selector, note)`.
    pub others: Vec<OtherView>,
    pub commits: Vec<CommitView>,
    pub history_error: Option<String>,
    pub runs: Vec<RunView>,
    pub recoveries: Vec<RecoveryView>,
}

#[derive(Clone, Debug, Serialize, TS)]
pub struct StateView {
    /// `failed`, `uninit`, `interrupted`, `unfinished`, `conflict`,
    /// `scan_incomplete`, `git_pending`, `changed`, `clean`.
    pub code: String,
    pub label: String,
    /// `ok`, `warn`, `bad`, `mute`.
    pub tone: String,
}

#[derive(Clone, Debug, Serialize, TS)]
pub struct NextView {
    pub text: String,
    pub tone: String,
    /// Button label; the button only opens the matching sheet.
    pub button: Option<String>,
    /// What the button opens: `init`, `reveal_meta`, `recovery`, `save_baseline`,
    /// `refresh`, `sync`, `archive_all`, `apply`, `plan`, `edit`, `archive`, `new_draft`.
    pub intent: Option<String>,
    pub round: Option<String>,
}

#[derive(Clone, Debug, Serialize, TS)]
pub struct Gate {
    pub ok: bool,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, TS)]
pub struct Gates {
    pub draft: Gate,
    pub plan: Gate,
    pub sync: Gate,
    pub archive: Gate,
    pub copy_baseline: Gate,
}

#[derive(Clone, Debug, Serialize, TS)]
pub struct LlmView {
    pub available: bool,
    /// e.g. `gpt-x@api.example.com（远程）`, `未配置`, `已禁用`.
    pub label: String,
    pub host: Option<String>,
    pub remote: bool,
    pub no_content: bool,
    pub reason: Option<String>,
    /// Environment prefix for equivalent `chat` commands; the key is a placeholder.
    pub env_prefix: String,
}

#[derive(Clone, Debug, Serialize, TS)]
pub struct ErrorView {
    pub message: String,
    pub hint: Option<String>,
}

#[derive(Clone, Debug, Serialize, TS)]
pub struct InterruptedView {
    pub recovery_files: Vec<String>,
    pub pending_marker: Option<String>,
    pub sync_log_tail: String,
}

#[derive(Clone, Debug, Serialize, TS)]
pub struct ChangeView {
    pub path: String,
    pub old_path: Option<String>,
    pub note: Option<String>,
}

#[derive(Clone, Debug, Serialize, TS)]
pub struct IssueView {
    /// `P` protected nested Git project, `S` skipped, `E` excluded, `!` error.
    pub tag: String,
    pub path: String,
    pub note: String,
}

#[derive(Clone, Debug, Serialize, TS)]
pub struct OverviewView {
    pub added: Vec<ChangeView>,
    pub missing: Vec<ChangeView>,
    pub moved: Vec<ChangeView>,
    pub changed: Vec<ChangeView>,
    pub unknown: Vec<ChangeView>,
    pub conflicts: Vec<String>,
    pub boundaries: Vec<IssueView>,
    pub errors: Vec<IssueView>,
    pub node_count: u32,
    pub git_head: Option<String>,
}

#[derive(Clone, Debug, Serialize, TS)]
pub struct NodeView {
    pub id: String,
    pub parent: Option<String>,
    pub depth: u32,
    pub name: String,
    pub dir: bool,
    pub description: String,
    pub path: String,
    pub child_dirs: u32,
    pub child_files: u32,
    /// Why `chat` never offers this node, if it doesn't.
    pub excluded: Option<String>,
}

#[derive(Clone, Debug, Serialize, TS)]
pub struct PlanView {
    pub name: String,
    pub sha: String,
    pub stage: String,
    pub stage_code: String,
    pub ops: u32,
    pub summary: String,
    pub script: Option<String>,
}

#[derive(Clone, Debug, Serialize, TS)]
pub struct RoundView {
    pub name: String,
    pub generated: bool,
    pub stage: String,
    pub stage_code: String,
    /// “受阻：…” while the workspace is not clean and the round is not done.
    pub blocked: Option<String>,
    /// Why `--acknowledge-review` is required, while it still blocks planning.
    pub ack: Option<String>,
    pub done: bool,
    pub invalid: Option<String>,
    pub plans: Vec<PlanView>,
    /// Name of the paired plan the round uses.
    pub active: Option<String>,
    /// Everything archiving this round moves.
    pub selectors: Vec<String>,
}

#[derive(Clone, Debug, Serialize, TS)]
pub struct OtherView {
    pub selector: String,
    pub note: String,
}

#[derive(Clone, Debug, Serialize, TS)]
pub struct CommitView {
    pub hash: String,
    /// `init`, `reconcile`, `refactor` or `other`.
    pub kind: String,
    pub subject: String,
}

#[derive(Clone, Debug, Serialize, TS)]
pub struct RunView {
    pub run_id: String,
    pub plan_id: String,
    pub status: String,
    pub completed: u32,
    pub total: u32,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, TS)]
pub struct StepView {
    pub position: u32,
    pub status: String,
    pub op: OpView,
    /// Where the planned object is now (a hint, not an identity check).
    pub disk: String,
}

#[derive(Clone, Debug, Serialize, TS)]
pub struct OpView {
    /// `mkdir`, `move_file` or `move_directory`.
    pub kind: String,
    pub from: Option<String>,
    pub to: String,
}

#[derive(Clone, Debug, Serialize, TS)]
pub struct RecoveryView {
    pub run_id: String,
    pub status: String,
    pub completed: u32,
    pub total: u32,
    pub plan_id: String,
    pub error: Option<String>,
    pub hint: Option<String>,
    pub guidance: String,
    pub steps: Vec<StepView>,
    pub finish_git: Gate,
    pub mark_resolved: Gate,
    /// The report could not be read.
    pub report_error: Option<String>,
}
/// What the review, plan and report segments of one round show (design §6.4).
#[derive(Clone, Debug, Serialize, TS)]
#[ts(export)]
pub struct RoundDetail {
    pub name: String,
    pub generated: bool,
    pub stage: String,
    pub stage_code: String,
    pub ack: Option<String>,
    pub invalid: Option<String>,
    /// The annotated target tree in display order.
    pub rows: Vec<TreeRowView>,
    /// Old nodes the target no longer declares.
    pub missing: Vec<String>,
    /// `(symbol, count)` for the summary chips.
    pub counts: Vec<MarkCount>,
    pub report: Option<ReportView>,
    pub plan: Option<PlanDetail>,
}

#[derive(Clone, Debug, Serialize, TS)]
pub struct TreeRowView {
    pub id: String,
    pub depth: u32,
    pub name: String,
    pub dir: bool,
    pub description: String,
    pub path: String,
    /// `+ ~ > ^ # ? !` (TUI design §5.3).
    pub mark: Option<String>,
    pub note: Option<String>,
    pub excluded: bool,
    /// Old path, new path, LLM decision and reason (marked nodes only).
    pub detail: Vec<String>,
}

#[derive(Clone, Debug, Serialize, TS)]
pub struct MarkCount {
    pub mark: String,
    pub count: u32,
}

#[derive(Clone, Debug, Serialize, TS)]
pub struct ReportView {
    pub edit_count: u32,
    pub bytes_read: u32,
    pub base_digest: String,
    pub target_digest: String,
    pub assignments: Vec<AssignView>,
    pub unresolved: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, TS)]
pub struct AssignView {
    pub file: String,
    /// `place`, `keep` or `review`.
    pub decision: String,
    pub target: Option<String>,
    /// Model output, shown as untrusted text.
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, TS)]
pub struct PlanDetail {
    pub name: String,
    pub sha: String,
    pub stage: String,
    pub stage_code: String,
    pub script: Option<String>,
    pub ops: Vec<OpView>,
    pub unresolved: Vec<String>,
    pub boundaries: Vec<IssueView>,
    pub warnings: Vec<String>,
}

/// Live check of unsaved editor text (design §6.5).
#[derive(Clone, Debug, Serialize, TS)]
#[ts(export)]
pub struct DraftCheckView {
    pub ok: bool,
    pub line: Option<u32>,
    pub error: Option<String>,
    pub hint: Option<String>,
    pub added: u32,
    pub moved: u32,
    pub renamed: u32,
    pub described: u32,
    /// Old file nodes the text no longer declares.
    pub missing_files: Vec<String>,
    /// File nodes with no source.
    pub new_files: Vec<String>,
    /// Planning this text needs `--acknowledge-review`.
    pub needs_ack: bool,
    /// The review gate refuses this text even with acknowledgement.
    pub review_error: Option<String>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[ts(export)]
pub struct ScopeView {
    pub ok: bool,
    pub visible_nodes: u32,
    pub editable_files: u32,
    pub excluded: u32,
    pub error: Option<String>,
    /// Classification may exceed the 256-file limit.
    pub warn_files: bool,
}

#[derive(Clone, Debug, Serialize, TS)]
#[ts(export)]
pub struct DraftTextView {
    pub text: String,
    pub digest: String,
    pub generated: bool,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum OutcomeView {
    Synced {
        structure_changed: bool,
        git_head: Option<String>,
    },
    Chatted {
        draft: String,
        review_items: u32,
        first_review: Option<String>,
    },
    Planned {
        file: String,
        written: bool,
        ops: u32,
        unresolved: Vec<String>,
    },
    Previewed {
        plan: String,
        ops: u32,
        token: String,
    },
    Applied {
        run_id: String,
        ops: u32,
        git_head: Option<String>,
    },
    Script {
        file: String,
    },
    Archived {
        into: String,
        moved: Vec<String>,
    },
    Recovered {
        run_id: String,
        status: String,
    },
    Draft {
        name: String,
    },
    Saved {
        name: String,
        digest: String,
    },
}

#[derive(Clone, Debug, Serialize, TS)]
#[ts(export)]
pub struct ActionResult {
    pub label: String,
    /// Equivalent command, or “GUI 内部操作”.
    pub command: String,
    /// `ok`, `review` (exit code 2 in the CLI) or `failed`.
    pub kind: String,
    pub message: String,
    pub error: Option<String>,
    pub hint: Option<String>,
    /// A run started and failed: open the recovery page.
    pub go_recovery: bool,
    pub outcome: Option<OutcomeView>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[ts(export)]
pub struct RecentView {
    pub index: u32,
    pub name: String,
    pub path: String,
    pub state: String,
    pub opened_ms: f64,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum WindowInfo {
    Welcome {
        recent: Vec<RecentView>,
        tools: bool,
        parent: Option<String>,
        child: Option<String>,
    },
    Workspace {
        root: String,
        inspector: bool,
    },
}

#[derive(Clone, Debug, Serialize, TS)]
#[ts(export)]
pub struct SettingsView {
    pub preferences: crate::settings::Preferences,
    pub key_saved: bool,
    /// `ORGANIZER_LLM_*` restriction switches set in the process environment.
    pub env_locked: Vec<String>,
    /// Other `ORGANIZER_LLM_*` variables present; the settings take precedence.
    pub env_ignored: Vec<String>,
    pub tools: bool,
    pub diagnostics: String,
}

/// Why a close or quit needs confirmation (design §8.3, §10).
#[derive(Clone, Debug, Serialize, TS)]
#[ts(export)]
pub struct GuardView {
    /// `applying`, `chatting` or `unsaved`.
    pub guard: String,
    /// The request was “quit”, not “close this window”.
    pub quit: bool,
}

/// Requests for small system integrations, handled by the Tauri shell.
#[derive(Clone, Debug, serde::Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum SystemRequest {
    InstallTools,
    FilesAndFolders,
    Help,
    /// Choose the workspace folder again in the open panel (TCC, design §9.1).
    Reauthorize,
    Inspector {
        open: bool,
    },
    OpenWith {
        round: String,
    },
    PickEditor,
}

/// Events pushed to one window, in order: for a job `progress`* → `done` →
/// `view` (after writes) → `idle`.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum GuiEvent {
    View { view: Box<ViewModel> },
    Progress { done: u32, total: u32 },
    Done { result: Box<ActionResult> },
    Idle,
}
