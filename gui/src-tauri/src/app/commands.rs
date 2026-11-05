//! IPC commands (design §3.4). Each is a thin wrapper over the core: the
//! WebView passes names and IDs; paths are resolved by `organizer_gui`.

use super::{AppState, windows};
use organizer_gui::{
    action::GuiAction,
    dto::{self, ActionResult, RecentView, SystemRequest, ViewModel, WindowInfo},
    session::RevealTarget,
    settings::{Preferences, WindowState},
    system,
};
use tauri::{AppHandle, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

type R<T> = Result<T, String>;

/// Runs the system “open” panel off the main thread, then opens the folder.
pub fn choose_and_open(app: &AppHandle) {
    if let Some(path) = app
        .dialog()
        .file()
        .set_title("打开文件夹")
        .blocking_pick_folder()
        && let Ok(path) = path.into_path()
    {
        let _ = windows::open_root(app, &path);
    }
}

#[tauri::command]
pub fn window_info(window: WebviewWindow, state: State<'_, AppState>) -> R<WindowInfo> {
    if let Ok(session) = state.session(window.label()) {
        let settings = state.core.settings.lock().unwrap();
        let inspector = settings.window(&session.root).is_none_or(|w| w.inspector);
        return Ok(WindowInfo::Workspace {
            root: session.root.display().to_string(),
            inspector,
        });
    }
    let settings = state.core.settings.lock().unwrap();
    let recent = settings
        .recent
        .iter()
        .enumerate()
        .map(|(i, r)| RecentView {
            index: u32::try_from(i).unwrap_or(0),
            name: r.root.file_name().map_or_else(
                || r.root.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            ),
            path: r.root.display().to_string(),
            state: r.state.clone(),
            opened_ms: r.opened_ms,
        })
        .collect();
    let pending = state.pending_parent.lock().unwrap().clone();
    Ok(WindowInfo::Welcome {
        recent,
        tools: system::command_line_tools(),
        child: pending.as_ref().map(|(c, _)| c.display().to_string()),
        parent: pending.map(|(_, p)| p.display().to_string()),
    })
}

#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> R<()> {
    choose_and_open(&app);
    Ok(())
}

#[tauri::command]
pub fn open_recent(app: AppHandle, state: State<'_, AppState>, index: u32) -> R<()> {
    let root = state
        .core
        .settings
        .lock()
        .unwrap()
        .recent
        .get(index as usize)
        .map(|r| r.root.clone());
    let root = root.ok_or("没有这一项")?;
    if !root.is_dir() {
        state
            .core
            .settings
            .lock()
            .unwrap()
            .recent
            .retain(|r| r.root != root);
        let _ = state.core.save();
        return Err(format!("{} 已不存在", root.display()));
    }
    windows::open_root(&app, &root)
}

/// `open = false` dismisses the “open the parent workspace” question.
#[tauri::command]
pub fn open_parent(app: AppHandle, state: State<'_, AppState>, open: bool) -> R<()> {
    let pending = state.pending_parent.lock().unwrap().take();
    match pending {
        Some((_, parent)) if open => windows::open_root(&app, &parent),
        _ => Ok(()),
    }
}

#[tauri::command]
pub fn snapshot(window: WebviewWindow, state: State<'_, AppState>) -> R<Option<ViewModel>> {
    Ok(state.session(window.label())?.view())
}

#[tauri::command]
pub fn refresh(window: WebviewWindow, state: State<'_, AppState>) -> R<()> {
    state.session(window.label())?.refresh();
    Ok(())
}

#[tauri::command]
pub async fn round_detail(
    window: WebviewWindow,
    state: State<'_, AppState>,
    name: String,
) -> R<dto::RoundDetail> {
    state.session(window.label())?.round_detail(&name)
}

#[tauri::command]
pub fn preview_scope(
    window: WebviewWindow,
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> R<dto::ScopeView> {
    state.session(window.label())?.preview_scope(&ids)
}

#[tauri::command]
pub async fn draft_text(
    window: WebviewWindow,
    state: State<'_, AppState>,
    name: String,
) -> R<dto::DraftTextView> {
    state.session(window.label())?.draft_text(&name)
}

#[tauri::command]
pub async fn check_draft(
    window: WebviewWindow,
    state: State<'_, AppState>,
    name: String,
    text: String,
) -> R<dto::DraftCheckView> {
    state.session(window.label())?.check_draft(&name, &text)
}

#[tauri::command]
pub async fn save_draft(
    window: WebviewWindow,
    state: State<'_, AppState>,
    name: String,
    digest: String,
    text: String,
) -> R<ActionResult> {
    state
        .session(window.label())?
        .save_draft(&name, &digest, &text)
}

#[tauri::command]
pub fn run(window: WebviewWindow, state: State<'_, AppState>, action: GuiAction) -> R<()> {
    state.session(window.label())?.run(action)
}

#[tauri::command]
pub fn command(window: WebviewWindow, state: State<'_, AppState>, action: GuiAction) -> R<String> {
    state.session(window.label())?.command(action)
}

#[tauri::command]
pub fn reveal(window: WebviewWindow, state: State<'_, AppState>, target: RevealTarget) -> R<()> {
    let path = state.session(window.label())?.reveal_path(&target)?;
    system::reveal(&path)
}

#[tauri::command]
pub fn settings(state: State<'_, AppState>) -> R<dto::SettingsView> {
    Ok(state.core.settings_view())
}

fn reemit(state: &AppState) {
    for session in state.sessions.lock().unwrap().values() {
        session.reemit();
    }
}

/// The page cannot choose an app to launch (only “选择应用…” can), and sending
/// the saved key to a new endpoint needs a native confirmation the WebView
/// cannot click (design §8.2).
#[tauri::command]
pub async fn set_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    mut prefs: Preferences,
) -> R<()> {
    let current = state.core.settings.lock().unwrap().preferences.clone();
    prefs.editor_app = prefs.editor_app.and(current.editor_app.clone());
    let url = |s: &str| s.trim().trim_end_matches('/').to_owned();
    let moved = url(&prefs.llm.base_url) != url(&current.llm.base_url);
    if moved && *state.core.key_saved.lock().unwrap() {
        use tauri_plugin_dialog::{MessageDialogButtons, MessageDialogKind};
        let shown = if prefs.llm.base_url.trim().is_empty() {
            "https://api.openai.com/v1"
        } else {
            prefs.llm.base_url.trim()
        };
        let ok = app
            .dialog()
            .message(format!(
                "之后的 LLM 请求和钥匙串中的 API Key 将发送到：\n{shown}"
            ))
            .title("更改 LLM 服务地址？")
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom(
                "更改地址".into(),
                "取消".into(),
            ))
            .blocking_show();
        if !ok {
            return Err("已取消：服务地址未更改".into());
        }
    }
    state.core.set_preferences(prefs)?;
    reemit(&state);
    Ok(())
}

#[tauri::command]
pub fn set_api_key(state: State<'_, AppState>, key: Option<String>) -> R<()> {
    state.core.set_api_key(key)?;
    reemit(&state);
    Ok(())
}

#[tauri::command]
pub async fn system(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, AppState>,
    request: SystemRequest,
) -> R<()> {
    match request {
        SystemRequest::InstallTools => system::install_command_line_tools(),
        SystemRequest::FilesAndFolders => system::open_url(system::FILES_AND_FOLDERS),
        SystemRequest::Help => system::open_url(system::HELP_URL),
        SystemRequest::Reauthorize => {
            let session = state.session(window.label())?;
            let picked = app
                .dialog()
                .file()
                .set_title("重新选择这个工作区以授予访问权限")
                .set_directory(&session.root)
                .blocking_pick_folder();
            if picked.is_some() {
                session.refresh();
            }
            Ok(())
        }
        SystemRequest::Inspector { open } => {
            let session = state.session(window.label())?;
            let mut settings = state.core.settings.lock().unwrap();
            let current = settings.window(&session.root).cloned();
            let (width, height) = current
                .as_ref()
                .map_or((1200.0, 780.0), |w| (w.width, w.height));
            settings.set_window(WindowState {
                root: session.root.clone(),
                width,
                height,
                inspector: open,
            });
            drop(settings);
            state.core.save()
        }
        SystemRequest::OpenWith { round } => {
            let session = state.session(window.label())?;
            let app_path = state
                .core
                .settings
                .lock()
                .unwrap()
                .preferences
                .editor_app
                .clone();
            let app_path = app_path.ok_or("先在“设置 › 通用”中选择外部编辑器")?;
            system::open_with(&app_path, &session.draft_path(&round)?)
        }
        SystemRequest::PickEditor => {
            let picked = app
                .dialog()
                .file()
                .set_title("选择外部编辑器")
                .set_directory("/Applications")
                .add_filter("应用程序", &["app"])
                .blocking_pick_file();
            if let Some(path) = picked.and_then(|p| p.into_path().ok()) {
                let mut prefs = state.core.settings.lock().unwrap().preferences.clone();
                prefs.editor_app = Some(path.display().to_string());
                state.core.set_preferences(prefs)?;
            }
            Ok(())
        }
    }
}

/// The editor reports unsaved changes so closing and quitting can ask first.
#[tauri::command]
pub fn set_guard(window: WebviewWindow, state: State<'_, AppState>, dirty: bool) -> R<()> {
    state.session(window.label())?.set_dirty(dirty);
    Ok(())
}

#[tauri::command]
pub fn force_close(app: AppHandle, window: WebviewWindow, quit: bool) -> R<()> {
    windows::force_close(&app, window.label(), quit);
    Ok(())
}
