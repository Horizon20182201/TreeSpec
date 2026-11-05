//! Precomputed child order and paths for drawing a TreeSpec without
//! recomputing anything per frame.

use organizer::tree::{NodeKind, TreeNode, TreeSpec, relative_paths};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Default)]
pub struct TreeIndex {
    pub nodes: HashMap<String, TreeNode>,
    pub paths: HashMap<String, String>,
    /// Children per parent (`None` = managed root), directories first, then by
    /// name: the same order as `render_tree`.
    pub children: HashMap<Option<String>, Vec<String>>,
}

impl TreeIndex {
    pub fn new(tree: &TreeSpec) -> Self {
        let paths = relative_paths(tree).unwrap_or_default();
        let nodes: HashMap<_, _> = tree
            .nodes
            .iter()
            .map(|n| (n.id.clone(), n.clone()))
            .collect();
        let mut children: HashMap<Option<String>, Vec<String>> = HashMap::new();
        for node in &tree.nodes {
            children
                .entry(node.parent_id.clone())
                .or_default()
                .push(node.id.clone());
        }
        for list in children.values_mut() {
            list.sort_by(|a, b| {
                let (a, b) = (&nodes[a], &nodes[b]);
                (a.kind, &a.name).cmp(&(b.kind, &b.name))
            });
        }
        Self {
            nodes,
            paths,
            children,
        }
    }

    pub fn is_dir(&self, id: &str) -> bool {
        self.nodes
            .get(id)
            .is_some_and(|n| n.kind == NodeKind::Directory)
    }

    pub fn kids(&self, parent: Option<&str>) -> &[String] {
        self.children
            .get(&parent.map(str::to_owned))
            .map_or(&[], Vec::as_slice)
    }

    /// `▾ `/`▸ ` for directories with children, two spaces otherwise, so
    /// names at the same depth line up.
    pub fn marker(&self, id: &str, collapsed: &HashSet<String>) -> &'static str {
        match (
            self.is_dir(id) && !self.kids(Some(id)).is_empty(),
            collapsed.contains(id),
        ) {
            (false, _) => "  ",
            (true, true) => "▸ ",
            (true, false) => "▾ ",
        }
    }

    /// Visible rows as (id, depth). `filter` keeps only those IDs (callers
    /// include ancestors); collapsed directories hide their descendants.
    pub fn rows(
        &self,
        collapsed: &HashSet<String>,
        filter: Option<&HashSet<String>>,
    ) -> Vec<(String, usize)> {
        let mut out = vec![];
        let mut stack: Vec<(String, usize)> = self
            .kids(None)
            .iter()
            .rev()
            .map(|id| (id.clone(), 0))
            .collect();
        while let Some((id, depth)) = stack.pop() {
            if filter.is_some_and(|f| !f.contains(&id)) {
                continue;
            }
            if !collapsed.contains(&id) {
                for child in self.kids(Some(&id)).iter().rev() {
                    stack.push((child.clone(), depth + 1));
                }
            }
            out.push((id, depth));
        }
        out
    }

    /// The IDs plus all of their ancestors.
    pub fn with_ancestors<'a>(&self, ids: impl IntoIterator<Item = &'a String>) -> HashSet<String> {
        let mut set = HashSet::new();
        for id in ids {
            let mut current = Some(id.clone());
            while let Some(id) = current {
                if !set.insert(id.clone()) {
                    break;
                }
                current = self.nodes.get(&id).and_then(|n| n.parent_id.clone());
            }
        }
        set
    }
}

/// Cursor/scroll/collapse state for one tree pane.
#[derive(Clone, Debug, Default)]
pub struct TreeView {
    pub collapsed: HashSet<String>,
    pub cursor: usize,
    pub offset: usize,
}

impl TreeView {
    /// Keeps the cursor inside `len` rows and visible in `height` lines.
    pub fn fit(&mut self, len: usize, height: usize) {
        self.cursor = self.cursor.min(len.saturating_sub(1));
        let height = height.max(1);
        if self.cursor < self.offset {
            self.offset = self.cursor;
        } else if self.cursor >= self.offset + height {
            self.offset = self.cursor + 1 - height;
        }
        self.offset = self.offset.min(len.saturating_sub(height));
    }
}
