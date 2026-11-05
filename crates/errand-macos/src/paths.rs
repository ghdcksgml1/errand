//! Where things live on a Mac.

use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Paths {
    pub home: PathBuf,
}

impl Paths {
    /// The signed-in user's; `HOME` decides, so a test can point it elsewhere.
    pub fn current() -> Paths {
        Paths { home: std::env::home_dir().unwrap_or_else(|| PathBuf::from("/")) }
    }

    /// Errand's own records.
    pub fn app_support(&self) -> PathBuf {
        self.home.join("Library/Application Support/Errand")
    }

    /// Where the app kept its records while it was called Dual Codex.
    pub fn legacy_app_support(&self) -> PathBuf {
        self.home.join("Library/Application Support/Dual Codex")
    }

    pub fn reset_history(&self) -> PathBuf {
        self.app_support().join("pet-history.plist")
    }

    pub fn session_tray(&self) -> PathBuf {
        self.app_support().join("session-tray.plist")
    }

    /// The work rooms the person is in, to join again on the next launch.
    pub fn joined_rooms(&self) -> PathBuf {
        self.app_support().join("rooms.plist")
    }

    /// What Claude Code last told the status line.
    pub fn claude_report(&self) -> PathBuf {
        self.app_support().join("claude-usage.plist")
    }

    /// Codex's login, sessions and settings.
    pub fn codex_home(&self) -> PathBuf {
        self.home.join(".codex")
    }

    /// The Claude desktop app's records of its Code sessions.
    pub fn claude_sessions(&self) -> PathBuf {
        self.home.join("Library/Application Support/Claude/claude-code-sessions")
    }

    /// Claude Code's transcripts.
    pub fn claude_projects(&self) -> PathBuf {
        self.home.join(".claude/projects")
    }
}
