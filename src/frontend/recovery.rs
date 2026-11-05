//! Recovery-page hints (TUI design §5.5).

use crate::{
    migration::{Operation, journal::RecoveryReport},
    scan::FileStamp,
};

fn same(a: &FileStamp, b: &FileStamp) -> bool {
    a.platform_file_key.is_some() && a.platform_file_key == b.platform_file_key
}

/// Where the planned object is now, compared with the run's current scan by
/// file key. A hint for manual checking only, never an identity decision.
pub fn disk(report: &RecoveryReport, op: &Operation) -> &'static str {
    let entries = &report.current_scan.entries;
    let at = |path: &str| entries.iter().find(|e| e.path == path);
    let (from, to) = match op {
        Operation::Mkdir { path, .. } => {
            return if at(path).is_some() {
                "在目标"
            } else {
                "不存在"
            };
        }
        Operation::MoveFile { from, to, .. } | Operation::MoveDirectory { from, to, .. } => {
            (from, to)
        }
    };
    let Some(before) = report.plan.preconditions.iter().find(|p| p.id == op.id()) else {
        return "?";
    };
    match (at(to), at(from)) {
        (Some(e), _) if same(&e.stamp, &before.entry.stamp) => "在目标",
        (_, Some(e)) if same(&e.stamp, &before.entry.stamp) => "在源",
        (Some(_), _) => "目标被占用",
        _ if entries.iter().any(|e| same(&e.stamp, &before.entry.stamp)) => "在别处",
        _ => "两端都不存在",
    }
}
