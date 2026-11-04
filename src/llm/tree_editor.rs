use super::{
    context::{Scope, validate_target},
    provider::{LlmProvider, LlmRequest, complete},
};
use crate::{
    error::{Result, fail},
    tree::{NodeKind, TreeNode, TreeSpec},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditorResponse {
    pub schema_version: u32,
    pub edits: Vec<Edit>,
    pub migration_intent: MigrationIntent,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Edit {
    AddDirectory {
        parent_id: Option<String>,
        name: String,
        description: Option<String>,
        local_ref: Option<String>,
    },
    RenameNode {
        id: String,
        name: String,
    },
    MoveNode {
        id: String,
        parent_id: Option<String>,
    },
    SetDescription {
        id: String,
        description: Option<String>,
    },
    RemoveEmptyDirectoryDeclaration {
        id: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentKind {
    StructureOnly,
    RedistributeExistingFiles,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationIntent {
    pub kind: IntentKind,
    pub scope_dir_ids: Vec<String>,
}

pub(crate) struct Edited {
    pub tree: TreeSpec,
    pub intent: MigrationIntent,
    pub edit_count: usize,
}

pub(crate) fn edit(
    provider: &impl LlmProvider,
    old: &TreeSpec,
    scope: &mut Scope,
    prompt: &str,
    classify: bool,
) -> Result<Edited> {
    let response: EditorResponse = complete(
        provider,
        LlmRequest {
            task: "tree_editor".into(),
            repair: false,
            output_schema: serde_json::from_str(include_str!("schemas/tree_editor.v1.json"))
                .expect("embedded schema"),
            input: json!({"schema_version":1,"user_intent":prompt,"classify_allowed":classify,"root_parent_allowed":scope.root_allowed,"data":{"nodes":scope.nodes},"rules":{"rename_move_directories_only":true,"immovable_directory_ids":scope.immovable}}),
        },
    )?;
    if response.schema_version != 1 || response.edits.len() > 64 {
        return Err(fail("unsupported TreeEditor schema version or edit count"));
    }
    if response.migration_intent.kind == IntentKind::RedistributeExistingFiles && !classify {
        return Err(fail(
            "TreeEditor requested file classification without --classify",
        ));
    }
    if response.migration_intent.kind == IntentKind::StructureOnly
        && !response.migration_intent.scope_dir_ids.is_empty()
    {
        return Err(fail("structure_only must have empty scope_dir_ids"));
    }
    let mut distinct = HashSet::new();
    for id in &response.migration_intent.scope_dir_ids {
        if !distinct.insert(id)
            || !scope.editable.contains(id)
            || !old
                .nodes
                .iter()
                .any(|n| n.id == *id && n.kind == NodeKind::Directory)
        {
            return Err(fail(
                "TreeEditor classification scope contains duplicate, unknown or unauthorized directory IDs",
            ));
        }
    }
    let mut tree = old.clone();
    let mut aliases = HashMap::new();
    for edit in &response.edits {
        match edit {
            Edit::AddDirectory {
                parent_id,
                name,
                description,
                local_ref,
            } => {
                validate_text(name, description.as_deref())?;
                let parent_id = parent(parent_id.as_deref(), &aliases, scope, &tree)?;
                let id = format!("d_{}", uuid::Uuid::new_v4().simple());
                if let Some(alias) = local_ref {
                    if !alias.starts_with('$')
                        || alias.len() < 2
                        || alias.len() > 64
                        || !alias[1..]
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
                        || aliases.contains_key(alias)
                    {
                        return Err(fail("invalid or duplicate local directory reference"));
                    }
                    aliases.insert(alias.clone(), id.clone());
                }
                scope.editable.insert(id.clone());
                tree.nodes.push(TreeNode {
                    id,
                    parent_id,
                    name: name.clone(),
                    kind: NodeKind::Directory,
                    description: description.clone(),
                });
            }
            Edit::RenameNode { id, name } => {
                validate_text(name, None)?;
                let id = resolve(id, &aliases, scope)?;
                movable(&tree, &id, scope)?;
                find_mut(&mut tree, &id)?.name = name.clone();
            }
            Edit::MoveNode { id, parent_id } => {
                let id = resolve(id, &aliases, scope)?;
                movable(&tree, &id, scope)?;
                let parent_id = parent(parent_id.as_deref(), &aliases, scope, &tree)?;
                find_mut(&mut tree, &id)?.parent_id = parent_id;
            }
            Edit::SetDescription { id, description } => {
                if description.as_ref().is_some_and(|text| text.len() > 1024) {
                    return Err(fail("description exceeds limit"));
                }
                let id = resolve(id, &aliases, scope)?;
                find_mut(&mut tree, &id)?.description = description.clone();
            }
            Edit::RemoveEmptyDirectoryDeclaration { id } => {
                let id = resolve(id, &aliases, scope)?;
                movable(&tree, &id, scope)?;
                if tree.nodes.iter().any(|n| n.parent_id.as_ref() == Some(&id)) {
                    return Err(fail("only empty directory declarations may be removed"));
                }
                tree.nodes.retain(|n| n.id != id);
                scope.editable.remove(&id);
            }
        }
        validate_target(&tree)?;
    }
    Ok(Edited {
        tree,
        intent: response.migration_intent,
        edit_count: response.edits.len(),
    })
}

fn validate_text(name: &str, description: Option<&str>) -> Result<()> {
    if name.len() > 255 || description.is_some_and(|text| text.len() > 1024) {
        return Err(fail("LLM name or description exceeds limit"));
    }
    Ok(())
}
fn resolve(id: &str, aliases: &HashMap<String, String>, scope: &Scope) -> Result<String> {
    let id = aliases.get(id).map_or(id, String::as_str);
    if !scope.editable.contains(id) {
        return Err(fail(
            "TreeEditor referenced an unknown, read-only, sensitive or out-of-scope ID",
        ));
    }
    Ok(id.into())
}
fn parent(
    id: Option<&str>,
    aliases: &HashMap<String, String>,
    scope: &Scope,
    tree: &TreeSpec,
) -> Result<Option<String>> {
    match id {
        None if scope.root_allowed => Ok(None),
        None => Err(fail(
            "scope does not authorize changes at the management root",
        )),
        Some(id) => {
            let id = resolve(id, aliases, scope)?;
            if !tree
                .nodes
                .iter()
                .any(|n| n.id == id && n.kind == NodeKind::Directory)
            {
                return Err(fail("parent is missing or is not a directory"));
            }
            Ok(Some(id))
        }
    }
}
fn movable(tree: &TreeSpec, id: &str, scope: &Scope) -> Result<()> {
    if scope.immovable.contains(id)
        || !tree
            .nodes
            .iter()
            .any(|n| n.id == id && n.kind == NodeKind::Directory)
    {
        return Err(fail(
            "TreeEditor rename/move/removal requires an editable directory without protected, skipped or sensitive descendants",
        ));
    }
    Ok(())
}
fn find_mut<'a>(tree: &'a mut TreeSpec, id: &str) -> Result<&'a mut TreeNode> {
    tree.nodes
        .iter_mut()
        .find(|n| n.id == id)
        .ok_or_else(|| fail("TreeEditor referenced a removed node"))
}
