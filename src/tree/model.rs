use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Directory,
    File,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeNode {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub kind: NodeKind,
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeSpec {
    pub version: u32,
    /// Storage order has no semantic meaning. Rendering sorts siblings.
    pub nodes: Vec<TreeNode>,
}

impl Default for TreeSpec {
    fn default() -> Self {
        Self {
            version: 1,
            nodes: Vec::new(),
        }
    }
}
