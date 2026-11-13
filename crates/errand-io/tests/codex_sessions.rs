//! Codex's real storage shapes, with invented messages and IDs. No installed apps or account data are used.

use errand_core::Timestamp;
use errand_core::sessions::{SessionRecords, SessionState, families};
use errand_io::session_scan::{SessionScanner, SessionSources};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;

fn at(seconds: i64) -> Timestamp {
    Timestamp::from_second(1_800_000_000 + seconds).unwrap()
}

fn event(seconds: i64, payload: Value) -> String {
    format!("{}\n", json!({"timestamp": at(seconds).to_string(), "type": "event_msg", "payload": payload}))
}

struct Fixture {
    root: tempfile::TempDir,
    state: Connection,
    history: Connection,
}

impl Fixture {
    fn new() -> Fixture {
        let root = tempfile::tempdir().unwrap();
        let state = Connection::open(root.path().join("state_5.sqlite")).unwrap();
        state.execute_batch(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT, name TEXT, title TEXT, first_user_message TEXT, cwd TEXT, updated_at INTEGER, source TEXT, thread_source TEXT);
             CREATE TABLE thread_read_receipts (thread_id TEXT PRIMARY KEY, first_unread_turn TEXT);",
        ).unwrap();
        let history = Connection::open(root.path().join("thread_history_1.sqlite")).unwrap();
        history.execute_batch(
            "CREATE TABLE thread_turns (thread_id TEXT, turn_id TEXT, rollout_ordinal INTEGER, status TEXT, started_at INTEGER, completed_at INTEGER);
             CREATE TABLE thread_items (thread_id TEXT, turn_id TEXT, item_id TEXT, rollout_ordinal INTEGER, created_at_ms INTEGER, item_json TEXT, item_type TEXT, started_at_ms INTEGER, completed_at_ms INTEGER);",
        ).unwrap();
        Fixture { root, state, history }
    }

    fn scanner(&self) -> SessionScanner {
        SessionScanner::new(SessionSources {
            codex_home: self.root.path().to_owned(), claude_sessions: self.root.path().join("missing-claude"), claude_projects: self.root.path().join("missing-projects"),
        })
    }

    fn thread(&self, id: &str, source: &str, kind: &str) -> PathBuf {
        let rollout = self.root.path().join(format!("{id}.jsonl"));
        self.state.execute("INSERT INTO threads VALUES (?1, ?2, ?1, '', '처음 요청', '/project', ?3, ?4, ?5)",
            (id, rollout.to_str().unwrap(), at(60).as_second(), source, kind)).unwrap();
        rollout
    }

    fn turn(&self, thread: &str, status: &str) {
        self.history.execute("INSERT INTO thread_turns VALUES (?1, 'turn', 1, ?2, ?3, ?4)",
            (thread, status, at(0).as_second(), (status != "inProgress").then(|| at(50).as_second()))).unwrap();
    }

    fn item(&self, thread: &str, second: i64, item: Value) {
        self.history.execute("INSERT INTO thread_items VALUES (?1, 'turn', ?2, ?3, ?4, ?5, ?6, ?4, ?4)",
            (thread, format!("item-{second}"), second, at(second).as_millisecond(), item.to_string(), item["type"].as_str().unwrap())).unwrap();
    }

    fn user(&self, thread: &str, second: i64, text: &str) {
        self.item(thread, second, json!({"type": "userMessage", "content": [{"type": "text", "text": text}]}));
    }
}

#[test]
fn delegated_agents_are_grouped_but_guardians_and_internal_threads_stay_out() {
    let fixture = Fixture::new();
    let records = SessionRecords::new(at(60));
    for (id, source, kind) in [
        ("parent", "vscode".to_owned(), "user"),
        ("child", json!({"subagent": {"thread_spawn": {"parent_thread_id": "parent", "depth": 1}}}).to_string(), "subagent"),
        ("grandchild", json!({"subagent": {"thread_spawn": {"parent_thread_id": "child", "depth": 2}}}).to_string(), "subagent"),
        ("guardian", json!({"subagent": {"other": "guardian"}}).to_string(), "subagent"),
        ("review", json!({"subagent": "review"}).to_string(), "subagent"),
        ("invalid", json!({"subagent": {"thread_spawn": {"parent_thread_id": ""}}}).to_string(), "subagent"),
        ("self", json!({"subagent": {"thread_spawn": {"parent_thread_id": "self"}}}).to_string(), "subagent"),
        ("cli", "cli".to_owned(), ""),
    ] {
        let file = fixture.thread(id, &source, kind);
        fs::write(&file, event(0, json!({"type": "task_started"}))).unwrap();
        fs::File::options().write(true).open(file).unwrap().set_modified(std::time::SystemTime::from(at(0))).unwrap();
    }
    let visible = records.visible(fixture.scanner().scan(&records, at(60)));
    assert_eq!(visible.len(), 4);
    let cards = families(&visible);
    assert_eq!(cards.len(), 2);
    let parent = cards.iter().find(|family| family.head.row.key == "codex:parent").unwrap();
    assert_eq!(parent.sub_tasks.len(), 2);
    assert!(parent.sub_tasks.iter().all(|session| SessionState::of(session) == SessionState::Running));
    assert_eq!(visible.iter().find(|s| s.row.key == "codex:child").unwrap().row.parent.as_deref(), Some("codex:parent"));
}

#[test]
fn paginated_history_replaces_a_compressed_rollout_without_touching_the_database() {
    let fixture = Fixture::new();
    fixture.thread("done", "vscode", "user");
    fixture.turn("done", "completed");
    fixture.user("done", 1, "마지막 요청");
    fixture.item("done", 50, json!({"type": "agentMessage", "phase": "final_answer", "text": "**수정** 완료"}));
    fixture.state.execute("INSERT INTO thread_read_receipts VALUES ('done', NULL)", []).unwrap();
    let before = fs::read(fixture.root.path().join("thread_history_1.sqlite")).unwrap();
    let records = SessionRecords::new(at(60));
    let visible = records.visible(fixture.scanner().scan(&records, at(60)));
    let row = &visible[0].row;
    assert_eq!((row.prompt.as_str(), row.result.as_deref()), ("마지막 요청", Some("수정 완료")));
    assert_eq!(row.finished_at, Some(at(50)));
    assert_eq!(SessionState::of(&visible[0]), SessionState::Seen);
    assert_eq!(fs::read(fixture.root.path().join("thread_history_1.sqlite")).unwrap(), before);
    assert!(!fixture.root.path().join("thread_history_1.sqlite-wal").exists());
    assert!(!fixture.root.path().join("thread_history_1.sqlite-shm").exists());
}

#[test]
fn paginated_async_questions_follow_answers_and_completion_in_a_live_wal() {
    let fixture = Fixture::new();
    fixture.history.pragma_update(None, "journal_mode", "WAL").unwrap();
    fixture.thread("asking", "vscode", "user");
    fixture.turn("asking", "inProgress");
    fixture.user("asking", 1, "기능 추가");
    fixture.item("asking", 2, json!({"type": "agentMessage", "id": "q", "phase": "final_answer", "delivery": "async", "questions": [{"title": "어느 방식으로 할까요?"}]}));
    let records = SessionRecords::new(at(60));
    let mut scanner = fixture.scanner();
    let visible = records.visible(scanner.scan(&records, at(3600)));
    assert_eq!(SessionState::of(&visible[0]), SessionState::Asking);
    assert!(!visible[0].row.stalled);
    fixture.user("asking", 3601, "첫 번째");
    let visible = records.visible(scanner.scan(&records, at(3602)));
    assert_eq!(SessionState::of(&visible[0]), SessionState::Running);
    assert_eq!(visible[0].row.prompt, "첫 번째");
    fixture.history.execute("UPDATE thread_turns SET status='interrupted', completed_at=?1", [at(3603).as_second()]).unwrap();
    let visible = records.visible(scanner.scan(&records, at(3604)));
    assert_eq!(SessionState::of(&visible[0]), SessionState::Aborted);
    assert!(visible[0].row.question.is_none());
}

#[test]
fn live_rollout_questions_take_precedence_over_an_older_projection() {
    let fixture = Fixture::new();
    let file = fixture.thread("asking", "vscode", "user");
    fixture.turn("asking", "completed");
    fixture.user("asking", 1, "예전 요청");
    let call = json!({"type": "function_call", "name": "request_user_input", "call_id": "q", "arguments": "{\"questions\":[{\"question\":\"계속할까요?\"}]}"});
    let question = format!("{}\n", json!({"timestamp": at(55).to_string(), "type": "response_item", "payload": call}));
    fs::write(&file, event(54, json!({"type": "task_started"})) + &question).unwrap();
    let records = SessionRecords::new(at(60));
    let mut scanner = fixture.scanner();
    let visible = records.visible(scanner.scan(&records, at(60)));
    assert_eq!(SessionState::of(&visible[0]), SessionState::Asking);
    // Migration can remove a previously cached rollout; its projection still supplies the card.
    fs::remove_file(file).unwrap();
    let visible = records.visible(scanner.scan(&records, at(60)));
    assert_eq!(visible[0].row.prompt, "예전 요청");
    assert_eq!(SessionState::of(&visible[0]), SessionState::New);
}

#[test]
fn history_failure_does_not_masquerade_as_a_successful_result() {
    let fixture = Fixture::new();
    fixture.thread("failed", "vscode", "user");
    fixture.turn("failed", "failed");
    fixture.user("failed", 1, "테스트 실행");
    fixture.item("failed", 2, json!({"type": "agentMessage", "phase": "final_answer", "text": "중간 결과"}));
    let records = SessionRecords::new(at(60));
    let visible = records.visible(fixture.scanner().scan(&records, at(60)));
    assert_eq!(SessionState::of(&visible[0]), SessionState::Unknown);
    assert!(visible[0].row.result.is_none());
    fixture.history.execute_batch("DROP TABLE thread_items").unwrap();
    assert_eq!(fixture.scanner().scan(&records, at(60)).len(), 1, "an unknown schema does not crash the scan");
}

#[test]
fn an_automated_continuation_keeps_the_last_user_request() {
    let fixture = Fixture::new();
    fixture.thread("continued", "vscode", "user");
    fixture.turn("continued", "completed");
    fixture.user("continued", 1, "실제 마지막 요청");
    fixture.history.execute("INSERT INTO thread_turns VALUES ('continued', 'continued-turn', 100, 'inProgress', ?1, NULL)", [at(55).as_second()]).unwrap();
    let records = SessionRecords::new(at(60));
    let visible = records.visible(fixture.scanner().scan(&records, at(60)));
    assert_eq!(visible[0].row.prompt, "실제 마지막 요청");
    assert_eq!(visible[0].row.prompt_at, at(1));
    assert_eq!(SessionState::of(&visible[0]), SessionState::Running);
}
