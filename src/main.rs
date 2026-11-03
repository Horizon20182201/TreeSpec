use std::{
    error::Error,
    fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
    process::ExitCode,
};

use clap::{Parser, Subcommand};
use organizer::tree::{
    compare_trees, parse_normalized_tree, parse_tree, parse_tree_with_base, render_tree,
};

#[derive(Parser)]
#[command(
    name = "organizer",
    version,
    about = "TreeSpec Organizer M0: structural metadata only"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create metadata directories and an initial draft; does not scan or create a baseline.
    Init {
        #[arg(long, default_value = ".")]
        root: PathBuf,
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
    /// Print a structural JSON diff, NOT an executable migration plan. Exit 2 if unresolved.
    Plan {
        #[arg(long)]
        from: PathBuf,
        #[arg(long)]
        to: PathBuf,
        /// Write to a new report file, otherwise print to stdout.
        #[arg(long)]
        out: Option<PathBuf>,
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
        Command::Init { root } => init(&root)?,
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
        Command::Plan { from, to, out } => {
            let old = parse_normalized_tree(&read_tree(&from)?)?;
            let new = parse_tree_with_base(&read_tree(&to)?, &old)?;
            let report = compare_trees(&old, &new)?;
            output(
                &(serde_json::to_string_pretty(&report)? + "\n"),
                out.as_deref(),
            )?;
            return Ok(if report.unresolved.is_empty() { 0 } else { 2 });
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

fn init(root: &Path) -> io::Result<()> {
    check_directory(root)?;
    let metadata = root.join(".organizer");
    // create_dir refuses an existing directory, file, or symlink. Never reuse state.
    let builder = fs::DirBuilder::new();
    #[cfg(unix)]
    let builder = {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = builder;
        builder.mode(0o700);
        builder
    };
    builder.create(&metadata)?;
    let result = (|| -> io::Result<()> {
        let mut log = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(metadata.join("init.log"))?;
        writeln!(
            log,
            "M0 metadata initialization started; no scan, baseline, or Git repository"
        )?;
        log.sync_all()?;
        for name in ["drafts", "plans", "state", "cache"] {
            builder.create(metadata.join(name))?;
            writeln!(log, "created directory {name}")?;
            log.sync_all()?;
        }
        output(
            "drafts/\nplans/\nstate/\ncache/\n*.log\n.env\n",
            Some(&metadata.join(".gitignore")),
        )?;
        output("!tree 1\n", Some(&metadata.join("drafts/initial.tree")))?;
        writeln!(log, "completed: .gitignore and drafts/initial.tree created")?;
        log.sync_all()
    })();
    if let Err(error) = result {
        return Err(io::Error::new(
            error.kind(),
            format!(
                "initialization incomplete at {}; inspect init.log before manual recovery: {error}",
                metadata.display()
            ),
        ));
    }
    println!(
        "Initialized M0 metadata at {}\nDraft: drafts/initial.tree\nNo filesystem scan, structure.tree baseline, SQLite, or Git repository was created (M1).",
        metadata.display()
    );
    Ok(())
}
