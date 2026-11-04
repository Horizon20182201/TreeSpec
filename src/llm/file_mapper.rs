use super::{
    ReviewItem,
    context::{MAX_FILES, Scope, TOTAL_CONTEXT_BYTES, read_excerpt, sensitive, validate_target},
    provider::{LlmProvider, LlmRequest, complete},
    tree_editor::MigrationIntent,
};
use crate::{
    error::{Result, fail},
    migration::plan::within,
    storage::SavedState,
    tree::{NodeKind, TreeSpec, relative_paths},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapperResponse {
    pub schema_version: u32,
    pub assignments: Vec<Assignment>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "decision", rename_all = "snake_case", deny_unknown_fields)]
pub enum Assignment {
    Place {
        file_id: String,
        target_dir_id: String,
        reason: String,
    },
    Keep {
        file_id: String,
        reason: String,
    },
    Review {
        file_id: String,
        reason: String,
    },
}
impl Assignment {
    fn id(&self) -> &str {
        match self {
            Self::Place { file_id, .. }
            | Self::Keep { file_id, .. }
            | Self::Review { file_id, .. } => file_id,
        }
    }
    fn reason(&self) -> &str {
        match self {
            Self::Place { reason, .. }
            | Self::Keep { reason, .. }
            | Self::Review { reason, .. } => reason,
        }
    }
}

pub(crate) struct Mapped {
    pub tree: TreeSpec,
    pub assignments: Vec<Assignment>,
    pub unresolved: Vec<ReviewItem>,
    pub context_bytes: usize,
}

pub(crate) fn map(
    provider: &impl LlmProvider,
    saved: &SavedState,
    draft: &TreeSpec,
    scope: &Scope,
    intent: &MigrationIntent,
    prompt: &str,
    read_content: bool,
) -> Result<Mapped> {
    let old_paths = relative_paths(&saved.tree)?;
    let draft_paths = relative_paths(draft)?;
    let mut files: Vec<_> = saved
        .tree
        .nodes
        .iter()
        .filter(|node| {
            node.kind == NodeKind::File
                && scope.editable.contains(&node.id)
                && (intent.scope_dir_ids.is_empty()
                    || intent
                        .scope_dir_ids
                        .iter()
                        .any(|id| within(&old_paths[&node.id], &old_paths[id])))
        })
        .collect();
    files.sort_by_key(|node| &old_paths[&node.id]);
    if files.len() > MAX_FILES {
        return Err(fail("FileMapper scope exceeds 256 files; narrow --scope"));
    }
    let file_ids: HashSet<_> = files.iter().map(|node| node.id.as_str()).collect();
    let mut candidates: Vec<_> = draft
        .nodes
        .iter()
        .filter(|node| {
            node.kind == NodeKind::Directory
                && scope.editable.contains(&node.id)
                && !sensitive(&draft_paths[&node.id])
        })
        .collect();
    candidates.sort_by_key(|node| &draft_paths[&node.id]);
    let candidate_ids: HashSet<_> = candidates.iter().map(|node| node.id.as_str()).collect();
    let mut budget = TOTAL_CONTEXT_BYTES;
    let mut contexts = vec![];
    for node in &files {
        let context = if read_content {
            read_excerpt(saved, &node.id, &mut budget)?
        } else {
            None
        };
        contexts.push(json!({"id":node.id,"path":old_paths[&node.id],"draft_path":draft_paths[&node.id],"context":context}));
    }
    if files.is_empty() {
        return Ok(Mapped {
            tree: draft.clone(),
            assignments: vec![],
            unresolved: vec![],
            context_bytes: 0,
        });
    }
    let response: MapperResponse = complete(
        provider,
        LlmRequest {
            task: "file_mapper".into(),
            repair: false,
            output_schema: serde_json::from_str(include_str!("schemas/file_mapper.v1.json"))
                .expect("embedded schema"),
            input: json!({"schema_version":1,"user_intent":prompt,"data":{"candidate_directories":candidates.iter().map(|node| json!({"id":node.id,"path":draft_paths[&node.id],"description":node.description})).collect::<Vec<_>>(),"files":contexts},"rules":{"keep_and_review_preserve_file_name_and_parent_id":true,"omitted_files_are_review":true}}),
        },
    )?;
    if response.schema_version != 1 || response.assignments.len() > MAX_FILES {
        return Err(fail("unsupported FileMapper version or assignment count"));
    }
    let mut seen = HashSet::new();
    let mut tree = draft.clone();
    let mut unresolved = vec![];
    for assignment in &response.assignments {
        let id = assignment.id();
        if !file_ids.contains(id) || !seen.insert(id) || assignment.reason().len() > 1024 {
            return Err(fail(
                "FileMapper returned duplicate/unknown/out-of-scope files or an oversized reason",
            ));
        }
        match assignment {
            Assignment::Place { target_dir_id, .. } => {
                if !candidate_ids.contains(target_dir_id.as_str()) {
                    return Err(fail(
                        "FileMapper target is not in the supplied candidate directories",
                    ));
                }
                tree.nodes
                    .iter_mut()
                    .find(|node| node.id == id)
                    .ok_or_else(|| fail("mapped file disappeared from draft"))?
                    .parent_id = Some(target_dir_id.clone());
            }
            Assignment::Review { reason, .. } => unresolved.push(ReviewItem {
                node_id: Some(id.into()),
                reason: reason.clone(),
            }),
            Assignment::Keep { .. } => {}
        }
    }
    for file in files {
        if !seen.contains(file.id.as_str()) {
            unresolved.push(ReviewItem {
                node_id: Some(file.id.clone()),
                reason: "FileMapper omitted this file; keep its parent/name and review manually"
                    .into(),
            });
        }
    }
    validate_target(&tree)?;
    unresolved.sort_by(|a, b| a.node_id.cmp(&b.node_id));
    Ok(Mapped {
        tree,
        assignments: response.assignments,
        unresolved,
        context_bytes: TOTAL_CONTEXT_BYTES - budget,
    })
}
