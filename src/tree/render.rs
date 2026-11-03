use std::collections::HashMap;

use crate::error::TreeError;

use super::{NodeKind, TreeNode, TreeSpec, validate_tree};

/// Canonical LF output, directories first, then case-sensitive Unicode order.
pub fn render_tree(tree: &TreeSpec) -> Result<String, TreeError> {
    validate_tree(tree)?;
    let mut children: HashMap<Option<&str>, Vec<&TreeNode>> = HashMap::new();
    for node in &tree.nodes {
        children
            .entry(node.parent_id.as_deref())
            .or_default()
            .push(node);
    }
    for nodes in children.values_mut() {
        nodes.sort_by(|a, b| (a.kind, &a.name).cmp(&(b.kind, &b.name)));
    }
    let mut output = String::from("!tree 1\n");
    let mut stack: Vec<_> = children
        .get(&None)
        .into_iter()
        .flatten()
        .rev()
        .map(|node| (*node, 0))
        .collect();
    while let Some((node, depth)) = stack.pop() {
        output.push_str(&"  ".repeat(depth));
        if needs_quotes(&node.name) {
            output.push_str(
                &serde_json::to_string(&node.name)
                    .map_err(|e| TreeError::invalid(e.to_string()))?,
            );
        } else {
            output.push_str(&node.name);
        }
        if node.kind == NodeKind::Directory {
            output.push('/');
        }
        output.push_str(" @");
        output.push_str(&node.id);
        if let Some(description) = &node.description {
            output.push_str(" #");
            if !description.is_empty() {
                output.push(' ');
                output.push_str(description);
            }
        }
        output.push('\n');
        if let Some(nodes) = children.get(&Some(node.id.as_str())) {
            stack.extend(nodes.iter().rev().map(|child| (*child, depth + 1)));
        }
    }
    Ok(output)
}

fn needs_quotes(name: &str) -> bool {
    name.trim() != name
        || name.starts_with(['#', '@', '!'])
        || name.contains(" #")
        || name.contains(" @")
        || name.chars().any(|c| c == '"' || c.is_control())
}
