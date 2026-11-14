//! What the task AI made of a day's work: what got done, what is left, and the same again in a report's short
//! lines. Errand keeps these summaries on this Mac, and only these, never the transcripts, so a day stays summed
//! up after the apps forget it. Everything here is pure: the request is built and the answer checked here, and
//! `errand-io` only runs the CLI and reads and writes the files.

use super::WorkRecord;
use super::view::{Callout, Journal};
use crate::sessions::text::preview;
use crate::{Language, Provider};
use jiff::Timestamp;
use jiff::civil::Date;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap};

/// What kind of work a finished task was, as the summary labels it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Feature,
    Fix,
    Refactor,
    Test,
    Config,
    Docs,
    Other,
}

impl Kind {
    pub const ALL: [Kind; 7] = [Kind::Feature, Kind::Fix, Kind::Refactor, Kind::Test, Kind::Config, Kind::Docs, Kind::Other];

    pub fn id(self) -> &'static str {
        match self {
            Kind::Feature => "feature",
            Kind::Fix => "fix",
            Kind::Refactor => "refactor",
            Kind::Test => "test",
            Kind::Config => "config",
            Kind::Docs => "docs",
            Kind::Other => "other",
        }
    }

    pub fn from_id(id: &str) -> Kind {
        Kind::ALL.into_iter().find(|kind| kind.id() == id).unwrap_or(Kind::Other)
    }

    pub fn name(self, language: Language) -> &'static str {
        match self {
            Kind::Feature => language.pick("기능 추가", "Features"),
            Kind::Fix => language.pick("버그 수정", "Fixes"),
            Kind::Refactor => language.pick("리팩터링", "Refactoring"),
            Kind::Test => language.pick("테스트", "Tests"),
            Kind::Config => language.pick("설정·배포", "Config & deploy"),
            Kind::Docs => language.pick("문서", "Docs"),
            Kind::Other => language.pick("기타", "Other"),
        }
    }
}

/// Why something is left over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Open {
    /// The agent asked the person something.
    Asking,
    Running,
    /// Done as asked, with something the agent said it did not do.
    Todo,
}

impl Open {
    pub fn id(self) -> &'static str {
        match self {
            Open::Asking => "asking",
            Open::Running => "running",
            Open::Todo => "todo",
        }
    }

    pub fn from_id(id: &str) -> Open {
        match id {
            "asking" => Open::Asking,
            "running" => Open::Running,
            _ => Open::Todo,
        }
    }
}

/// One finished piece of work in a sentence.
#[derive(Clone, Debug, PartialEq)]
pub struct Done {
    /// The task it came from; none for a line the person added.
    pub key: Option<String>,
    pub project: String,
    /// In the window's plain polite voice.
    pub text: String,
    /// The same as a report's short line.
    pub report: String,
    pub kind: Kind,
}

/// Something not finished, and why.
#[derive(Clone, Debug, PartialEq)]
pub struct Left {
    pub key: Option<String>,
    pub title: String,
    pub text: String,
    pub report: String,
    pub open: Open,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DaySummary {
    pub date: Date,
    pub made_at: Timestamp,
    /// The CLI that wrote it.
    pub by: Provider,
    pub language: Language,
    pub done: Vec<Done>,
    pub left: Vec<Left>,
    /// What the day's records looked like when it was made, so later work shows it is out of date.
    pub basis: String,
    /// The person changed it; it is not made again on its own.
    pub edited: bool,
}

/// The summaries kept, and the one being made.
#[derive(Clone, Debug, PartialEq)]
pub struct Summaries {
    pub days: BTreeMap<Date, DaySummary>,
    pub making: Option<Date>,
    /// Days whose last try failed, and why; they are not tried again on their own while Errand runs.
    pub failed: BTreeMap<Date, String>,
    /// Finished days are summed up on their own, once each.
    pub auto: bool,
    /// The summaries could not be read; nothing is written over them.
    pub unreadable: bool,
}

impl Default for Summaries {
    fn default() -> Summaries {
        Summaries { days: BTreeMap::new(), making: None, failed: BTreeMap::new(), auto: true, unreadable: false }
    }
}

/// A calendar day, as the morning bubble remembers it.
pub type NudgeDate = Date;

/// How far back finished days are summed up on their own when the window opens.
pub const AUTO_DAYS: i64 = 7;
/// The hour after which today is summed up on its own, as the day's work is mostly done by then.
pub const EVENING: i8 = 18;

/// One task as the request describes it to the AI.
#[derive(Clone, Debug, PartialEq)]
pub struct Brief {
    pub key: String,
    pub project: String,
    pub title: String,
    /// done, asking, running or stopped.
    pub state: &'static str,
    pub asks: Vec<String>,
    pub answers: Vec<String>,
    pub commits: Vec<String>,
    pub pull_requests: Vec<u32>,
    pub files: usize,
    pub minutes: i64,
}

/// Everything one day's summary is made from.
#[derive(Clone, Debug, PartialEq)]
pub struct SummaryInput {
    pub date: Date,
    pub basis: String,
    pub language: Language,
    pub tasks: Vec<Brief>,
}

/// At most this many tasks go in one request; a busier day is summed up from its latest ones.
const MOST_TASKS: usize = 30;

impl SummaryInput {
    pub fn finished(&self) -> usize {
        self.tasks.iter().filter(|task| task.state == "done").count()
    }

    /// The tasks as the JSON the prompt carries.
    pub fn tasks_json(&self) -> Value {
        Value::Array(
            self.tasks
                .iter()
                .map(|task| {
                    json!({
                        "task": task.key, "project": task.project, "title": task.title, "state": task.state,
                        "asks": task.asks, "answers": task.answers, "commits": task.commits,
                        "pull_requests": task.pull_requests, "files_changed": task.files, "minutes": task.minutes,
                    })
                })
                .collect(),
        )
    }
}

/// A short, stable fingerprint of what a day's records hold: each task and how many of its requests that day
/// ended. A request that has only started changes nothing, so new work alone does not make a summary out of date.
pub fn basis(records: &[&WorkRecord], from: Timestamp, to: Timestamp) -> String {
    let mut parts: Vec<String> = records
        .iter()
        .filter_map(|record| {
            let ended = record.turns_between(from, to).filter(|turn| turn.finished_at.is_some() || turn.aborted).count();
            (ended > 0).then(|| format!("{}:{ended}", record.key))
        })
        .collect();
    parts.sort();
    // FNV-1a: enough to tell two days' records apart, with nothing to add to the build.
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in parts.join(";").bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn clean(value: &Value, limit: usize) -> Option<String> {
    let text = value.as_str()?.trim().replace(['\n', '\r', '\t'], " ");
    let text: String = text.chars().filter(|c| !c.is_control()).collect();
    (!text.is_empty()).then(|| text.chars().take(limit).collect())
}

impl Journal<'_> {
    fn day_bounds(&self, date: Date) -> (Timestamp, Timestamp) {
        (self.start_of(date), self.start_of(date.tomorrow().unwrap_or(date)))
    }

    fn day_records(&self, date: Date) -> Vec<&WorkRecord> {
        let (from, to) = self.day_bounds(date);
        self.records.iter().filter(|record| record.turns_between(from, to).next().is_some()).collect()
    }

    /// The day's fingerprint as the records are now.
    pub fn day_basis(&self, date: Date) -> String {
        let (from, to) = self.day_bounds(date);
        basis(&self.day_records(date), from, to)
    }

    /// What a summary of `date` would be made from; none for a day with no finished work.
    pub fn summary_input(&self, date: Date) -> Option<SummaryInput> {
        let (from, to) = self.day_bounds(date);
        let mut records = self.day_records(date);
        records.sort_by_key(|record| std::cmp::Reverse(record.turns_between(from, to).last().map(|turn| turn.asked_at)));
        records.truncate(MOST_TASKS);
        let tasks: Vec<Brief> = records
            .iter()
            .map(|record| {
                let turns: Vec<_> = record.turns_between(from, to).collect();
                let last = turns.last();
                let state = match self.callout_by(record, to) {
                    Some(Callout::Asking) => "asking",
                    Some(Callout::Running) => "running",
                    _ if last.is_some_and(|turn| turn.finished_at.is_none() && !turn.aborted) => "stopped",
                    _ => "done",
                };
                let asks: Vec<String> = turns.iter().rev().take(4).rev().map(|turn| preview(&turn.ask, 200)).collect();
                let answers: Vec<String> = turns.iter().rev().filter_map(|turn| turn.result.as_deref()).take(3).collect::<Vec<_>>().into_iter().rev().map(|result| preview(result, 400)).collect();
                let commits = record.commits.iter().filter(|commit| commit.at >= from && commit.at < to).take(8).map(|commit| preview(&commit.message, 120)).collect();
                let pull_requests = record.pull_requests.iter().filter(|pull| pull.at >= from && pull.at < to).map(|pull| pull.number).collect();
                let minutes = turns.iter().filter_map(|turn| turn.duration()).map(|span| span.as_mins()).sum();
                Brief { key: record.key.clone(), project: record.project().to_owned(), title: preview(&record.title, 80), state, asks, answers, commits, pull_requests, files: record.files.len(), minutes }
            })
            .collect();
        let input = SummaryInput { date, basis: basis(&self.day_records(date), from, to), language: self.language, tasks };
        (input.finished() > 0).then_some(input)
    }

    /// The day the window should sum up next on its own, if any: today in the evening, else the latest finished
    /// day of the last week that has finished work and no summary, or one made before more work ended.
    pub fn next_to_summarize(&self, summaries: &Summaries) -> Option<Date> {
        if !summaries.auto || summaries.unreadable || summaries.making.is_some() {
            return None;
        }
        let today = self.today();
        let evening = self.now.to_zoned(self.zone.clone()).hour() >= EVENING;
        let wanted = |date: Date| {
            let failed = summaries.failed.contains_key(&date);
            let kept = summaries.days.get(&date);
            let fresh = kept.is_some_and(|summary| summary.edited || summary.basis == self.day_basis(date));
            !failed && !fresh && self.summary_input(date).is_some()
        };
        if evening && wanted(today) {
            return Some(today);
        }
        (1..=AUTO_DAYS).filter_map(|back| today.checked_sub(jiff::ToSpan::days(back)).ok()).find(|date| wanted(*date))
    }

    /// The marks a summary line carries: the commits and pull requests its task left that day.
    pub fn summary_marks(&self, date: Date, key: Option<&str>) -> String {
        let Some(record) = key.and_then(|key| self.records.iter().find(|record| record.key == key)) else { return String::new() };
        let (from, to) = self.day_bounds(date);
        let commits = record.commits.iter().filter(|commit| commit.at >= from && commit.at < to).count();
        let pulls: Vec<u32> = record.pull_requests.iter().filter(|pull| pull.at >= from && pull.at < to).map(|pull| pull.number).collect();
        self.marks(commits, &pulls)
    }

    /// A day as a report to paste: what got done by project, then what is still going and what comes next.
    pub fn daily_report(&self, summary: &DaySummary) -> String {
        let title = match self.language {
            Language::Korean => format!("{} ({}) 업무 보고", self.month_day(summary.date), self.weekday(summary.date.weekday())),
            Language::English => format!("Daily report, {}", self.month_day(summary.date)),
        };
        let mut text = format!("{title}\n");
        self.report_sections(&mut text, &[summary], false);
        text
    }

    /// A week of summaries as one report: the done lines in day order, then what is open at the latest day.
    pub fn weekly_report(&self, monday: Date, summaries: &[&DaySummary]) -> String {
        let sunday = monday.checked_add(jiff::ToSpan::days(6)).unwrap_or(monday);
        let title = match self.language {
            Language::Korean => format!("{} – {} 주간 보고", self.month_day(monday), self.month_day(sunday)),
            Language::English => format!("Weekly report, {} – {}", self.month_day(monday), self.month_day(sunday)),
        };
        let mut text = format!("{title}\n");
        self.report_sections(&mut text, summaries, true);
        text
    }

    fn report_sections(&self, text: &mut String, summaries: &[&DaySummary], weekly: bool) {
        let mut section = |heading: &str, lines: Vec<String>| {
            if !lines.is_empty() {
                text.push_str(&format!("\n[{heading}]\n"));
                for line in lines {
                    text.push_str(&format!("- {line}\n"));
                }
            }
        };
        let done: Vec<String> = summaries
            .iter()
            .flat_map(|summary| {
                summary.done.iter().map(|item| {
                    let marks = self.summary_marks(summary.date, item.key.as_deref());
                    let head = if item.project.is_empty() { String::new() } else { format!("{}: ", item.project) };
                    if marks.is_empty() { format!("{head}{}", item.report) } else { format!("{head}{} ({marks})", item.report) }
                })
            })
            .collect();
        section(self.language.pick("한 일", "Done"), done);
        // What is open is what the latest summary says is open.
        let latest = summaries.iter().max_by_key(|summary| summary.date);
        let left = |pick: fn(Open) -> bool| -> Vec<String> {
            latest
                .map(|summary| summary.left.iter().filter(|item| pick(item.open)).map(|item| format!("{}: {}", item.title, item.report)).collect())
                .unwrap_or_default()
        };
        section(self.language.pick("진행 중", "In progress"), left(|open| open != Open::Todo));
        let next = if weekly { self.language.pick("다음 주", "Next week") } else { self.language.pick("다음에 할 일", "Next") };
        section(next, left(|open| open == Open::Todo));
    }
}

impl DaySummary {
    /// Reads the AI's answer against the request: only tasks the request named, every line cleaned and capped.
    pub fn from_answer(answer: &Value, input: &SummaryInput, made_at: Timestamp, by: Provider) -> Result<DaySummary, String> {
        let tasks: HashMap<&str, &Brief> = input.tasks.iter().map(|task| (task.key.as_str(), task)).collect();
        let list = |name: &str| answer.get(name).and_then(Value::as_array).cloned().ok_or_else(|| format!("missing {name}"));
        let mut per_task: HashMap<String, usize> = HashMap::new();
        let mut done = Vec::new();
        for item in list("done")? {
            let Some(task) = item.get("task").and_then(Value::as_str).and_then(|key| tasks.get(key)) else { continue };
            let (Some(text), Some(report)) = (item.get("text").and_then(|value| clean(value, 140)), item.get("report").and_then(|value| clean(value, 140))) else { continue };
            let count = per_task.entry(task.key.clone()).or_default();
            if *count >= 2 || done.len() >= 24 {
                continue;
            }
            *count += 1;
            done.push(Done { key: Some(task.key.clone()), project: task.project.clone(), text, report, kind: Kind::from_id(item.get("kind").and_then(Value::as_str).unwrap_or("other")) });
        }
        let mut left = Vec::new();
        for item in list("left")? {
            let Some(task) = item.get("task").and_then(Value::as_str).and_then(|key| tasks.get(key)) else { continue };
            let (Some(text), Some(report)) = (item.get("text").and_then(|value| clean(value, 140)), item.get("report").and_then(|value| clean(value, 140))) else { continue };
            if left.len() >= 12 {
                break;
            }
            left.push(Left { key: Some(task.key.clone()), title: task.title.clone(), text, report, open: Open::from_id(item.get("state").and_then(Value::as_str).unwrap_or("todo")) });
        }
        if done.is_empty() {
            return Err("empty summary".into());
        }
        Ok(DaySummary { date: input.date, made_at, by, language: input.language, done, left, basis: input.basis.clone(), edited: false })
    }

    /// The summary as Errand stores it.
    pub fn to_json(&self) -> Value {
        json!({
            "version": 1,
            "date": self.date.to_string(),
            "made_at": self.made_at.to_string(),
            "by": match self.by { Provider::Codex => "codex", Provider::Claude => "claude" },
            "language": self.language.tag(),
            "basis": self.basis,
            "edited": self.edited,
            "done": self.done.iter().map(|item| json!({"task": item.key, "project": item.project, "text": item.text, "report": item.report, "kind": item.kind.id()})).collect::<Vec<_>>(),
            "left": self.left.iter().map(|item| json!({"task": item.key, "title": item.title, "text": item.text, "report": item.report, "state": item.open.id()})).collect::<Vec<_>>(),
        })
    }

    pub fn from_json(value: &Value) -> Option<DaySummary> {
        let object: &Map<String, Value> = value.as_object()?;
        if object.get("version")?.as_u64()? != 1 {
            return None;
        }
        let text = |item: &Value, name: &str| item.get(name).and_then(Value::as_str).map(str::to_owned);
        let key = |item: &Value| item.get("task").and_then(Value::as_str).map(str::to_owned);
        Some(DaySummary {
            date: object.get("date")?.as_str()?.parse().ok()?,
            made_at: object.get("made_at")?.as_str()?.parse().ok()?,
            by: if object.get("by")?.as_str()? == "claude" { Provider::Claude } else { Provider::Codex },
            language: Language::from_tag(object.get("language")?.as_str()?)?,
            basis: object.get("basis")?.as_str()?.to_owned(),
            edited: object.get("edited")?.as_bool()?,
            done: object
                .get("done")?
                .as_array()?
                .iter()
                .filter_map(|item| Some(Done { key: key(item), project: text(item, "project")?, text: text(item, "text")?, report: text(item, "report")?, kind: Kind::from_id(&text(item, "kind")?) }))
                .collect(),
            left: object
                .get("left")?
                .as_array()?
                .iter()
                .filter_map(|item| Some(Left { key: key(item), title: text(item, "title")?, text: text(item, "text")?, report: text(item, "report")?, open: Open::from_id(&text(item, "state")?) }))
                .collect(),
        })
    }

    /// The summary as plain text to edit: a heading, then a line per item as "project · sentence".
    pub fn edit_text(&self) -> String {
        let (done, left) = match self.language {
            Language::Korean => ("[한 일]", "[남은 일]"),
            Language::English => ("[Done]", "[Left]"),
        };
        let mut text = format!("{done}\n");
        for item in &self.done {
            text.push_str(&if item.project.is_empty() { format!("{}\n", item.text) } else { format!("{} · {}\n", item.project, item.text) });
        }
        text.push_str(&format!("\n{left}\n"));
        for item in &self.left {
            text.push_str(&format!("{} · {}\n", item.title, item.text));
        }
        text
    }

    /// The person's edit read back. Each line keeps the task, kind and state of the line it came from: the same
    /// words first, else the most alike line under the same project or title. A line whose words changed takes them
    /// as its report line too, a new line has no task, and a deleted line takes nothing with it.
    pub fn with_edit(&self, edited: &str) -> DaySummary {
        let mut done_lines = Vec::new();
        let mut left_lines = Vec::new();
        let mut in_left = false;
        for line in edited.lines().map(str::trim).filter(|line| !line.is_empty()) {
            if line.starts_with('[') && line.ends_with(']') {
                in_left = line.contains("남은") || line.to_ascii_lowercase().contains("left");
                continue;
            }
            let line = line.trim_start_matches(['-', '•', '*']).trim();
            let (head, words) = match line.split_once(" · ") {
                Some((head, words)) => (head.trim().to_owned(), words.trim().to_owned()),
                None => (String::new(), line.to_owned()),
            };
            if words.is_empty() {
                continue;
            }
            if in_left { left_lines.push((head, words)) } else { done_lines.push((head, words)) }
        }
        let done_old: Vec<(&str, &str)> = self.done.iter().map(|item| (item.project.as_str(), item.text.as_str())).collect();
        let done = origins(&done_lines, &done_old)
            .into_iter()
            .zip(done_lines)
            .map(|(origin, (project, text))| match origin.map(|index| &self.done[index]) {
                Some(old) => Done { report: if old.text == text { old.report.clone() } else { text.clone() }, project: if project.is_empty() { old.project.clone() } else { project }, text, ..old.clone() },
                None => Done { key: None, project, report: text.clone(), text, kind: Kind::Other },
            })
            .collect();
        let left_old: Vec<(&str, &str)> = self.left.iter().map(|item| (item.title.as_str(), item.text.as_str())).collect();
        let left = origins(&left_lines, &left_old)
            .into_iter()
            .zip(left_lines)
            .map(|(origin, (title, text))| match origin.map(|index| &self.left[index]) {
                Some(old) => Left { report: if old.text == text { old.report.clone() } else { text.clone() }, title: if title.is_empty() { old.title.clone() } else { title }, text, ..old.clone() },
                None => Left { key: None, title, report: text.clone(), text, open: Open::Todo },
            })
            .collect();
        DaySummary { done, left, edited: true, ..self.clone() }
    }
}

/// Which earlier line each edited line came from: an unused line with the same words, else the unused line under
/// the same heading whose words share the most letters with it. A line with a heading of its own that no earlier
/// line has, or too little in common, is new.
fn origins(lines: &[(String, String)], old: &[(&str, &str)]) -> Vec<Option<usize>> {
    let mut used = vec![false; old.len()];
    let mut found: Vec<Option<usize>> = vec![None; lines.len()];
    for (index, (_, text)) in lines.iter().enumerate() {
        if let Some(earlier) = (0..old.len()).find(|&earlier| !used[earlier] && old[earlier].1 == text) {
            used[earlier] = true;
            found[index] = Some(earlier);
        }
    }
    let alike = |a: &str, b: &str| {
        let mut letters: Vec<char> = b.chars().filter(|c| !c.is_whitespace()).collect();
        let shared = a.chars().filter(|c| !c.is_whitespace()).filter(|c| letters.iter().position(|other| other == c).map(|at| letters.swap_remove(at)).is_some()).count();
        shared as f64 / a.chars().chain(b.chars()).filter(|c| !c.is_whitespace()).count().max(1) as f64 * 2.0
    };
    for (index, (head, text)) in lines.iter().enumerate() {
        if found[index].is_some() {
            continue;
        }
        let best = (0..old.len())
            .filter(|&earlier| !used[earlier] && (head.is_empty() || old[earlier].0 == head))
            .map(|earlier| (earlier, alike(text, old[earlier].1)))
            .filter(|(_, score)| *score >= 0.3)
            .max_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((earlier, _)) = best {
            used[earlier] = true;
            found[index] = Some(earlier);
        }
    }
    found
}
