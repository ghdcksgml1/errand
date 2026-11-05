//! Errand on macOS: the services behind `errand_core`'s ports and the AppKit shell.
#![cfg(target_os = "macos")]

pub mod apps;
pub mod http;
pub mod identity;
pub mod locale;
pub mod paths;
pub mod profile;
pub mod shell;
pub mod updater;
