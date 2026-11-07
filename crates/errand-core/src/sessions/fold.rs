//! Reading a session's latest turn out of its JSONL record, a chunk at a time.

use super::text::{is_automated_prompt, message_text};
use crate::json;
use jiff::Timestamp;
use memchr::memmem;
use serde_json::{Map, Value};

/// What one record says about its latest turn. Folding a later chunk into it continues where the last left off.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Turn {
    pub running: bool,
    pub aborted: bool,
    pub started_at: Option<Timestamp>,
    pub finished_at: Option<Timestamp>,
    pub prompt: Option<String>,
    pub prompt_at: Option<Timestamp>,
    pub result: Option<String>,
    /// Claude Code's own note of the last prompt, for when the transcript tail holds none.
    pub last_prompt: Option<String>,
    /// What the turn waits on the person to answer before it can go on.
    pub question: Option<Question>,
}

/// A question an agent put to the person, waiting for the answer.
#[derive(Clone, Debug, PartialEq)]
pub struct Question {
    /// The tool call the answer comes back to.
    pub id: String,
    pub asked_at: Timestamp,
    /// What it asks, in order; none for a plan waiting to be approved.
    pub questions: Vec<String>,
}

/// Claude Code's tools that hold the turn until the person answers.
const ASKING_TOOLS: [&str; 2] = ["AskUserQuestion", "ExitPlanMode"];

impl Turn {
    fn start(&mut self, at: Timestamp) {
        self.running = true;
        self.started_at = Some(at);
        self.aborted = false;
        self.finished_at = None;
        self.result = None;
        self.question = None;
    }

    fn finish(&mut self, at: Timestamp, aborted: bool, result: Option<String>) {
        self.running = false;
        self.finished_at = Some(at);
        self.aborted = aborted;
        self.result = result;
        self.question = None;
    }
}

/// An ISO 8601 time as the apps write it, with or without fractional seconds.
pub fn parse_timestamp(value: Option<&Value>) -> Option<Timestamp> {
    value?.as_str()?.parse().ok()
}

/// Calls `block` with each complete line that contains one of `needles` and parses as a JSON object.
/// The needles skip parsing the many lines that cannot matter; a torn or invalid line is passed over.
fn each_json_line(data: &[u8], needles: &[&[u8]], mut block: impl FnMut(&Map<String, Value>)) {
    for line in data.split(|byte| *byte == b'\n') {
        if line.is_empty() || !needles.iter().any(|needle| memmem::find(line, needle).is_some()) {
            continue;
        }
        if let Ok(Value::Object(object)) = serde_json::from_slice::<Value>(line) {
            block(&object);
        }
    }
}

/// Folds Codex rollout events: `task_started`, `task_complete`, `turn_aborted` and the user's messages.
pub fn fold_codex(turn: &mut Turn, data: &[u8]) {
    let needles: [&[u8]; 4] = [b"\"task_started\"", b"\"task_complete\"", b"\"turn_aborted\"", b"\"UserMessage\""];
    each_json_line(data, &needles, |line| {
        let Some(payload) = json::object(line.get("payload")) else { return };
        if line.get("type").and_then(Value::as_str) != Some("event_msg") {
            return;
        }
        let Some(at) = parse_timestamp(line.get("timestamp")) else { return };
        match payload.get("type").and_then(Value::as_str) {
            Some("task_started") => turn.start(at),
            Some(kind @ ("task_complete" | "turn_aborted")) => {
                let aborted = kind == "turn_aborted";
                let result = if aborted { None } else { json::text(payload.get("last_agent_message")).map(str::to_owned) };
                turn.finish(at, aborted, result);
            }
            Some("item_completed") => {
                let Some(item) = json::object(payload.get("item")) else { return };
                if item.get("type").and_then(Value::as_str) != Some("UserMessage") {
                    return;
                }
                let text = message_text(item.get("content"));
                if !text.is_empty() {
                    turn.prompt = Some(text);
                    turn.prompt_at = Some(at);
                }
            }
            _ => {}
        }
    });
}

/// Folds a Claude Code transcript. Side chains, tool results and meta lines neither end nor replace the
/// person's prompt; an automated event starts work but keeps the prompt they typed. A question put to the person
/// waits until its tool call has a result: the answer, or the question turned down.
pub fn fold_claude(turn: &mut Turn, data: &[u8]) {
    let needles: [&[u8]; 5] = [b"\"type\":\"user\"", b"\"type\":\"assistant\"", b"\"stop_hook_summary\"", b"\"turn_duration\"", b"\"last-prompt\""];
    each_json_line(data, &needles, |line| {
        let kind = line.get("type").and_then(Value::as_str);
        if kind == Some("last-prompt") {
            if let Some(prompt) = json::text(line.get("lastPrompt")) {
                turn.last_prompt = Some(prompt.to_owned());
            }
            return;
        }
        let Some(at) = parse_timestamp(line.get("timestamp")) else { return };
        if json::flag(line.get("isSidechain")) {
            return;
        }
        if kind == Some("system") {
            let subtype = line.get("subtype").and_then(Value::as_str);
            if matches!(subtype, Some("stop_hook_summary" | "turn_duration")) && turn.running {
                turn.running = false;
                turn.finished_at = Some(at);
                turn.question = None;
            }
            return;
        }
        let Some(message) = json::object(line.get("message")) else { return };
        if kind == Some("assistant") {
            if let Some(question) = question_in(message, at) {
                turn.question = Some(question);
            }
            if message.get("stop_reason").and_then(Value::as_str) == Some("end_turn") {
                let text = message_text(message.get("content"));
                turn.finish(at, false, (!text.is_empty()).then_some(text));
            } else if !turn.running && turn.finished_at.is_some() {
                // A stop hook can send the turn back to work without a new prompt.
                turn.running = true;
                turn.finished_at = None;
                turn.result = None;
            }
            return;
        }
        if kind != Some("user") || ["isMeta", "isCompactSummary", "isVisibleInTranscriptOnly"].iter().any(|key| json::flag(line.get(*key))) {
            return;
        }
        let content = message.get("content");
        let results: Vec<&str> = content
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_object)
            .filter(|part| part.get("type").and_then(Value::as_str) == Some("tool_result"))
            .map(|part| part.get("tool_use_id").and_then(Value::as_str).unwrap_or_default())
            .collect();
        if !results.is_empty() {
            if turn.question.as_ref().is_some_and(|question| results.contains(&question.id.as_str())) {
                turn.question = None;
            }
            return;
        }
        let text = message_text(content);
        if text.starts_with("[Request interrupted") {
            turn.finish(at, true, None);
            return;
        }
        turn.start(at);
        if !is_automated_prompt(&text) {
            turn.prompt = Some(text);
            turn.prompt_at = Some(at);
        }
    });
}

/// The question a Claude Code message puts to the person with a tool that waits for the answer, if it does.
fn question_in(message: &Map<String, Value>, at: Timestamp) -> Option<Question> {
    let parts = message.get("content")?.as_array()?;
    parts.iter().filter_map(Value::as_object).find_map(|part| {
        let name = part.get("name").and_then(Value::as_str)?;
        if part.get("type").and_then(Value::as_str) != Some("tool_use") || !ASKING_TOOLS.contains(&name) {
            return None;
        }
        let asked = json::object(part.get("input")).and_then(|input| input.get("questions")).and_then(Value::as_array);
        let questions = asked.into_iter().flatten().filter_map(|question| json::text(question.get("question"))).map(str::to_owned).collect();
        Some(Question { id: json::text(part.get("id"))?.to_owned(), asked_at: at, questions })
    })
}
