//! Claude Code's own `/usage`: Errand runs the official Claude Code and reads what it prints, as it does Codex's
//! `app-server`. The login stays Claude Code's and no model is called, so it costs no usage. It runs only when the
//! person presses refresh, never on a timer, which is why it is its own call and not part of reading usage.

use errand_core::json;
use serde_json::Value;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// Claude Code takes about two seconds to start and answer.
pub const ANSWER_TIMEOUT: Duration = Duration::from_secs(20);
/// More than `/usage` would ever print.
const ANSWER_LIMIT: u64 = 1024 * 1024;
/// Inherited settings that would make it answer for some other account.
const ACCOUNT_OVERRIDES: [&str; 3] = ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "CLAUDE_CODE_OAUTH_TOKEN"];
/// A press of refresh is not a session: it keeps no transcript and starts none of the person's hooks or servers.
const ARGUMENTS: [&str; 8] =
    ["-p", "/usage", "--output-format", "json", "--no-session-persistence", "--strict-mcp-config", "--settings", r#"{"disableAllHooks":true}"#];

/// What `/usage` printed, or None when no readable answer came in time.
pub fn usage_text(executable: &Path, timeout: Duration) -> Option<String> {
    let mut command = Command::new(executable);
    // Nothing of a project should be in play, so it does not run in one.
    command.args(ARGUMENTS).current_dir(std::env::temp_dir()).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    for key in ACCOUNT_OVERRIDES {
        command.env_remove(key);
    }
    let mut child = command.spawn().ok()?;
    let output = child.stdout.take()?;
    let (sender, answer) = mpsc::channel();
    thread::spawn(move || {
        let mut text = Vec::new();
        let _ = output.take(ANSWER_LIMIT).read_to_end(&mut text);
        let _ = sender.send(text);
    });
    let text = answer.recv_timeout(timeout);
    let _ = child.kill();
    let _ = child.wait();
    let reply: Value = serde_json::from_slice(&text.ok()?).ok()?;
    json::text(reply.get("result")).map(str::to_owned)
}
