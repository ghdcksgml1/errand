//! Ported from the persistence parts of the Objective-C PetStateTests and the live-database parts of its
//! SessionTrackerTests, plus records that build wrote.

use errand_core::format::{seconds, timestamp_from_seconds};
use errand_core::reset_history::ResetHistory;
use errand_core::rooms::Visibility;
use errand_core::sessions::{SessionRecords, VisibleSession};
use errand_core::usage::{Span, UsageWindow, WindowName};
use errand_core::{Provider, Timestamp};
use errand_io::session_scan::{SessionScanner, SessionSources};
use errand_io::stores;
use serde_json::{Value, json};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::SystemTime;

fn at(seconds: f64) -> Timestamp {
    timestamp_from_seconds(seconds).unwrap()
}

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stores").join(name)
}

#[test]
fn records_written_by_the_objective_c_build_load_with_their_sub_second_times() {
    let history = stores::load_reset_history(&fixture("pet-history.plist"));
    assert_eq!(history.last_reset(Provider::Codex, "codex/primary"), Some(at(1792044000.25)));
    assert_eq!(history.last_reset(Provider::Claude, "five_hour"), None);
    let accounts: Vec<&str> = history.records().map(|(account, _, _)| account).collect();
    assert_eq!(accounts, ["a", "b", "c"], "the retired account's history is kept as it was");

    let records = stores::read_session_records(&fixture("session-tray.plist"), at(0.0)).unwrap();
    assert_eq!(records.tracking_start, at(1792000000.5));
    let entries: Vec<(&str, Option<f64>, Option<f64>)> =
        records.entries().map(|(key, record)| (key, record.completed_at.map(seconds), record.seen_at.map(seconds))).collect();
    assert_eq!(entries, [("claude:local_1", None, Some(1792043500.75)), ("codex:t-1", Some(1792043000.125), None)]);

    let folder = tempfile::tempdir().unwrap();
    let history_path = folder.path().join("pet-history.plist");
    let tray_path = folder.path().join("session-tray.plist");
    stores::save_reset_history(&history_path, &history).unwrap();
    stores::save_session_records(&tray_path, &records).unwrap();
    assert_eq!(stores::load_reset_history(&history_path), history, "what Rust saves reads back the same");
    assert_eq!(stores::read_session_records(&tray_path, at(0.0)).unwrap(), records);
    assert!(fs::read(&history_path).unwrap().starts_with(b"bplist00"), "a binary property list, as before");
}

#[test]
fn the_rooms_joined_survive_a_restart_as_their_ids_and_whether_each_is_private() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("rooms.plist");
    assert!(stores::load_joined_rooms(&path).is_empty(), "no file, no rooms");
    let rooms = vec![("late-night".to_owned(), Visibility::Public), ("team-x7k2m9q4w8e1".to_owned(), Visibility::Private)];
    stores::save_joined_rooms(&path, &rooms).unwrap();
    assert_eq!(stores::load_joined_rooms(&path), rooms, "in the order they were joined");
    let saved = plist::Value::from_file(&path).unwrap();
    let keys: Vec<Vec<&str>> = saved.as_dictionary().unwrap()["rooms"]
        .as_array()
        .unwrap()
        .iter()
        .map(|room| room.as_dictionary().unwrap().keys().map(String::as_str).collect())
        .collect();
    assert_eq!(keys, [["id", "private"], ["id", "private"]], "nothing about the people in them");
    stores::save_joined_rooms(&path, &[]).unwrap();
    assert!(stores::load_joined_rooms(&path).is_empty(), "out of every room");
    fs::write(&path, "not a property list").unwrap();
    assert!(stores::load_joined_rooms(&path).is_empty());
}

fn record(history: &mut ResetHistory, provider: Provider, identifier: &str, deadline: f64, now: f64) {
    let window = UsageWindow { identifier: identifier.to_owned(), name: WindowName::span(Span::Numbered(1)), remaining_percent: Some(50), resets_at: Some(deadline) };
    history.record(provider, &[window], at(now));
}

#[test]
fn reset_history_survives_restarts_and_shrugs_off_bad_files() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("nested/state.plist");
    let mut first = stores::load_reset_history(&path);
    record(&mut first, Provider::Codex, "codex/primary", 100.0, 90.0);
    stores::save_reset_history(&path, &first).unwrap();
    let mut second = stores::load_reset_history(&path);
    assert_eq!(second.last_reset(Provider::Codex, "codex/primary"), None, "a saved first observation is still only a baseline");
    record(&mut second, Provider::Codex, "codex/primary", 200.0, 110.0);
    record(&mut second, Provider::Claude, "codex/secondary", 100.0, 90.0);
    record(&mut second, Provider::Claude, "codex/secondary", 300.0, 150.0);
    stores::save_reset_history(&path, &second).unwrap();
    let third = stores::load_reset_history(&path);
    assert_eq!(third.last_reset(Provider::Codex, "codex/primary"), Some(at(110.0)), "a baseline survives restart and detects the next rollover");
    assert_eq!(third.last_reset(Provider::Claude, "codex/secondary"), Some(at(150.0)));
    assert_eq!(third.last_reset(Provider::Codex, "codex/secondary"), None, "services and windows stay apart on disk");

    let malformed = folder.path().join("malformed.plist");
    fs::write(&malformed, "not a property list").unwrap();
    let mut loaded = stores::load_reset_history(&malformed);
    record(&mut loaded, Provider::Codex, "codex/primary", 100.0, 150.0);
    assert_eq!(loaded.last_reset(Provider::Codex, "codex/primary"), None, "malformed content starts with an empty baseline");
    plist::to_file_xml(&malformed, &plist::Value::Array(vec!["unexpected root".into()])).unwrap();
    assert_eq!(stores::load_reset_history(&malformed), ResetHistory::new(), "a wrong root is ignored");
    let corrupt = plist::Value::from_file(&path).unwrap();
    let mut corrupt = corrupt.into_dictionary().unwrap();
    let mut accounts = plist::Dictionary::new();
    let mut windows = plist::Dictionary::new();
    let mut bad = plist::Dictionary::new();
    bad.insert("resetsAt".into(), "bad".into());
    bad.insert("lastReset".into(), "bad".into());
    windows.insert("codex/primary".into(), bad.into());
    windows.insert("codex/secondary".into(), plist::Value::Array(vec![]));
    accounts.insert("a".into(), windows.into());
    accounts.insert("c".into(), "bad".into());
    corrupt.insert("accounts".into(), accounts.into());
    plist::to_file_binary(&malformed, &corrupt).unwrap();
    let mut loaded = stores::load_reset_history(&malformed);
    record(&mut loaded, Provider::Codex, "codex/primary", 100.0, 150.0);
    assert_eq!(loaded.last_reset(Provider::Codex, "codex/primary"), None, "malformed records cannot fabricate a reset");

    let blocked = folder.path().join("not-a-directory");
    fs::write(&blocked, "file").unwrap();
    assert!(stores::save_reset_history(&blocked.join("state.plist"), &third).is_err(), "a failed save is reported, and memory is untouched");
}

fn stamp(at: Timestamp) -> String {
    at.strftime("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}
fn line(object: Value) -> String {
    format!("{object}\n")
}
fn codex_prompt(at: Timestamp, text: &str) -> String {
    let event = |at: Timestamp, payload: Value| line(json!({"timestamp": stamp(at), "type": "event_msg", "payload": payload}));
    event(at, json!({"type": "task_started"}))
        + &event(at, json!({"type": "item_completed", "item": {"type": "UserMessage", "content": [{"type": "text", "text": text}]}}))
}
fn keys(visible: &[VisibleSession]) -> Vec<&str> {
    visible.iter().map(|session| session.row.key.as_str()).collect()
}
fn set_modified(path: &Path, at: Timestamp) {
    File::options().write(true).open(path).unwrap().set_modified(SystemTime::from(at)).unwrap();
}

#[test]
fn checked_cards_persist_and_new_requests_come_through_codexs_live_database() {
    let now = errand_io::now();
    let ago = |seconds: i64| now - jiff::SignedDuration::from_secs(seconds);
    let root = tempfile::tempdir().unwrap();
    let codex_home = root.path().join("codex");
    fs::create_dir_all(&codex_home).unwrap();
    let rollout = codex_home.join("rollout-user.jsonl");
    let done = line(json!({"timestamp": stamp(ago(30)), "type": "event_msg", "payload": {"type": "task_complete", "last_agent_message": "고쳤어요"}}));
    fs::write(&rollout, codex_prompt(ago(60), "로그인 버그 고쳐줘") + &done).unwrap();
    let database = codex_home.join("state_5.sqlite");
    let db = rusqlite::Connection::open(&database).unwrap();
    db.execute_batch(
        "CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT, name TEXT, title TEXT, first_user_message TEXT, cwd TEXT, updated_at INTEGER, source TEXT, thread_source TEXT);
         CREATE TABLE thread_read_receipts (thread_id TEXT PRIMARY KEY, first_unread_turn TEXT, revision TEXT, last_published_turn TEXT);",
    )
    .unwrap();
    db.execute("INSERT INTO threads VALUES ('t-user', ?1, '로그인 수정', '', '첫 요청', '/Users/me/app', ?2, 'vscode', 'user')", (rollout.to_str().unwrap(), ago(30).as_second()))
        .unwrap();
    drop(db);
    let projects = root.path().join("projects/-Users-me-docs");
    fs::create_dir_all(&projects).unwrap();
    let transcript = line(json!({"type": "user", "isSidechain": false, "timestamp": stamp(ago(120)), "message": {"role": "user", "content": "README 정리해줘"}}))
        + &line(json!({"type": "assistant", "isSidechain": false, "timestamp": stamp(ago(50)), "message": {"role": "assistant", "stop_reason": "end_turn", "content": "정리했어요"}}));
    fs::write(projects.join("cli-1.jsonl"), transcript).unwrap();
    let sessions = root.path().join("claude-code-sessions/account/org");
    fs::create_dir_all(&sessions).unwrap();
    fs::write(sessions.join("local_abc.json"), json!({"sessionId": "local_abc", "cliSessionId": "cli-1", "cwd": "/Users/me/docs", "title": "README"}).to_string())
        .unwrap();

    let storage = root.path().join("Errand/session-tray.plist");
    let sources = SessionSources { codex_home: codex_home.clone(), claude_sessions: root.path().join("claude-code-sessions"), claude_projects: root.path().join("projects") };
    let mut scanner = SessionScanner::new(sources.clone());
    let mut records = stores::read_session_records(&storage, now).unwrap_or_else(|| SessionRecords::new(now));
    let visible = records.visible(scanner.scan(&records, now));
    assert_eq!(keys(&visible), ["codex:t-user", "claude:local_abc"]);

    records.mark_seen("codex:t-user", now);
    records.complete("codex:t-user", now);
    records.mark_seen("claude:local_abc", now);
    stores::save_session_records(&storage, &records).unwrap();
    assert_eq!(keys(&records.visible(scanner.scan(&records, now))), ["claude:local_abc"], "only checking a card off removes it");

    // A new request after checking off, written while Codex holds its database open in WAL mode.
    let later = now + jiff::SignedDuration::from_secs(5);
    OpenOptions::new().append(true).open(&rollout).unwrap().write_all(codex_prompt(later, "테스트도 추가해줘").as_bytes()).unwrap();
    let writer = rusqlite::Connection::open(&database).unwrap();
    writer.pragma_update(None, "journal_mode", "WAL").unwrap();
    writer.execute("UPDATE threads SET updated_at = ?1 WHERE id = 't-user'", [later.as_second() + 1]).unwrap();
    let visible = records.visible(scanner.scan(&records, later));
    let codex = &visible.iter().find(|session| session.row.key == "codex:t-user").expect("the session comes back").row;
    assert!(codex.running && codex.prompt == "테스트도 추가해줘", "read through the live WAL: {codex:?}");

    set_modified(&rollout, later - jiff::SignedDuration::from_mins(30));
    let visible = records.visible(scanner.scan(&records, later));
    drop(writer);
    assert!(visible[0].row.stalled, "running work silent for a long time is flagged");
    assert_eq!(keys(&visible), ["codex:t-user", "claude:local_abc"], "a new request puts its session on top");

    let mut reopened = stores::read_session_records(&storage, later).unwrap();
    assert_eq!(reopened.tracking_start, records.tracking_start, "the tracking start survives relaunch");
    let visible = reopened.visible(SessionScanner::new(sources).scan(&reopened, later));
    assert!(visible.iter().any(|session| session.row.key == "claude:local_abc" && session.seen), "seen marks survive relaunch");
    reopened.complete("codex:t-user", later + jiff::SignedDuration::from_secs(1));
    reopened.complete("claude:local_abc", later + jiff::SignedDuration::from_secs(1));
    assert!(reopened.visible(scanner.scan(&reopened, later)).is_empty(), "checking every card off empties the tray");
    stores::save_session_records(&storage, &reopened).unwrap();
    let saved = plist::Value::from_file(&storage).unwrap();
    let fields: std::collections::BTreeSet<&str> = saved.as_dictionary().unwrap()["sessions"]
        .as_dictionary()
        .unwrap()
        .values()
        .flat_map(|record| record.as_dictionary().unwrap().keys().map(String::as_str))
        .collect();
    assert_eq!(fields.into_iter().collect::<Vec<_>>(), ["completedAt", "seenAt"], "only times are stored, never prompts or answers");
}
