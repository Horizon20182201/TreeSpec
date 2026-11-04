use crate::{
    error::{Result, fail},
    tree::{NodeKind, TreeNode, TreeSpec, validate_tree},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileStamp {
    pub platform_file_key: Option<String>,
    pub size: u64,
    pub mtime: Option<String>,
    pub birthtime: Option<String>,
    pub ctime: Option<String>,
    pub links: u64,
}

impl FileStamp {
    pub(crate) fn from_metadata(meta: &fs::Metadata) -> Self {
        let mut stamp = Self {
            platform_file_key: None,
            size: meta.len(),
            mtime: meta.modified().ok().map(timestamp),
            birthtime: meta.created().ok().map(timestamp),
            ctime: None,
            links: 1,
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            stamp.platform_file_key = Some(format!("{}:{}", meta.dev(), meta.ino()));
            stamp.ctime = Some(format!("{}:{}", meta.ctime(), meta.ctime_nsec()));
            stamp.links = meta.nlink();
        }
        stamp
    }
}

fn timestamp(time: SystemTime) -> String {
    match time.duration_since(UNIX_EPOCH) {
        Ok(value) => format!("{}:{:09}", value.as_secs(), value.subsec_nanos()),
        Err(value) => format!(
            "-{}:{:09}",
            value.duration().as_secs(),
            value.duration().subsec_nanos()
        ),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScanEntry {
    pub path: String,
    pub kind: NodeKind,
    pub stamp: FileStamp,
    pub protected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScanIssue {
    pub path: String,
    pub kind: ScanIssueKind,
    pub reason: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanIssueKind {
    Error,
    Skipped,
    Protected,
    Excluded,
}

#[derive(Clone, Debug, Serialize)]
pub struct ScanSnapshot {
    pub scan_id: String,
    pub root: PathBuf,
    pub complete: bool,
    pub entries: Vec<ScanEntry>,
    pub warnings: Vec<ScanIssue>,
    pub errors: Vec<ScanIssue>,
}

/// Enumerates metadata only. Linux traversal uses directory descriptors and
/// O_NOFOLLOW, so replacing an entry with a symlink cannot redirect recursion.
pub fn scan(root: &Path) -> Result<ScanSnapshot> {
    let root = crate::metadata::canonical_root(root)?;
    let mut snapshot = ScanSnapshot {
        scan_id: format!("scan_{}", Uuid::new_v4()),
        root,
        complete: true,
        entries: vec![],
        warnings: vec![],
        errors: vec![],
    };
    #[cfg(target_os = "linux")]
    linux::walk(&mut snapshot)?;
    #[cfg(not(target_os = "linux"))]
    return Err(fail(
        "M1 scanning currently requires Linux descriptor-relative traversal",
    ));
    snapshot.entries.sort_by(|a, b| a.path.cmp(&b.path));
    snapshot.warnings.sort_by(|a, b| a.path.cmp(&b.path));
    snapshot.complete = snapshot.errors.is_empty();
    Ok(snapshot)
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use rustix::fs::{Dir, Mode, OFlags, open, openat};
    use std::{ffi::OsStr, fs::File, os::unix::ffi::OsStrExt};

    const DIRECTORY: OFlags = OFlags::RDONLY
        .union(OFlags::DIRECTORY)
        .union(OFlags::NOFOLLOW)
        .union(OFlags::CLOEXEC);
    const METADATA: OFlags = OFlags::PATH.union(OFlags::NOFOLLOW).union(OFlags::CLOEXEC);

    struct Frame {
        file: File,
        path: String,
        before: FileStamp,
        entries: Dir,
    }

    pub(super) fn walk(snapshot: &mut ScanSnapshot) -> Result<()> {
        let root = File::from(open(&snapshot.root, DIRECTORY, Mode::empty())?);
        let mut stack = vec![frame(root, String::new())?];
        while let Some(parent) = stack.last_mut() {
            let entry = match parent.entries.next() {
                Some(Ok(entry)) => entry,
                Some(Err(error)) => {
                    snapshot.errors.push(issue(&parent.path, error));
                    stack.pop();
                    continue;
                }
                None => {
                    match parent.file.metadata() {
                        Ok(meta) if FileStamp::from_metadata(&meta) == parent.before => {}
                        Ok(_) => snapshot
                            .errors
                            .push(issue(&parent.path, "directory changed during scan")),
                        Err(error) => snapshot.errors.push(issue(&parent.path, error)),
                    }
                    stack.pop();
                    continue;
                }
            };
            let bytes = entry.file_name().to_bytes();
            if matches!(bytes, b"." | b"..") {
                continue;
            }
            let name = OsStr::from_bytes(bytes);
            if name == ".organizer" {
                if !parent.path.is_empty() {
                    snapshot.warnings.push(ScanIssue {
                        path: format!("{}/.organizer", parent.path),
                        kind: ScanIssueKind::Excluded,
                        reason: "nested management metadata is excluded".into(),
                    });
                }
                continue;
            }
            let Some(name_text) = name.to_str() else {
                snapshot
                    .errors
                    .push(issue(&parent.path, format!("non-UTF-8 filename {name:?}")));
                continue;
            };
            let path = if parent.path.is_empty() {
                name_text.to_owned()
            } else {
                format!("{}/{name_text}", parent.path)
            };
            if name_text == ".git" {
                snapshot.warnings.push(warning(
                    &path,
                    ScanIssueKind::Excluded,
                    "Git metadata is excluded",
                ));
                continue;
            }
            let result = inspect(&parent.file, name, &path);
            match result {
                Ok((None, _)) => snapshot.warnings.push(warning(
                    &path,
                    ScanIssueKind::Skipped,
                    "symlink or special file skipped",
                )),
                Ok((Some(entry), child)) => {
                    if entry.protected {
                        snapshot.warnings.push(warning(
                            &path,
                            ScanIssueKind::Protected,
                            "protected nested Git project; contents not scanned",
                        ));
                    }
                    snapshot.entries.push(entry);
                    if let Some(child) = child {
                        stack.push(child);
                    }
                }
                Err(error) => snapshot.errors.push(issue(&path, error)),
            }
        }
        Ok(())
    }

    fn frame(file: File, path: String) -> Result<Frame> {
        let before = FileStamp::from_metadata(&file.metadata()?);
        let entries = Dir::read_from(&file)?;
        Ok(Frame {
            file,
            path,
            before,
            entries,
        })
    }

    fn inspect(
        parent: &File,
        name: &OsStr,
        path: &str,
    ) -> Result<(Option<ScanEntry>, Option<Frame>)> {
        let handle = File::from(openat(parent, name, METADATA, Mode::empty())?);
        let meta = handle.metadata()?;
        let kind = if meta.is_dir() {
            NodeKind::Directory
        } else if meta.is_file() {
            NodeKind::File
        } else {
            return Ok((None, None));
        };
        validate_tree(&TreeSpec {
            version: 1,
            nodes: vec![TreeNode {
                id: "check".into(),
                parent_id: None,
                name: name.to_str().ok_or_else(|| fail("non-UTF-8 name"))?.into(),
                kind,
                description: None,
            }],
        })?;
        let stamp = FileStamp::from_metadata(&meta);
        let mut protected = false;
        let mut child = None;
        if kind == NodeKind::Directory {
            let file = File::from(openat(parent, name, DIRECTORY, Mode::empty())?);
            if FileStamp::from_metadata(&file.metadata()?) != stamp {
                return Err(fail("directory replaced during scan"));
            }
            match openat(&file, ".git", METADATA, Mode::empty()) {
                Ok(_) => protected = true,
                Err(rustix::io::Errno::NOENT) => {}
                Err(error) => return Err(error.into()),
            }
            if !protected {
                child = Some(frame(file, path.to_owned())?);
            }
        }
        Ok((
            Some(ScanEntry {
                path: path.to_owned(),
                kind,
                stamp,
                protected,
            }),
            child,
        ))
    }

    fn issue(path: &str, error: impl std::fmt::Display) -> ScanIssue {
        ScanIssue {
            path: path.to_owned(),
            kind: ScanIssueKind::Error,
            reason: error.to_string(),
        }
    }

    fn warning(path: &str, kind: ScanIssueKind, reason: &str) -> ScanIssue {
        ScanIssue {
            path: path.to_owned(),
            kind,
            reason: reason.to_owned(),
        }
    }
}
