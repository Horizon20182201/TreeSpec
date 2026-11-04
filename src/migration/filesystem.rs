#[cfg(target_os = "linux")]
mod linux {
    use crate::migration::{
        MigrationPlan, Operation,
        plan::{parent, within},
    };
    use crate::{
        error::{Result, fail},
        scan::{FileStamp, ScanEntry},
        tree::NodeKind,
    };
    use rustix::fs::{Mode, OFlags, RenameFlags, ResolveFlags, mkdirat, openat2, renameat_with};
    use std::{
        collections::BTreeMap,
        fs::File,
        path::{Path, PathBuf},
    };

    const DIRECTORY: OFlags = OFlags::RDONLY
        .union(OFlags::DIRECTORY)
        .union(OFlags::CLOEXEC)
        .union(OFlags::NOFOLLOW);
    const RESOLVE: ResolveFlags = ResolveFlags::BENEATH
        .union(ResolveFlags::NO_SYMLINKS)
        .union(ResolveFlags::NO_XDEV);

    pub(crate) struct Filesystem {
        root: File,
        path: PathBuf,
        identity: FileStamp,
    }

    fn same_identity(a: &FileStamp, b: &FileStamp) -> bool {
        a.platform_file_key.is_some()
            && a.platform_file_key == b.platform_file_key
            && a.birthtime == b.birthtime
    }

    impl Filesystem {
        pub(crate) fn open(path: &Path, identity: &FileStamp) -> Result<Self> {
            // openat2 is required: older kernels/unsupported filesystems fail closed.
            let root = File::from(openat2(
                rustix::fs::CWD,
                path,
                DIRECTORY,
                Mode::empty(),
                ResolveFlags::NO_SYMLINKS,
            )?);
            if !same_identity(identity, &FileStamp::from_metadata(&root.metadata()?)) {
                return Err(fail("root identity changed"));
            }
            Ok(Self {
                root,
                path: path.to_owned(),
                identity: identity.clone(),
            })
        }

        fn open_relative(&self, path: &str, flags: OFlags) -> Result<File> {
            Ok(File::from(openat2(
                &self.root,
                if path.is_empty() { "." } else { path },
                flags,
                Mode::empty(),
                RESOLVE,
            )?))
        }

        fn inspect(&self, path: &str) -> Result<(FileStamp, NodeKind)> {
            let file =
                self.open_relative(path, OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC)?;
            let metadata = file.metadata()?;
            let kind = if metadata.is_file() {
                NodeKind::File
            } else if metadata.is_dir() {
                NodeKind::Directory
            } else {
                return Err(fail("source is not a regular file/directory"));
            };
            Ok((FileStamp::from_metadata(&metadata), kind))
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
            if !same_identity(expected, &FileStamp::from_metadata(&directory.metadata()?)) {
                return Err(fail("parent directory identity changed"));
            }
            if !path.is_empty() {
                match rustix::fs::openat(
                    &directory,
                    ".git",
                    OFlags::PATH | OFlags::NOFOLLOW,
                    Mode::empty(),
                ) {
                    Err(rustix::io::Errno::NOENT) => {}
                    Ok(_) => return Err(fail("parent became a protected Git project")),
                    Err(error) => return Err(error.into()),
                }
            }
            Ok(directory)
        }

        fn attached(&self, directory: &File, path: &str) -> Result<()> {
            let current = self.open_relative(path, DIRECTORY)?;
            if !same_identity(
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
                    mkdirat(&target_parent, target_name, Mode::RWXU)?;
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
                    renameat_with(
                        &source_parent,
                        from.rsplit('/').next().unwrap(),
                        &target_parent,
                        target_name,
                        RenameFlags::NOREPLACE,
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
                if !same_identity(&before.stamp, &stamp) {
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

#[cfg(target_os = "linux")]
pub(crate) use linux::Filesystem;

#[cfg(not(target_os = "linux"))]
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
                "M2 requires Linux openat2 and atomic no-replace rename",
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
#[cfg(not(target_os = "linux"))]
pub(crate) use unsupported::Filesystem;
