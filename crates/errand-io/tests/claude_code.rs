//! Asking a stand-in Claude Code for `/usage`, as a press of refresh does.
#![cfg(unix)]

use errand_core::Provider;
use errand_core::format;
use errand_core::ports::{HttpClient, HttpFailure, HttpRequest, HttpResponse};
use errand_core::usage::UsageError;
use errand_io::api::{self, Api};
use errand_io::claude;
use errand_io::codex::CodexSource;
use serde_json::json;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Far enough ahead to outlive this file.
const ANSWER: &str = "Current session: 4% used · resets Dec 31, 2099 at 3:50am (Asia/Seoul)\nCurrent week (all models): 42% used · resets Dec 31, 2099 at 5am (Asia/Seoul)\nCurrent week (Fable): 26% used · resets Dec 31, 2099 at 5am (Asia/Seoul)\n";

/// Asking Claude Code must not need any of this.
struct NoNetwork;

impl HttpClient for NoNetwork {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, HttpFailure> {
        panic!("Errand itself sends nothing to ask Claude Code: {}", request.url)
    }
}

/// A stand-in that notes how it was run and prints `reply`, as `claude --output-format json` does.
fn stand_in(folder: &Path, reply: &str) -> PathBuf {
    let executable = folder.join("claude");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{0}/arguments\"\nprintf '%s' \"$ANTHROPIC_API_KEY\" > \"{0}/key\"\npwd -P > \"{0}/directory\"\ncat <<'REPLY'\n{reply}\nREPLY\n",
        folder.display()
    );
    fs::write(&executable, script).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    executable
}

fn reply(text: &str) -> String {
    json!({"type": "result", "subtype": "success", "num_turns": 0, "total_cost_usd": 0, "result": text}).to_string()
}

fn api(folder: &Path, executable: Option<PathBuf>) -> Api {
    let codex = CodexSource { executable: None, home: folder.join(".codex"), directory: folder.to_owned() };
    Api::new(Arc::new(NoNetwork), folder.join("claude-usage.plist"), codex).with_claude_code(move || executable.clone())
}

#[test]
fn asking_runs_claude_code_as_no_session_and_keeps_what_it_says() {
    let folder = tempfile::tempdir().unwrap();
    let executable = stand_in(folder.path(), &reply(ANSWER));
    // SAFETY: set before any thread of this test reads the environment.
    unsafe { std::env::set_var("ANTHROPIC_API_KEY", "would-answer-for-someone-else") };
    let before = errand_io::now();
    let reading = api(folder.path(), Some(executable)).ask_claude_code().unwrap();
    assert!(reading.reported_at.is_some_and(|at| at >= before), "the card says it is this fresh");
    let left: Vec<_> = reading.windows.iter().map(|window| (window.identifier.as_str(), window.remaining_percent)).collect();
    assert_eq!(left, [("five_hour", Some(96)), ("seven_day", Some(58)), ("limits/Fable", Some(74))]);

    let arguments = fs::read_to_string(folder.path().join("arguments")).unwrap();
    assert_eq!(
        arguments.lines().collect::<Vec<_>>(),
        ["-p", "/usage", "--output-format", "json", "--no-session-persistence", "--strict-mcp-config", "--settings", r#"{"disableAllHooks":true}"#],
        "no transcript, and none of the person's hooks or servers"
    );
    assert_eq!(fs::read_to_string(folder.path().join("key")).unwrap(), "", "inherited keys cannot pick another account");
    let directory = fs::read_to_string(folder.path().join("directory")).unwrap();
    assert_eq!(Path::new(directory.trim()), fs::canonicalize(std::env::temp_dir()).unwrap(), "it does not run in any project");

    let later = api(folder.path(), None).usage(Provider::Claude).unwrap();
    assert_eq!(later.windows, reading.windows, "the timer reads what was kept, models included");
}

#[test]
fn silence_shows_what_is_on_record_and_only_fails_with_nothing_to_show() {
    let folder = tempfile::tempdir().unwrap();
    let silent = stand_in(folder.path(), &reply("Unknown skill: usage"));
    assert_eq!(api(folder.path(), Some(silent.clone())).ask_claude_code(), Err(UsageError::ClaudeAskFailed), "no record, no answer");
    assert_eq!(api(folder.path(), None).ask_claude_code(), Err(UsageError::ClaudeAskFailed), "no Claude Code to ask");

    let report = folder.path().join("claude-usage.plist");
    claude::keep_status_line(&report, b"{}", errand_io::now()).unwrap();
    assert_eq!(api(folder.path(), Some(silent.clone())).ask_claude_code(), Err(UsageError::ClaudeAskFailed), "connected, with no limits yet");

    let now = errand_io::now();
    let later = format::seconds(now).floor() + 3600.0;
    let input = json!({"rate_limits": {"five_hour": {"used_percentage": 2, "resets_at": later}}});
    claude::keep_status_line(&report, input.to_string().as_bytes(), now).unwrap();
    let reading = api(folder.path(), Some(silent)).ask_claude_code().unwrap();
    assert_eq!(reading.reported_at, Some(now), "a Claude Code too old for `/usage` does not get in the way of the status line");
    assert_eq!(reading.windows.len(), 1);
}

#[test]
fn an_answer_that_is_not_json_or_never_comes_is_no_answer() {
    let folder = tempfile::tempdir().unwrap();
    let garbled = stand_in(folder.path(), "not json");
    assert_eq!(api::claude::usage_text(&garbled, Duration::from_secs(5)), None);
    let hung = folder.path().join("hung");
    fs::write(&hung, "#!/bin/sh\nexec sleep 60\n").unwrap();
    fs::set_permissions(&hung, fs::Permissions::from_mode(0o755)).unwrap();
    let started = Instant::now();
    assert_eq!(api::claude::usage_text(&hung, Duration::from_millis(200)), None);
    assert!(started.elapsed() < Duration::from_secs(5), "it does not wait out a stuck Claude Code");
    assert_eq!(api::claude::usage_text(&folder.path().join("missing"), Duration::from_secs(1)), None);
}
