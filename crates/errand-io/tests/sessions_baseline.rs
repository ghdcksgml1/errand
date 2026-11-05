//! The Rust tracker shows the same cards the Objective-C tracker showed for the same records.
//! The fixture and the cards were captured from the Objective-C build by scripts/baseline/capture_sessions.py,
//! removed along with that build; `git show f7e2fb6:scripts/baseline/capture_sessions.py` still has it.

use errand_core::format::{seconds, timestamp_from_seconds};
use errand_core::sessions::{SessionRecords, VisibleSession};
use errand_io::session_scan::{SessionScanner, SessionSources};
use serde_json::{Map, Value, json};
use std::fs::{self, File};
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

fn fixture(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sessions").join(name);
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

/// Writes the records the spec describes, file times included, under `root`.
pub fn materialize(spec: &Value, root: &Path) {
    for file in spec["files"].as_array().unwrap() {
        let path = root.join(file["path"].as_str().unwrap());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, file["text"].as_str().unwrap()).unwrap();
        let mtime = UNIX_EPOCH + Duration::from_secs_f64(file["mtime"].as_f64().unwrap());
        File::options().write(true).open(&path).unwrap().set_modified(mtime).unwrap();
    }
    let quoted = root.to_str().unwrap().replace('\'', "''");
    for (relative, statements) in spec["databases"].as_object().unwrap() {
        let db = rusqlite::Connection::open(root.join(relative)).unwrap();
        for statement in statements.as_array().unwrap() {
            db.execute_batch(&statement.as_str().unwrap().replace("{root}", &quoted)).unwrap();
        }
    }
}

/// A card in the shape the Objective-C baseline printed it.
fn card(session: &VisibleSession) -> Value {
    let row = &session.row;
    let mut card = Map::new();
    card.insert("key".into(), json!(row.key));
    card.insert("source".into(), json!(row.provider.storage_key()));
    card.insert("title".into(), json!(row.title));
    card.insert("prompt".into(), json!(row.prompt));
    card.insert("promptAt".into(), json!(seconds(row.prompt_at)));
    card.insert("running".into(), json!(row.running));
    card.insert("aborted".into(), json!(row.aborted));
    if let Some(finished) = row.finished_at {
        card.insert("finishedAt".into(), json!(seconds(finished)));
    }
    if let Some(result) = &row.result {
        card.insert("result".into(), json!(result));
    }
    if let Some(action) = &row.action {
        card.insert("action".into(), json!(action));
    }
    card.insert("folder".into(), json!(row.folder));
    card.insert("url".into(), json!(row.url));
    card.insert("seenInApp".into(), json!(row.seen_in_app));
    card.insert("stalled".into(), json!(row.stalled));
    card.insert("seen".into(), json!(session.seen));
    Value::Object(card)
}

fn same(expected: &Value, actual: &Value) -> bool {
    match (expected, actual) {
        (Value::Number(expected), Value::Number(actual)) => (expected.as_f64().unwrap() - actual.as_f64().unwrap()).abs() < 1e-3,
        // Objective-C boxed `a && b` as an int, so some flags were printed as 0 and 1.
        (Value::Number(expected), Value::Bool(actual)) => expected.as_f64() == Some(if *actual { 1.0 } else { 0.0 }),
        (Value::Object(expected), Value::Object(actual)) => {
            expected.len() == actual.len() && expected.iter().all(|(key, value)| actual.get(key).is_some_and(|other| same(value, other)))
        }
        _ => expected == actual,
    }
}

#[test]
fn rust_cards_match_the_objective_c_baseline() {
    let spec = fixture("spec.json");
    let expected = fixture("expected.json");
    let root = tempfile::tempdir().unwrap();
    materialize(&spec, root.path());
    let database = root.path().join("codex/state_12.sqlite");
    let now = timestamp_from_seconds(spec["now"].as_f64().unwrap()).unwrap();
    let before: Vec<_> = fs::read_dir(root.path().join("codex")).unwrap().flatten().map(|entry| entry.file_name()).collect();

    let mut scanner = SessionScanner::new(SessionSources {
        codex_home: root.path().join("codex"),
        claude_sessions: root.path().join("claude-code-sessions"),
        claude_projects: root.path().join("projects"),
    });
    let records = SessionRecords::new(now);
    let cards: Vec<Value> = records.visible(scanner.scan(&records, now)).iter().map(card).collect();
    let expected = expected.as_array().unwrap();
    let keys = |cards: &[Value]| cards.iter().map(|card| card["key"].as_str().unwrap().to_owned()).collect::<Vec<_>>();
    assert_eq!(keys(&cards), keys(expected), "the same cards in the same order");
    for (actual, expected) in cards.iter().zip(expected) {
        assert!(same(expected, actual), "{}\nexpected {expected:#}\nactual   {actual:#}", expected["key"]);
    }

    let after: Vec<_> = fs::read_dir(root.path().join("codex")).unwrap().flatten().map(|entry| entry.file_name()).collect();
    assert_eq!(before, after, "reading Codex's database creates no -wal or -shm files");
    assert!(database.exists());

    let again: Vec<Value> = records.visible(scanner.scan(&records, now)).iter().map(card).collect();
    assert_eq!(again, cards, "a second scan from the cache reads the same");
}
