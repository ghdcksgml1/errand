//! Command-line modes for checking what Errand reads, without opening its windows.

use crate::platform::{self, Platform};
use errand_core::format::local_zone;
use errand_core::sessions::{SessionRecords, VisibleSession};
use errand_core::usage::{self, UsageWindow};
use errand_core::{Language, Provider, Timestamp};
use errand_io::session_scan::SessionScanner;
use errand_io::stores;
use serde_json::{Map, Value, json};
use std::io::{self, Read};

/// More than Claude Code's status line input would ever be.
const STATUS_LINE_INPUT: u64 = 1024 * 1024;

/// Runs the mode the arguments ask for, returning its exit code; None when they ask for the app itself.
pub fn run(arguments: &[String], language: Language) -> Option<i32> {
    let has = |flag: &str| arguments.iter().any(|argument| argument == flag);
    if has("--claude-status-line") {
        Some(keep_status_line())
    } else if has("--usage") {
        Some(print_usage(language, has("--ask-claude")))
    } else if has("--sessions") {
        Some(print_sessions(language))
    } else if has("--inspect") {
        Some(print_inspection(language))
    } else {
        None
    }
}

/// The command Claude Code runs for its status line: keeps the limits on its input and prints nothing, so the line
/// stays empty. A status line has nowhere to show an error, and the next run tries again.
fn keep_status_line() -> i32 {
    let mut input = Vec::new();
    if io::stdin().take(STATUS_LINE_INPUT).read_to_end(&mut input).is_ok() {
        let _ = errand_io::claude::keep_status_line(&platform::claude_report(), &input, errand_io::now());
    }
    0
}

fn print(value: &Value) {
    println!("{}", serde_json::to_string_pretty(value).unwrap_or_default());
}

/// Whole seconds print as integers, as the services sent them.
fn seconds(value: f64) -> Value {
    if value.fract() == 0.0 && value.abs() < 9.0e15 { json!(value as i64) } else { json!(value) }
}

pub fn windows_json(windows: &[UsageWindow], language: Language) -> Value {
    windows
        .iter()
        .map(|window| {
            let title = language.window_title(&window.name);
            json!({"identifier": window.identifier, "title": title, "remainingPercent": window.remaining_percent, "resetsAt": window.resets_at.map(seconds)})
        })
        .collect()
}

/// Both services' usage as JSON; exits 1 when either read fails. `ask_claude` asks Claude Code too, as pressing
/// refresh does.
fn print_usage(language: Language, ask_claude: bool) -> i32 {
    let platform = Platform::current(language);
    let zone = local_zone();
    let mut results = Map::new();
    let mut failed = false;
    for provider in Provider::ALL {
        let read = if ask_claude && provider == Provider::Claude { platform.api.ask_claude_code() } else { platform.api.usage(provider) };
        let result = match read {
            Ok(reading) => json!({
                "windows": windows_json(&reading.windows, language),
                "summary": usage::summary(&reading.windows, language),
                "resets": usage::reset_details(&reading.windows, &zone, language),
                "reportedAt": reading.reported_at.map(iso),
            }),
            Err(error) => {
                failed = true;
                json!({"error": language.usage_error(&error)})
            }
        };
        results.insert(provider.storage_key().to_owned(), result);
    }
    print(&Value::Object(results));
    i32::from(failed)
}

fn iso(at: Timestamp) -> String {
    at.strftime("%Y-%m-%dT%H:%M:%SZ").to_string()
}

pub fn session_json(session: &VisibleSession) -> Value {
    let row = &session.row;
    let mut card = json!({
        "key": row.key, "source": row.provider.storage_key(), "title": row.title, "prompt": row.prompt, "promptAt": iso(row.prompt_at),
        "running": row.running, "aborted": row.aborted, "folder": row.folder, "url": row.url, "seenInApp": row.seen_in_app,
        "stalled": row.stalled, "seen": session.seen,
    });
    if let Some(finished) = row.finished_at {
        card["finishedAt"] = json!(iso(finished));
    }
    if let Some(result) = &row.result {
        card["result"] = json!(result);
    }
    if let Some(action) = &row.action {
        card["action"] = json!(action);
    }
    card
}

/// The task cards as the tray would show them now.
fn print_sessions(language: Language) -> i32 {
    let platform = Platform::current(language);
    let now = errand_io::now();
    // Reads the tray's records when the app has made them, and never creates them from here.
    let records = stores::read_session_records(&platform.session_tray, now).unwrap_or_else(|| SessionRecords::new(now));
    let visible = records.visible(SessionScanner::new(platform.sessions.clone()).scan(&records, now));
    print(&Value::Array(visible.iter().map(session_json).collect()));
    0
}

/// What Errand can find on this computer.
fn print_inspection(language: Language) -> i32 {
    let platform = Platform::current(language);
    let codex_pid = platform.running(Provider::Codex);
    let claude_pid = platform.running(Provider::Claude);
    print(&json!({
        "officialAppFound": platform.app_installed(Provider::Codex),
        "codexExecutableFound": platform.codex.executable.is_some(),
        "a": {"running": codex_pid.is_some(), "pid": codex_pid.unwrap_or(0), "authFilePresent": platform.codex.home.join("auth.json").exists()},
        "c": {
            "running": claude_pid.is_some(), "pid": claude_pid.unwrap_or(0),
            "claudeAppFound": platform.app_installed(Provider::Claude),
            "claudeCodeAccountPresent": errand_io::claude::account(&platform.home).email.is_some(),
        },
    }));
    0
}
