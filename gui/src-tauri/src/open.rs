//! Choosing the managed root for a folder the user opened (design §4.1).

use organizer::sync::resolve_root;
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub enum Opened {
    /// `root` is managed, or uninitialized with no managed ancestor.
    Ready(PathBuf),
    /// `root` is inside the managed `parent`; offer to open the parent only.
    /// Initializing inside would make two roots manage the same files.
    Parent { root: PathBuf, parent: PathBuf },
}

/// The same root resolution as the CLI (`resolve_root`), then a search for a
/// managed ancestor.
pub fn resolve(path: &Path) -> Result<Opened, String> {
    let root = resolve_root(Some(path)).map_err(|e| e.to_string())?;
    if root.join(".organizer").exists() {
        return Ok(Opened::Ready(root));
    }
    let parent = root
        .ancestors()
        .skip(1)
        .find(|dir| dir.join(".organizer").is_dir())
        .map(Path::to_owned);
    Ok(match parent {
        Some(parent) => Opened::Parent { root, parent },
        None => Opened::Ready(root),
    })
}

/// Warning for roots whose initial scan may be slow (design §6.1).
pub fn init_warning(root: &Path) -> Option<String> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let volume_root = root.parent() == Some(Path::new("/Volumes")) || root == Path::new("/Users");
    if root == Path::new("/") || home.as_deref() == Some(root) || volume_root {
        Some("这是主目录、卷根目录或系统根目录，扫描可能很慢。建议选择更具体的文件夹。".into())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_managed_parents_and_warns_on_broad_roots() {
        let temp = tempfile::tempdir().unwrap();
        let inner = temp.path().join("Inbox/Scans");
        std::fs::create_dir_all(&inner).unwrap();
        let outer = resolve_root(Some(temp.path())).unwrap();
        assert_eq!(
            resolve(&inner).unwrap(),
            Opened::Ready(resolve_root(Some(&inner)).unwrap())
        );
        organizer::sync::init(temp.path()).unwrap();
        assert_eq!(resolve(temp.path()).unwrap(), Opened::Ready(outer.clone()));
        assert!(
            matches!(resolve(&inner).unwrap(), Opened::Parent { parent, .. } if parent == outer)
        );
        assert!(resolve(&temp.path().join("missing")).is_err());
        assert!(init_warning(Path::new("/")).is_some());
        assert!(init_warning(&outer).is_none());
    }
}
