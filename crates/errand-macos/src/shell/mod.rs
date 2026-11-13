//! The AppKit shell: Errand's menu bar item, floating widget and windows, drawn from `errand_core`'s view
//! models. Everything here runs on the main thread.

mod app;
mod avatar_editor;
pub mod battery;
mod compact;
mod dashboard;
pub mod settings;
mod details;
mod dock;
pub mod kit;
pub mod marks;
pub mod panel;
pub mod preview;
mod rooms;
pub mod spinner;
pub mod status;
pub mod task_drop;
mod task_form;
mod tray;
mod widget;

pub use app::{MacShell, MainPoster, migrate_legacy_records, run, store_preferences, stored_preferences, take_over};
pub use widget::Widget;
