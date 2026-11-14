//! Finds the person's sessions in what Codex and Claude record on this computer, reading only. The apps'
//! files are never written, and Codex's database never gains `-wal` or `-shm` files from being read.

mod codex_history;

use errand_core::Timestamp;
use errand_core::sessions::{
    ClaudeMeta, CodexThread, SessionRecords, SessionRow, Turn, claude_row, codex_row, fold_claude, fold_codex, text::claude_project_directory,
};
use jiff::SignedDuration;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A first read starts this far from the end of a record, and goes further back only when that holds no turn.
const SHORT_TAIL: u64 = 512 * 1024;
const LONG_TAIL: u64 = 8 * 1024 * 1024;

/// Where each app keeps its records.
#[derive(Clone, Debug)]
pub struct SessionSources {
    /// Codex's home: `state_N.sqlite` and the rollouts it names.
    pub codex_home: PathBuf,
    /// The Claude desktop app's `claude-code-sessions` folder.
    pub claude_sessions: PathBuf,
    /// Claude Code's `projects` folder of transcripts.
    pub claude_projects: PathBuf,
}

#[derive(Clone, Copy)]
enum Format {
    Codex,
    Claude,
}

/// Scans keep what they read: each record is folded only for the bytes appended since the last scan, so long
/// transcripts stay cheap to follow. Use it from one thread.
pub struct SessionScanner {
    sources: SessionSources,
    turns: HashMap<PathBuf, (u64, Turn)>,
    metadata: HashMap<PathBuf, (Timestamp, ClaudeMeta)>,
    transcripts: HashMap<String, PathBuf>,
}

impl SessionScanner {
    pub fn new(sources: SessionSources) -> SessionScanner {
        SessionScanner { sources, turns: HashMap::new(), metadata: HashMap::new(), transcripts: HashMap::new() }
    }

    /// Every recent session in both apps. `records` decides how far back to look; pass a snapshot of the
    /// tray's records and filter the result with the current ones.
    pub fn scan(&mut self, records: &SessionRecords, now: Timestamp) -> Vec<SessionRow> {
        let mut rows = self.codex_sessions(records, now);
        rows.extend(self.claude_sessions(records, now));
        rows
    }

    /// The latest turn in a record and when the record last changed.
    fn turn_at(&mut self, path: &Path, format: Format) -> Option<(Turn, Option<Timestamp>)> {
        let attributes = fs::metadata(path).ok()?;
        let size = attributes.len();
        let modified = attributes.modified().ok().and_then(|time| Timestamp::try_from(time).ok());
        let fold = |turn: &mut Turn, data: &[u8]| match format {
            Format::Codex => fold_codex(turn, data),
            Format::Claude => fold_claude(turn, data),
        };
        let (consumed, turn) = match self.turns.get(path) {
            Some((consumed, cached)) if size >= *consumed && size - consumed <= LONG_TAIL => {
                let mut turn = cached.clone();
                let mut consumed = *consumed;
                if size > consumed {
                    let chunk = read_range(path, consumed, size - consumed);
                    let complete = last_newline(&chunk);
                    fold(&mut turn, &chunk[..complete]);
                    consumed += complete as u64;
                }
                (consumed, turn)
            }
            _ => {
                let mut result = (0, Turn::default());
                for limit in [SHORT_TAIL, LONG_TAIL] {
                    let offset = size.saturating_sub(limit);
                    let chunk = read_range(path, offset, size - offset);
                    let end = last_newline(&chunk);
                    // Reading from the middle of a record starts inside a line.
                    let begin = if offset > 0 { chunk[..end].iter().position(|byte| *byte == b'\n').map_or(end, |newline| newline + 1) } else { 0 };
                    let mut turn = Turn::default();
                    fold(&mut turn, &chunk[begin..end]);
                    let found = turn.prompt_at.is_some() || turn.started_at.is_some();
                    result = (offset + end as u64, turn);
                    if found || offset == 0 {
                        break;
                    }
                }
                result
            }
        };
        self.turns.insert(path.to_owned(), (consumed, turn.clone()));
        Some((turn, modified))
    }

    /// The newest `state_N.sqlite` in Codex's home, by number.
    fn codex_database(&self) -> Option<PathBuf> {
        let entries = fs::read_dir(&self.sources.codex_home).ok()?;
        let mut best: Option<(i64, PathBuf)> = None;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(version) = name.to_str().and_then(|name| name.strip_prefix("state_")?.strip_suffix(".sqlite")).map(leading_integer) else {
                continue;
            };
            if best.as_ref().is_none_or(|(highest, _)| version > *highest) {
                best = Some((version, entry.path()));
            }
        }
        best.map(|(_, path)| path)
    }

    fn codex_sessions(&mut self, records: &SessionRecords, now: Timestamp) -> Vec<SessionRow> {
        let Some(path) = self.codex_database() else { return Vec::new() };
        let Some(db) = open_read_only(&path) else { return Vec::new() };
        let since = (records.tracking_start - errand_core::sessions::records::BOOTSTRAP_WINDOW).as_second();
        let (threads, read) = codex_threads(&db, since);
        drop(db);
        // Newer Codex versions compress old rollouts after projecting them into this database.
        let history = open_read_only(&self.sources.codex_home.join("thread_history_1.sqlite"));
        let mut rows = Vec::new();
        for thread in threads {
            let key = format!("codex:{}", thread.id);
            // The database stores whole seconds; anything not newer cannot hold a newer prompt.
            if thread.updated_at < records.floor(&key) - SignedDuration::from_secs(1) {
                continue;
            }
            let (turn, modified) = self.turn_at(Path::new(&thread.rollout_path), Format::Codex)
                .filter(|(turn, _)| turn.started_at.is_some() || turn.prompt_at.is_some() || turn.finished_at.is_some() || turn.question.is_some())
                .or_else(|| history.as_ref().and_then(|db| codex_history::latest_turn(db, &thread.id)))
                .unwrap_or_default();
            rows.push(codex_row(&thread, &turn, modified, read.contains(&thread.id), now));
        }
        rows
    }

    fn claude_metadata(&mut self, path: &Path, modified: Timestamp) -> Option<ClaudeMeta> {
        if let Some((cached, meta)) = self.metadata.get(path)
            && *cached == modified
        {
            return Some(meta.clone());
        }
        let json: serde_json::Value = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
        let meta = ClaudeMeta::from_json(&json)?;
        self.metadata.insert(path.to_owned(), (modified, meta.clone()));
        Some(meta)
    }

    /// Claude Code keeps a transcript in its working directory's folder; one moved elsewhere is still found.
    fn transcript(&mut self, session: &str, cwd: Option<&str>) -> Option<PathBuf> {
        if let Some(cached) = self.transcripts.get(session)
            && cached.exists()
        {
            return Some(cached.clone());
        }
        let name = format!("{session}.jsonl");
        let mut folders: Vec<String> = cwd.filter(|cwd| !cwd.is_empty()).map(claude_project_directory).into_iter().collect();
        folders.extend(list(&self.sources.claude_projects));
        let found = folders.into_iter().map(|folder| self.sources.claude_projects.join(folder).join(&name)).find(|candidate| candidate.exists())?;
        self.transcripts.insert(session.to_owned(), found.clone());
        Some(found)
    }

    /// The session files, newest first, with when each last changed.
    fn claude_files(&self) -> Vec<(Timestamp, PathBuf)> {
        let root = &self.sources.claude_sessions;
        let mut files = Vec::new();
        for account in list(root) {
            for organization in list(&root.join(&account)) {
                let folder = root.join(&account).join(&organization);
                for name in list(&folder) {
                    let path = folder.join(&name);
                    if name.starts_with("local_")
                        && name.ends_with(".json")
                        && let Some(modified) = fs::metadata(&path).and_then(|attributes| attributes.modified()).ok().and_then(|time| Timestamp::try_from(time).ok())
                    {
                        files.push((modified, path));
                    }
                }
            }
        }
        files.sort_by_key(|(modified, _)| Reverse(*modified));
        files
    }

    fn claude_sessions(&mut self, records: &SessionRecords, now: Timestamp) -> Vec<SessionRow> {
        let mut rows = Vec::new();
        // The session the app put on screen last, which may be one no card shows.
        let mut on_screen: Option<(Timestamp, String)> = None;
        for (modified, path) in self.claude_files() {
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else { continue };
            let key = format!("claude:{stem}");
            // Every new prompt rewrites the session file, so an untouched file cannot hold one.
            let current = modified > records.floor(&key);
            // Coming on screen rewrites it too, so a file older than the newest of those says nothing new.
            if !current && on_screen.as_ref().is_some_and(|(focused, _)| modified < *focused) {
                continue;
            }
            let Some(meta) = self.claude_metadata(&path, modified) else { continue };
            if let Some(focused) = meta.last_focused_at
                && on_screen.as_ref().is_none_or(|(newest, _)| focused > *newest)
            {
                on_screen = Some((focused, key));
            }
            if !current || meta.session_id.is_none() {
                continue;
            }
            let transcript = meta.cli_session_id.clone().and_then(|cli| self.transcript(&cli, meta.cwd.as_deref()));
            let (turn, transcript_modified) = transcript.and_then(|path| self.turn_at(&path, Format::Claude)).unwrap_or_default();
            rows.extend(claude_row(stem, &meta, &turn, modified, transcript_modified, now));
        }
        if let Some((_, key)) = on_screen
            && let Some(row) = rows.iter_mut().find(|row| row.key == key)
        {
            row.on_screen = true;
        }
        rows
    }
}

/// The names in a folder; nothing when it cannot be read.
pub(crate) fn list(folder: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(folder) else { return Vec::new() };
    entries.flatten().filter_map(|entry| entry.file_name().into_string().ok()).collect()
}

/// `length` bytes from `offset`, or fewer at the end; nothing when the file cannot be read.
fn read_range(path: &Path, offset: u64, length: u64) -> Vec<u8> {
    let mut data = Vec::new();
    if let Ok(mut file) = File::open(path)
        && file.seek(SeekFrom::Start(offset)).is_ok()
    {
        let _ = file.take(length).read_to_end(&mut data);
    }
    data
}

/// The length up to and including the last newline: only whole lines are folded.
fn last_newline(data: &[u8]) -> usize {
    data.iter().rposition(|byte| *byte == b'\n').map_or(0, |index| index + 1)
}

/// The number a name starts with, as Foundation's `integerValue` reads it; 0 without one.
pub(crate) fn leading_integer(text: &str) -> i64 {
    let text = text.trim_start();
    let (sign, digits) = match text.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, text.strip_prefix('+').unwrap_or(text)),
    };
    let digits: String = digits.chars().take_while(char::is_ascii_digit).collect();
    digits.parse::<i64>().map_or(0, |value| sign * value)
}

/// Opens Codex's database without creating its `-wal` or `-shm` files. A live database with a WAL is read
/// read-only; a checkpointed one is read as an immutable snapshot.
pub(crate) fn open_read_only(path: &Path) -> Option<Connection> {
    let wal = PathBuf::from(format!("{}-wal", path.display())).exists();
    let options: &[&str] = if wal { &["mode=ro", "immutable=1"] } else { &["immutable=1"] };
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    options.iter().find_map(|option| {
        let db = Connection::open_with_flags(format!("file:{}?{option}", uri_path(path)), flags).ok()?;
        db.busy_timeout(Duration::from_millis(250)).ok()?;
        Some(db)
    })
}

/// A file path as a `file:` URI path, with what a URI cannot hold percent-encoded.
fn uri_path(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut encoded = String::new();
    if !text.starts_with('/') {
        // A Windows drive path becomes file:///C:/...
        encoded.push('/');
    }
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => encoded.push(byte as char),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    format!("//{encoded}")
}

fn text(value: ValueRef<'_>) -> Option<String> {
    match value {
        ValueRef::Text(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
        ValueRef::Integer(number) => Some(number.to_string()),
        ValueRef::Real(number) => Some(number.to_string()),
        ValueRef::Blob(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
        ValueRef::Null => None,
    }
}

/// The person's threads and explicitly spawned agents, plus the threads Codex has marked read.
/// Guardian, review and other internal agents stay out; a structured spawn source names a real parent.
pub(crate) fn codex_threads(db: &Connection, since: i64) -> (Vec<CodexThread>, HashSet<String>) {
    let query = "SELECT id, rollout_path, COALESCE(NULLIF(name, ''), NULLIF(title, ''), first_user_message), first_user_message, cwd, updated_at, source, thread_source \
                 FROM threads WHERE updated_at >= ?";
    let mut threads = Vec::new();
    if let Ok(mut statement) = db.prepare(query)
        && let Ok(mut rows) = statement.query([since])
    {
        while let Ok(Some(row)) = rows.next() {
            let column = |index: usize| row.get_ref(index).ok().and_then(text);
            let (Some(id), Some(rollout_path)) = (column(0).filter(|id| !id.is_empty()), column(1).filter(|path| !path.is_empty())) else { continue };
            let source = column(6).unwrap_or_default();
            let kind = column(7).unwrap_or_default();
            let source_json = serde_json::from_str::<serde_json::Value>(&source).ok();
            let subagent = source_json.as_ref().and_then(|source| source.get("subagent"));
            let parent_thread_id = subagent.and_then(|agent| agent.get("thread_spawn"))
                .and_then(|spawn| spawn.get("parent_thread_id")).and_then(serde_json::Value::as_str)
                .filter(|parent| !parent.is_empty() && *parent != id).map(str::to_owned);
            let user = subagent.is_none() && (kind == "user" || (kind.is_empty() && matches!(source.as_str(), "vscode" | "cli" | "exec")));
            if !user && parent_thread_id.is_none() {
                continue;
            }
            let updated = match row.get_ref(5) {
                Ok(ValueRef::Integer(seconds)) => seconds,
                Ok(ValueRef::Real(seconds)) => seconds as i64,
                _ => 0,
            };
            threads.push(CodexThread {
                id,
                rollout_path,
                title: column(2).unwrap_or_default(),
                first_message: column(3).unwrap_or_default(),
                cwd: column(4).unwrap_or_default(),
                updated_at: Timestamp::from_second(updated).unwrap_or(Timestamp::UNIX_EPOCH),
                parent_thread_id,
            });
        }
    }
    // A receipt with no first unread turn means the thread was read in Codex.
    let mut read = HashSet::new();
    if let Ok(mut statement) = db.prepare("SELECT thread_id FROM thread_read_receipts WHERE first_unread_turn IS NULL")
        && let Ok(mut rows) = statement.query([])
    {
        while let Ok(Some(row)) = rows.next() {
            read.extend(row.get_ref(0).ok().and_then(text));
        }
    }
    (threads, read)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_versions_read_like_foundation_integers() {
        assert_eq!([leading_integer("12"), leading_integer("5x"), leading_integer(""), leading_integer("-3")], [12, 5, 0, -3]);
    }

    #[test]
    fn database_paths_become_uris() {
        assert_eq!(uri_path(Path::new("/Users/me/My Codex/state_5.sqlite")), "///Users/me/My%20Codex/state_5.sqlite");
        assert_eq!(uri_path(Path::new(r"C:\Users\me\.codex\state_5.sqlite")), "///C:/Users/me/.codex/state_5.sqlite");
    }
}
