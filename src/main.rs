use std::{
    error::Error,
    fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
    process::ExitCode,
};

use clap::{Parser, Subcommand};
use organizer::migration::{self, MigrationPlan};
use organizer::sync as management;
use organizer::tree::{
    compare_trees, parse_normalized_tree, parse_tree, parse_tree_with_base, render_tree,
};

#[derive(Parser)]
#[command(
    name = "organizer",
    version,
    about = "TreeSpec Organizer M2: reviewed plans and safe file migration"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scan files and initialize SQLite plus a dedicated structure-only Git repository.
    Init {
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Read-only scan showing added/missing/moved/changed/unknown/conflicts as JSON.
    Status {
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Record confirmed filesystem changes; never move or delete managed files.
    Sync {
        #[arg(long)]
        root: Option<PathBuf>,
        /// Record ambiguous identities as missing + new; never infer their moves.
        #[arg(long)]
        accept_unknown: bool,
    },
    /// Show the local structure snapshot history.
    History {
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Validate a TreeSpec document (missing IDs are allowed).
    Validate { tree: PathBuf },
    /// Render canonical TreeSpec and assign missing IDs; existing output is never overwritten.
    Normalize {
        tree: PathBuf,
        /// Inherit missing IDs only at the same path and kind in this normalized baseline.
        #[arg(long)]
        base: Option<PathBuf>,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Build a verified migration plan (or a document-only diff outside a managed root).
    Plan {
        /// Use this managed root; never infer a root from the input document paths.
        #[arg(long)]
        root: Option<PathBuf>,
        /// Explicitly compare documents without looking for a managed root.
        #[arg(long, conflicts_with = "root")]
        structural_only: bool,
        #[arg(long)]
        from: PathBuf,
        #[arg(long)]
        to: PathBuf,
        /// Write to a new report file, otherwise print to stdout.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Preview a plan by default; --yes explicitly authorizes filesystem changes.
    Apply {
        plan: PathBuf,
        #[arg(long)]
        root: Option<PathBuf>,
        #[arg(long, conflicts_with = "yes")]
        dry_run: bool,
        #[arg(long)]
        yes: bool,
    },
    /// Write a safely quoted script that previews a plan through organizer apply.
    ExportScript {
        plan: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Inspect a journaled run without replaying moves.
    Recover {
        run_id: String,
        #[arg(long)]
        root: Option<PathBuf>,
        /// Retry only the Git recording of an already published migration.
        #[arg(long)]
        finish_git: bool,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<u8, Box<dyn Error>> {
    match cli.command {
        Command::Init { root } => {
            let report = management::init(&root)?;
            output(&(serde_json::to_string_pretty(&report)? + "\n"), None)?;
        }
        Command::Status { root } => {
            let report = management::status(&management::resolve_root(root.as_deref())?)?;
            let blocked = !report.scan_complete
                || !report.conflicts.is_empty()
                || !report.unknown.is_empty()
                || report.pending_git;
            output(&(serde_json::to_string_pretty(&report)? + "\n"), None)?;
            return Ok(if blocked { 2 } else { 0 });
        }
        Command::Sync {
            root,
            accept_unknown,
        } => {
            let report =
                management::sync(&management::resolve_root(root.as_deref())?, accept_unknown)?;
            output(&(serde_json::to_string_pretty(&report)? + "\n"), None)?;
        }
        Command::History { root } => {
            output(
                &management::history(&management::resolve_root(root.as_deref())?)?,
                None,
            )?;
        }
        Command::Validate { tree } => {
            let parsed = parse_tree(&read_tree(&tree)?)?;
            println!("Valid TreeSpec v1: {} nodes", parsed.nodes.len());
        }
        Command::Normalize { tree, base, out } => {
            let source = read_tree(&tree)?;
            let parsed = match base {
                Some(base) => {
                    parse_tree_with_base(&source, &parse_normalized_tree(&read_tree(&base)?)?)?
                }
                None => parse_tree(&source)?,
            };
            output(&render_tree(&parsed)?, out.as_deref())?;
        }
        Command::Plan {
            from,
            to,
            out,
            root,
            structural_only,
        } => {
            let old = parse_normalized_tree(&read_tree(&from)?)?;
            let managed_root = if structural_only {
                None
            } else if let Some(root) = root {
                Some(management::resolve_root(Some(&root))?)
            } else {
                management::discover_root()?
            };
            let new = parse_tree_with_base(&read_tree(&to)?, &old)?;
            if let Some(root) = managed_root {
                let plan = migration::plan(&root, &old, &new)?;
                output(
                    &(serde_json::to_string_pretty(&plan)? + "\n"),
                    out.as_deref(),
                )?;
                return Ok(if plan.unresolved.is_empty() { 0 } else { 2 });
            }
            let report = compare_trees(&old, &new)?;
            output(
                &(serde_json::to_string_pretty(&report)? + "\n"),
                out.as_deref(),
            )?;
            return Ok(if report.unresolved.is_empty() { 0 } else { 2 });
        }
        Command::Apply {
            plan,
            root,
            dry_run: _,
            yes,
        } => {
            let root = management::resolve_root(root.as_deref())?;
            let plan: MigrationPlan = serde_json::from_str(&read_tree(&plan)?)?;
            if yes {
                let report = migration::apply_plan(&plan, &root)?;
                output(&(serde_json::to_string_pretty(&report)? + "\n"), None)?;
            } else {
                let report = migration::validate_plan(&plan, &root)?;
                output(&(serde_json::to_string_pretty(&report)? + "\n"), None)?;
                eprintln!(
                    "Dry-run only; no changes made. Execute with --yes after reviewing the plan."
                );
            }
        }
        Command::ExportScript { plan, out } => {
            output(&migration::export_script(&plan)?, Some(&out))?;
        }
        Command::Recover {
            run_id,
            root,
            finish_git,
        } => {
            let report = migration::journal::recover(
                &management::resolve_root(root.as_deref())?,
                &run_id,
                finish_git,
            )?;
            output(&(serde_json::to_string_pretty(&report)? + "\n"), None)?;
        }
    }
    Ok(0)
}

fn read_tree(path: &Path) -> io::Result<String> {
    fs::read_to_string(path)
        .map_err(|e| io::Error::new(e.kind(), format!("cannot read {}: {e}", path.display())))
}

fn output(contents: &str, path: Option<&Path>) -> io::Result<()> {
    match path {
        Some(path) => {
            let parent = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            check_directory(parent)?;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(|e| {
                    io::Error::new(
                        e.kind(),
                        format!("cannot create {} (output must be new): {e}", path.display()),
                    )
                })?;
            file.write_all(contents.as_bytes())?;
            file.sync_all()
        }
        None => io::stdout().lock().write_all(contents.as_bytes()),
    }
}

/// Reject existing symlink ancestors and lexical traversal for metadata writes.
/// This is not the concurrent, descriptor-relative migration executor of M2.
fn check_directory(path: &Path) -> io::Result<()> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut current = PathBuf::new();
    for component in absolute.components() {
        if component == Component::ParentDir {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "metadata output paths must not contain ..",
            ));
        }
        current.push(component);
        let metadata = fs::symlink_metadata(&current)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("not a plain directory: {}", current.display()),
            ));
        }
    }
    Ok(())
}
