//! Descriptor-relative filesystem primitives shared by scanning, migration and
//! bounded LLM content reads.
//!
//! Linux resolves paths with `openat2` and its `BENEATH`/`NO_SYMLINKS`/`NO_XDEV`
//! flags. macOS has no `openat2`; this module builds the same guarantees from a
//! component-wise `openat` walk: every component is opened with `O_NOFOLLOW`,
//! every directory must stay on the root's device, and file operations only use
//! leaf names relative to descriptors that were already checked.
//!
//! Callers still verify object identity before and after an operation. The
//! helpers here only ensure that a path cannot leave the managed root, follow a
//! symlink, or cross a mount boundary while it is being resolved.

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod supported {
    use crate::{
        error::{Result, fail},
        scan::FileStamp,
        tree::NodeKind,
    };
    use rustix::fs::{AtFlags, Mode, OFlags, RenameFlags, mkdirat, openat, renameat_with, statat};
    use std::{ffi::OsStr, fs::File, path::Path};

    #[cfg(target_os = "linux")]
    use rustix::fs::{ResolveFlags, openat2};

    #[cfg(target_os = "macos")]
    use std::os::unix::fs::MetadataExt;

    /// A directory descriptor that cannot itself be a symlink.
    pub(crate) const DIRECTORY: OFlags = OFlags::RDONLY
        .union(OFlags::DIRECTORY)
        .union(OFlags::CLOEXEC)
        .union(OFlags::NOFOLLOW);

    const READ: OFlags = OFlags::RDONLY
        .union(OFlags::NOFOLLOW)
        .union(OFlags::NONBLOCK)
        .union(OFlags::CLOEXEC);

    /// Metadata-only handle. Linux has `O_PATH`; macOS opens the object
    /// read-only without blocking on FIFOs or following symlinks.
    #[cfg(target_os = "linux")]
    const METADATA: OFlags = OFlags::PATH.union(OFlags::NOFOLLOW).union(OFlags::CLOEXEC);

    #[cfg(target_os = "linux")]
    const RESOLVE: ResolveFlags = ResolveFlags::BENEATH
        .union(ResolveFlags::NO_SYMLINKS)
        .union(ResolveFlags::NO_XDEV);

    pub(crate) fn same_identity(a: &FileStamp, b: &FileStamp) -> bool {
        a.platform_file_key.is_some()
            && a.platform_file_key == b.platform_file_key
            && a.birthtime == b.birthtime
    }

    fn validate_relative(path: &str) -> Result<()> {
        if path.contains('\0')
            || path
                .split('/')
                .any(|part| part.is_empty() || matches!(part, "." | "..") || part.contains('\0'))
        {
            return Err(fail(format!("unsafe relative path: {path:?}")));
        }
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn split_parent(path: &str) -> (&str, &str) {
        path.rsplit_once('/')
            .map_or(("", path), |(parent, name)| (parent, name))
    }

    fn kind(metadata: &std::fs::Metadata) -> Result<NodeKind> {
        if metadata.is_file() {
            Ok(NodeKind::File)
        } else if metadata.is_dir() {
            Ok(NodeKind::Directory)
        } else {
            Err(fail("source is not a regular file/directory"))
        }
    }

    /// Opens the managed root without following a final symlink. Linux also
    /// rejects symlink ancestors through `openat2(NO_SYMLINKS)`; on macOS the
    /// canonicalized root is opened no-follow and its identity is verified by
    /// the caller.
    pub(crate) fn open_root(path: &Path) -> Result<File> {
        #[cfg(target_os = "linux")]
        {
            Ok(File::from(openat2(
                rustix::fs::CWD,
                path,
                DIRECTORY,
                Mode::empty(),
                ResolveFlags::NO_SYMLINKS,
            )?))
        }
        #[cfg(target_os = "macos")]
        {
            // The caller canonicalizes the root and verifies its identity
            // immediately afterwards, so following an ancestor link while
            // opening cannot switch to a different managed directory.
            Ok(File::from(rustix::fs::open(
                path,
                DIRECTORY,
                Mode::empty(),
            )?))
        }
    }

    /// Resolves `relative` strictly beneath `root` and opens it with `flags`.
    /// `relative` may be empty for the root itself.
    pub(crate) fn open_beneath(root: &File, relative: &str, flags: OFlags) -> Result<File> {
        if relative.is_empty() {
            return Ok(File::from(openat(root, ".", flags, Mode::empty())?));
        }
        validate_relative(relative)?;
        #[cfg(target_os = "linux")]
        {
            Ok(File::from(openat2(
                root,
                relative,
                flags,
                Mode::empty(),
                RESOLVE,
            )?))
        }
        #[cfg(target_os = "macos")]
        {
            let root_device = root.metadata()?.dev();
            let mut directory = root.try_clone()?;
            let mut parts = relative.split('/').peekable();
            while let Some(part) = parts.next() {
                let leaf = parts.peek().is_none();
                let next = if leaf {
                    File::from(openat(
                        &directory,
                        part,
                        flags.union(OFlags::NOFOLLOW),
                        Mode::empty(),
                    )?)
                } else {
                    File::from(openat(&directory, part, DIRECTORY, Mode::empty())?)
                };
                if next.metadata()?.dev() != root_device {
                    return Err(fail(format!(
                        "path crosses a filesystem boundary: {relative:?}"
                    )));
                }
                directory = next;
            }
            Ok(directory)
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn open_directory_beneath(root: &File, relative: &str) -> Result<File> {
        open_beneath(root, relative, DIRECTORY)
    }

    pub(crate) fn open_read_beneath(root: &File, relative: &str) -> Result<File> {
        open_beneath(root, relative, READ)
    }

    /// Opens a direct child directory of an already anchored descriptor.
    pub(crate) fn open_child_directory(parent: &File, name: &OsStr) -> Result<File> {
        Ok(File::from(openat(parent, name, DIRECTORY, Mode::empty())?))
    }

    /// Inspects an object beneath the root without following symlinks and
    /// returns its metadata stamp. `relative` may be empty for the root.
    pub(crate) fn inspect_beneath(root: &File, relative: &str) -> Result<(FileStamp, NodeKind)> {
        if relative.is_empty() {
            let metadata = root.metadata()?;
            return Ok((FileStamp::from_metadata(&metadata), kind(&metadata)?));
        }
        validate_relative(relative)?;
        #[cfg(target_os = "linux")]
        {
            let file = open_beneath(root, relative, METADATA)?;
            let metadata = file.metadata()?;
            Ok((FileStamp::from_metadata(&metadata), kind(&metadata)?))
        }
        #[cfg(target_os = "macos")]
        {
            let (parent, name) = split_parent(relative);
            let directory = open_directory_beneath(root, parent)?;
            let stat = statat(&directory, name, AtFlags::SYMLINK_NOFOLLOW)?;
            let kind = match rustix::fs::FileType::from_raw_mode(stat.st_mode) {
                rustix::fs::FileType::RegularFile => NodeKind::File,
                rustix::fs::FileType::Directory => NodeKind::Directory,
                _ => return Err(fail("source is not a regular file/directory")),
            };
            Ok((stamp_from_stat(&stat), kind))
        }
    }

    /// Reports whether a direct child exists, without following a symlink.
    pub(crate) fn exists_child(parent: &File, name: &str) -> Result<bool> {
        match statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(_) => Ok(true),
            Err(rustix::io::Errno::NOENT) => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    /// Atomically renames a leaf without replacing an existing destination.
    /// Both platforms provide a no-replace primitive (`RENAME_NOREPLACE` /
    /// `RENAME_EXCL`); there is deliberately no overwrite fallback.
    pub(crate) fn rename_noreplace(
        source_directory: &File,
        source: &str,
        target_directory: &File,
        target: &str,
    ) -> Result<()> {
        renameat_with(
            source_directory,
            source,
            target_directory,
            target,
            RenameFlags::NOREPLACE,
        )?;
        Ok(())
    }

    /// Creates a directory with mode `0700` (subject to the process umask).
    pub(crate) fn create_directory(directory: &File, name: &str) -> Result<()> {
        mkdirat(directory, name, Mode::RWXU)?;
        Ok(())
    }

    /// Builds the same stamp as [`FileStamp::from_metadata`] from a `fstatat`
    /// result, including macOS birthtime. Used by the metadata-only scan path.
    #[cfg(target_os = "macos")]
    pub(crate) fn stamp_from_stat(stat: &rustix::fs::Stat) -> FileStamp {
        use std::time::{Duration, UNIX_EPOCH};

        fn timestamp(seconds: i64, nanos: i64) -> String {
            let total = i128::from(seconds) * 1_000_000_000 + i128::from(nanos);
            let time = if total >= 0 {
                UNIX_EPOCH + Duration::from_nanos(total as u64)
            } else {
                UNIX_EPOCH - Duration::from_nanos((-total) as u64)
            };
            match time.duration_since(UNIX_EPOCH) {
                Ok(value) => format!("{}:{:09}", value.as_secs(), value.subsec_nanos()),
                Err(value) => format!(
                    "-{}:{:09}",
                    value.duration().as_secs(),
                    value.duration().subsec_nanos()
                ),
            }
        }

        FileStamp {
            platform_file_key: Some(format!("{}:{}", stat.st_dev as u64, stat.st_ino)),
            size: stat.st_size.max(0) as u64,
            mtime: Some(timestamp(stat.st_mtime, stat.st_mtime_nsec)),
            birthtime: Some(timestamp(stat.st_birthtime, stat.st_birthtime_nsec)),
            // Keep the same unpadded representation as FileStamp::from_metadata.
            ctime: Some(format!("{}:{}", stat.st_ctime, stat.st_ctime_nsec)),
            links: stat.st_nlink as u64,
        }
    }

    #[cfg(all(test, target_os = "macos"))]
    mod tests {
        use super::*;
        use std::fs;

        #[test]
        fn stat_stamp_matches_opened_metadata_for_files_and_directories() {
            let temp = tempfile::tempdir().unwrap();
            fs::write(temp.path().join("file"), "contents").unwrap();
            fs::create_dir(temp.path().join("directory")).unwrap();
            let parent = File::open(temp.path()).unwrap();
            for name in ["file", "directory"] {
                let stat = statat(&parent, name, AtFlags::SYMLINK_NOFOLLOW).unwrap();
                let from_stat = stamp_from_stat(&stat);
                let from_metadata = FileStamp::from_metadata(
                    &fs::symlink_metadata(temp.path().join(name)).unwrap(),
                );
                assert_eq!(
                    from_stat.ctime,
                    Some(format!("{}:{}", stat.st_ctime, stat.st_ctime_nsec)),
                    "ctime must stay unpadded like FileStamp::from_metadata"
                );
                assert_eq!(from_stat, from_metadata, "{name}");
            }
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) use supported::*;
