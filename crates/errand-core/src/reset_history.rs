//! When each limit was seen to refill. The services report only the next refill, so a reset is recorded
//! when a refill time that had already passed gives way to a later one, at the moment Errand noticed.

use crate::{Provider, format, usage::UsageWindow};
use jiff::Timestamp;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResetRecord {
    /// The refill time last reported, Unix seconds.
    pub resets_at: f64,
    /// When Errand saw that refill happen.
    pub last_reset: Option<Timestamp>,
}

/// Records by storage key (`a`, `c`) and window identifier. Keys of retired accounts loaded from disk are
/// kept as they were.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResetHistory {
    accounts: BTreeMap<String, BTreeMap<String, ResetRecord>>,
}

fn valid_seconds(seconds: f64) -> bool {
    seconds.is_finite() && seconds > 0.0
}

impl ResetHistory {
    pub fn new() -> ResetHistory {
        ResetHistory::default()
    }

    /// A record read back from storage. Malformed ones are dropped rather than trusted.
    pub fn insert_loaded(&mut self, account: &str, identifier: &str, resets_at: f64, last_reset: Option<Timestamp>) {
        if account.is_empty() || identifier.is_empty() || !valid_seconds(resets_at) {
            return;
        }
        let last_reset = last_reset.filter(|at| valid_seconds(format::seconds(*at)));
        self.accounts.entry(account.to_owned()).or_default().insert(identifier.to_owned(), ResetRecord { resets_at, last_reset });
    }

    /// Notes the refill times just read. Returns whether anything changed, so the caller saves only then.
    pub fn record(&mut self, provider: Provider, windows: &[UsageWindow], at: Timestamp) -> bool {
        let now = format::seconds(at);
        if !valid_seconds(now) {
            return false;
        }
        let mut changed = false;
        for window in windows {
            let Some(next) = window.valid_reset() else { continue };
            if window.identifier.is_empty() {
                continue;
            }
            let records = self.accounts.entry(provider.storage_key().to_owned()).or_default();
            let previous = records.get(&window.identifier).copied();
            if previous.is_some_and(|previous| previous.resets_at == next) {
                continue;
            }
            let mut record = previous.unwrap_or(ResetRecord { resets_at: next, last_reset: None });
            // A deadline moving later before the old one passed is a correction, not a reset.
            if let Some(previous) = previous
                && next > previous.resets_at
                && previous.resets_at <= now
            {
                record.last_reset = Some(at);
            }
            record.resets_at = next;
            records.insert(window.identifier.clone(), record);
            changed = true;
        }
        changed
    }

    pub fn last_reset(&self, provider: Provider, identifier: &str) -> Option<Timestamp> {
        self.accounts.get(provider.storage_key())?.get(identifier)?.last_reset
    }

    /// Every record as (storage key, window identifier, record), for saving.
    pub fn records(&self) -> impl Iterator<Item = (&str, &str, &ResetRecord)> {
        self.accounts
            .iter()
            .flat_map(|(account, records)| records.iter().map(move |(identifier, record)| (account.as_str(), identifier.as_str(), record)))
    }
}
