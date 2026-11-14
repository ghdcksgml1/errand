//! Prints how many work records the desktop window would show for the last N days (default 14), with counts only.
use errand_io::journal_scan::JournalScanner;
use errand_io::session_scan::SessionSources;
use jiff::SignedDuration;
use std::path::PathBuf;

fn main() {
    let days: i64 = std::env::args().nth(1).and_then(|arg| arg.parse().ok()).unwrap_or(14);
    let home = PathBuf::from(std::env::var("HOME").expect("HOME"));
    let mut scanner = JournalScanner::new(SessionSources {
        codex_home: home.join(".codex"),
        claude_sessions: home.join("Library/Application Support/Claude/claude-code-sessions"),
        claude_projects: home.join(".claude/projects"),
    });
    let started = std::time::Instant::now();
    let records = scanner.scan(errand_io::now() - SignedDuration::from_hours(24 * days));
    let elapsed = started.elapsed();
    let turns: usize = records.iter().map(|record| record.turns.len()).sum();
    let commits: usize = records.iter().map(|record| record.commits.len()).sum();
    let prs: usize = records.iter().map(|record| record.pull_requests.len()).sum();
    let files: usize = records.iter().map(|record| record.files.len()).sum();
    let desktop = records.iter().filter(|record| record.url.is_some()).count();
    println!("{} records ({} openable), {turns} requests, {commits} commits, {prs} PRs, {files} files, in {elapsed:?}", records.len(), desktop);
    let again = std::time::Instant::now();
    scanner.scan(errand_io::now() - SignedDuration::from_hours(24 * days));
    println!("second scan from cache: {:?}", again.elapsed());
}
