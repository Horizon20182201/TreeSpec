//! LLMs propose bounded, scoped data. Only migration::apply_plan moves files.
mod context;
pub mod file_mapper;
pub mod provider;
pub mod tree_editor;

use crate::{
    error::{Result, fail},
    metadata,
    migration::{
        plan::{PlanningContext, build_plan, digest},
        validate,
    },
    tree::{NodeKind, TreeSpec, render_tree},
};
use context::Scope;
use provider::{LlmPolicy, LlmProvider};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

pub const MAX_PROMPT_BYTES: usize = 8192;
const REVIEW_MARKER: &str = "# organizer-review-v1: ";

/// What a `chat` call with these `--scope` IDs would offer the provider,
/// computed from the indexed baseline without scanning or network access.
#[derive(Clone, Debug, Serialize)]
pub struct ScopePreview {
    /// Context nodes including read-only ancestors (limit [`MAX_SCOPE_NODES`]).
    pub visible_nodes: usize,
    /// Existing files inside the editable scope; FileMapper accepts at most
    /// [`MAX_CLASSIFY_FILES`] after the model narrows the scope.
    pub editable_files: usize,
    /// Protected or sensitive nodes never sent to the provider.
    pub excluded_ids: Vec<String>,
}

pub const MAX_SCOPE_NODES: usize = context::MAX_NODES;
pub const MAX_CLASSIFY_FILES: usize = context::MAX_FILES;

/// Same scope validation as `chat` (unknown, sensitive or protected scope IDs
/// and the node limit are errors), but read-only and offline.
pub fn preview_scope(
    saved: &crate::storage::SavedState,
    scope_ids: &[String],
) -> Result<ScopePreview> {
    let scope = Scope::new(saved, &[], scope_ids)?;
    let editable_files = scope
        .nodes
        .iter()
        .filter(|node| node.editable && node.kind == NodeKind::File)
        .count();
    let mut excluded_ids: Vec<_> = scope.excluded.into_iter().collect();
    excluded_ids.sort();
    Ok(ScopePreview {
        visible_nodes: scope.nodes.len(),
        editable_files,
        excluded_ids,
    })
}

/// IDs of nodes `chat` never offers to the provider or accepts as `--scope`.
pub fn excluded_ids(saved: &crate::storage::SavedState) -> Vec<String> {
    let mut ids: Vec<_> = context::excluded_ids(saved).into_iter().collect();
    ids.sort();
    ids
}

pub fn has_review_marker(source: &str) -> bool {
    source.lines().any(|line| line.starts_with(REVIEW_MARKER))
}

/// The review-marker lines of a draft, in order.
pub(crate) fn review_marker_lines(source: &str) -> Vec<&str> {
    source
        .lines()
        .filter(|line| line.starts_with(REVIEW_MARKER))
        .collect()
}

#[derive(Clone, Debug, Default)]
pub struct ChatOptions {
    pub prompt: String,
    pub scope_ids: Vec<String>,
    pub classify: bool,
    pub read_content: bool,
    /// Front-end restrictions, combined with the environment switches.
    pub policy: LlmPolicy,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewItem {
    pub node_id: Option<String>,
    pub reason: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatReport {
    pub schema_version: u32,
    pub review_id: String,
    pub root: PathBuf,
    pub draft: PathBuf,
    pub review: PathBuf,
    pub base_tree_digest: String,
    pub target_tree_digest: String,
    pub edit_count: usize,
    pub context_bytes_read: usize,
    pub assignments: Vec<file_mapper::Assignment>,
    pub unresolved: Vec<ReviewItem>,
    pub warnings: Vec<String>,
}

pub fn chat_with_provider(
    root: &Path,
    provider: &impl LlmProvider,
    options: &ChatOptions,
    out: Option<&Path>,
) -> Result<ChatReport> {
    let policy = options.policy.effective()?;
    if policy.disabled {
        return Err(fail("LLM is disabled; use manual Tree drafts"));
    }
    if options.read_content && policy.no_content {
        return Err(fail(
            "ORGANIZER_LLM_NO_CONTENT=1 or the no-content setting prohibits --read-content",
        ));
    }
    if options.prompt.trim().is_empty() || options.prompt.len() > MAX_PROMPT_BYTES {
        return Err(fail(
            "chat requires a nonempty request of at most 8192 bytes",
        ));
    }
    let (lock, saved, snapshot) = validate::context(root, false)?;
    let mut scope = Scope::new(&saved, &snapshot.warnings, &options.scope_ids)?;
    let review_id = uuid::Uuid::new_v4().to_string();
    let (draft_path, review_path) = output_paths(&saved.root, out, &review_id)?;
    drop(lock); // Never hold Organizer's metadata lock across HTTP calls.
    let edited = tree_editor::edit(
        provider,
        &saved.tree,
        &mut scope,
        &options.prompt,
        options.classify,
    )?;
    let mapped = if edited.intent.kind == tree_editor::IntentKind::RedistributeExistingFiles {
        file_mapper::map(
            provider,
            &saved,
            &edited.tree,
            &scope,
            &edited.intent,
            &options.prompt,
            options.read_content,
        )?
    } else {
        file_mapper::Mapped {
            tree: edited.tree,
            assignments: vec![],
            unresolved: vec![],
            context_bytes: 0,
        }
    };
    preserve_files(&saved.tree, &mapped.tree)?;
    let plan = build_plan(&PlanningContext {
        saved: &saved,
        snapshot: &snapshot,
        target: &mapped.tree,
    })?;
    let mut unresolved = mapped.unresolved;
    unresolved.extend(plan.unresolved.into_iter().map(|reason| ReviewItem {
        node_id: None,
        reason,
    }));
    let target_text = render_tree(&mapped.tree)?;
    let mut warnings = vec!["Draft only. Review the complete Tree and report, then plan and apply explicitly. No managed files were moved or committed.".into()];
    if scope.excluded_count > 0 {
        warnings.push(format!(
            "{} sensitive/protected nodes excluded from LLM candidates and retained locally",
            scope.excluded_count
        ));
    }
    if options.read_content {
        warnings.push("Only bounded UTF-8 excerpts from supported text files were eligible; PDF/binary/unreadable-as-text/private-key material falls back to filename context. Content may have been sent to the configured provider.".into());
    }
    let report = ChatReport {
        schema_version: 1,
        review_id,
        root: saved.root.clone(),
        draft: draft_path,
        review: review_path,
        base_tree_digest: digest(&saved.tree_text),
        target_tree_digest: digest(&target_text),
        edit_count: edited.edit_count,
        context_bytes_read: mapped.context_bytes,
        assignments: mapped.assignments,
        unresolved,
        warnings,
    };
    let (_lock, current, current_scan) = validate::context(&saved.root, true)?;
    if current.generation != saved.generation
        || current.tree_text != saved.tree_text
        || current.git_head != saved.git_head
        || current_scan.entries != snapshot.entries
        || current_scan.warnings != snapshot.warnings
    {
        return Err(fail(
            "baseline or disk changed while waiting for LLM; no draft published, sync and retry",
        ));
    }
    let report_text = serde_json::to_string_pretty(&report)
        .map_err(|_| fail("cannot serialize review report"))?;
    if report_text.len() >= provider::MAX_RESPONSE_BYTES {
        return Err(fail(
            "draft review report exceeds size limit; narrow --scope",
        ));
    }
    // Publish report first. A failure can leave an orphan report, but never a
    // generated Tree that silently loses its review requirements.
    metadata::write_snapshot(&report.review, None, &(report_text + "\n"))?;
    metadata::write_snapshot(
        &report.draft,
        None,
        &format!("{REVIEW_MARKER}{}\n{target_text}", report.review_id),
    )?;
    Ok(report)
}

fn preserve_files(old: &TreeSpec, new: &TreeSpec) -> Result<()> {
    let files = |tree: &TreeSpec| -> HashSet<String> {
        tree.nodes
            .iter()
            .filter(|n| n.kind == NodeKind::File)
            .map(|n| n.id.clone())
            .collect()
    };
    if files(old) != files(new) {
        return Err(fail(
            "LLM draft must preserve every existing file ID exactly once",
        ));
    }
    context::validate_target(new)
}

pub fn review_path(draft: &Path) -> Result<PathBuf> {
    let name = draft
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| fail("draft path must have a UTF-8 filename"))?;
    Ok(draft.with_file_name(format!("{name}.review.json")))
}

fn output_paths(root: &Path, out: Option<&Path>, id: &str) -> Result<(PathBuf, PathBuf)> {
    let path = out
        .map(Path::to_owned)
        .unwrap_or_else(|| root.join(format!(".organizer/drafts/chat_{id}.tree")));
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = metadata::canonical_root(parent)?;
    if parent.starts_with(root) && !parent.starts_with(root.join(".organizer/drafts")) {
        return Err(fail(
            "chat output inside a managed root must be under .organizer/drafts; alternatively choose an external directory",
        ));
    }
    let name = path
        .file_name()
        .ok_or_else(|| fail("missing draft filename"))?;
    let draft = parent.join(name);
    let review = review_path(&draft)?;
    if metadata::exists(&draft)? || metadata::exists(&review)? {
        return Err(fail(
            "chat draft and review output must both be new; existing files are never overwritten",
        ));
    }
    Ok((draft, review))
}

/// CLI artifact-level review gate. The core planner still independently checks
/// the target AST and filesystem; neither the marker nor report grants execution.
pub fn check_review(
    root: Option<&Path>,
    draft_path: &Path,
    source: &str,
    old: &TreeSpec,
    target: &TreeSpec,
    acknowledged: bool,
) -> Result<()> {
    let markers: Vec<_> = source
        .lines()
        .filter_map(|line| line.strip_prefix(REVIEW_MARKER))
        .collect();
    let sidecar = review_path(draft_path)?;
    let sidecar_exists = metadata::exists(&sidecar)?;
    if markers.is_empty() && !sidecar_exists {
        return Ok(());
    }
    if markers.len() != 1 || !sidecar_exists {
        return Err(fail(
            "LLM draft review marker/report is missing or duplicated; retain the generated pair",
        ));
    }
    metadata::plain_file(&sidecar)?;
    use std::io::Read;
    let mut bytes = vec![];
    fs::File::open(&sidecar)?
        .take(provider::MAX_RESPONSE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > provider::MAX_RESPONSE_BYTES {
        return Err(fail("draft review report exceeds size limit"));
    }
    let report: ChatReport =
        serde_json::from_slice(&bytes).map_err(|_| fail("invalid draft review report"))?;
    if report.schema_version != 1
        || report.review_id != markers[0]
        || uuid::Uuid::parse_str(&report.review_id).is_err()
        || report.base_tree_digest != digest(&render_tree(old)?)
        || root.is_some_and(|root| report.root != root)
    {
        return Err(fail(
            "draft review report does not match its marker, baseline or selected root",
        ));
    }
    if (!report.unresolved.is_empty() || report.target_tree_digest != digest(&render_tree(target)?))
        && !acknowledged
    {
        return Err(fail(
            "LLM draft contains unresolved review items or was edited after generation; review the Tree/report and use plan --acknowledge-review to explicitly accept the current target (unmapped files stay in place)",
        ));
    }
    Ok(())
}
