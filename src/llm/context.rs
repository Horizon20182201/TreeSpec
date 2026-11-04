use crate::{
    error::{Result, fail},
    migration::plan::within,
    scan::{FileStamp, ScanSnapshot},
    storage::SavedState,
    tree::{NodeKind, TreeSpec, relative_paths},
};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

pub const MAX_NODES: usize = 512;
pub const MAX_FILES: usize = 256;
pub const FILE_CONTEXT_BYTES: usize = 4096;
pub const TOTAL_CONTEXT_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct NodeContext {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub kind: NodeKind,
    pub path: String,
    pub description: Option<String>,
    pub editable: bool,
}

pub(crate) struct Scope {
    pub editable: HashSet<String>,
    pub immovable: HashSet<String>,
    pub root_allowed: bool,
    pub nodes: Vec<NodeContext>,
    pub excluded_count: usize,
}

pub(crate) fn sensitive(path: &str) -> bool {
    path.split('/').any(|name| {
        let name = name.to_ascii_lowercase();
        matches!(
            name.as_str(),
            ".env"
                | ".ssh"
                | ".aws"
                | ".gnupg"
                | ".kube"
                | "credentials"
                | "credentials.json"
                | "id_rsa"
                | "id_dsa"
                | "id_ecdsa"
                | "id_ed25519"
        ) || name.starts_with(".env.")
            || [".pem", ".key", ".p12", ".pfx", ".keystore"]
                .iter()
                .any(|suffix| name.ends_with(suffix))
    })
}

impl Scope {
    pub(crate) fn new(
        saved: &SavedState,
        snapshot: &ScanSnapshot,
        selected: &[String],
    ) -> Result<Self> {
        let paths = relative_paths(&saved.tree)?;
        let nodes: HashMap<_, _> = saved
            .tree
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), n))
            .collect();
        let excluded: HashSet<_> = saved
            .records
            .iter()
            .filter(|r| r.entry.protected || sensitive(&r.entry.path))
            .map(|r| r.logical_id.clone())
            .collect();
        let mut roots = vec![];
        let mut distinct = HashSet::new();
        for id in selected {
            let node = nodes
                .get(id.as_str())
                .ok_or_else(|| fail("--scope must be an existing directory ID"))?;
            if node.kind != NodeKind::Directory || excluded.contains(id) || !distinct.insert(id) {
                return Err(fail(
                    "--scope is duplicated, sensitive, protected or not a directory",
                ));
            }
            roots.push(paths[id].as_str());
        }
        let editable: HashSet<_> = nodes
            .values()
            .filter(|n| {
                !excluded.contains(&n.id)
                    && (roots.is_empty() || roots.iter().any(|root| within(&paths[&n.id], root)))
            })
            .map(|n| n.id.clone())
            .collect();
        let mut visible = editable.clone();
        for id in &editable {
            let mut parent = nodes[id.as_str()].parent_id.as_deref();
            while let Some(id) = parent {
                visible.insert(id.into());
                parent = nodes[id].parent_id.as_deref();
            }
        }
        if visible.len() > MAX_NODES {
            return Err(fail(
                "LLM scope exceeds 512 nodes; narrow it with --scope DIRECTORY_ID",
            ));
        }
        let immovable = editable
            .iter()
            .filter(|id| {
                nodes[id.as_str()].kind == NodeKind::Directory
                    && (excluded
                        .iter()
                        .any(|excluded| within(&paths[excluded], &paths[*id]))
                        || snapshot
                            .warnings
                            .iter()
                            .any(|warning| within(&warning.path, &paths[*id])))
            })
            .cloned()
            .collect();
        let mut context: Vec<_> = visible
            .iter()
            .map(|id| {
                let node = nodes[id.as_str()];
                NodeContext {
                    id: id.clone(),
                    parent_id: node.parent_id.clone(),
                    name: node.name.clone(),
                    kind: node.kind,
                    path: paths[id].clone(),
                    description: node.description.clone(),
                    editable: editable.contains(id),
                }
            })
            .collect();
        context.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Self {
            editable,
            immovable,
            root_allowed: selected.is_empty(),
            nodes: context,
            excluded_count: excluded.len(),
        })
    }
}

pub(crate) fn read_excerpt(
    saved: &SavedState,
    id: &str,
    budget: &mut usize,
) -> Result<Option<String>> {
    let record = saved
        .records
        .iter()
        .find(|r| r.logical_id == id)
        .ok_or_else(|| fail("unknown context source"))?;
    let path = &record.entry.path;
    let extension = path
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase());
    if sensitive(path)
        || record.entry.protected
        || record.entry.kind != NodeKind::File
        || record.entry.stamp.links != 1
        || !extension.as_deref().is_some_and(|ext| {
            matches!(
                ext,
                "txt"
                    | "md"
                    | "rst"
                    | "csv"
                    | "json"
                    | "toml"
                    | "yaml"
                    | "yml"
                    | "rs"
                    | "py"
                    | "c"
                    | "h"
                    | "cpp"
                    | "js"
                    | "ts"
                    | "html"
                    | "css"
                    | "log"
            )
        })
        || *budget == 0
    {
        return Ok(None);
    }
    #[cfg(target_os = "linux")]
    {
        use rustix::fs::{Mode, OFlags, ResolveFlags, openat2};
        use std::{fs::File, io::Read};
        let root = File::from(openat2(
            rustix::fs::CWD,
            &saved.root,
            OFlags::PATH | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
            ResolveFlags::NO_SYMLINKS,
        )?);
        let root_stamp = FileStamp::from_metadata(&root.metadata()?);
        if root_stamp.platform_file_key != saved.root_stamp.platform_file_key
            || root_stamp.birthtime != saved.root_stamp.birthtime
        {
            return Err(fail("root changed before reading LLM context"));
        }
        let mut file = File::from(openat2(
            &root,
            path.as_str(),
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
            ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_XDEV,
        )?);
        let metadata = file.metadata()?;
        if !metadata.is_file() || FileStamp::from_metadata(&metadata) != record.entry.stamp {
            return Err(fail("context source changed; sync and retry chat"));
        }
        let limit = FILE_CONTEXT_BYTES.min(*budget);
        let mut bytes = vec![];
        (&mut file).take(limit as u64).read_to_end(&mut bytes)?;
        *budget -= bytes.len();
        if FileStamp::from_metadata(&file.metadata()?) != record.entry.stamp {
            return Err(fail("context source changed during read"));
        }
        if bytes.contains(&0) {
            return Ok(None);
        }
        let text = match std::str::from_utf8(&bytes) {
            Ok(text) => text,
            Err(error) if error.error_len().is_none() => {
                std::str::from_utf8(&bytes[..error.valid_up_to()])
                    .map_err(|_| fail("invalid UTF-8 context"))?
            }
            Err(_) => return Ok(None),
        };
        // A conservative second check for common private-key blocks, including
        // keys disguised as text files. This is not a general secret detector.
        if text.contains("PRIVATE KEY-----") {
            return Ok(None);
        }
        Ok(Some(text.into()))
    }
    #[cfg(not(target_os = "linux"))]
    Err(fail("safe content context requires Linux"))
}

pub(crate) fn validate_target(tree: &TreeSpec) -> Result<()> {
    let paths = relative_paths(tree).map_err(|_| {
        fail("LLM proposed invalid Tree names, descriptions, collisions or parent relationships")
    })?;
    for path in paths.values() {
        crate::migration::validate::relative_path(path)
            .map_err(|_| fail("LLM proposed a reserved or escaping path"))?;
    }
    Ok(())
}
