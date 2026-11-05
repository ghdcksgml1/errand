//! One task card's facts, built from what each app records about a session.

use super::fold::Turn;
use super::text::{last_path_component, preview};
use crate::{Provider, json};
use jiff::{SignedDuration, Timestamp};
use serde_json::Value;

/// Running work that has written nothing for this long probably needs a look.
pub const STALL_INTERVAL: SignedDuration = SignedDuration::from_mins(20);

/// A session as the apps record it, before Errand's own checked and opened marks.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionRow {
    /// `codex:<thread>` or `claude:<local session file>`.
    pub key: String,
    pub provider: Provider,
    pub title: String,
    /// The person's latest request, one line.
    pub prompt: String,
    pub prompt_at: Timestamp,
    pub running: bool,
    pub aborted: bool,
    pub finished_at: Option<Timestamp>,
    pub result: Option<String>,
    /// What the app says the person should do next.
    pub action: Option<String>,
    pub folder: String,
    /// Opens the session in its app.
    pub url: String,
    /// The answer was already read in the app itself.
    pub seen_in_app: bool,
    pub stalled: bool,
}

fn stalled(turn: &Turn, modified: Option<Timestamp>, now: Timestamp) -> bool {
    turn.running && modified.is_some_and(|modified| now.duration_since(modified) > STALL_INTERVAL)
}

/// A user thread from Codex's state database.
#[derive(Clone, Debug, PartialEq)]
pub struct CodexThread {
    pub id: String,
    pub rollout_path: String,
    /// The name, else the title, else the first message.
    pub title: String,
    pub first_message: String,
    pub cwd: String,
    pub updated_at: Timestamp,
}

/// `read` means Codex has a read receipt with no unread turn; `modified` is the rollout file's.
pub fn codex_row(thread: &CodexThread, turn: &Turn, modified: Option<Timestamp>, read: bool, now: Timestamp) -> SessionRow {
    SessionRow {
        key: format!("{}{}", Provider::Codex.session_key_prefix(), thread.id),
        provider: Provider::Codex,
        title: preview(&thread.title, 80),
        prompt: preview(turn.prompt.as_deref().unwrap_or(&thread.first_message), 280),
        prompt_at: turn.prompt_at.or(turn.started_at).unwrap_or(thread.updated_at),
        running: turn.running,
        aborted: turn.aborted,
        finished_at: turn.finished_at,
        result: turn.result.as_deref().map(|result| preview(result, 200)),
        action: None,
        folder: last_path_component(&thread.cwd).to_owned(),
        url: format!("codex://threads/{}", thread.id),
        seen_in_app: read,
        stalled: stalled(turn, modified, now),
    }
}

/// The Claude desktop app's record of one Code session.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClaudeMeta {
    pub session_id: Option<String>,
    /// The Claude Code session whose transcript holds the turns.
    pub cli_session_id: Option<String>,
    pub cwd: Option<String>,
    pub title: Option<String>,
    pub last_assistant_uuid: Option<String>,
    /// The answer the app's one-line summary describes.
    pub post_turn_summary_for: Option<String>,
    pub created_at: Option<Timestamp>,
    pub latest_user_frame_at: Option<Timestamp>,
    pub last_focused_at: Option<Timestamp>,
    pub summary: Option<String>,
    pub action: Option<String>,
}

fn milliseconds(value: Option<&Value>) -> Option<Timestamp> {
    json::number(value).filter(|ms| *ms > 0.0).and_then(|ms| crate::format::timestamp_from_seconds(ms / 1000.0))
}

impl ClaudeMeta {
    pub fn from_json(json: &Value) -> Option<ClaudeMeta> {
        let object = json.as_object()?;
        let text = |key: &str| json::text(object.get(key)).map(str::to_owned);
        // The app stores some turn summaries as JSON text inside its JSON.
        let summary = match object.get("postTurnSummary") {
            Some(Value::String(inner)) => serde_json::from_str::<Value>(inner).ok(),
            other => other.cloned(),
        };
        let summary = summary.as_ref().and_then(Value::as_object);
        Some(ClaudeMeta {
            session_id: text("sessionId"),
            cli_session_id: text("cliSessionId"),
            cwd: text("cwd"),
            title: text("title"),
            last_assistant_uuid: text("lastAssistantUuid"),
            post_turn_summary_for: text("postTurnSummaryFor"),
            created_at: milliseconds(object.get("createdAt")),
            latest_user_frame_at: milliseconds(object.get("latestUserFrameAt")),
            last_focused_at: milliseconds(object.get("lastFocusedAt")),
            summary: summary.and_then(|summary| json::text(summary.get("status_detail"))).map(str::to_owned),
            action: summary.and_then(|summary| json::text(summary.get("needs_action"))).map(str::to_owned),
        })
    }
}

/// `name` is the session file's name without `.json`; `modified` is that file's, and `transcript_modified`
/// the transcript's. A row needs a session ID to open.
pub fn claude_row(
    name: &str,
    meta: &ClaudeMeta,
    turn: &Turn,
    modified: Timestamp,
    transcript_modified: Option<Timestamp>,
    now: Timestamp,
) -> Option<SessionRow> {
    let session_id = meta.session_id.as_deref()?;
    let folder = meta.cwd.as_deref().map(last_path_component);
    // The app's own one-line summary is used only when it describes the latest answer.
    let summary_current = !turn.running && meta.summary.is_some() && meta.post_turn_summary_for.is_some() && meta.post_turn_summary_for == meta.last_assistant_uuid;
    let result = if summary_current { meta.summary.as_deref() } else { turn.result.as_deref() };
    Some(SessionRow {
        key: format!("{}{name}", Provider::Claude.session_key_prefix()),
        provider: Provider::Claude,
        // Without a title or a folder the view names it as a Claude session, in the person's language.
        title: preview(meta.title.as_deref().or(folder).unwrap_or(""), 80),
        prompt: preview(turn.prompt.as_deref().or(turn.last_prompt.as_deref()).unwrap_or(""), 280),
        prompt_at: turn.prompt_at.or(meta.latest_user_frame_at).or(meta.created_at).unwrap_or(modified),
        running: turn.running,
        aborted: turn.aborted,
        finished_at: turn.finished_at,
        result: result.map(|result| preview(result, 200)),
        action: meta.action.as_deref().filter(|_| summary_current).map(|action| preview(action, 160)),
        folder: folder.unwrap_or("").to_owned(),
        url: format!("claude://code/continue?session={session_id}"),
        seen_in_app: turn.finished_at.zip(meta.last_focused_at).is_some_and(|(finished, focused)| focused >= finished),
        stalled: stalled(turn, transcript_modified, now),
    })
}
