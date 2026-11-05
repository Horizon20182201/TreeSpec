//! The menu bar (design §4.3). Items that act on a workspace are forwarded to
//! the focused window as `menu` events; the page decides what they open.

use super::{AppState, quit, windows};
use organizer_gui::system;
use tauri::{
    AppHandle, Emitter, Manager, Wry,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu, SubmenuBuilder},
};

fn item(
    app: &AppHandle,
    id: &str,
    text: &str,
    accel: Option<&str>,
) -> tauri::Result<MenuItem<Wry>> {
    MenuItem::with_id(app, id, text, true, accel)
}

pub fn install(app: &AppHandle) -> tauri::Result<()> {
    app.set_menu(build(app)?).map(drop)
}

fn build(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let sep = || PredefinedMenuItem::separator(app);
    let app_menu = SubmenuBuilder::new(app, "TreeSpec Organizer")
        .item(&PredefinedMenuItem::about(
            app,
            Some("关于 TreeSpec Organizer"),
            None,
        )?)
        .item(&sep()?)
        .item(&item(app, "settings", "设置…", Some("CmdOrCtrl+,"))?)
        .item(&sep()?)
        .item(&PredefinedMenuItem::services(app, None)?)
        .item(&sep()?)
        .item(&PredefinedMenuItem::hide(app, None)?)
        .item(&PredefinedMenuItem::hide_others(app, None)?)
        .item(&PredefinedMenuItem::show_all(app, None)?)
        .item(&sep()?)
        // Custom, so ⌘Q passes the same guard as closing a window.
        .item(&item(
            app,
            "quit",
            "退出 TreeSpec Organizer",
            Some("CmdOrCtrl+Q"),
        )?)
        .build()?;
    let recent = app
        .state::<AppState>()
        .core
        .settings
        .lock()
        .unwrap()
        .recent
        .clone();
    let mut recent_menu = SubmenuBuilder::new(app, "最近打开");
    for (i, r) in recent.iter().enumerate() {
        let text = r.root.display().to_string();
        recent_menu = recent_menu.item(&item(app, &format!("recent-{i}"), &text, None)?);
    }
    let recent_menu = recent_menu.build()?;
    recent_menu.set_enabled(!recent.is_empty())?;
    let file = SubmenuBuilder::new(app, "文件")
        .item(&item(app, "open", "打开文件夹…", Some("CmdOrCtrl+O"))?)
        .item(&recent_menu)
        .item(&sep()?)
        .item(&PredefinedMenuItem::close_window(app, Some("关闭窗口"))?)
        .item(&item(
            app,
            "reveal-root",
            "在 Finder 中显示工作区",
            Some("Alt+CmdOrCtrl+R"),
        )?)
        .build()?;
    let edit = SubmenuBuilder::new(app, "编辑")
        .item(&PredefinedMenuItem::undo(app, None)?)
        .item(&PredefinedMenuItem::redo(app, None)?)
        .item(&sep()?)
        .item(&PredefinedMenuItem::cut(app, None)?)
        .item(&PredefinedMenuItem::copy(app, None)?)
        .item(&PredefinedMenuItem::paste(app, None)?)
        .item(&PredefinedMenuItem::select_all(app, None)?)
        .item(&sep()?)
        .item(&item(app, "find", "查找", Some("CmdOrCtrl+F"))?)
        .build()?;
    let view = SubmenuBuilder::new(app, "显示")
        .item(&item(app, "page-overview", "概览", Some("CmdOrCtrl+1"))?)
        .item(&item(app, "page-structure", "结构", Some("CmdOrCtrl+2"))?)
        .item(&item(app, "page-organize", "整理", Some("CmdOrCtrl+3"))?)
        .item(&item(app, "page-history", "历史", Some("CmdOrCtrl+4"))?)
        .item(&item(app, "page-recovery", "恢复", Some("CmdOrCtrl+5"))?)
        .item(&sep()?)
        .item(&item(
            app,
            "toggle-inspector",
            "显示/隐藏检查器",
            Some("Alt+CmdOrCtrl+0"),
        )?)
        .item(&item(
            app,
            "toggle-log",
            "显示/隐藏活动日志",
            Some("Shift+CmdOrCtrl+L"),
        )?)
        .item(&item(app, "toggle-changes", "只看变化", None)?)
        .item(&sep()?)
        .item(&item(app, "refresh", "重新扫描", Some("CmdOrCtrl+R"))?)
        .build()?;
    let organize = SubmenuBuilder::new(app, "整理")
        .item(&item(
            app,
            "new-chat",
            "新建 LLM 草案…",
            Some("CmdOrCtrl+N"),
        )?)
        .item(&item(
            app,
            "new-copy",
            "复制当前结构为草案…",
            Some("Shift+CmdOrCtrl+N"),
        )?)
        .item(&item(app, "save", "保存草案", Some("CmdOrCtrl+S"))?)
        .item(&sep()?)
        .item(&item(app, "plan", "生成计划…", Some("Alt+CmdOrCtrl+P"))?)
        .item(&item(app, "dry-run", "预览", Some("Alt+CmdOrCtrl+D"))?)
        // Moving files, bulk archiving and recovery have no shortcuts.
        .item(&item(app, "apply", "执行迁移…", None)?)
        .item(&item(app, "export", "导出脚本…", None)?)
        .item(&sep()?)
        .item(&item(app, "archive", "归档本轮…", None)?)
        .item(&item(app, "archive-all", "全部归档…", None)?)
        .build()?;
    let recovery = SubmenuBuilder::new(app, "恢复")
        .item(&item(app, "recheck", "重新检查", None)?)
        .item(&item(app, "finish-git", "补记 Git…", None)?)
        .item(&item(app, "mark-resolved", "标记已人工处理…", None)?)
        .build()?;
    let window = SubmenuBuilder::new(app, "窗口")
        .item(&PredefinedMenuItem::minimize(app, None)?)
        .item(&PredefinedMenuItem::maximize(app, None)?)
        .item(&sep()?)
        .item(&PredefinedMenuItem::fullscreen(app, None)?)
        .build()?;
    let help: Submenu<Wry> = SubmenuBuilder::new(app, "帮助")
        .item(&item(app, "help", "TreeSpec Organizer 帮助", None)?)
        .build()?;
    Menu::with_items(
        app,
        &[
            &app_menu, &file, &edit, &view, &organize, &recovery, &window, &help,
        ],
    )
}

fn focused(app: &AppHandle) -> Option<String> {
    app.webview_windows()
        .into_iter()
        .find(|(_, w)| w.is_focused().unwrap_or(false))
        .map(|(label, _)| label)
}

pub fn on_event(app: &AppHandle, event: MenuEvent) {
    let id = event.id().0.as_str();
    match id {
        "quit" => quit(app),
        "help" => {
            let _ = system::open_url(system::HELP_URL);
        }
        "open" => {
            let app = app.clone();
            std::thread::spawn(move || super::commands::choose_and_open(&app));
        }
        _ if id.starts_with("recent-") => {
            let index = id["recent-".len()..].parse::<usize>().unwrap_or(usize::MAX);
            let root = app
                .state::<AppState>()
                .core
                .settings
                .lock()
                .unwrap()
                .recent
                .get(index)
                .map(|r| r.root.clone());
            if let Some(root) = root {
                let _ = windows::open_root(app, &root);
            }
        }
        _ => {
            if let Some(label) = focused(app) {
                let _ = app.emit_to(&label, "menu", id.to_owned());
            }
        }
    }
}
