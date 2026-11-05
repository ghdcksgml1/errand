//! Reads what Codex and Claude leave on this computer, keeps Errand's own records, and calls the services
//! themselves, every call through [`api`]. Standard-library I/O only; anything the OS must do arrives
//! through `errand_core`'s ports.
#![forbid(unsafe_code)]

pub mod api;
pub mod claude;
pub mod codex;
pub mod rooms;
pub mod session_scan;
pub mod stores;

use errand_core::Timestamp;
use jiff::Unit;

/// The current time at the precision Errand's records keep, so a time saved and read back is the same time.
pub fn now() -> Timestamp {
    let now = Timestamp::now();
    now.round(Unit::Microsecond).unwrap_or(now)
}
