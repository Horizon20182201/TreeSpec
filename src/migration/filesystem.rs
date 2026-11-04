#[cfg(any(target_os = "linux", target_os = "macos"))]
mod native {
    use crate::{
        error::{Result, fail},
        fsops::{self, DIRECTORY},
        migration::{
            MigrationPlan, Operation,
            plan::{parent, within},
        },
        scan::{FileStamp, ScanEntry},
        tree::NodeKind,
    };
    use std::{
        collections::BTreeMap,
        fs::File,
        path::{Path, PathBuf},
    };

    pub(crate) struct Filesystem {
        root: File,
        path: PathBuf,
        identity: FileStamp,
    }

    impl Filesystem {
        pub(crate) fn open(path: &Path, identity: &FileStamp) -> Result<Self> {
            let root = fsops::open_root(path)?;
            if !fsops::same_identity(identity, &FileStamp::from_metadata(&root.metadata()?)) {
                return Err(fail("root identity changed"));
            }
            Ok(Self {
                root,
                path: path.to_owned(),
                identity: identity.clone(),
            })
        }

        fn open_relative(&self, path: &str, flags: rustix::fs::OFlags) -> Result<File> {
            fsops::open_beneath(&self.root, path, flags)
        }

        fn inspect(&self, path: &str) -> Result<(FileStamp, NodeKind)> {
            fsops::inspect_beneath(&self.root, path)
        }

        pub(crate) fn preflight(&self, plan: &MigrationPlan) -> Result<()> {
            for condition in &plan.preconditions {
                let (stamp, kind) = self.inspect(&condition.entry.path)?;
                if stamp != condition.entry.stamp || kind != condition.entry.kind {
                    return Err(fail(format!("source changed: {:?}", condition.entry.path)));
                }
            }
            Ok(())
        }

        fn directory(&self, path: &str, entries: &BTreeMap<String, ScanEntry>) -> Result<File> {
            let directory = self.open_relative(path, DIRECTORY)?;
            let expected = if path.is_empty() {
                &self.identity
            } else {
                let entry = entries
                    .values()
                    .find(|entry| entry.path == path)
                    .ok_or_else(|| fail(format!("unplanned parent directory: {path:?}")))?;
                if entry.kind != NodeKind::Directory || entry.protected {
                    return Err(fail("protected or invalid destination parent"));
                }
                &entry.stamp
            };
            if !fsops::same_identity(expected, &FileStamp::from_metadata(&directory.metadata()?)) {
                return Err(fail("parent directory identity changed"));
            }
            if !path.is_empty() && fsops::exists_child(&directory, ".git")? {
                return Err(fail("parent became a protected Git project"));
            }
            Ok(directory)
        }

        fn attached(&self, directory: &File, path: &str) -> Result<()> {
            let current = self.open_relative(path, DIRECTORY)?;
            if !fsops::same_identity(
                &FileStamp::from_metadata(&directory.metadata()?),
                &FileStamp::from_metadata(&current.metadata()?),
            ) {
                return Err(fail("parent directory detached/replaced during migration"));
            }
            Ok(())
        }

        pub(crate) fn execute(
            &self,
            operation: &Operation,
            entries: &BTreeMap<String, ScanEntry>,
        ) -> Result<ScanEntry> {
            // Reopen by absolute path to detect root replacement; all actual
            // mutations use the original root descriptor and leaf-only names.
            Self::open(&self.path, &self.identity)?;
            let target = operation.destination();
            let target_parent = self.directory(parent(target), entries)?;
            let target_name = target.rsplit('/').next().unwrap();
            let kind = match operation {
                Operation::Mkdir { .. } => {
                    self.attached(&target_parent, parent(target))?;
                    fsops::create_directory(&target_parent, target_name)?;
                    NodeKind::Directory
                }
                Operation::MoveFile { from, .. } | Operation::MoveDirectory { from, .. } => {
                    let expected = entries
                        .get(operation.id())
                        .ok_or_else(|| fail("source ID missing from execution state"))?;
                    if expected.path != *from
                        || (expected.kind == NodeKind::Directory && within(target, from))
                    {
                        return Err(fail("invalid source path or self-nesting move"));
                    }
                    let source_parent = self.directory(parent(from), entries)?;
                    let (stamp, kind) = self.inspect(from)?;
                    if stamp != expected.stamp || kind != expected.kind {
                        return Err(fail("source changed immediately before rename"));
                    }
                    self.attached(&source_parent, parent(from))?;
                    self.attached(&target_parent, parent(target))?;
                    fsops::rename_noreplace(
                        &source_parent,
                        from.rsplit('/').next().unwrap(),
                        &target_parent,
                        target_name,
                    )?;
                    source_parent.sync_all()?;
                    self.attached(&source_parent, parent(from))?;
                    kind
                }
            };
            target_parent.sync_all()?;
            self.attached(&target_parent, parent(target))?;
            let (stamp, actual_kind) = self.inspect(target)?;
            if kind != actual_kind {
                return Err(fail("target type changed after operation"));
            }
            if let Some(before) = entries.get(operation.id()) {
                if !fsops::same_identity(&before.stamp, &stamp) {
                    return Err(fail(
                        "source identity changed during rename; inspect recovery log",
                    ));
                }
                if before.stamp.size != stamp.size
                    || before.stamp.mtime != stamp.mtime
                    || before.stamp.links != stamp.links
                {
                    return Err(fail(
                        "source attributes changed during rename; inspect recovery log",
                    ));
                }
            }
            Ok(ScanEntry {
                path: target.into(),
                kind,
                stamp,
                protected: false,
            })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::fs;

        #[test]
        fn replaced_parent_cannot_redirect_an_anchored_operation() {
            let root = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            fs::create_dir(root.path().join("parent")).unwrap();
            fs::write(root.path().join("parent/source"), "keep").unwrap();
            fs::write(outside.path().join("source"), "outside").unwrap();
            let identity = FileStamp::from_metadata(&fs::metadata(root.path()).unwrap());
            let filesystem = Filesystem::open(root.path(), &identity).unwrap();
            let entries = ["parent", "parent/source"]
                .into_iter()
                .map(|path| {
                    let (stamp, kind) = filesystem.inspect(path).unwrap();
                    (
                        path.into(),
                        ScanEntry {
                            path: path.into(),
                            kind,
                            stamp,
                            protected: false,
                        },
                    )
                })
                .collect();
            fs::rename(root.path().join("parent"), root.path().join("saved")).unwrap();
            std::os::unix::fs::symlink(outside.path(), root.path().join("parent")).unwrap();
            assert!(
                filesystem
                    .execute(
                        &Operation::MoveFile {
                            file_id: "parent/source".into(),
                            from: "parent/source".into(),
                            to: "parent/target".into()
                        },
                        &entries
                    )
                    .is_err()
            );
            assert_eq!(fs::read(outside.path().join("source")).unwrap(), b"outside");
            assert!(!outside.path().join("target").exists());
            assert_eq!(fs::read(root.path().join("saved/source")).unwrap(), b"keep");
        }

        #[test]
        fn atomic_rename_never_replaces_an_existing_destination() {
            let dir = tempfile::tempdir().unwrap();
            fs::write(dir.path().join("source"), "source bytes").unwrap();
            let identity = FileStamp::from_metadata(&fs::metadata(dir.path()).unwrap());
            let filesystem = Filesystem::open(dir.path(), &identity).unwrap();
            let (stamp, kind) = filesystem.inspect("source").unwrap();
            let entries = BTreeMap::from([(
                "id".into(),
                ScanEntry {
                    path: "source".into(),
                    kind,
                    stamp,
                    protected: false,
                },
            )]);
            // The destination appears after the source preflight.
            fs::write(dir.path().join("target"), "must survive").unwrap();
            assert!(
                filesystem
                    .execute(
                        &Operation::MoveFile {
                            file_id: "id".into(),
                            from: "source".into(),
                            to: "target".into()
                        },
                        &entries
                    )
                    .is_err()
            );
            assert_eq!(
                fs::read(dir.path().join("source")).unwrap(),
                b"source bytes"
            );
            assert_eq!(
                fs::read(dir.path().join("target")).unwrap(),
                b"must survive"
            );
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) use native::Filesystem;

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod unsupported {
    use crate::{
        error::{Result, fail},
        migration::{MigrationPlan, Operation},
        scan::{FileStamp, ScanEntry},
    };
    use std::{collections::BTreeMap, path::Path};
    pub(crate) struct Filesystem;
    impl Filesystem {
        pub(crate) fn open(_: &Path, _: &FileStamp) -> Result<Self> {
            Err(fail(
                "migration requires Linux or macOS descriptor-safe filesystem support",
            ))
        }
        pub(crate) fn preflight(&self, _: &MigrationPlan) -> Result<()> {
            Err(fail("unsupported platform"))
        }
        pub(crate) fn execute(
            &self,
            _: &Operation,
            _: &BTreeMap<String, ScanEntry>,
        ) -> Result<ScanEntry> {
            Err(fail("unsupported platform"))
        }
    }
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(crate) use unsupported::Filesystem;
