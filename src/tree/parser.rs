use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use crate::error::TreeError;

use super::validate::{validate_description, validate_id, validate_name};
use super::{NodeKind, TreeNode, TreeSpec, relative_paths, validate_tree};

struct Declaration {
    line: usize,
    parent: Option<usize>,
    name: String,
    kind: NodeKind,
    id: Option<String>,
    description: Option<String>,
}

/// Parses a standalone document and assigns deterministic IDs to missing IDs.
pub fn parse_tree(source: &str) -> Result<TreeSpec, TreeError> {
    normalize(declarations(source)?, None)
}

/// Requires persisted identities, suitable for a Diff baseline.
pub fn parse_normalized_tree(source: &str) -> Result<TreeSpec, TreeError> {
    let declarations = declarations(source)?;
    if let Some(node) = declarations.iter().find(|node| node.id.is_none()) {
        return Err(TreeError::at(
            node.line,
            "baseline requires explicit IDs; run normalize first and retain its output",
        ));
    }
    normalize(declarations, None)
}

/// Only missing IDs at the same path AND kind inherit baseline identities.
/// Explicit IDs are authoritative; similarity never implies identity.
pub fn parse_tree_with_base(source: &str, base: &TreeSpec) -> Result<TreeSpec, TreeError> {
    validate_tree(base)?;
    normalize(declarations(source)?, Some(base))
}

fn declarations(source: &str) -> Result<Vec<Declaration>, TreeError> {
    let mut result: Vec<Declaration> = Vec::new();
    let mut ancestors: Vec<usize> = Vec::new();
    let mut header = false;
    let mut ids = HashSet::new();
    let mut siblings = HashSet::new();
    for (index, line) in source.lines().enumerate() {
        let number = index + 1;
        if line.contains(['\t', '\r', '\0']) {
            return Err(TreeError::at(
                number,
                "literal Tab, CR, and NUL are forbidden; use JSON escapes in names",
            ));
        }
        let content = line.trim_start_matches(' ');
        if content.is_empty() || content.starts_with('#') {
            continue;
        }
        let indent = line.len() - content.len();
        if !header {
            if indent != 0 || content.trim_end_matches(' ') != "!tree 1" {
                return Err(TreeError::at(
                    number,
                    "first declaration must be exactly !tree 1",
                ));
            }
            header = true;
            continue;
        }
        if content.starts_with('!') {
            return Err(TreeError::at(
                number,
                "unexpected directive; quote names starting with !",
            ));
        }
        if indent % 2 != 0 {
            return Err(TreeError::at(
                number,
                "indentation must use two spaces per level",
            ));
        }
        let depth = indent / 2;
        if depth > ancestors.len() {
            return Err(TreeError::at(
                number,
                "indentation skips a level or has no parent",
            ));
        }
        ancestors.truncate(depth);
        let parent = ancestors.last().copied();
        if let Some(parent) = parent
            && result[parent].kind != NodeKind::Directory
        {
            return Err(TreeError::at(number, "a file cannot have children"));
        }
        let mut declaration =
            parse_declaration(content).map_err(|e| TreeError::at(number, e.to_string()))?;
        declaration.line = number;
        declaration.parent = parent;
        if let Some(id) = &declaration.id
            && !ids.insert(id.clone())
        {
            return Err(TreeError::at(number, format!("duplicate ID {id:?}")));
        }
        if !siblings.insert((parent, declaration.name.clone())) {
            return Err(TreeError::at(
                number,
                format!("duplicate sibling name {:?}", declaration.name),
            ));
        }
        ancestors.push(result.len());
        result.push(declaration);
    }
    if !header {
        return Err(TreeError::at(1, "missing !tree 1 header"));
    }
    Ok(result)
}

fn parse_declaration(content: &str) -> Result<Declaration, TreeError> {
    let (name, kind, mut suffix) = if content.starts_with('"') {
        let mut strings = serde_json::Deserializer::from_str(content).into_iter::<String>();
        let name = strings
            .next()
            .ok_or_else(|| TreeError::invalid("missing quoted name"))?
            .map_err(|e| TreeError::invalid(format!("invalid JSON name: {e}")))?;
        let suffix = &content[strings.byte_offset()..];
        let (kind, suffix) = match suffix.strip_prefix('/') {
            Some(rest) => (NodeKind::Directory, rest),
            None => (NodeKind::File, suffix),
        };
        (name, kind, suffix)
    } else {
        let end = content
            .as_bytes()
            .windows(2)
            .position(|pair| pair == b" @" || pair == b" #")
            .unwrap_or(content.len());
        let token = content[..end].trim_end_matches(' ');
        if token.contains('"') || token.starts_with('@') {
            return Err(TreeError::invalid("ambiguous name; use a JSON quoted name"));
        }
        let (name, kind) = match token.strip_suffix('/') {
            Some(name) => (name, NodeKind::Directory),
            None => (token, NodeKind::File),
        };
        (name.to_owned(), kind, &content[end..])
    };
    validate_name(&name)?;
    if !suffix.is_empty() && !suffix.starts_with(' ') {
        return Err(TreeError::invalid("expected a space after the name"));
    }
    suffix = suffix.trim_start_matches(' ');
    let mut id = None;
    if let Some(after_at) = suffix.strip_prefix('@') {
        let end = after_at.find(' ').unwrap_or(after_at.len());
        let token = &after_at[..end];
        validate_id(token)?;
        id = Some(token.to_owned());
        suffix = after_at[end..].trim_start_matches(' ');
    }
    let description = if let Some(after_hash) = suffix.strip_prefix('#') {
        Some(after_hash.trim().to_owned())
    } else if suffix.is_empty() {
        None
    } else {
        return Err(TreeError::invalid(
            "expected @ID followed by optional # description",
        ));
    };
    validate_description(description.as_deref())?;
    Ok(Declaration {
        line: 0,
        parent: None,
        name,
        kind,
        id,
        description,
    })
}

fn normalize(
    declarations: Vec<Declaration>,
    base: Option<&TreeSpec>,
) -> Result<TreeSpec, TreeError> {
    let base_paths = base.map(relative_paths).transpose()?.unwrap_or_default();
    let base_nodes: HashMap<_, _> = base
        .into_iter()
        .flat_map(|tree| &tree.nodes)
        .map(|node| (base_paths[&node.id].as_str(), node))
        .collect();
    let mut used: HashSet<String> = declarations
        .iter()
        .filter_map(|node| node.id.clone())
        .collect();
    // Reserve all baseline identities, including nodes omitted by the draft.
    let mut reserved = used.clone();
    reserved.extend(
        base.into_iter()
            .flat_map(|tree| &tree.nodes)
            .map(|node| node.id.clone()),
    );
    let mut tree = TreeSpec::default();
    let mut paths: Vec<String> = Vec::with_capacity(declarations.len());
    for node in declarations {
        let path = match node.parent {
            Some(parent) => format!("{}/{}", paths[parent], node.name),
            None => node.name.clone(),
        };
        let id = match node.id {
            Some(id) => id,
            None => {
                if let Some(old) = base_nodes
                    .get(path.as_str())
                    .filter(|old| old.kind == node.kind)
                {
                    if !used.insert(old.id.clone()) {
                        return Err(TreeError::at(
                            node.line,
                            format!(
                                "inherited ID {:?} is already claimed; give this node an explicit new ID",
                                old.id
                            ),
                        ));
                    }
                    old.id.clone()
                } else {
                    let prefix = if node.kind == NodeKind::Directory {
                        "d"
                    } else {
                        "f"
                    };
                    let mut salt = 0u64;
                    let id = loop {
                        let key = format!("treespec:v1:{prefix}:{salt}:{path}");
                        let candidate = format!(
                            "{prefix}_{}",
                            Uuid::new_v5(&Uuid::NAMESPACE_URL, key.as_bytes())
                        );
                        if reserved.insert(candidate.clone()) {
                            break candidate;
                        }
                        salt += 1;
                    };
                    used.insert(id.clone());
                    id
                }
            }
        };
        let parent_id = node.parent.map(|index| tree.nodes[index].id.clone());
        tree.nodes.push(TreeNode {
            id,
            parent_id,
            name: node.name,
            kind: node.kind,
            description: node.description,
        });
        paths.push(path);
    }
    validate_tree(&tree)?;
    Ok(tree)
}
