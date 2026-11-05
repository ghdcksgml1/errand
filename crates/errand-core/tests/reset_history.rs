//! Ported from the history parts of Tests/PetStateTests.m. Codex stands in for the old account "a" and
//! Claude for "b"; the rules do not depend on which service it is.

use errand_core::format::timestamp_from_seconds;
use errand_core::reset_history::ResetHistory;
use errand_core::usage::{Span, UsageWindow, WindowName};
use errand_core::{Provider, Timestamp};

fn at(seconds: f64) -> Timestamp {
    timestamp_from_seconds(seconds).unwrap()
}

fn record(history: &mut ResetHistory, provider: Provider, identifier: &str, deadline: Option<f64>, now: f64) -> bool {
    let window = UsageWindow { identifier: identifier.to_owned(), name: WindowName::span(Span::Numbered(1)), remaining_percent: Some(50), resets_at: deadline };
    history.record(provider, &[window], at(now))
}

const A: Provider = Provider::Codex;
const B: Provider = Provider::Claude;

#[test]
fn a_deadline_rollover_records_when_it_was_observed() {
    let mut history = ResetHistory::new();
    record(&mut history, A, "codex/primary", Some(100.0), 90.0);
    assert_eq!(history.last_reset(A, "codex/primary"), None, "a first observation creates only a baseline");
    record(&mut history, B, "codex/primary", Some(80.0), 90.0);
    assert_eq!(history.last_reset(B, "codex/primary"), None, "a first expired deadline still does not fabricate a reset");
    record(&mut history, A, "codex/primary", Some(200.0), 110.0);
    assert_eq!(history.last_reset(A, "codex/primary"), Some(at(110.0)), "a rollover records the actual observation time");
    assert_eq!(history.last_reset(B, "codex/primary"), None, "a reset in one service does not leak to another");
    assert_eq!(history.last_reset(A, "codex/secondary"), None, "a reset in one window does not leak to another");
    record(&mut history, A, "codex/primary", Some(200.0), 150.0);
    assert_eq!(history.last_reset(A, "codex/primary"), Some(at(110.0)), "repeated deadlines keep the original reset time");
    record(&mut history, A, "codex/primary", Some(300.0), 200.0);
    assert_eq!(history.last_reset(A, "codex/primary"), Some(at(200.0)), "a deadline reached exactly now permits a rollover");
}

#[test]
fn missing_and_invalid_deadlines_preserve_history() {
    let mut history = ResetHistory::new();
    record(&mut history, A, "codex/primary", Some(100.0), 90.0);
    record(&mut history, A, "codex/primary", Some(300.0), 200.0);
    for deadline in [None, Some(f64::NAN), Some(-1.0), Some(0.0)] {
        assert!(!record(&mut history, A, "codex/primary", deadline, 250.0), "{deadline:?} changes nothing");
    }
    assert!(!history.record(A, &[], at(250.0)));
    assert_eq!(history.last_reset(A, "codex/primary"), Some(at(200.0)));
    record(&mut history, A, "codex/primary", Some(400.0), 310.0);
    assert_eq!(history.last_reset(A, "codex/primary"), Some(at(310.0)), "missing data does not erase the baseline for a later rollover");
    record(&mut history, A, "codex/primary", Some(350.0), 320.0);
    assert_eq!(history.last_reset(A, "codex/primary"), Some(at(310.0)), "a deadline moving backward is not a reset");
    record(&mut history, A, "codex/primary", Some(500.0), 360.0);
    assert_eq!(history.last_reset(A, "codex/primary"), Some(at(360.0)), "a backward correction becomes the baseline for the next rollover");
}

#[test]
fn windows_keep_independent_histories() {
    let mut history = ResetHistory::new();
    let window = |identifier: &str, deadline: f64| UsageWindow {
        identifier: identifier.to_owned(),
        name: WindowName::span(Span::Numbered(1)),
        remaining_percent: Some(80),
        resets_at: Some(deadline),
    };
    history.record(A, &[window("codex/secondary", 100.0), window("model/primary", 100.0)], at(90.0));
    history.record(A, &[window("codex/secondary", 200.0), window("model/primary", 100.0)], at(120.0));
    assert_eq!(history.last_reset(A, "codex/secondary"), Some(at(120.0)));
    assert_eq!(history.last_reset(A, "model/primary"), None);
}

#[test]
fn future_corrections_are_not_resets() {
    let mut history = ResetHistory::new();
    record(&mut history, A, "codex/primary", Some(200.0), 100.0);
    record(&mut history, A, "codex/primary", Some(300.0), 150.0);
    assert_eq!(history.last_reset(A, "codex/primary"), None, "a future deadline correction is not a reset");
    record(&mut history, A, "codex/primary", Some(400.0), 250.0);
    assert_eq!(history.last_reset(A, "codex/primary"), None, "corrections replace the baseline instead of keeping an expired one");
    record(&mut history, A, "codex/primary", Some(500.0), 450.0);
    assert_eq!(history.last_reset(A, "codex/primary"), Some(at(450.0)), "a real rollover after corrections is observed normally");
    record(&mut history, B, "codex/primary", None, 100.0);
    record(&mut history, B, "codex/primary", Some(100.0), 150.0);
    assert_eq!(history.last_reset(B, "codex/primary"), None, "an invalid first deadline does not invent a baseline");
}

#[test]
fn empty_identifiers_and_malformed_loaded_records_create_no_history() {
    let mut history = ResetHistory::new();
    record(&mut history, A, "", Some(100.0), 90.0);
    record(&mut history, A, "", Some(200.0), 110.0);
    assert_eq!(history.last_reset(A, ""), None);
    history.insert_loaded("", "codex/primary", 100.0, None);
    history.insert_loaded("a", "", 100.0, None);
    history.insert_loaded("a", "codex/primary", f64::NAN, Some(at(10.0)));
    history.insert_loaded("c", "five_hour", 100.0, Some(Timestamp::UNIX_EPOCH));
    assert_eq!(history.records().count(), 1, "only the valid record is kept");
    assert_eq!(history.last_reset(B, "five_hour"), None, "a zero reset time is dropped");
    record(&mut history, B, "five_hour", Some(200.0), 150.0);
    assert_eq!(history.last_reset(B, "five_hour"), Some(at(150.0)), "a loaded baseline detects the next rollover");
}

#[test]
fn records_of_the_retired_account_are_kept_as_loaded() {
    let mut history = ResetHistory::new();
    history.insert_loaded("b", "codex/primary", 100.0, Some(at(50.0)));
    record(&mut history, A, "codex/primary", Some(100.0), 90.0);
    let saved: Vec<(&str, &str)> = history.records().map(|(account, identifier, _)| (account, identifier)).collect();
    assert_eq!(saved, [("a", "codex/primary"), ("b", "codex/primary")]);
}
