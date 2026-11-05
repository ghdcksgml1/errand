//! Errand's own records on disk: reset history, the tray's checked and opened marks, the work rooms the person
//! is in, and the limits Claude Code last reported. Binary property lists in the shape the Objective-C build wrote,
//! so each build reads what the other saved; binary also keeps sub-second times, which XML would round down to
//! before a request.

use errand_core::Timestamp;
use errand_core::reset_history::ResetHistory;
use errand_core::rooms::Visibility;
use errand_core::sessions::{SessionRecord, SessionRecords};
use errand_core::usage::claude::{self, ClaudeReport};
use jiff::Unit;
use plist::{Date, Dictionary, Value};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;
use std::time::SystemTime;

/// Property-list dates are seconds since 2001 as doubles, which hold microseconds but not nanoseconds; rounding
/// to the microsecond gives back the time that was saved.
fn timestamp(value: Option<&Value>) -> Option<Timestamp> {
    Timestamp::try_from(SystemTime::from(value?.as_date()?)).ok()?.round(Unit::Microsecond).ok()
}

fn date(at: Timestamp) -> Value {
    Value::Date(Date::from(SystemTime::from(at)))
}

fn seconds(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Integer(integer) => integer.as_signed().map(|integer| integer as f64),
        Value::Real(real) => Some(*real),
        _ => None,
    }
}

/// Writes through a temporary file, so a crash never leaves half a record.
fn write_atomically(path: &Path, value: &Value) -> io::Result<()> {
    let mut data = Vec::new();
    value.to_writer_binary(&mut data).map_err(io::Error::other)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let temporary = path.with_file_name(format!(".{name}.{}", std::process::id()));
    fs::write(&temporary, data)?;
    fs::rename(&temporary, path).inspect_err(|_| {
        let _ = fs::remove_file(&temporary);
    })
}

/// Reset history from `pet-history.plist`. A missing, malformed or other-version file starts empty.
pub fn load_reset_history(path: &Path) -> ResetHistory {
    let mut history = ResetHistory::new();
    let Ok(Value::Dictionary(root)) = Value::from_file(path) else { return history };
    if seconds(root.get("version")) != Some(1.0) {
        return history;
    }
    for (account, windows) in root.get("accounts").and_then(Value::as_dictionary).into_iter().flatten() {
        for (identifier, record) in windows.as_dictionary().into_iter().flatten() {
            let Some(record) = record.as_dictionary() else { continue };
            if let Some(resets_at) = seconds(record.get("resetsAt")) {
                history.insert_loaded(account, identifier, resets_at, timestamp(record.get("lastReset")));
            }
        }
    }
    history
}

pub fn save_reset_history(path: &Path, history: &ResetHistory) -> io::Result<()> {
    let mut accounts: BTreeMap<&str, Dictionary> = BTreeMap::new();
    for (account, identifier, record) in history.records() {
        let mut stored = Dictionary::new();
        // Whole seconds stay integers, as the services report them.
        let resets_at = if record.resets_at.fract() == 0.0 && record.resets_at.abs() < 9.0e15 {
            Value::Integer((record.resets_at as i64).into())
        } else {
            Value::Real(record.resets_at)
        };
        stored.insert("resetsAt".to_owned(), resets_at);
        if let Some(at) = record.last_reset {
            stored.insert("lastReset".to_owned(), date(at));
        }
        accounts.entry(account).or_default().insert(identifier.to_owned(), Value::Dictionary(stored));
    }
    let mut root = Dictionary::new();
    root.insert("version".to_owned(), Value::Integer(1.into()));
    root.insert("accounts".to_owned(), Value::Dictionary(accounts.into_iter().map(|(account, windows)| (account.to_owned(), Value::Dictionary(windows))).collect()));
    write_atomically(path, &Value::Dictionary(root))
}

/// The tray's records from `session-tray.plist`, or None when there is no readable file yet. A file without
/// a start time starts tracking `now`.
pub fn read_session_records(path: &Path, now: Timestamp) -> Option<SessionRecords> {
    let Ok(Value::Dictionary(root)) = Value::from_file(path) else { return None };
    let mut records = SessionRecords::new(timestamp(root.get("startedAt")).unwrap_or(now));
    for (key, record) in root.get("sessions").and_then(Value::as_dictionary).into_iter().flatten() {
        let Some(record) = record.as_dictionary() else { continue };
        records.insert_loaded(key, SessionRecord { completed_at: timestamp(record.get("completedAt")), seen_at: timestamp(record.get("seenAt")) });
    }
    Some(records)
}

/// Only completion and opening times are stored, never requests or answers.
pub fn save_session_records(path: &Path, records: &SessionRecords) -> io::Result<()> {
    let mut sessions = Dictionary::new();
    for (key, record) in records.entries() {
        let mut stored = Dictionary::new();
        if let Some(at) = record.completed_at {
            stored.insert("completedAt".to_owned(), date(at));
        }
        if let Some(at) = record.seen_at {
            stored.insert("seenAt".to_owned(), date(at));
        }
        sessions.insert(key.to_owned(), Value::Dictionary(stored));
    }
    let mut root = Dictionary::new();
    root.insert("startedAt".to_owned(), date(records.tracking_start));
    root.insert("sessions".to_owned(), Value::Dictionary(sessions));
    write_atomically(path, &Value::Dictionary(root))
}

/// No more rooms than this come back from `rooms.plist`; the server lets the person be in five at once.
const REMEMBERED_ROOMS: usize = 20;

/// The work rooms the person was in, from `rooms.plist`, in the order they joined them, and how visible each
/// was. A missing, malformed or other-version file is no rooms.
pub fn load_joined_rooms(path: &Path) -> Vec<(String, Visibility)> {
    let Ok(Value::Dictionary(root)) = Value::from_file(path) else { return Vec::new() };
    if seconds(root.get("version")) != Some(1.0) {
        return Vec::new();
    }
    let rooms = root.get("rooms").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_dictionary);
    rooms
        .filter_map(|room| {
            let id = room.get("id")?.as_string()?.to_owned();
            let private = room.get("private").and_then(Value::as_boolean).unwrap_or(false);
            Some((id, if private { Visibility::Private } else { Visibility::Public }))
        })
        .take(REMEMBERED_ROOMS)
        .collect()
}

/// Only each room's ID and whether it is private: nothing about the people in it.
pub fn save_joined_rooms(path: &Path, rooms: &[(String, Visibility)]) -> io::Result<()> {
    let rooms = rooms
        .iter()
        .map(|(id, visibility)| {
            let mut room = Dictionary::new();
            room.insert("id".to_owned(), Value::String(id.clone()));
            room.insert("private".to_owned(), Value::Boolean(*visibility == Visibility::Private));
            Value::Dictionary(room)
        })
        .collect();
    let mut root = Dictionary::new();
    root.insert("version".to_owned(), Value::Integer(1.into()));
    root.insert("rooms".to_owned(), Value::Array(rooms));
    write_atomically(path, &Value::Dictionary(root))
}

/// What Claude Code last told the status line, from `claude-usage.plist`, or None when there is no readable
/// record: the status line has not reached Errand.
pub fn read_claude_report(path: &Path) -> Option<ClaudeReport> {
    let Ok(Value::Dictionary(root)) = Value::from_file(path) else { return None };
    if seconds(root.get("version")) != Some(1.0) {
        return None;
    }
    let limits = root.get("limits").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_dictionary);
    let windows = limits
        .filter_map(|limit| {
            let remaining = seconds(limit.get("remainingPercent"))? as i64;
            claude::window(limit.get("id")?.as_string()?, remaining, seconds(limit.get("resetsAt"))?)
        })
        .collect();
    Some(ClaudeReport { reported_at: timestamp(root.get("reportedAt")), windows })
}

/// Only each limit's share left and deadline: nothing else in the status line's input is kept.
pub fn save_claude_report(path: &Path, report: &ClaudeReport) -> io::Result<()> {
    let limits = report
        .windows
        .iter()
        .filter_map(|window| {
            let mut limit = Dictionary::new();
            limit.insert("id".to_owned(), Value::String(window.identifier.clone()));
            limit.insert("remainingPercent".to_owned(), Value::Integer(window.remaining_percent?.into()));
            limit.insert("resetsAt".to_owned(), Value::Real(window.resets_at?));
            Some(Value::Dictionary(limit))
        })
        .collect();
    let mut root = Dictionary::new();
    root.insert("version".to_owned(), Value::Integer(1.into()));
    if let Some(at) = report.reported_at {
        root.insert("reportedAt".to_owned(), date(at));
    }
    root.insert("limits".to_owned(), Value::Array(limits));
    write_atomically(path, &Value::Dictionary(root))
}
