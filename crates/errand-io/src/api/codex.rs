//! Codex: its usage through the official `app-server` API, spoken to in JSON lines. The login and its
//! renewal stay the official app's; Errand never copies or changes Codex's tokens.

use crate::codex::CodexSource;
use errand_core::usage::UsageError;
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

/// How long one request may take.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// An answer without a line break this long is not an answer.
const LINE_LIMIT: usize = 8 * 1024 * 1024;
/// Inherited settings that would make the API sign in as someone else, or read another state database.
const ACCOUNT_OVERRIDES: [&str; 7] = [
    "OPENAI_API_KEY",
    "CODEX_API_KEY",
    "CODEX_ACCESS_TOKEN",
    "OPENAI_IDENTITY_TOKEN_FILE",
    "OPENAI_IDENTITY_PROVIDER_ID",
    "CODEX_SQLITE_HOME",
    "CODEX_ELECTRON_AGENT_RUN_ID",
];

/// A running `codex app-server --stdio`, spoken to in JSON lines.
pub struct AppServer {
    child: Child,
    input: Option<ChildStdin>,
    lines: Receiver<Vec<u8>>,
    serial: i64,
}

impl AppServer {
    pub fn start(source: &CodexSource) -> Result<AppServer, UsageError> {
        let executable = source.executable.as_deref().ok_or(UsageError::CodexMissing)?;
        let mut command = Command::new(executable);
        command.args(["app-server", "--stdio"]).current_dir(&source.directory).env("CODEX_HOME", &source.home);
        for key in ACCOUNT_OVERRIDES {
            command.env_remove(key);
        }
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // No console window flashes up for the API.
            command.creation_flags(0x0800_0000);
        }
        let mut child = command.spawn().map_err(|error| UsageError::CodexLaunch(error.to_string()))?;
        let input = child.stdin.take();
        let mut output = child.stdout.take().ok_or(UsageError::CodexClosed)?;
        let (sender, lines) = mpsc::channel();
        thread::spawn(move || {
            let mut pending = Vec::new();
            let mut chunk = [0u8; 8192];
            while let Ok(count) = output.read(&mut chunk) {
                if count == 0 {
                    break;
                }
                pending.extend_from_slice(&chunk[..count]);
                while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
                    let line: Vec<u8> = pending.drain(..=end).collect();
                    if sender.send(line).is_err() {
                        return;
                    }
                }
                if pending.len() > LINE_LIMIT {
                    break;
                }
            }
        });
        let mut server = AppServer { child, input, lines, serial: 0 };
        server.request("initialize", json!({"clientInfo": {"name": "errand", "version": env!("CARGO_PKG_VERSION")}, "capabilities": {"experimentalApi": true}}))?;
        server.write(br#"{"method":"initialized"}"#)?;
        Ok(server)
    }

    fn write(&mut self, message: &[u8]) -> Result<(), UsageError> {
        let input = self.input.as_mut().ok_or(UsageError::CodexClosed)?;
        let mut line = message.to_vec();
        line.push(b'\n');
        input.write_all(&line).and_then(|()| input.flush()).map_err(|_| UsageError::CodexClosed)
    }

    /// One request and its result object.
    pub fn request(&mut self, method: &str, params: Value) -> Result<Value, UsageError> {
        self.serial += 1;
        let id = self.serial;
        self.write(json!({"id": id, "method": method, "params": params}).to_string().as_bytes())?;
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let line = match self.lines.recv_timeout(left) {
                Ok(line) => line,
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => break,
            };
            let Ok(Value::Object(response)) = serde_json::from_slice::<Value>(&line) else { continue };
            if response.get("id").and_then(Value::as_i64) != Some(id) {
                continue;
            }
            return match response.get("result") {
                Some(result @ Value::Object(_)) if response.get("error").is_none() => Ok(result.clone()),
                _ => Err(UsageError::CodexFailed(method.to_owned())),
            };
        }
        Err(UsageError::CodexNoAnswer(method.to_owned()))
    }
}

impl Drop for AppServer {
    fn drop(&mut self) {
        drop(self.input.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The signed-in account's `account/rateLimits/read` result. Reading usage never sends a model request.
pub fn rate_limits(source: &CodexSource) -> Result<Value, UsageError> {
    if !source.home.exists() {
        return Err(UsageError::CodexSignIn);
    }
    AppServer::start(source)?.request("account/rateLimits/read", json!({"excludeResetCreditDetails": true}))
}
