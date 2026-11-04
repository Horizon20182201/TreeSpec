use crate::{
    error::{Result, fail},
    scan::{FileStamp, ScanEntry},
    tree::{NodeKind, TreeSpec, parse_normalized_tree, relative_paths},
};
use rusqlite::{Connection, OpenFlags, params};
use serde::Serialize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize)]
pub struct IndexedNode {
    pub logical_id: String,
    pub entry: ScanEntry,
    pub last_seen_scan: String,
}

#[derive(Clone, Debug)]
pub struct SavedState {
    pub root: PathBuf,
    pub root_stamp: FileStamp,
    pub namespace: String,
    pub generation: u64,
    pub tree: TreeSpec,
    pub tree_text: String,
    pub records: Vec<IndexedNode>,
    pub git_head: Option<String>,
    pub pending_git: bool,
}

pub fn load(path: &Path) -> Result<SavedState> {
    crate::metadata::plain_file(path)?;
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != 1 {
        return Err(fail(format!(
            "unsupported identity database version {version}"
        )));
    }
    let mut statement = connection.prepare("SELECT key, value FROM metadata")?;
    let pairs = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let metadata: HashMap<String, String> = pairs.collect::<std::result::Result<_, _>>()?;
    let get = |key: &str| {
        metadata
            .get(key)
            .cloned()
            .ok_or_else(|| fail(format!("missing database metadata: {key}")))
    };
    let tree_text = get("tree")?;
    let tree = parse_normalized_tree(&tree_text)?;
    let mut statement = connection.prepare("SELECT logical_id, last_path, kind, platform_file_key, size, mtime, birthtime, ctime, links, protected, last_seen_scan FROM nodes ORDER BY last_path")?;
    let mut rows = statement.query([])?;
    let mut records = Vec::new();
    while let Some(row) = rows.next()? {
        let kind = match row.get::<_, String>(2)?.as_str() {
            "directory" => NodeKind::Directory,
            "file" => NodeKind::File,
            _ => return Err(fail("invalid kind in identity database")),
        };
        records.push(IndexedNode {
            logical_id: row.get(0)?,
            last_seen_scan: row.get(10)?,
            entry: ScanEntry {
                path: row.get(1)?,
                kind,
                protected: row.get(9)?,
                stamp: FileStamp {
                    platform_file_key: row.get(3)?,
                    size: number(&row.get::<_, String>(4)?)?,
                    mtime: row.get(5)?,
                    birthtime: row.get(6)?,
                    ctime: row.get(7)?,
                    links: number(&row.get::<_, String>(8)?)?,
                },
            },
        });
    }
    let paths = relative_paths(&tree)?;
    let nodes: HashMap<_, _> = tree
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    if paths.len() != records.len()
        || records.iter().any(|record| {
            paths.get(&record.logical_id) != Some(&record.entry.path)
                || nodes[record.logical_id.as_str()].kind != record.entry.kind
        })
    {
        return Err(fail("identity database does not match its saved Tree"));
    }
    let root_stamp = serde_json::from_str(&get("root_stamp")?)
        .map_err(|error| fail(format!("invalid root identity: {error}")))?;
    let namespace = get("namespace")?;
    uuid::Uuid::parse_str(&namespace).map_err(|_| fail("invalid identity namespace"))?;
    let pending_git = match get("pending_git")?.as_str() {
        "0" => false,
        "1" => true,
        _ => return Err(fail("invalid pending Git state")),
    };
    let git_head = metadata.get("git_head").filter(|s| !s.is_empty()).cloned();
    if git_head
        .as_deref()
        .is_some_and(|head| head.len() != 40 || !head.bytes().all(|c| c.is_ascii_hexdigit()))
    {
        return Err(fail("invalid Git commit ID in identity database"));
    }
    Ok(SavedState {
        root: PathBuf::from(get("root")?),
        root_stamp,
        namespace,
        generation: number(&get("generation")?)?,
        tree,
        tree_text,
        records,
        git_head,
        pending_git,
    })
}

fn number(value: &str) -> Result<u64> {
    value
        .parse()
        .map_err(|_| fail("invalid integer in identity database"))
}

/// The caller owns the metadata lock and pending marker covering the Tree file.
pub(crate) fn save(path: &Path, state: &SavedState, create: bool) -> Result<()> {
    if create {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
    } else {
        crate::metadata::plain_file(path)?;
    }
    let mut connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;")?;
    let transaction = connection.transaction()?;
    transaction.execute_batch("CREATE TABLE IF NOT EXISTS metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS nodes (logical_id TEXT PRIMARY KEY, last_path TEXT NOT NULL UNIQUE, kind TEXT NOT NULL,
        platform_file_key TEXT, size TEXT NOT NULL, mtime TEXT, birthtime TEXT, ctime TEXT, links TEXT NOT NULL,
        protected INTEGER NOT NULL, last_seen_scan TEXT NOT NULL);
        PRAGMA user_version=1; DELETE FROM nodes;")?;
    for record in &state.records {
        let entry = &record.entry;
        let stamp = &entry.stamp;
        transaction.execute(
            "INSERT INTO nodes VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                record.logical_id,
                entry.path,
                if entry.kind == NodeKind::Directory {
                    "directory"
                } else {
                    "file"
                },
                stamp.platform_file_key,
                stamp.size.to_string(),
                stamp.mtime,
                stamp.birthtime,
                stamp.ctime,
                stamp.links.to_string(),
                entry.protected,
                record.last_seen_scan
            ],
        )?;
    }
    let root = state
        .root
        .to_str()
        .ok_or_else(|| fail("non-UTF-8 management root"))?;
    let root_stamp =
        serde_json::to_string(&state.root_stamp).map_err(|error| fail(error.to_string()))?;
    for (key, value) in [
        ("root", root.to_owned()),
        ("root_stamp", root_stamp),
        ("namespace", state.namespace.clone()),
        ("generation", state.generation.to_string()),
        ("tree", state.tree_text.clone()),
        ("git_head", state.git_head.clone().unwrap_or_default()),
        (
            "pending_git",
            if state.pending_git {
                "1".into()
            } else {
                "0".into()
            },
        ),
    ] {
        transaction.execute(
            "INSERT OR REPLACE INTO metadata VALUES (?1,?2)",
            params![key, value],
        )?;
    }
    transaction.commit()?;
    Ok(())
}
