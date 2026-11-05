//! UI-independent core of the TreeSpec Organizer GUI (docs/GUI_DESIGN.md §3.3).
//!
//! Everything here builds and tests without Tauri: sessions (one worker per
//! window), the command whitelist that turns names into paths, the view models
//! sent to the WebView, settings and the Keychain. The Tauri shell in
//! `src/app/` (feature `app`) only wires these to windows, menus and IPC.

pub mod action;
pub mod dto;
pub mod open;
pub mod secrets;
pub mod session;
pub mod settings;
pub mod system;
pub mod token;

#[cfg(test)]
mod tests;

/// Bundle identifier; TCC grants and the Keychain item are bound to it.
pub const BUNDLE_ID: &str = "io.github.horizon20182201.treespec-organizer";
