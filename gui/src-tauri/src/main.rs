//! TreeSpec Organizer for macOS. The Tauri shell is in `app/`; everything it
//! calls is in the `organizer_gui` library.

mod app;

fn main() {
    app::run();
}
