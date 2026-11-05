//! Errand's rules, shared by every platform: what the usage numbers mean, when a limit reset,
//! which tasks to show and how. Nothing here touches files, the network or the OS.
#![forbid(unsafe_code)]

pub mod avatar;
pub mod format;
pub mod i18n;
pub mod icons;
pub mod json;
pub mod layout;
pub mod model;
pub mod ports;
pub mod provider;
pub mod reset_history;
pub mod rooms;
pub mod sessions;
pub mod theme;
pub mod update;
pub mod usage;
pub mod view;

pub use i18n::Language;
pub use jiff::{Timestamp, tz::TimeZone};
pub use provider::{PerProvider, Provider};
