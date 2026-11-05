//! Front-end logic shared by the TUI and the GUI (docs/GUI_DESIGN.md §3.2):
//! the refresh snapshot, workspace state and organizing rounds (TUI design §3),
//! review annotations, actions and their equivalent commands, the serial
//! worker, next-step suggestions, LLM configuration summary and text escaping.
//! Nothing here draws anything or depends on a UI toolkit.

pub mod action;
pub mod check;
pub mod llm_info;
pub mod load;
pub mod next;
pub mod recovery;
pub mod review;
pub mod rounds;
pub mod state;
pub mod text;
pub mod tree_index;
pub mod worker;
