use super::{MigrationPlan, Operation};
use crate::{
    error::{Result, fail},
    metadata,
    scan::{ScanSnapshot, scan},
};
use rusqlite::{Connection, OpenFlags, params};
use serde::Serialize;
use std::{fs, path::Path};

const DATABASE: &str = "state/operations.sqlite";

fn open(directory: &Path, write: bool) -> Result<Connection> {
    let path = directory.join(DATABASE);
    metadata::plain_file(&path)?;
    for suffix in ["-journal", "-wal", "-shm"] {
        if metadata::exists(&directory.join(format!("{DATABASE}{suffix}")))? {
            return Err(fail(
                "operation journal has SQLite recovery files; preserve them and inspect with SQLite before continuing",
            ));
        }
    }
    let flags = if write {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    } else {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    };
    let connection = Connection::open_with_flags(path, flags | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != 1 {
        return Err(fail("unsupported or incomplete operation journal schema"));
    }
    if write {
        connection.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;")?;
    }
    Ok(connection)
}

pub(crate) fn ensure_idle(directory: &Path) -> Result<()> {
    if !metadata::exists(&directory.join(DATABASE))? {
        return Ok(());
    }
    let connection = open(directory, false)?;
    let inconsistent: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM runs WHERE status='complete' AND
          (completed != (SELECT COUNT(*) FROM steps WHERE steps.run_id=runs.run_id)
           OR EXISTS(SELECT 1 FROM steps WHERE steps.run_id=runs.run_id AND status!='done')))",
        [],
        |row| row.get(0),
    )?;
    if inconsistent {
        return Err(fail(
            "inconsistent completed migration journal; preserve it for manual inspection",
        ));
    }
    let mut query = connection
        .prepare("SELECT run_id, status FROM runs WHERE status NOT IN ('complete', 'resolved') ORDER BY rowid")?;
    let mut rows = query.query([])?;
    if let Some(row) = rows.next()? {
        let run: String = row.get(0)?;
        let status: String = row.get(1)?;
        return Err(fail(format!(
            "unfinished migration {run} ({status}); inspect organizer recover {run}; automatic replay is disabled"
        )));
    }
    Ok(())
}

pub(crate) struct Journal {
    connection: Connection,
    pub run_id: String,
}

impl Journal {
    pub(crate) fn begin(directory: &Path, plan: &MigrationPlan) -> Result<Self> {
        let path = directory.join(DATABASE);
        if !metadata::exists(&path)? {
            metadata::write_snapshot(&path, None, "")?;
            let connection = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
            connection.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;
                BEGIN IMMEDIATE;
                CREATE TABLE runs (run_id TEXT PRIMARY KEY, plan_id TEXT NOT NULL UNIQUE, plan_json TEXT NOT NULL, status TEXT NOT NULL, error TEXT, completed INTEGER NOT NULL);
                CREATE TABLE steps (run_id TEXT NOT NULL, position INTEGER NOT NULL, operation TEXT NOT NULL, status TEXT NOT NULL, PRIMARY KEY(run_id, position));
                PRAGMA user_version=1; COMMIT;")?;
            fs::File::open(directory.join("state"))?.sync_all()?;
        }
        let mut connection = open(directory, true)?;
        let run_id = format!("run_{}", uuid::Uuid::new_v4());
        let transaction = connection.transaction()?;
        let plan_json = serde_json::to_string(plan).map_err(|e| fail(e.to_string()))?;
        transaction
            .execute(
                "INSERT INTO runs VALUES (?1,?2,?3,'pending',NULL,0)",
                params![run_id, plan.plan_id, plan_json],
            )
            .map_err(|e| fail(format!("cannot start run (a plan may only run once): {e}")))?;
        for (position, operation) in plan.operations.iter().enumerate() {
            transaction.execute(
                "INSERT INTO steps VALUES (?1,?2,?3,'pending')",
                params![
                    run_id,
                    position,
                    serde_json::to_string(operation).map_err(|e| fail(e.to_string()))?
                ],
            )?;
        }
        transaction.commit()?;
        Ok(Self { connection, run_id })
    }

    pub(crate) fn resume_metadata(directory: &Path, run_id: &str) -> Result<Self> {
        Ok(Self {
            connection: open(directory, true)?,
            run_id: run_id.into(),
        })
    }

    pub(crate) fn done(&mut self, position: usize) -> Result<()> {
        let transaction = self.connection.transaction()?;
        if transaction.execute(
            "UPDATE steps SET status='done' WHERE run_id=?1 AND position=?2 AND status='pending'",
            params![self.run_id, position],
        )? != 1
        {
            return Err(fail("inconsistent operation journal"));
        }
        if transaction.execute(
            "UPDATE runs SET completed=completed+1 WHERE run_id=?1 AND completed=?2",
            params![self.run_id, position],
        )? != 1
        {
            return Err(fail("inconsistent operation count in journal"));
        }
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn status(&self, status: &str, error: Option<&str>) -> Result<()> {
        if self.connection.execute(
            "UPDATE runs SET status=?2,error=?3 WHERE run_id=?1",
            params![self.run_id, status, error],
        )? != 1
        {
            return Err(fail("migration run not found"));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct StepReport {
    pub position: usize,
    pub operation: Operation,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct RecoveryReport {
    pub run_id: String,
    pub status: String,
    pub completed_steps: usize,
    pub error: Option<String>,
    pub steps: Vec<StepReport>,
    pub plan: MigrationPlan,
    pub current_scan: ScanSnapshot,
    pub metadata_update_pending: bool,
    pub guidance: String,
}

pub(crate) fn inspect(directory: &Path, run_id: &str) -> Result<RecoveryReport> {
    let connection = open(directory, false)?;
    let (plan, status, error, completed_steps): (String, String, Option<String>, usize) =
        connection.query_row(
            "SELECT plan_json,status,error,completed FROM runs WHERE run_id=?1",
            [run_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
    let plan: MigrationPlan = serde_json::from_str(&plan).map_err(|e| fail(e.to_string()))?;
    super::validate::document(&plan)?;
    let root = directory
        .parent()
        .ok_or_else(|| fail("missing managed root"))?;
    if plan.root != root {
        return Err(fail(
            "journal root does not match the selected managed root",
        ));
    }
    let mut query = connection
        .prepare("SELECT position,operation,status FROM steps WHERE run_id=?1 ORDER BY position")?;
    let mut rows = query.query([run_id])?;
    let mut steps = vec![];
    while let Some(row) = rows.next()? {
        steps.push(StepReport {
            position: row.get(0)?,
            operation: serde_json::from_str(&row.get::<_, String>(1)?)
                .map_err(|e| fail(e.to_string()))?,
            status: row.get(2)?,
        });
    }
    if steps.len() != plan.operations.len()
        || completed_steps > steps.len()
        || !matches!(
            status.as_str(),
            "pending" | "publishing" | "git_pending" | "failed" | "complete" | "resolved"
        )
        || (matches!(status.as_str(), "publishing" | "git_pending" | "complete")
            && completed_steps != steps.len())
        || steps.iter().enumerate().any(|(index, step)| {
            step.position != index
                || step.operation != plan.operations[index]
                || step.status
                    != if index < completed_steps {
                        "done"
                    } else {
                        "pending"
                    }
        })
    {
        return Err(fail(
            "inconsistent migration journal; preserve the database for manual inspection",
        ));
    }
    Ok(RecoveryReport {
        run_id: run_id.into(), status, completed_steps, error, steps, plan,
        current_scan: scan(root)?, metadata_update_pending: metadata::exists(&directory.join("state/pending.json"))?,
        guidance: "Read-only inspection; pending steps may already have reached disk before a crash. Compare IDs, source/target paths and the current scan; do not replay. Partial file operations require manual recovery; afterwards close the run with recover RUN_ID --mark-resolved and record the actual disk state with sync. If all moves and metadata publication completed and only Git is pending, use recover RUN_ID --finish-git.".into(),
    })
}

pub fn recover(root: &Path, run_id: &str, finish_git: bool) -> Result<RecoveryReport> {
    let root = metadata::canonical_root(root)?;
    let directory = root.join(".organizer");
    metadata::layout_files(&directory)?;
    let _lock = metadata::lock(&directory, finish_git)?;
    metadata::layout_files(&directory)?;
    let report = inspect(&directory, run_id)?;
    if finish_git {
        super::execute::finish_recovered_git(&directory, &report)?;
    }
    if finish_git {
        inspect(&directory, run_id)
    } else {
        Ok(report)
    }
}

/// One row of the operation journal, without the embedded plan.
#[derive(Clone, Debug, Serialize)]
pub struct RunSummary {
    pub run_id: String,
    pub plan_id: String,
    pub status: String,
    pub completed_steps: usize,
    pub total_steps: usize,
    pub error: Option<String>,
}

impl RunSummary {
    /// `complete` and `resolved` are terminal; any other status blocks
    /// status/sync/plan/apply until it is inspected and closed.
    pub fn is_finished(&self) -> bool {
        matches!(self.status.as_str(), "complete" | "resolved")
    }
}

pub(crate) fn summaries(directory: &Path) -> Result<Vec<RunSummary>> {
    if !metadata::exists(&directory.join(DATABASE))? {
        return Ok(vec![]);
    }
    let connection = open(directory, false)?;
    let mut query = connection.prepare(
        "SELECT run_id, plan_id, status, completed, error,
           (SELECT COUNT(*) FROM steps WHERE steps.run_id = runs.run_id)
         FROM runs ORDER BY rowid",
    )?;
    let rows = query.query_map([], |row| {
        Ok(RunSummary {
            run_id: row.get(0)?,
            plan_id: row.get(1)?,
            status: row.get(2)?,
            completed_steps: row.get(3)?,
            error: row.get(4)?,
            total_steps: row.get(5)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Read-only list of all journaled runs, oldest first.
pub fn list_runs(root: &Path) -> Result<Vec<RunSummary>> {
    let root = metadata::canonical_root(root)?;
    let directory = root.join(".organizer");
    metadata::layout_files(&directory)?;
    let _lock = metadata::lock(&directory, false)?;
    summaries(&directory)
}

/// Closes an interrupted run after the user recovered the files by hand.
///
/// This never touches managed files, the baseline, the index or Git. It only
/// records that the run will not be continued, so that `sync` can then record
/// the actual disk state. It refuses runs whose metadata publication started
/// (use `--finish-git` or manual metadata recovery for those), and the plan can
/// still never run again because plan IDs are unique in the journal.
pub fn mark_resolved(root: &Path, run_id: &str) -> Result<RecoveryReport> {
    let root = metadata::canonical_root(root)?;
    let directory = root.join(".organizer");
    metadata::layout_files(&directory)?;
    let _lock = metadata::lock(&directory, true)?;
    metadata::layout_files(&directory)?;
    let report = inspect(&directory, run_id)?;
    match report.status.as_str() {
        "complete" | "resolved" => {
            return Err(fail(format!(
                "run {run_id} is already {}; nothing to resolve",
                report.status
            )));
        }
        "git_pending" => {
            return Err(fail(
                "files and metadata of this run are published; use recover RUN_ID --finish-git instead of --mark-resolved",
            ));
        }
        _ => {}
    }
    if report.metadata_update_pending {
        return Err(fail(
            "metadata publication was interrupted (state/pending.json exists); back up .organizer and recover the metadata manually, --mark-resolved cannot repair it",
        ));
    }
    let saved = crate::sync::load(&root)?;
    if saved.generation != report.plan.base_generation
        || super::plan::digest(&saved.tree_text) != report.plan.base_tree_digest
    {
        return Err(fail(
            "the indexed baseline is no longer the run's base (metadata may be published); inspect manually or use --finish-git",
        ));
    }
    let note = match &report.error {
        Some(error) => format!("{error} [marked resolved by the user after manual recovery]"),
        None => "[marked resolved by the user after manual recovery]".to_owned(),
    };
    Journal::resume_metadata(&directory, run_id)?.status("resolved", Some(&note))?;
    metadata::log(
        &directory,
        &format!(
            "recover: run {run_id} marked resolved after manual recovery; record the disk state with sync"
        ),
    )?;
    inspect(&directory, run_id)
}
