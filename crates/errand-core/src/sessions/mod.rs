//! The tasks the person handed Codex and Claude, read from what each app records.

pub mod fold;
pub mod records;
pub mod rows;
pub mod text;

pub use fold::{Question, Turn, fold_claude, fold_codex};
pub use records::{Family, SessionRecord, SessionRecords, VisibleSession, families};
pub use rows::{ClaudeMeta, CodexThread, SessionRow, claude_row, codex_row};

/// What the person's cards come to, as rooms hear them: answers to check, with stalled requests and questions
/// waiting on the person among them, requests running, and answers opened but not yet checked off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TaskCounts {
    pub new: usize,
    pub running: usize,
    pub seen: usize,
}

/// Where a card stands, in the order a glance needs: something wrong, working, then results.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SessionState {
    /// An answer not yet opened.
    New,
    /// An answer already opened, here or in the app.
    Seen,
    Running,
    /// Running, and waiting on the person to answer a question.
    Asking,
    /// Running, but silent for a long time.
    Stalled,
    Aborted,
    /// Nothing says how the turn ended.
    Unknown,
    /// Checked off and waiting to clear; a card shows this in place of its own state.
    Done,
}

impl SessionState {
    pub fn of(session: &VisibleSession) -> SessionState {
        let row = &session.row;
        if row.stalled {
            SessionState::Stalled
        } else if row.running && row.question.is_some() {
            SessionState::Asking
        } else if row.running {
            SessionState::Running
        } else if row.aborted {
            SessionState::Aborted
        } else if row.finished_at.is_none() {
            SessionState::Unknown
        } else if session.seen {
            SessionState::Seen
        } else {
            SessionState::New
        }
    }

    /// The Lucide icon on its chip.
    pub fn icon(self) -> &'static str {
        match self {
            SessionState::New => "circle-check",
            SessionState::Seen => "check-check",
            SessionState::Running => "loader-circle",
            SessionState::Asking => "message-circle-question-mark",
            SessionState::Stalled => "circle-alert",
            SessionState::Aborted => "circle-slash",
            SessionState::Unknown => "circle-question-mark",
            SessionState::Done => "check",
        }
    }
}
