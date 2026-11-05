//! Welcome and workspace windows (design §4.1, §10).

use super::{AppState, guard_name, quit};
use organizer_gui::{
    dto::{GuardView, GuiEvent},
    open::{Opened, resolve},
    session::{Guard, Session},
    settings::WindowState,
    system,
};
use std::{
    hash::{Hash, Hasher},
    path::Path,
    sync::Arc,
};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, Window, WindowEvent};

pub const WELCOME: &str = "welcome";

fn label(root: &Path) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    root.hash(&mut hasher);
    format!("ws-{:016x}", hasher.finish())
}

fn builder<'a>(
    app: &'a AppHandle,
    label: &str,
    title: &str,
) -> WebviewWindowBuilder<'a, tauri::Wry, AppHandle> {
    let builder =
        WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into())).title(title);
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(18.0, 20.0));
    builder
}

pub fn welcome(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(WELCOME) {
        let _ = window.emit_to(WELCOME, "welcome", ());
        return window.set_focus();
    }
    builder(app, WELCOME, "TreeSpec Organizer")
        .inner_size(760.0, 520.0)
        .resizable(false)
        .build()
        .map(drop)
}

/// Opens a folder chosen by the user: a workspace window, or the welcome window
/// asking about a managed parent or the command line tools.
pub fn open_root(app: &AppHandle, path: &Path) -> Result<(), String> {
    if !system::command_line_tools() {
        welcome(app).map_err(|e| e.to_string())?;
        return Err("需要先安装 Xcode 命令行工具".into());
    }
    match resolve(path)? {
        Opened::Parent { root, parent } => {
            *app.state::<AppState>().pending_parent.lock().unwrap() = Some((root, parent));
            welcome(app).map_err(|e| e.to_string())
        }
        Opened::Ready(root) => workspace(app, &root),
    }
}

fn workspace(app: &AppHandle, root: &Path) -> Result<(), String> {
    let label = label(root);
    if let Some(window) = app.get_webview_window(&label) {
        return window.set_focus().map_err(|e| e.to_string());
    }
    let state = app.state::<AppState>();
    let size = state
        .core
        .settings
        .lock()
        .unwrap()
        .window(root)
        .map(|w| (w.width, w.height));
    let handle = app.clone();
    let target = label.clone();
    let sink = Arc::new(move |event: GuiEvent| {
        let _ = handle.emit_to(&target, "gui", event);
    });
    // The session exists before the page loads, so `snapshot` always finds it.
    // Check and insert under one lock: two opens of the same root race otherwise.
    let session = {
        let mut sessions = state.sessions.lock().unwrap();
        if sessions.contains_key(&label) {
            return Ok(());
        }
        let session = Session::spawn(root.to_owned(), state.core.clone(), sink, None);
        sessions.insert(label.clone(), session.clone());
        session
    };
    let name = root.file_name().map_or_else(
        || root.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let (width, height) = size.unwrap_or((1200.0, 780.0));
    let built = builder(app, &label, &name)
        .inner_size(width, height)
        .min_inner_size(960.0, 600.0)
        .build();
    if let Err(e) = built {
        let mut sessions = state.sessions.lock().unwrap();
        if sessions
            .get(&label)
            .is_some_and(|s| Arc::ptr_eq(s, &session))
        {
            sessions.remove(&label);
        }
        return Err(e.to_string());
    }
    {
        let mut settings = state.core.settings.lock().unwrap();
        settings.touch_recent(root, "已打开");
    }
    let _ = state.core.save();
    super::menu::install(app).map_err(|e| e.to_string())?;
    if let Some(welcome) = app.get_webview_window(WELCOME) {
        let _ = welcome.close();
    }
    Ok(())
}

fn workspace_windows(app: &AppHandle) -> usize {
    app.webview_windows()
        .keys()
        .filter(|l| l.starts_with("ws-"))
        .count()
}

pub fn on_event(window: &Window, event: &WindowEvent) {
    let app = window.app_handle();
    let state = app.state::<AppState>();
    let label = window.label().to_owned();
    match event {
        WindowEvent::CloseRequested { api, .. } => {
            let quitting = *state.quitting.lock().unwrap();
            let forced = state.forced.lock().unwrap().contains(&label);
            let session = state.sessions.lock().unwrap().get(&label).cloned();
            if let Some(session) = &session
                && !quitting
                && !forced
                && let Some(guard) = session.guard()
            {
                api.prevent_close();
                let _ = window.emit_to(
                    &label,
                    "guard",
                    GuardView {
                        guard: guard_name(guard).into(),
                        quit: false,
                    },
                );
                return;
            }
            if let Some(session) = session {
                remember(window, &session.root);
                if !quitting && workspace_windows(app) == 1 {
                    let _ = welcome(app);
                }
            }
        }
        WindowEvent::Destroyed => {
            state.sessions.lock().unwrap().remove(&label);
            state.forced.lock().unwrap().remove(&label);
        }
        WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) => {
            for path in paths.iter().filter(|p| p.is_dir()) {
                let _ = open_root(app, path);
            }
        }
        WindowEvent::Focused(true) => {
            let _ = window.emit_to(&label, "activated", ());
            let scan = state
                .core
                .settings
                .lock()
                .unwrap()
                .preferences
                .scan_on_activate;
            let session = state.sessions.lock().unwrap().get(&label).cloned();
            if let Some(session) = session.filter(|s| scan && s.busy().is_none()) {
                session.refresh();
            }
        }
        _ => {}
    }
}

fn remember(window: &Window, root: &Path) {
    let state = window.app_handle().state::<AppState>();
    let scale = window.scale_factor().unwrap_or(1.0);
    let Ok(size) = window.inner_size() else {
        return;
    };
    let size = size.to_logical::<f64>(scale);
    let mut settings = state.core.settings.lock().unwrap();
    let inspector = settings.window(root).is_none_or(|w| w.inspector);
    settings.set_window(WindowState {
        root: root.to_owned(),
        width: size.width,
        height: size.height,
        inspector,
    });
    drop(settings);
    let _ = state.core.save();
}

/// “仍然关闭/仍然退出” after a guard warning.
/// Closing one window during a migration is never forced: the migration would
/// keep running with no window to guard it. Quitting is (the journal recovers).
pub fn force_close(app: &AppHandle, label: &str, quit_after: bool) {
    let state = app.state::<AppState>();
    let applying = state
        .sessions
        .lock()
        .unwrap()
        .get(label)
        .is_some_and(|s| s.guard() == Some(Guard::Applying));
    if applying && !quit_after {
        return;
    }
    state.forced.lock().unwrap().insert(label.to_owned());
    if quit_after {
        quit(app);
    } else if let Some(window) = app.get_webview_window(label) {
        let _ = window.close();
    }
}
