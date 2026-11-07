//! Errand's own marks on task cards: when each was checked off and when it was opened from Errand.
//! Only these times are kept, never requests or answers.

use super::rows::SessionRow;
use jiff::{SignedDuration, Timestamp};
use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::iter;

/// The first launch picks up today's work instead of starting empty.
pub const BOOTSTRAP_WINDOW: SignedDuration = SignedDuration::from_hours(12);
/// How many sessions up Errand follows a sub-task, as far as the Claude app itself does.
const FAMILY_DEPTH: usize = 32;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SessionRecord {
    pub completed_at: Option<Timestamp>,
    pub seen_at: Option<Timestamp>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionRecords {
    /// When Errand first tracked sessions; kept across launches so old work does not flood back.
    pub tracking_start: Timestamp,
    entries: BTreeMap<String, SessionRecord>,
}

/// A card the tray shows: its session and whether its latest answer has been seen.
#[derive(Clone, Debug, PartialEq)]
pub struct VisibleSession {
    pub row: SessionRow,
    pub seen: bool,
}

/// One card's sessions: the session leading it, and the sub-tasks that show as lines in it.
#[derive(Clone, Debug, PartialEq)]
pub struct Family<'a> {
    pub head: &'a VisibleSession,
    pub sub_tasks: Vec<&'a VisibleSession>,
}

impl Family<'_> {
    fn sessions(&self) -> impl Iterator<Item = &VisibleSession> {
        iter::once(self.head).chain(self.sub_tasks.iter().copied())
    }
}

/// The session whose card this one shows in: the furthest it came from among those shown. None leads a card of its
/// own, which a session whose parent is not shown does, and so does one that turns out to be its own ancestor.
fn head_of<'a>(session: &VisibleSession, shown: &'a [VisibleSession]) -> Option<&'a VisibleSession> {
    let mut head = None;
    let mut at = session;
    for _ in 0..FAMILY_DEPTH {
        let Some(parent) = at.row.parent.as_deref().and_then(|key| shown.iter().find(|other| other.row.key == key)) else { break };
        if parent.row.key == session.row.key {
            return None;
        }
        head = Some(parent);
        at = parent;
    }
    head
}

/// The cards the sessions make, in the order the sessions leading them come; each sub-task keeps its order too.
pub fn families(shown: &[VisibleSession]) -> Vec<Family<'_>> {
    let mut families: Vec<Family> = shown.iter().filter(|session| head_of(session, shown).is_none()).map(|head| Family { head, sub_tasks: Vec::new() }).collect();
    for session in shown {
        if let Some(head) = head_of(session, shown)
            && let Some(family) = families.iter_mut().find(|family| family.head.row.key == head.row.key)
        {
            family.sub_tasks.push(session);
        }
    }
    families
}

impl SessionRecords {
    pub fn new(tracking_start: Timestamp) -> SessionRecords {
        SessionRecords { tracking_start, entries: BTreeMap::new() }
    }

    pub fn insert_loaded(&mut self, key: &str, record: SessionRecord) {
        self.entries.insert(key.to_owned(), record);
    }

    pub fn entries(&self) -> impl Iterator<Item = (&str, &SessionRecord)> {
        self.entries.iter().map(|(key, record)| (key.as_str(), record))
    }

    pub fn complete(&mut self, key: &str, at: Timestamp) {
        self.entries.entry(key.to_owned()).or_default().completed_at = Some(at);
    }

    pub fn mark_seen(&mut self, key: &str, at: Timestamp) {
        self.entries.entry(key.to_owned()).or_default().seen_at = Some(at);
    }

    /// Requests at or before this cannot show: the card was checked off after them, or they predate tracking.
    pub fn floor(&self, key: &str) -> Timestamp {
        let bootstrap = self.tracking_start - BOOTSTRAP_WINDOW;
        match self.entries.get(key).and_then(|record| record.completed_at) {
            Some(completed) if completed > bootstrap => completed,
            _ => bootstrap,
        }
    }

    /// Its latest answer has been read: in the app, or since it landed by Errand's own mark.
    pub fn seen(&self, row: &SessionRow) -> bool {
        let seen_at = self.entries.get(&row.key).and_then(|record| record.seen_at);
        row.finished_at.is_some_and(|finished| !row.running && (row.seen_in_app || seen_at.is_some_and(|seen| seen >= finished)))
    }

    /// The cards to show, newest request first. Opening or finishing a card never moves it; only a new
    /// request brings its session to the top. A session's sub-tasks stay with it, and the newest request among
    /// them says where they all go.
    pub fn visible(&self, rows: Vec<SessionRow>) -> Vec<VisibleSession> {
        let mut visible: Vec<VisibleSession> = rows
            .into_iter()
            .filter(|row| row.prompt_at > self.floor(&row.key))
            .map(|row| {
                let seen = self.seen(&row);
                VisibleSession { row, seen }
            })
            .collect();
        visible.sort_by(|left, right| right.row.prompt_at.cmp(&left.row.prompt_at).then_with(|| left.row.key.cmp(&right.row.key)));
        let mut cards = families(&visible);
        cards.sort_by_key(|family| Reverse(family.sessions().map(|session| session.row.prompt_at).max()));
        let order: Vec<String> = cards.iter().flat_map(Family::sessions).map(|session| session.row.key.clone()).collect();
        visible.sort_by_key(|session| order.iter().position(|key| *key == session.row.key));
        visible
    }
}
