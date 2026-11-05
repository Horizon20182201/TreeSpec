//! Workspace state model (design §3.1). Pure functions over a [`Snapshot`].

use super::{rounds::Rounds, tree_index::TreeIndex};
use organizer::{
    migration::journal::{RecoveryReport, RunSummary},
    sync::DetailedStatus,
    workspace::Probe,
};
use std::{collections::HashSet, time::Instant};

pub struct RecoveryEntry {
    pub run: RunSummary,
    pub report: Result<RecoveryReport, String>,
}

/// Everything one refresh read from disk.
pub struct Snapshot {
    pub probe: Result<Probe, String>,
    /// Only attempted when no unfinished run or interruption blocks `status`.
    pub status: Option<Result<DetailedStatus, String>>,
    pub recoveries: Vec<RecoveryEntry>,
    pub rounds: Rounds,
    pub baseline: TreeIndex,
    pub excluded: HashSet<String>,
    /// `history` lines as (short hash, subject), newest first.
    pub history: Result<Vec<(String, String)>, String>,
    pub scanned_at: Instant,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Workspace {
    Failed(String),
    Uninitialized,
    Interrupted,
    Unfinished,
    Conflict,
    ScanIncomplete,
    GitPending,
    Changed,
    Clean,
}

impl Snapshot {
    pub fn detailed(&self) -> Option<&DetailedStatus> {
        self.status.as_ref().and_then(|s| s.as_ref().ok())
    }

    pub fn unfinished(&self) -> bool {
        self.probe
            .as_ref()
            .is_ok_and(|p| p.unfinished_runs().next().is_some())
    }

    /// Design §3.1, first match wins.
    pub fn classify(&self) -> Workspace {
        let probe = match &self.probe {
            Ok(probe) => probe,
            Err(error) => return Workspace::Failed(error.clone()),
        };
        if !probe.initialized {
            return Workspace::Uninitialized;
        }
        if !probe.recovery_files.is_empty() {
            return Workspace::Interrupted;
        }
        if self.unfinished() {
            return Workspace::Unfinished;
        }
        if probe.pending_marker.is_some() {
            return Workspace::Interrupted;
        }
        let status = match &self.status {
            Some(Ok(status)) => status,
            Some(Err(error)) => return Workspace::Failed(error.clone()),
            None => return Workspace::Failed("status was not read".into()),
        };
        let report = &status.report;
        if !status.baseline_conflicts.is_empty()
            || (report.scan_complete && !report.conflicts.is_empty())
        {
            return Workspace::Conflict;
        }
        if !report.scan_complete {
            return Workspace::ScanIncomplete;
        }
        if report.pending_git {
            return Workspace::GitPending;
        }
        if !(report.added.is_empty()
            && report.missing.is_empty()
            && report.moved.is_empty()
            && report.changed.is_empty()
            && report.unknown.is_empty())
        {
            return Workspace::Changed;
        }
        Workspace::Clean
    }

    pub fn has_pending(&self) -> bool {
        self.detailed().is_some_and(|s| !s.pending.is_empty())
    }
}

impl Workspace {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Failed(_) => "无法读取状态",
            Self::Uninitialized => "未初始化",
            Self::Interrupted => "元数据更新中断，需人工检查",
            Self::Unfinished => "迁移未完成",
            Self::Conflict => "基线冲突",
            Self::ScanIncomplete => "扫描不完整",
            Self::GitPending => "Git 待补记",
            Self::Changed => "磁盘有变化，待同步",
            Self::Clean => "已同步",
        }
    }

    pub fn can_draft(&self) -> bool {
        *self == Self::Clean
    }
    pub fn can_plan(&self) -> bool {
        *self == Self::Clean
    }
    pub fn can_sync(&self, pending: bool) -> bool {
        matches!(self, Self::GitPending | Self::Changed | Self::Clean) && !pending
    }
    pub fn can_archive(&self) -> bool {
        matches!(
            self,
            Self::Conflict | Self::ScanIncomplete | Self::GitPending | Self::Changed | Self::Clean
        )
    }
    /// Design §3.1: saving the edited `structure.tree` as a draft is allowed
    /// in the conflict state too.
    pub fn can_copy_baseline(&self) -> bool {
        matches!(self, Self::Clean | Self::Conflict)
    }

    /// Why planning/drafting is unavailable, for "受阻" labels.
    pub fn blocked_reason(&self) -> Option<&'static str> {
        (*self != Self::Clean).then_some(match self {
            Self::Changed => "受阻：磁盘有变化",
            Self::GitPending => "受阻：Git 待补记",
            Self::Conflict => "受阻：基线冲突",
            Self::ScanIncomplete => "受阻：扫描不完整",
            Self::Unfinished => "受阻：迁移未完成",
            _ => "受阻",
        })
    }
}
