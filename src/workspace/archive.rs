use super::{Artifact, ArtifactKind, artifacts};
use crate::{
    error::{Result, fail},
    metadata,
};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize)]
pub struct ArchiveReport {
    /// New directory under `.organizer/archive/`.
    pub archive: PathBuf,
    /// Moved entries as `drafts/NAME` / `plans/NAME`.
    pub moved: Vec<String>,
}

/// Moves pending drafts/plans into `.organizer/archive/<UTC time>-<label>/`
/// with no-replace renames, so they stop blocking `sync`. `selection` holds
/// `drafts/NAME` or `plans/NAME` entries; empty means everything pending.
/// A draft and its `NAME.review.json` report always move together. Nothing is
/// deleted, and managed files, the baseline, the index and Git are untouched.
pub fn archive(root: &Path, selection: &[String]) -> Result<ArchiveReport> {
    let root = metadata::canonical_root(root)?;
    let directory = root.join(".organizer");
    metadata::layout(&directory)?;
    let _lock = metadata::lock(&directory, true)?;
    metadata::layout(&directory)?;
    let pending = artifacts(&directory)?;
    let chosen = select(&pending, selection)?;
    if chosen.is_empty() {
        return Err(fail("nothing to archive: drafts/ and plans/ are empty"));
    }
    if let Some(entry) = chosen
        .iter()
        .find(|a| a.path.file_name().and_then(|n| n.to_str()).is_none())
    {
        return Err(fail(format!(
            "cannot archive non-UTF-8 name {:?}; move it out of {} by hand",
            entry.name, entry.directory
        )));
    }
    let base = directory.join("archive");
    if !metadata::exists(&base)? {
        metadata::create_directory(&base)?;
    }
    metadata::plain_directory(&base)?;
    let label = chosen
        .iter()
        .find(|a| a.directory == "drafts" && !a.name.ends_with(".review.json"))
        .map(|a| label(a.name.strip_suffix(".tree").unwrap_or(&a.name)))
        .unwrap_or_else(|| {
            if selection.is_empty() {
                "all".into()
            } else {
                "plans".into()
            }
        });
    let stamp = utc_stamp(SystemTime::now());
    let mut target = base.join(format!("{stamp}-{label}"));
    let mut counter = 2;
    while metadata::exists(&target)? {
        target = base.join(format!("{stamp}-{label}-{counter}"));
        counter += 1;
    }
    metadata::create_directory(&target)?;
    for parent in ["drafts", "plans"] {
        if chosen.iter().any(|a| a.directory == parent) {
            metadata::create_directory(&target.join(parent))?;
        }
    }
    let moved = move_entries(&directory, &target, &chosen)?;
    metadata::log(
        &directory,
        &format!(
            "archive: moved {} to {}",
            moved.join(", "),
            target.display()
        ),
    )?;
    Ok(ArchiveReport {
        archive: target,
        moved,
    })
}

fn select<'a>(pending: &'a [Artifact], selection: &[String]) -> Result<Vec<&'a Artifact>> {
    if selection.is_empty() {
        return Ok(pending.iter().collect());
    }
    let mut chosen: Vec<&Artifact> = vec![];
    for selector in selection {
        let entry = pending
            .iter()
            .find(|a| &a.selector() == selector)
            .ok_or_else(|| {
                fail(format!(
                    "{selector:?} is not a pending drafts/ or plans/ entry"
                ))
            })?;
        let mut names = vec![entry.name.clone()];
        // Keep generated drafts paired with their review report.
        if entry.directory == "drafts" {
            match entry.name.strip_suffix(".review.json") {
                Some(draft) => names.push(draft.to_owned()),
                None => names.push(format!("{}.review.json", entry.name)),
            }
        }
        for name in names {
            if let Some(found) = pending
                .iter()
                .find(|a| a.directory == entry.directory && a.name == name)
                && !chosen.iter().any(|c| std::ptr::eq(*c, found))
            {
                chosen.push(found);
            }
        }
    }
    Ok(chosen)
}

fn label(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    if cleaned.is_empty() {
        "draft".into()
    } else {
        cleaned
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn move_entries(directory: &Path, target: &Path, chosen: &[&Artifact]) -> Result<Vec<String>> {
    use crate::fsops;
    let mut moved = vec![];
    for parent in ["drafts", "plans"] {
        let entries: Vec<_> = chosen.iter().filter(|a| a.directory == parent).collect();
        if entries.is_empty() {
            continue;
        }
        let source = fsops::open_root(&directory.join(parent))?;
        let destination = fsops::open_root(&target.join(parent))?;
        for entry in entries {
            if entry.kind == ArtifactKind::Other {
                return Err(fail(format!(
                    "{} is a symlink or special file; move it by hand (already moved: {moved:?})",
                    entry.selector()
                )));
            }
            fsops::rename_noreplace(&source, &entry.name, &destination, &entry.name).map_err(
                |error| {
                    fail(format!(
                        "cannot archive {}: {error} (already moved: {moved:?})",
                        entry.selector()
                    ))
                },
            )?;
            moved.push(entry.selector());
        }
        source.sync_all()?;
        destination.sync_all()?;
    }
    std::fs::File::open(target)?.sync_all()?;
    Ok(moved)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn move_entries(_: &Path, _: &Path, _: &[&Artifact]) -> Result<Vec<String>> {
    Err(fail("archive requires Linux or macOS no-replace renames"))
}

/// `YYYYMMDDTHHMMSSZ` in UTC, used only to name archive directories.
pub(crate) fn utc_stamp(time: SystemTime) -> String {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    // Howard Hinnant's days-from-civil inverse for the proleptic Gregorian calendar.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}
