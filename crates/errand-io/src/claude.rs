//! Claude Code on this computer: who is signed in, from `~/.claude.json`, and the limits it last gave, through the
//! status line or `/usage`. Errand never reads Claude Code's login.

use crate::stores;
use errand_core::{Timestamp, json};
use serde_json::{Map, Value};
use std::fs;
use std::io;
use std::path::Path;

/// Who is signed in to Claude Code, from `~/.claude.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClaudeAccount {
    pub email: Option<String>,
    /// What the person asked Claude to call them.
    pub nickname: Option<String>,
}

pub fn account(home: &Path) -> ClaudeAccount {
    let config = json_file(&home.join(".claude.json"), 16 * 1024 * 1024);
    let field = |key: &str| {
        let value = config.as_ref()?.get("oauthAccount")?.get(key);
        json::text(value).filter(|text| json::utf16_len(text) < 256).map(str::to_owned)
    };
    ClaudeAccount { email: field("emailAddress"), nickname: field("displayName") }
}

/// Keeps the limits in one status line input, for the app to read. The record is made even when the input has
/// none, as at the start of a session, so the app can tell that the status line reaches it. Input that is not
/// JSON was not sent by Claude Code, and changes nothing.
pub fn keep_status_line(path: &Path, input: &[u8], now: Timestamp) -> io::Result<()> {
    let Ok(input) = serde_json::from_slice::<Value>(input) else { return Ok(()) };
    let mut report = stores::read_claude_report(path).unwrap_or_default();
    report.hear(&input, now);
    stores::save_claude_report(path, &report)
}

/// Keeps the limits in what `/usage` printed; false when it named none, and the record stays as it was.
pub fn keep_usage_text(path: &Path, text: &str, now: Timestamp) -> io::Result<bool> {
    let mut report = stores::read_claude_report(path).unwrap_or_default();
    if !report.hear_usage(text, now) {
        return Ok(false);
    }
    stores::save_claude_report(path, &report).map(|()| true)
}

/// A JSON object from a file no bigger than `limit`.
fn json_file(path: &Path, limit: u64) -> Option<Map<String, Value>> {
    if fs::metadata(path).ok()?.len() > limit {
        return None;
    }
    match serde_json::from_slice(&fs::read(path).ok()?).ok()? {
        Value::Object(object) => Some(object),
        _ => None,
    }
}
