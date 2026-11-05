//! The command Claude Code runs for its status line, run the way Claude Code runs it.
#![cfg(target_os = "macos")]

use errand_io::stores;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn report(home: &Path) -> PathBuf {
    home.join("Library/Application Support/Errand/claude-usage.plist")
}

fn status_line(home: &Path, input: &str) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_errand"));
    let command = command.arg("--claude-status-line").env("HOME", home).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn it_keeps_the_limits_and_leaves_the_line_empty() {
    let home = tempfile::tempdir().unwrap();
    let input = r#"{"session_id": "abc", "rate_limits": {"five_hour": {"used_percentage": 2, "resets_at": 4102444800}, "seven_day": {"used_percentage": 42, "resets_at": 4102531200}}}"#;
    let output = status_line(home.path(), input);
    assert!(output.status.success());
    assert!(output.stdout.is_empty() && output.stderr.is_empty(), "whatever it printed would show in Claude Code");
    let kept = stores::read_claude_report(&report(home.path())).unwrap();
    assert!(kept.reported_at.is_some());
    assert_eq!(kept.windows.iter().map(|window| window.remaining_percent).collect::<Vec<_>>(), [Some(98), Some(58)]);
}

#[test]
fn it_never_fails_on_what_it_is_given() {
    let home = tempfile::tempdir().unwrap();
    for input in ["", "not json"] {
        let output = status_line(home.path(), input);
        assert!(output.status.success() && output.stdout.is_empty() && output.stderr.is_empty(), "{input:?}");
    }
    assert!(!report(home.path()).exists());
}
