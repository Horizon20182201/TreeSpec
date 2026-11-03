use std::collections::{HashMap, HashSet};

use crate::error::TreeError;

use super::{NodeKind, TreeSpec};

pub(crate) fn validate_id(id: &str) -> Result<(), TreeError> {
    if id.is_empty()
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(TreeError::invalid(format!(
            "invalid ID {id:?}; expected [A-Za-z0-9_-]+"
        )));
    }
    Ok(())
}

pub(crate) fn validate_name(name: &str) -> Result<(), TreeError> {
    if name.is_empty()
        || matches!(name, "." | ".." | ".organizer")
        || name.contains(['/', '\\', '\0'])
    {
        return Err(TreeError::invalid(format!(
            "unsafe or reserved path segment {name:?}"
        )));
    }
    #[cfg(windows)]
    {
        let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
        let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ["COM", "LPT"].iter().any(|prefix| {
                stem.strip_prefix(prefix).is_some_and(|suffix| {
                    matches!(
                        suffix,
                        "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                    )
                })
            });
        if name
            .chars()
            .any(|c| c.is_control() || "<>:\"|?*".contains(c))
            || name.ends_with(['.', ' '])
            || device
            || name.eq_ignore_ascii_case(".organizer")
        {
            return Err(TreeError::invalid(format!(
                "invalid Windows path segment {name:?}"
            )));
        }
    }
    Ok(())
}

pub(crate) fn validate_description(description: Option<&str>) -> Result<(), TreeError> {
    if let Some(description) = description
        && (description.chars().any(char::is_control) || description.trim() != description)
    {
        return Err(TreeError::invalid(
            "description must be a trimmed, single line without control characters",
        ));
    }
    Ok(())
}

/// Checks programmatically constructed ASTs as well as parsed documents.
/// Iterative parent traversal avoids recursion limits for deep trees.
pub fn validate_tree(tree: &TreeSpec) -> Result<(), TreeError> {
    if tree.version != 1 {
        return Err(TreeError::UnsupportedVersion(tree.version));
    }
    let mut by_id = HashMap::with_capacity(tree.nodes.len());
    let mut siblings = HashSet::with_capacity(tree.nodes.len());
    for (index, node) in tree.nodes.iter().enumerate() {
        validate_id(&node.id)?;
        validate_name(&node.name)?;
        validate_description(node.description.as_deref())?;
        if by_id.insert(node.id.as_str(), index).is_some() {
            return Err(TreeError::invalid(format!("duplicate ID {:?}", node.id)));
        }
        if !siblings.insert((node.parent_id.as_deref(), node.name.as_str())) {
            return Err(TreeError::invalid(format!(
                "duplicate sibling name {:?}",
                node.name
            )));
        }
    }
    for node in &tree.nodes {
        if let Some(parent) = node.parent_id.as_deref() {
            let Some(&index) = by_id.get(parent) else {
                return Err(TreeError::invalid(format!(
                    "node {:?} references missing parent {parent:?}",
                    node.id
                )));
            };
            if tree.nodes[index].kind != NodeKind::Directory {
                return Err(TreeError::invalid(format!(
                    "node {:?} has a file as parent",
                    node.id
                )));
            }
        }
    }
    // 0 = unvisited, 1 = current chain, 2 = checked.
    let mut state = vec![0u8; tree.nodes.len()];
    for start in 0..tree.nodes.len() {
        let mut chain = Vec::new();
        let mut current = Some(start);
        while let Some(index) = current {
            match state[index] {
                1 => {
                    return Err(TreeError::invalid(format!(
                        "parent cycle at {:?}",
                        tree.nodes[index].id
                    )));
                }
                2 => break,
                _ => {}
            }
            state[index] = 1;
            chain.push(index);
            current = tree.nodes[index].parent_id.as_deref().map(|id| by_id[id]);
        }
        for index in chain {
            state[index] = 2;
        }
    }
    Ok(())
}

/// Portable relative paths with `/` separators; these are data, not OS operations.
pub fn relative_paths(tree: &TreeSpec) -> Result<HashMap<String, String>, TreeError> {
    validate_tree(tree)?;
    Ok(paths_validated(tree))
}

pub(crate) fn paths_validated(tree: &TreeSpec) -> HashMap<String, String> {
    let by_id: HashMap<_, _> = tree
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let mut paths: HashMap<String, String> = HashMap::with_capacity(tree.nodes.len());
    for node in &tree.nodes {
        let mut chain = Vec::new();
        let mut current = Some(node);
        let mut prefix = String::new();
        while let Some(node) = current {
            if let Some(path) = paths.get(&node.id) {
                prefix = path.clone();
                break;
            }
            chain.push(node);
            current = node.parent_id.as_deref().map(|id| by_id[id]);
        }
        for node in chain.into_iter().rev() {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(&node.name);
            paths.insert(node.id.clone(), prefix.clone());
        }
    }
    paths
}
