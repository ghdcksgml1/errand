//! Reads whole transcripts for the desktop window's records, read-only like the widget's scanner. Errand keeps
//! nothing of them on disk: what the window shows is read again from the apps' own files, and lasts as long as
//! they do.

use crate::session_scan::{SessionSources, codex_threads, leading_integer, list, open_read_only};
use errand_core::journal::{Transcript, WorkRecord, read_claude, read_codex};
use errand_core::sessions::ClaudeMeta;
use errand_core::{Provider, Timestamp};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// What the Claude desktop app knows about a terminal transcript it runs: the widget's key, a title and a link.
struct DesktopSession {
    key: String,
    title: Option<String>,
    url: Option<String>,
}

/// Keeps every transcript it read until the file changes. Use it from one thread.
pub struct JournalScanner {
    sources: SessionSources,
    read: HashMap<PathBuf, (u64, Option<Timestamp>, Transcript)>,
}

impl JournalScanner {
    pub fn new(sources: SessionSources) -> JournalScanner {
        JournalScanner { sources, read: HashMap::new() }
    }

    /// Every session that asked or answered anything since `since`, oldest first.
    pub fn scan(&mut self, since: Timestamp) -> Vec<WorkRecord> {
        let mut records = self.claude(since);
        records.extend(self.codex(since));
        records.retain(|record| record.turns.iter().any(|turn| turn.asked_at >= since || turn.finished_at.is_some_and(|done| done >= since)));
        records.sort_by_key(|record| record.started_at());
        records
    }

    fn transcript(&mut self, path: &Path, read: fn(&[u8]) -> Transcript) -> Option<Transcript> {
        let attributes = fs::metadata(path).ok()?;
        let modified = attributes.modified().ok().and_then(|time| Timestamp::try_from(time).ok());
        if let Some((length, when, transcript)) = self.read.get(path)
            && *length == attributes.len()
            && *when == modified
        {
            return Some(transcript.clone());
        }
        let transcript = read(&fs::read(path).ok()?);
        self.read.insert(path.to_owned(), (attributes.len(), modified, transcript.clone()));
        Some(transcript)
    }

    /// The desktop app's sessions by the terminal transcript each one runs.
    fn desktop_sessions(&self) -> HashMap<String, DesktopSession> {
        let root = &self.sources.claude_sessions;
        let mut sessions = HashMap::new();
        for account in list(root) {
            for organization in list(&root.join(&account)) {
                let folder = root.join(&account).join(&organization);
                for name in list(&folder) {
                    let Some(stem) = name.strip_prefix("local_").and(name.strip_suffix(".json")) else { continue };
                    let Ok(bytes) = fs::read(folder.join(&name)) else { continue };
                    let Some(meta) = serde_json::from_slice(&bytes).ok().as_ref().and_then(ClaudeMeta::from_json) else { continue };
                    let Some(cli) = meta.cli_session_id.clone() else { continue };
                    sessions.insert(cli, DesktopSession {
                        key: format!("claude:{stem}"),
                        title: meta.title.clone(),
                        url: meta.session_id.as_ref().map(|session| format!("claude://code/continue?session={session}")),
                    });
                }
            }
        }
        sessions
    }

    fn claude(&mut self, since: Timestamp) -> Vec<WorkRecord> {
        let desktop = self.desktop_sessions();
        let projects = self.sources.claude_projects.clone();
        let mut records = Vec::new();
        for project in list(&projects) {
            let folder = projects.join(&project);
            for name in list(&folder) {
                let Some(stem) = name.strip_suffix(".jsonl") else { continue };
                let path = folder.join(&name);
                let recent = fs::metadata(&path).and_then(|attributes| attributes.modified()).ok().and_then(|time| Timestamp::try_from(time).ok());
                if recent.is_none_or(|modified| modified < since) {
                    continue;
                }
                let Some(transcript) = self.transcript(&path, read_claude) else { continue };
                if transcript.turns.is_empty() {
                    continue;
                }
                let id = transcript.id.clone().unwrap_or_else(|| stem.to_owned());
                let record = match desktop.get(&id) {
                    Some(session) => WorkRecord::from_transcript(session.key.clone(), Provider::Claude, transcript, session.title.clone(), session.url.clone()),
                    None => WorkRecord::from_transcript(format!("claude-code:{id}"), Provider::Claude, transcript, None, None),
                };
                records.push(record);
            }
        }
        records
    }

    fn codex(&mut self, since: Timestamp) -> Vec<WorkRecord> {
        let Some(database) = codex_database(&self.sources.codex_home) else { return Vec::new() };
        let Some(db) = open_read_only(&database) else { return Vec::new() };
        let (threads, _) = codex_threads(&db, since.as_second());
        drop(db);
        let mut records = Vec::new();
        // An agent a thread spawned is part of its parent's work, as on the widget's cards.
        for thread in threads.into_iter().filter(|thread| thread.parent_thread_id.is_none()) {
            let Some(transcript) = self.transcript(Path::new(&thread.rollout_path), read_codex) else { continue };
            if transcript.turns.is_empty() {
                continue;
            }
            let mut transcript = transcript;
            if transcript.folder.as_deref().is_none_or(str::is_empty) && !thread.cwd.is_empty() {
                transcript.folder = Some(thread.cwd.clone());
            }
            let title = Some(thread.title.clone()).filter(|title| !title.is_empty());
            records.push(WorkRecord::from_transcript(format!("codex:{}", thread.id), Provider::Codex, transcript, title, Some(format!("codex://threads/{}", thread.id))));
        }
        records
    }
}

/// The newest `state_N.sqlite` in Codex's home, by number.
fn codex_database(home: &Path) -> Option<PathBuf> {
    let mut best: Option<(i64, PathBuf)> = None;
    for name in list(home) {
        let Some(version) = name.strip_prefix("state_").and_then(|rest| rest.strip_suffix(".sqlite")).map(leading_integer) else { continue };
        if best.as_ref().is_none_or(|(highest, _)| version > *highest) {
            best = Some((version, home.join(&name)));
        }
    }
    best.map(|(_, path)| path)
}
