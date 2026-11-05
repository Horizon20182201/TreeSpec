use std::{
    error::Error,
    io::{self, BufRead, IsTerminal, Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{Parser, Subcommand};
use organizer::llm::{
    self, ChatOptions,
    provider::{OpenAiCompatible, ProviderConfig},
};
use organizer::migration::{self, MigrationPlan};
use organizer::sync as management;
use organizer::tree::{parse_normalized_tree, parse_tree, parse_tree_with_base, render_tree};

// The TUI belongs to the binary, not the library: it can only reach the
// library's public API, the same surface the CLI commands use.
#[cfg(feature = "tui")]
mod tui;

#[derive(Parser)]
#[command(
    name = "organizer",
    version,
    about = "TreeSpec Organizer M3: LLM-assisted drafts and safe reviewed migration"
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
    /// Propose a complete Tree draft via an LLM; never apply or commit it.
    Chat {
        /// One request, or read from stdin when omitted.
        prompt: Option<String>,
        #[arg(long)]
        root: Option<PathBuf>,
        /// Restrict editable context to these existing directory IDs (repeatable).
        #[arg(long = "scope")]
        scope_ids: Vec<String>,
        /// Allow a separate FileMapper call for semantic file placement.
        #[arg(long)]
        classify: bool,
        /// Opt into bounded excerpts of eligible text files for FileMapper.
        #[arg(long, requires = "classify")]
        read_content: bool,
        /// New draft path; defaults to .organizer/drafts/chat_UUID.tree.
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
        /// After manual review, accept pending LLM decisions or an edited draft.
        #[arg(long)]
        acknowledge_review: bool,
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
        #[arg(long, conflicts_with = "mark_resolved")]
        finish_git: bool,
        /// After recovering files by hand, close a run whose metadata was never
        /// published; then record the actual disk state with sync.
        #[arg(long)]
        mark_resolved: bool,
    },
    /// Move pending drafts/plans into .organizer/archive so sync can continue.
    Archive {
        #[arg(long)]
        root: Option<PathBuf>,
        /// drafts/NAME or plans/NAME entries; a draft keeps its review report.
        #[arg(required_unless_present = "all", conflicts_with = "all")]
        entries: Vec<String>,
        /// Archive every pending drafts/ and plans/ entry.
        #[arg(long)]
        all: bool,
    },
    /// Interactive terminal interface over these commands.
    #[cfg(feature = "tui")]
    Tui {
        #[arg(long)]
        root: Option<PathBuf>,
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
            if llm::has_review_marker(&source) || llm::review_path(&tree)?.exists() {
                return Err("LLM drafts are already normalized; edit the generated Tree directly and retain its review marker/report, then use plan --acknowledge-review after review".into());
            }
            let parsed = match base {
                Some(base) => {
                    parse_tree_with_base(&source, &parse_normalized_tree(&read_tree(&base)?)?)?
                }
                None => parse_tree(&source)?,
            };
            output(&render_tree(&parsed)?, out.as_deref())?;
        }
        Command::Chat {
            prompt,
            root,
            scope_ids,
            classify,
            read_content,
            out,
        } => {
            let provider = OpenAiCompatible::new(ProviderConfig::from_env()?)?;
            let root = management::resolve_root(root.as_deref())?;
            let prompt = match prompt {
                Some(prompt) => prompt,
                None => read_prompt()?,
            };
            let report = llm::chat_with_provider(
                &root,
                &provider,
                &ChatOptions {
                    prompt,
                    scope_ids,
                    classify,
                    read_content,
                    policy: Default::default(),
                },
                out.as_deref(),
            )?;
            output(&(serde_json::to_string_pretty(&report)? + "\n"), None)?;
            return Ok(if report.unresolved.is_empty() { 0 } else { 2 });
        }
        Command::Plan {
            from,
            to,
            out,
            root,
            structural_only,
            acknowledge_review,
        } => {
            let managed_root = if structural_only {
                None
            } else if let Some(root) = root {
                Some(management::resolve_root(Some(&root))?)
            } else {
                management::discover_root()?
            };
            let (text, unresolved) = match migration::plan_documents(
                managed_root.as_deref(),
                &from,
                &to,
                acknowledge_review,
            )? {
                migration::PlanOutcome::Migration(plan) => (
                    serde_json::to_string_pretty(&plan)?,
                    !plan.unresolved.is_empty(),
                ),
                migration::PlanOutcome::Diff(report) => (
                    serde_json::to_string_pretty(&report)?,
                    !report.unresolved.is_empty(),
                ),
            };
            output(&(text + "\n"), out.as_deref())?;
            return Ok(if unresolved { 2 } else { 0 });
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
            mark_resolved,
        } => {
            let root = management::resolve_root(root.as_deref())?;
            let report = if mark_resolved {
                migration::journal::mark_resolved(&root, &run_id)?
            } else {
                migration::journal::recover(&root, &run_id, finish_git)?
            };
            output(&(serde_json::to_string_pretty(&report)? + "\n"), None)?;
        }
        Command::Archive { root, entries, all } => {
            let root = management::resolve_root(root.as_deref())?;
            let entries = if all { vec![] } else { entries };
            let report = organizer::workspace::archive(&root, &entries)?;
            output(&(serde_json::to_string_pretty(&report)? + "\n"), None)?;
        }
        #[cfg(feature = "tui")]
        Command::Tui { root } => return tui::run(root),
    }
    Ok(0)
}

fn read_prompt() -> io::Result<String> {
    let mut prompt = String::new();
    let stdin = io::stdin();
    let mut input = stdin.lock().take(llm::MAX_PROMPT_BYTES as u64 + 1);
    if stdin.is_terminal() {
        eprintln!("Describe the desired organization (one line):");
        input.read_line(&mut prompt)?;
    } else {
        input.read_to_string(&mut prompt)?;
    }
    Ok(prompt)
}

fn read_tree(path: &Path) -> io::Result<String> {
    organizer::output::read_text(path)
}

fn output(contents: &str, path: Option<&Path>) -> io::Result<()> {
    match path {
        Some(path) => organizer::output::write_new(path, contents),
        None => io::stdout().lock().write_all(contents.as_bytes()),
    }
}
