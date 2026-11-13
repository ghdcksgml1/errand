//! Codex's recorded questions, including asynchronous questions whose tool result is only a receipt.

use errand_core::sessions::{CodexThread, SessionState, Turn, VisibleSession, codex_row, fold_codex};
use errand_core::Timestamp;
use serde_json::{Value, json};

fn at(second: i64) -> Timestamp {
    Timestamp::from_second(1_800_000_000 + second).unwrap()
}

fn record(second: i64, kind: &str, payload: Value) -> String {
    format!("{}\n", json!({"timestamp": at(second).to_string(), "type": kind, "payload": payload}))
}

fn event(second: i64, payload: Value) -> String {
    record(second, "event_msg", payload)
}

fn start() -> String {
    event(0, json!({"type": "task_started"}))
        + &event(1, json!({"type": "item_completed", "item": {"type": "UserMessage", "content": [{"type": "text", "text": "고쳐줘"}]}}))
}

fn call(second: i64, name: &str, id: &str, arguments: Value) -> String {
    record(second, "response_item", json!({"type": "function_call", "name": name, "call_id": id, "arguments": arguments.to_string()}))
}

fn output(second: i64, id: &str, output: Value) -> String {
    record(second, "response_item", json!({"type": "function_call_output", "call_id": id, "output": output.to_string()}))
}

fn fold(text: &str) -> Turn {
    let mut turn = Turn::default();
    fold_codex(&mut turn, text.as_bytes());
    turn
}

#[test]
fn codex_waits_for_the_matching_question_result_across_scans() {
    let asked = start() + &call(2, "request_user_input", "q1", json!({"questions": [{"question": "어느 안으로 할까요?"}, {"question": "색은요?"}]}));
    let mut turn = fold(&asked);
    let question = turn.question.as_ref().expect("Codex's pending question");
    assert_eq!((question.id.as_str(), question.asked_at), ("q1", at(2)));
    assert_eq!(question.questions, ["어느 안으로 할까요?", "색은요?"]);
    fold_codex(&mut turn, output(3, "other", json!({"answers": {}})).as_bytes());
    assert!(turn.question.is_some(), "an unrelated result is not the person's answer");
    fold_codex(&mut turn, output(4, "q1", json!({"answers": {}})).as_bytes());
    assert!(turn.running && turn.question.is_none(), "an answer, cancellation or tool error ends the wait");
    assert_eq!(turn.prompt.as_deref(), Some("고쳐줘"));

    for ending in [json!({"type": "task_complete"}), json!({"type": "turn_aborted"}), json!({"type": "task_started"})] {
        assert!(fold(&(asked.clone() + &event(5, ending))).question.is_none(), "turn boundaries clear pending questions");
    }
}

#[test]
fn async_acknowledgement_does_not_count_as_an_answer() {
    let asked = start() + &call(2, "functions.request_user_input_async", "async-q", json!({"questions": [{"title": "어느 색으로 할까요?"}]}));
    let receipt = output(3, "async-q", json!({"accepted": true}));
    let mut turn = fold(&(asked.clone() + &receipt));
    assert!(turn.running && turn.question.is_some(), "accepting the question for display is not answering it");
    fold_codex(&mut turn, event(4, json!({"type": "item_completed", "item": {"type": "AgentMessage", "content": [{"type": "text", "text": "계속 작업합니다"}]}})).as_bytes());
    assert!(turn.question.is_some(), "the agent can keep working while the question is open");
    fold_codex(&mut turn, event(5, json!({"type": "item_completed", "item": {"type": "UserMessage", "content": [{"type": "text", "text": "파란색"}]}})).as_bytes());
    assert!(turn.running && turn.question.is_none());
    assert!(fold(&(asked + &output(3, "async-q", json!({"error": "could not display"})))).question.is_none());
}

#[test]
fn recorded_async_messages_work_even_without_the_tool_call() {
    let asked = event(2, json!({"type": "item_completed", "item": {
        "type": "AgentMessage", "id": "message-q", "delivery": "async", "phase": "final_answer",
        "content": [], "questions": [{"title": "진행할까요?", "options": ["계속", "중단"]}]
    }}));
    let turn = fold(&(start() + &asked));
    assert_eq!(turn.question.as_ref().unwrap().questions, ["진행할까요?"]);
    assert!(turn.running, "the final_answer phase on an async question does not finish a turn");
    let ended = fold(&(start() + &asked + &event(3, json!({"type": "task_complete", "last_agent_message": "완료"}))));
    assert!(!ended.running && ended.question.is_none());
}

#[test]
fn malformed_or_unrelated_tools_do_not_ask_the_person() {
    for payload in [
        json!({"type": "function_call", "name": "request_user_input", "call_id": "bad", "arguments": "{"}),
        json!({"type": "function_call", "name": "request_user_input", "call_id": "empty", "arguments": "{\"questions\":[]}"}),
        json!({"type": "function_call", "name": "exec", "call_id": "exec", "arguments": "{\"questions\":[{\"question\":\"example\"}]}"}),
    ] {
        assert!(fold(&(start() + &record(2, "response_item", payload))).question.is_none());
    }
    assert!(fold(&(start() + &event(2, json!({"type": "item_completed", "item": {"type": "AgentMessage", "content": [{"type": "text", "text": "질문 예시: 계속할까요?"}]}})))).question.is_none());
}

#[test]
fn codex_question_reaches_the_card_and_does_not_stall() {
    let turn = fold(&(start() + &call(2, "request_user_input", "q1", json!({"questions": [{"question": "**진행**할까요?"}]}))));
    let thread = CodexThread {
        id: "thread".to_owned(), rollout_path: "unused".to_owned(), title: "수정".to_owned(),
        first_message: "고쳐줘".to_owned(), cwd: "/project".to_owned(), updated_at: at(2),
        parent_thread_id: None,
    };
    let row = codex_row(&thread, &turn, Some(at(2)), false, at(3600));
    assert!(!row.stalled);
    assert_eq!(row.question.as_ref().unwrap().questions, ["진행할까요?"]);
    assert_eq!(SessionState::of(&VisibleSession { row, seen: false }), SessionState::Asking);
}
