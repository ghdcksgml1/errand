//! The work record: everything a session did, read whole from what Codex and Claude keep on this computer.
//! The desktop window draws its days, weeks, months and reports from these. Nothing here is stored by
//! Errand; a record lasts as long as the app's own transcript does.

mod parse;
pub mod view;

pub use parse::{read_claude, read_codex};

use crate::Provider;
use jiff::{SignedDuration, Timestamp};

/// One request the person made and how it ended.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkTurn {
    pub asked_at: Timestamp,
    pub ask: String,
    pub finished_at: Option<Timestamp>,
    /// The agent's last words for the turn.
    pub result: Option<String>,
    pub aborted: bool,
}

impl WorkTurn {
    /// How long the agent worked on it; nothing while it runs.
    pub fn duration(&self) -> Option<SignedDuration> {
        self.finished_at.map(|finished| finished.duration_since(self.asked_at)).filter(|span| span.is_positive())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Commit {
    /// As short as the tool printed it.
    pub sha: String,
    pub message: String,
    pub at: Timestamp,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PullRequest {
    pub number: u32,
    pub url: String,
    pub at: Timestamp,
}

/// A file the session changed, with every edit to it added up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    pub added: u32,
    pub removed: u32,
    /// The session made the file.
    pub created: bool,
}

/// What one transcript says, before Errand knows which app session it belongs to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Transcript {
    /// The app's own id for the conversation.
    pub id: Option<String>,
    /// The title the app gave it, or the person did.
    pub title: Option<String>,
    pub folder: Option<String>,
    pub branch: Option<String>,
    pub turns: Vec<WorkTurn>,
    pub commits: Vec<Commit>,
    pub pull_requests: Vec<PullRequest>,
    pub files: Vec<FileChange>,
}

/// A session as the desktop window shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkRecord {
    /// The widget's key for the same session where it has one, so the two agree on its state.
    pub key: String,
    pub provider: Provider,
    pub title: String,
    pub folder: String,
    pub branch: Option<String>,
    /// Where the app opens it; none for a terminal session the app cannot open.
    pub url: Option<String>,
    pub turns: Vec<WorkTurn>,
    pub commits: Vec<Commit>,
    pub pull_requests: Vec<PullRequest>,
    pub files: Vec<FileChange>,
}

impl WorkRecord {
    pub fn from_transcript(key: String, provider: Provider, transcript: Transcript, title: Option<String>, url: Option<String>) -> WorkRecord {
        let folder = transcript.folder.clone().unwrap_or_default();
        let title = title
            .filter(|title| !title.trim().is_empty())
            .or(transcript.title)
            .or_else(|| transcript.turns.first().map(|turn| first_line(&turn.ask)))
            .unwrap_or_default();
        WorkRecord {
            key,
            provider,
            title,
            folder,
            branch: transcript.branch,
            url,
            turns: transcript.turns,
            commits: transcript.commits,
            pull_requests: transcript.pull_requests,
            files: transcript.files,
        }
    }

    pub fn started_at(&self) -> Option<Timestamp> {
        self.turns.first().map(|turn| turn.asked_at)
    }

    /// When the last answer came; nothing while the latest request runs.
    pub fn ended_at(&self) -> Option<Timestamp> {
        self.turns.last().and_then(|turn| turn.finished_at)
    }

    /// The time the agents spent working, request by request.
    pub fn working_time(&self) -> SignedDuration {
        self.turns.iter().filter_map(WorkTurn::duration).fold(SignedDuration::ZERO, |total, span| total + span)
    }

    /// The folder's last component, which the window calls the project. A Claude Code worktree counts as the
    /// repository it was cut from.
    pub fn project(&self) -> &str {
        let folder = self.folder.find("/.claude/worktrees/").map_or(self.folder.as_str(), |index| &self.folder[..index]);
        crate::sessions::text::last_path_component(folder)
    }

    /// The turns asked within `[from, to)`.
    pub fn turns_between(&self, from: Timestamp, to: Timestamp) -> impl Iterator<Item = &WorkTurn> {
        self.turns.iter().filter(move |turn| turn.asked_at >= from && turn.asked_at < to)
    }
}

/// The first line, trimmed, for a title made from a request.
fn first_line(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
    crate::sessions::text::preview(line, 80)
}
