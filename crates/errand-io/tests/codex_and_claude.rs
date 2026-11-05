//! The accounts on this computer, and Claude's usage as Claude Code told the status line. Ported from the
//! account parts of Tests/ClaudeUsageTests.m and Tests/ProfileStoreTests.m.

use errand_core::Provider;
use errand_core::format;
use errand_core::ports::{HttpClient, HttpFailure, HttpRequest, HttpResponse};
use errand_core::usage::{Reading, UsageError};
use errand_io::api::{self, Api};
use errand_io::codex::{self, CodexSource};
use errand_io::{claude, stores};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

fn token(claims: Value) -> String {
    let payload: String = claims.to_string().bytes().collect::<Vec<_>>().chunks(3).map(encode_chunk).collect();
    format!("header.{}.signature", payload.trim_end_matches('='))
}

fn encode_chunk(chunk: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let bytes = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
    let bits = u32::from(bytes[0]) << 16 | u32::from(bytes[1]) << 8 | u32::from(bytes[2]);
    (0..4).map(|index| if index <= chunk.len() { ALPHABET[(bits >> (18 - 6 * index) & 63) as usize] as char } else { '=' }).collect()
}

#[test]
fn the_codex_account_label_comes_from_its_identity_token() {
    let home = tempfile::tempdir().unwrap();
    assert_eq!(codex::account(home.path()), codex::CodexAccount::default(), "no login, no label");
    fs::write(home.path().join("auth.json"), "original credentials").unwrap();
    assert_eq!(codex::account(home.path()).email, None, "malformed auth data neither crashes nor exposes its contents");
    let auth = json!({"tokens": {"id_token": token(json!({"email": "second@example.test", "name": "Second Person"}))}});
    fs::write(home.path().join("auth.json"), auth.to_string()).unwrap();
    let account = codex::account(home.path());
    assert_eq!((account.email.as_deref(), account.name.as_deref()), (Some("second@example.test"), Some("Second Person")));
}

#[test]
fn the_claude_account_label_and_nickname_come_from_claude_codes_config() {
    let home = tempfile::tempdir().unwrap();
    assert_eq!(claude::account(home.path()).email, None, "no Claude Code config means no label");
    fs::write(home.path().join(".claude.json"), r#"{"oauthAccount":{"emailAddress":"person@example.com"}}"#).unwrap();
    let account = claude::account(home.path());
    assert_eq!((account.email.as_deref(), account.nickname), (Some("person@example.com"), None));
    fs::write(home.path().join(".claude.json"), r#"{"oauthAccount":{"emailAddress":"person@example.com","displayName":"Sam"}}"#).unwrap();
    assert_eq!(claude::account(home.path()).nickname.as_deref(), Some("Sam"));
}

/// Reading Claude's usage must not need any of this.
struct NoNetwork;

impl HttpClient for NoNetwork {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, HttpFailure> {
        panic!("nothing is sent to read Claude's usage: {}", request.url)
    }
}

fn api(report: &Path) -> Api {
    let home = report.parent().unwrap();
    let codex = CodexSource { executable: None, home: home.join(".codex"), directory: home.to_owned() };
    Api::new(Arc::new(NoNetwork), report.to_owned(), codex)
}

#[test]
fn claude_usage_waits_for_the_status_line_to_reach_errand() {
    let folder = tempfile::tempdir().unwrap();
    let report = folder.path().join("claude-usage.plist");
    assert_eq!(api(&report).usage(Provider::Claude), Err(UsageError::ClaudeNotHeard));
    claude::keep_status_line(&report, b"not what Claude Code sends", errand_io::now()).unwrap();
    assert!(!report.exists(), "input that is not JSON changes nothing");
    claude::keep_status_line(&report, json!({"session_id": "abc"}).to_string().as_bytes(), errand_io::now()).unwrap();
    assert_eq!(api(&report).usage(Provider::Claude), Ok(Reading::default()), "connected, before the session's first answer");
}

#[test]
fn claude_usage_is_what_claude_code_last_reported_and_nothing_else_is_kept() {
    let folder = tempfile::tempdir().unwrap();
    let report = folder.path().join("nested/claude-usage.plist");
    let now = errand_io::now();
    let later = format::seconds(now).floor() + 3600.0;
    let input = json!({
        "session_id": "secret-session", "cwd": "/Users/me/secret-project", "transcript_path": "/Users/me/secret-transcript.jsonl",
        "rate_limits": {"five_hour": {"used_percentage": 2, "resets_at": later}, "seven_day": {"used_percentage": 42.5, "resets_at": later + 3600.0}},
    });
    claude::keep_status_line(&report, input.to_string().as_bytes(), now).unwrap();
    let reading = api(&report).usage(Provider::Claude).unwrap();
    assert_eq!(reading.reported_at, Some(now), "the app shows when Claude Code spoke");
    let left: Vec<_> = reading.windows.iter().map(|window| (window.identifier.as_str(), window.remaining_percent, window.resets_at)).collect();
    assert_eq!(left, [("five_hour", Some(98), Some(later)), ("seven_day", Some(57), Some(later + 3600.0))]);
    let kept = String::from_utf8_lossy(&fs::read(&report).unwrap()).into_owned();
    assert!(!kept.contains("secret"), "only the limits are kept");
}

#[test]
fn each_status_line_run_adds_to_the_record() {
    let folder = tempfile::tempdir().unwrap();
    let report = folder.path().join("claude-usage.plist");
    let deadline = format::seconds(errand_io::now()).floor() + 3600.0;
    let run = |used: i64| {
        let input = json!({"rate_limits": {"five_hour": {"used_percentage": used, "resets_at": deadline}}});
        claude::keep_status_line(&report, input.to_string().as_bytes(), errand_io::now()).unwrap();
        stores::read_claude_report(&report).unwrap().windows[0].remaining_percent
    };
    assert_eq!(run(40), Some(60));
    assert_eq!(run(10), Some(60), "an idle session repeating an old share takes nothing back, across runs too");
    assert_eq!(run(50), Some(50));
}

#[test]
fn an_unreadable_record_reads_as_not_connected_and_the_next_run_replaces_it() {
    let folder = tempfile::tempdir().unwrap();
    let report = folder.path().join("claude-usage.plist");
    fs::write(&report, "not a property list").unwrap();
    assert_eq!(api(&report).usage(Provider::Claude), Err(UsageError::ClaudeNotHeard));
    claude::keep_status_line(&report, b"{}", errand_io::now()).unwrap();
    assert_eq!(api(&report).usage(Provider::Claude), Ok(Reading::default()));
}

#[test]
fn retry_after_reads_seconds_or_a_date_and_waits_fifteen_minutes_at_most() {
    // 2015-10-21T07:28:00Z.
    let now = UNIX_EPOCH + Duration::from_secs(1_445_412_480);
    assert_eq!(api::retry_after(" 120 ", now), Some(Duration::from_secs(120)));
    assert_eq!(api::retry_after("Wed, 21 Oct 2015 07:30:00 GMT", now), Some(Duration::from_secs(120)));
    assert_eq!(api::retry_after("Wed, 21 Oct 2015 07:00:00 GMT", now), Some(Duration::ZERO), "a date already past asks for no wait");
    assert_eq!(api::retry_after("86400", now), Some(Duration::from_secs(15 * 60)));
    assert_eq!(api::retry_after("soon", now), None);
}
