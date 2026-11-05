//! Errand's own marks on task cards: when each was checked off and when it was opened from Errand.
//! Only these times are kept, never requests or answers.

use super::rows::SessionRow;
use jiff::{SignedDuration, Timestamp};
use std::collections::BTreeMap;

/// The first launch picks up today's work instead of starting empty.
pub const BOOTSTRAP_WINDOW: SignedDuration = SignedDuration::from_hours(12);

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

    /// The cards to show, newest request first. Opening or finishing a card never moves it; only a new
    /// request brings its session to the top.
    pub fn visible(&self, rows: Vec<SessionRow>) -> Vec<VisibleSession> {
        let mut visible: Vec<VisibleSession> = rows
            .into_iter()
            .filter(|row| row.prompt_at > self.floor(&row.key))
            .map(|row| {
                let seen_at = self.entries.get(&row.key).and_then(|record| record.seen_at);
                let seen = row.finished_at.is_some_and(|finished| !row.running && (row.seen_in_app || seen_at.is_some_and(|seen| seen >= finished)));
                VisibleSession { row, seen }
            })
            .collect();
        visible.sort_by(|left, right| right.row.prompt_at.cmp(&left.row.prompt_at).then_with(|| left.row.key.cmp(&right.row.key)));
        visible
    }
}
