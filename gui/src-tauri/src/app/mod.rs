//! Tauri wiring: windows, menus, IPC commands and the macOS lifecycle
//! (design §3.3, §4, §8.3, §10). No organizing logic lives here.

mod commands;
mod menu;
mod windows;

use organizer_gui::{
    dto::GuardView,
    secrets::Keychain,
    session::{AppCore, Guard, Session},
};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::{Emitter, Manager, RunEvent};

pub struct AppState {
    pub core: Arc<AppCore>,
    /// Window label → session.
    pub sessions: Mutex<HashMap<String, Arc<Session>>>,
    /// Labels the user chose to close despite a guard.
    pub forced: Mutex<HashSet<String>>,
    pub quitting: Mutex<bool>,
    /// A folder inside a managed root, waiting for “open the parent”.
    pub pending_parent: Mutex<Option<(PathBuf, PathBuf)>>,
}

impl AppState {
    pub fn session(&self, label: &str) -> Result<Arc<Session>, String> {
        self.sessions
            .lock()
            .unwrap()
            .get(label)
            .cloned()
            .ok_or_else(|| "这个窗口没有打开工作区".to_owned())
    }
}

pub fn guard_name(guard: Guard) -> &'static str {
    match guard {
        Guard::Applying => "applying",
        Guard::Chatting => "chatting",
        Guard::Unsaved => "unsaved",
    }
}

/// ⌘Q and “仍然退出”: asks the first guarded window, otherwise quits.
pub fn quit(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    let forced = state.forced.lock().unwrap().clone();
    let guarded = state
        .sessions
        .lock()
        .unwrap()
        .iter()
        .filter(|(label, _)| !forced.contains(*label))
        .find_map(|(label, s)| s.guard().map(|g| (label.clone(), g)));
    if let Some((label, guard)) = guarded {
        if let Some(window) = app.get_webview_window(&label) {
            let _ = window.set_focus();
        }
        let _ = app.emit_to(
            &label,
            "guard",
            GuardView {
                guard: guard_name(guard).into(),
                quit: true,
            },
        );
        return;
    }
    let roots: Vec<PathBuf> = state
        .sessions
        .lock()
        .unwrap()
        .values()
        .map(|s| s.root.clone())
        .collect();
    state.core.settings.lock().unwrap().open = roots;
    let _ = state.core.save();
    *state.quitting.lock().unwrap() = true;
    app.exit(0);
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::window_info,
            commands::pick_folder,
            commands::open_recent,
            commands::open_parent,
            commands::snapshot,
            commands::refresh,
            commands::round_detail,
            commands::preview_scope,
            commands::draft_text,
            commands::check_draft,
            commands::save_draft,
            commands::run,
            commands::command,
            commands::reveal,
            commands::settings,
            commands::set_settings,
            commands::set_api_key,
            commands::system,
            commands::set_guard,
            commands::force_close,
        ])
        .setup(|app| {
            let dir = app.path().app_config_dir()?;
            let core = AppCore::new(dir.join("settings.json"), Box::new(Keychain));
            let reopen = {
                let settings = core.settings.lock().unwrap();
                if settings.preferences.reopen_last {
                    settings.open.clone()
                } else {
                    vec![]
                }
            };
            app.manage(AppState {
                core,
                sessions: Mutex::default(),
                forced: Mutex::default(),
                quitting: Mutex::new(false),
                pending_parent: Mutex::default(),
            });
            menu::install(app.handle())?;
            let mut opened = false;
            for root in reopen {
                opened |= windows::open_root(app.handle(), &root).is_ok();
            }
            if !opened {
                windows::welcome(app.handle())?;
            }
            Ok(())
        })
        .on_window_event(windows::on_event)
        .on_menu_event(menu::on_event)
        .build(tauri::generate_context!())
        .expect("cannot start TreeSpec Organizer")
        .run(|app, event| match event {
            // Logout, shutdown or the last window: never while a migration runs.
            RunEvent::ExitRequested { api, .. } => {
                let state = app.state::<AppState>();
                if *state.quitting.lock().unwrap() {
                    return;
                }
                let applying = state
                    .sessions
                    .lock()
                    .unwrap()
                    .values()
                    .any(|s| s.guard() == Some(Guard::Applying));
                if applying {
                    api.prevent_exit();
                    quit(app);
                }
            }
            #[cfg(target_os = "macos")]
            RunEvent::Opened { urls } => {
                for url in urls {
                    if let Ok(path) = url.to_file_path() {
                        let _ = windows::open_root(app, &path);
                    }
                }
            }
            #[cfg(target_os = "macos")]
            RunEvent::Reopen {
                has_visible_windows: false,
                ..
            } => {
                let _ = windows::welcome(app);
            }
            _ => {}
        });
}
