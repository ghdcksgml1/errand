//! Reading a whole transcript into a [`Transcript`]: each request with its answer, the commits and pull
//! requests the agent made, and the files it changed. Lenient like the rest of Errand's reads: a line that does
//! not parse, or a field of the wrong type, is passed over.

use super::{Commit, FileChange, PullRequest, Transcript, WorkTurn};
use crate::json;
use crate::sessions::fold::parse_timestamp;
use crate::sessions::text::{is_automated_prompt, message_text};
use jiff::{SignedDuration, Timestamp};
use regex::Regex;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::sync::OnceLock;

/// `git commit` prints `[branch abc1234] Message`, or `[branch (root-commit) abc1234] Message`.
fn commit_line() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"(?m)^\[[^\]\s]+(?: \(root-commit\))? ([0-9a-f]{7,40})\] (.+)$").expect("valid pattern"))
}

/// `gh pr create` prints the new pull request's address on a line of its own.
fn pull_request_line() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"(?m)^\s*(https://github\.com/[\w.-]+/[\w.-]+/pull/(\d+))\s*$").expect("valid pattern"))
}

/// A request the same as the one just made, this close after it, is the same request logged twice.
const ECHO: SignedDuration = SignedDuration::from_secs(2);

#[derive(Default)]
struct Reader {
    transcript: Transcript,
    custom_title: Option<String>,
    ai_title: Option<String>,
    /// The request still being worked on, as an index into the turns.
    open: Option<usize>,
    /// Commit messages printed by `git commit`, by the hash it printed.
    printed: HashMap<String, String>,
    files: HashMap<String, usize>,
    /// The agent's latest words in the request being worked on, and when it last did anything: older transcripts
    /// mark no end, so these stand in for the answer and the time it came.
    last_words: Option<String>,
    last_activity: Option<Timestamp>,
}

/// The person's own words: blocks an app puts in front of them, such as `<artifact-view-context …>…</…>`, are
/// taken off. Such blocks are tags whose names hold a hyphen or an underscore, which typed text seldom starts with.
pub(crate) fn own_words(text: &str) -> String {
    let mut rest = text.trim_start();
    while let Some(open) = rest.strip_prefix('<') {
        let name: String = open.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
        if name.is_empty() || !name.contains(['-', '_']) {
            break;
        }
        let Some(head_end) = rest.find('>') else { break };
        if rest[..head_end].ends_with('/') {
            rest = rest[head_end + 1..].trim_start();
            continue;
        }
        let closing = format!("</{name}>");
        match rest.find(&closing) {
            Some(close) => rest = rest[close + closing.len()..].trim_start(),
            None => break,
        }
    }
    rest.trim_end().to_owned()
}

impl Reader {
    fn ask(&mut self, at: Timestamp, ask: String) {
        let ask = own_words(&ask);
        if ask.is_empty() {
            return;
        }
        if let Some(last) = self.transcript.turns.last()
            && last.ask == ask
            && at.duration_since(last.asked_at).abs() <= ECHO
        {
            return;
        }
        // A new request ends one that never said it had, when the agent last did anything.
        self.close_unmarked(Some(at));
        self.last_words = None;
        self.last_activity = None;
        self.transcript.turns.push(WorkTurn { asked_at: at, ask, finished_at: None, result: None, aborted: false });
        self.open = Some(self.transcript.turns.len() - 1);
    }

    /// Ends the latest request. It stays the latest until the next one, so an answer written after its end, as
    /// Claude Code does when it closes the thinking before the text, still lands on it.
    fn finish(&mut self, at: Timestamp, result: Option<String>, aborted: bool) {
        let Some(turn) = self.open.and_then(|open| self.transcript.turns.get_mut(open)) else { return };
        turn.finished_at = Some(at);
        turn.aborted = aborted;
        if result.is_some() {
            turn.result = result;
        }
    }

    /// Gives the latest request the agent's last words when nothing marked them as its answer, and an end when
    /// `next` shows the person had moved on.
    fn close_unmarked(&mut self, next: Option<Timestamp>) {
        let (words, activity) = (self.last_words.take(), self.last_activity);
        let Some(turn) = self.open.and_then(|open| self.transcript.turns.get_mut(open)) else { return };
        if turn.result.is_none() && !turn.aborted {
            turn.result = words;
        }
        if turn.finished_at.is_none()
            && let Some(next) = next
        {
            turn.finished_at = Some(activity.filter(|done| *done > turn.asked_at).unwrap_or(next));
        }
    }

    /// The agent said something in the request being worked on.
    fn spoke(&mut self, at: Timestamp, words: String) {
        self.last_activity = Some(at);
        if !words.trim().is_empty() {
            self.last_words = Some(words);
        }
    }

    /// The end of a turn the transcript marks only by its timing line, after its answer.
    fn settle(&mut self, at: Timestamp) {
        if let Some(turn) = self.open.and_then(|open| self.transcript.turns.get_mut(open))
            && turn.finished_at.is_none()
        {
            turn.finished_at = Some(at);
        }
    }

    fn commit(&mut self, sha: &str, message: Option<String>, at: Timestamp) {
        let short: String = sha.chars().take(7).collect();
        if self.transcript.commits.iter().any(|commit| commit.sha.starts_with(&short) || short.starts_with(&commit.sha)) {
            return;
        }
        let message = message.or_else(|| self.printed.get(&short).cloned()).unwrap_or_default();
        self.transcript.commits.push(Commit { sha: short, message, at });
    }

    /// Commits and pull requests a command printed.
    fn printed_output(&mut self, output: &str, at: Timestamp, commits_too: bool) {
        for capture in commit_line().captures_iter(output) {
            let short: String = capture[1].chars().take(7).collect();
            let message = capture[2].trim().to_owned();
            self.printed.insert(short.clone(), message.clone());
            if commits_too {
                self.commit(&short, Some(message), at);
            }
        }
        for capture in pull_request_line().captures_iter(output) {
            let Ok(number) = capture[2].parse::<u32>() else { continue };
            if !self.transcript.pull_requests.iter().any(|pull| pull.number == number) {
                self.transcript.pull_requests.push(PullRequest { number, url: capture[1].to_owned(), at });
            }
        }
    }

    fn file(&mut self, path: &str, added: u32, removed: u32, created: bool) {
        if path.is_empty() {
            return;
        }
        let index = *self.files.entry(path.to_owned()).or_insert_with(|| {
            self.transcript.files.push(FileChange { path: path.to_owned(), added: 0, removed: 0, created: false });
            self.transcript.files.len() - 1
        });
        let file = &mut self.transcript.files[index];
        file.added += added;
        file.removed += removed;
        file.created |= created;
    }

    /// Every file an `apply_patch` envelope touches, with its added and removed lines.
    fn patch(&mut self, text: &str) {
        if !text.contains("*** Begin Patch") {
            return;
        }
        let mut current: Option<(String, bool)> = None;
        let mut counts = (0, 0);
        let flush = |reader: &mut Reader, current: &mut Option<(String, bool)>, counts: &mut (u32, u32)| {
            if let Some((path, created)) = current.take() {
                reader.file(&path, counts.0, counts.1, created);
            }
            *counts = (0, 0);
        };
        for line in text.lines() {
            if let Some(path) = line.strip_prefix("*** Update File: ") {
                flush(self, &mut current, &mut counts);
                current = Some((path.trim().to_owned(), false));
            } else if let Some(path) = line.strip_prefix("*** Add File: ") {
                flush(self, &mut current, &mut counts);
                current = Some((path.trim().to_owned(), true));
            } else if line.starts_with("*** ") {
                flush(self, &mut current, &mut counts);
            } else if current.is_some() {
                if line.starts_with('+') {
                    counts.0 += 1;
                } else if line.starts_with('-') {
                    counts.1 += 1;
                }
            }
        }
        flush(self, &mut current, &mut counts);
    }

    fn done(mut self) -> Transcript {
        // The last request keeps running unless something ended it; it still gets the words said so far.
        self.close_unmarked(None);
        self.transcript.title = self.custom_title.or(self.ai_title);
        self.transcript
    }
}

fn each_line(data: &[u8], mut block: impl FnMut(&Map<String, Value>)) {
    for line in data.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        if let Ok(Value::Object(object)) = serde_json::from_slice::<Value>(line) {
            block(&object);
        }
    }
}

/// A tool's output as text, whether a string, a list of text parts, or JSON wrapping either.
fn output_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(object)) => output_text(object.get("output").or_else(|| object.get("stdout"))),
            _ => text.clone(),
        },
        // Each part can itself be JSON text wrapping the command's output.
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| json::text(part.get("text")).or_else(|| part.as_str()))
            .map(|text| output_text(Some(&Value::String(text.to_owned()))))
            .collect::<Vec<_>>()
            .join("\n"),
        Some(Value::Object(object)) => output_text(object.get("output").or_else(|| object.get("stdout")).or_else(|| object.get("text"))),
        _ => String::new(),
    }
}

/// Reads a Claude Code transcript. Side chains are an agent's own helpers: their edits and commits count, their
/// messages are not the person's requests.
pub fn read_claude(data: &[u8]) -> Transcript {
    let mut reader = Reader::default();
    each_line(data, |line| {
        let kind = line.get("type").and_then(Value::as_str);
        match kind {
            Some("custom-title") => reader.custom_title = json::text(line.get("customTitle")).map(str::to_owned),
            Some("ai-title") => reader.ai_title = json::text(line.get("aiTitle")).map(str::to_owned),
            Some("summary") if reader.ai_title.is_none() => reader.ai_title = json::text(line.get("summary")).map(str::to_owned),
            _ => {}
        }
        if reader.transcript.id.is_none() {
            reader.transcript.id = json::text(line.get("sessionId")).map(str::to_owned);
        }
        let Some(at) = parse_timestamp(line.get("timestamp")) else { return };
        let sidechain = json::flag(line.get("isSidechain"));
        if !sidechain {
            if let Some(cwd) = json::text(line.get("cwd")) {
                reader.transcript.folder = Some(cwd.to_owned());
            }
            if let Some(branch) = json::text(line.get("gitBranch")).filter(|branch| *branch != "HEAD") {
                reader.transcript.branch = Some(branch.to_owned());
            }
        }
        match kind {
            Some("user") => claude_user(&mut reader, line, at, sidechain),
            Some("assistant") if !sidechain => {
                let Some(message) = json::object(line.get("message")) else { return };
                let text = message_text(message.get("content"));
                if matches!(message.get("stop_reason").and_then(Value::as_str), Some("end_turn" | "stop_sequence")) {
                    reader.finish(at, (!text.trim().is_empty()).then_some(text.clone()), false);
                }
                reader.spoke(at, text);
            }
            Some("system") if !sidechain => {
                if matches!(line.get("subtype").and_then(Value::as_str), Some("turn_duration" | "stop_hook_summary")) {
                    reader.settle(at);
                }
            }
            _ => {}
        }
    });
    reader.done()
}

fn claude_user(reader: &mut Reader, line: &Map<String, Value>, at: Timestamp, sidechain: bool) {
    if let Some(result) = json::object(line.get("toolUseResult")) {
        let stdout = json::text(result.get("stdout")).unwrap_or_default().to_owned();
        let operation = json::object(result.get("gitOperation"));
        let committed = operation.and_then(|op| json::object(op.get("commit"))).and_then(|commit| json::text(commit.get("sha")));
        reader.printed_output(&stdout, at, operation.is_none());
        if let Some(sha) = committed {
            reader.commit(sha, None, at);
        }
        if let Some(patch) = result.get("structuredPatch").and_then(Value::as_array) {
            let (mut added, mut removed) = (0, 0);
            for hunk in patch {
                for change in hunk.get("lines").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                    if change.starts_with('+') {
                        added += 1;
                    } else if change.starts_with('-') {
                        removed += 1;
                    }
                }
            }
            let created = result.get("type").and_then(Value::as_str) == Some("create");
            if created && patch.is_empty() {
                added = json::text(result.get("content")).map_or(0, |content| content.lines().count() as u32);
            }
            reader.file(json::text(result.get("filePath")).unwrap_or_default(), added, removed, created);
        }
    }
    if sidechain || ["isMeta", "isCompactSummary", "isVisibleInTranscriptOnly"].iter().any(|key| json::flag(line.get(*key))) {
        return;
    }
    let Some(message) = json::object(line.get("message")) else { return };
    let content = message.get("content");
    let tool_result = content
        .and_then(Value::as_array)
        .is_some_and(|parts| parts.iter().any(|part| part.get("type").and_then(Value::as_str) == Some("tool_result")));
    if tool_result {
        return;
    }
    let text = message_text(content);
    if text.starts_with("[Request interrupted") {
        reader.finish(at, None, true);
    } else if !is_automated_prompt(&text) {
        reader.ask(at, text);
    }
}

/// Reads a Codex rollout.
pub fn read_codex(data: &[u8]) -> Transcript {
    let mut reader = Reader::default();
    each_line(data, |line| {
        let Some(payload) = json::object(line.get("payload")) else { return };
        let kind = line.get("type").and_then(Value::as_str);
        if matches!(kind, Some("session_meta" | "turn_context")) {
            if kind == Some("session_meta") && reader.transcript.id.is_none() {
                reader.transcript.id = json::text(payload.get("id")).or_else(|| json::text(payload.get("session_id"))).map(str::to_owned);
            }
            if let Some(cwd) = json::text(payload.get("cwd")) {
                reader.transcript.folder = Some(cwd.to_owned());
            }
            if let Some(branch) = json::object(payload.get("git")).and_then(|git| json::text(git.get("branch"))) {
                reader.transcript.branch = Some(branch.to_owned());
            }
            return;
        }
        let Some(at) = parse_timestamp(line.get("timestamp")) else { return };
        match (kind, payload.get("type").and_then(Value::as_str)) {
            (Some("event_msg"), Some("item_completed")) => {
                let Some(item) = json::object(payload.get("item")) else { return };
                match item.get("type").and_then(Value::as_str) {
                    Some("UserMessage" | "userMessage") => codex_ask(&mut reader, at, message_text(item.get("content"))),
                    Some("CommandExecution" | "commandExecution") => {
                        let output = json::text(item.get("aggregated_output")).or_else(|| json::text(item.get("aggregatedOutput"))).unwrap_or_default().to_owned();
                        reader.printed_output(&output, at, true);
                    }
                    _ => {}
                }
            }
            (Some("event_msg"), Some("user_message")) => codex_ask(&mut reader, at, json::text(payload.get("message")).unwrap_or_default().to_owned()),
            (Some("event_msg"), Some("task_complete")) => {
                reader.finish(at, json::text(payload.get("last_agent_message")).map(str::to_owned), false);
            }
            (Some("event_msg"), Some("turn_aborted")) => reader.finish(at, None, true),
            (Some("response_item"), Some("function_call" | "custom_tool_call")) => {
                let input = payload.get("input").or_else(|| payload.get("arguments"));
                if let Some(text) = input.and_then(Value::as_str) {
                    reader.patch(text);
                    // Arguments can be JSON holding the patch, unescaped only once decoded.
                    if let Ok(Value::Object(arguments)) = serde_json::from_str::<Value>(text) {
                        for value in arguments.values() {
                            if let Some(inner) = value.as_str() {
                                reader.patch(inner);
                            }
                        }
                    }
                }
            }
            (Some("response_item"), Some("function_call_output" | "custom_tool_call_output")) => {
                let output = output_text(payload.get("output"));
                reader.printed_output(&output, at, true);
            }
            _ => {}
        }
    });
    reader.done()
}

fn codex_ask(reader: &mut Reader, at: Timestamp, text: String) {
    if !is_automated_prompt(&text) {
        reader.ask(at, text);
    }
}
