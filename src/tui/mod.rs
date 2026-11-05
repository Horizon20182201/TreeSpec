//! `organizer tui`: an interactive front end over the CLI commands
//! (docs/TUI_DESIGN.md). It moves user files only through `apply_plan` and
//! leaves every safety check to the library.

mod action;
mod app;
mod llm_info;
mod load;
mod review;
mod rounds;
mod state;
mod text;
mod tree_index;
mod view;
mod worker;

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests;

use organizer::sync as management;
use ratatui::crossterm::{
    event::{self, Event as TermEvent, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use std::{
    error::Error,
    io::{IsTerminal, stdout},
    path::PathBuf,
    sync::mpsc::TryRecvError,
    time::Duration,
};

pub fn run(root: Option<PathBuf>) -> Result<u8, Box<dyn Error>> {
    if !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() {
        return Err(
            "organizer tui needs an interactive terminal; use the other commands in scripts".into(),
        );
    }
    // Same root selection as the other commands; an uninitialized directory
    // opens the initialization page instead of failing.
    let root = match root {
        Some(root) => management::resolve_root(Some(&root))?,
        None => match management::discover_root()? {
            Some(root) => root,
            None => management::resolve_root(Some(&std::env::current_dir()?))?,
        },
    };
    let (jobs, events) = worker::spawn(worker::Context {
        root: root.clone(),
        provider: Box::new(llm_info::provider),
    });
    let mut app = app::App::new(root, llm_info::detect());
    let mut terminal = ratatui::try_init()?;
    let result = (|| -> Result<(), Box<dyn Error>> {
        while !app.quit {
            for job in app.outbox.drain(..) {
                jobs.send(job)
                    .map_err(|_| "the worker stopped unexpectedly")?;
            }
            terminal.draw(|frame| view::draw(frame, &mut app))?;
            if event::poll(Duration::from_millis(200))?
                && let TermEvent::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
            {
                app.key(key);
            }
            loop {
                match events.try_recv() {
                    Ok(event) => app.handle(event),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        return Err("the worker stopped unexpectedly".into());
                    }
                }
            }
            if let Some(path) = app.pending_editor.take() {
                if let Err(error) = edit(&path) {
                    app.flash(format!("无法启动编辑器：{error}"), true);
                }
                terminal.clear()?;
                app.refresh();
            }
        }
        Ok(())
    })();
    ratatui::restore();
    result.map(|()| 0)
}

/// Suspends the TUI and runs `$VISUAL`/`$EDITOR`/`vi` on `path`. The editor
/// setting is split on whitespace; the path is a separate argument and never
/// passes through a shell.
fn edit(path: &std::path::Path) -> Result<(), Box<dyn Error>> {
    let setting = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "vi".into());
    let mut parts = setting.split_whitespace();
    let program = parts.next().unwrap_or("vi");
    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen)?;
    let status = std::process::Command::new(program)
        .args(parts)
        .arg(path)
        .status();
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    let status = status?;
    if !status.success() {
        return Err(format!("{program} exited with {status}").into());
    }
    Ok(())
}
