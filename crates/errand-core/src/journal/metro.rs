//! The week as a metro map: a line per project, a stop per task on each day it was worked. The project worked on
//! longest each day takes the top lane, so the lines cross where the week's attention moved. Pull requests are
//! interchanges, work waiting on the person's answer is a dashed stop, and lines with work still open run on to a
//! terminal at the end.

use super::WorkRecord;
use super::view::{Callout, Journal};
use crate::Language;
use jiff::civil::Date;
use jiff::{SignedDuration, ToSpan};
use std::collections::HashSet;

/// Lines with a colour of their own; any further project shares one grey line.
pub const COLOURED_LINES: usize = 4;
/// Stops with their name written, per line and day, so a busy day stays readable.
const NAMED_PER_DAY: usize = 2;
/// The hours a day's stretch on the map covers.
const FIRST_HOUR: f64 = 8.0;
const HOURS: f64 = 12.0;

#[derive(Clone, Debug, PartialEq)]
pub struct MetroDay {
    pub label: String,
    /// The AI time worked that day.
    pub total: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MetroLine {
    pub name: String,
    /// Its colour, by slot; none for the grey line.
    pub colour: Option<usize>,
    /// Its lane on each day of the map, 0 at the top.
    pub lanes: Vec<usize>,
    /// The line's week for the legend: its tasks and time.
    pub legend: String,
    /// Tasks on it still open at the week's end, which run on to the terminal.
    pub carried: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MetroStop {
    pub key: String,
    pub line: usize,
    /// Index into the map's days.
    pub day: usize,
    /// When the day's work on it started, as a fraction of 08:00–20:00.
    pub at: f64,
    pub minutes: i64,
    pub title: String,
    pub pull: Option<String>,
    pub asking: bool,
    /// Its name above the line, below it, or left out on a busy day.
    pub name_above: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MetroView {
    pub title: String,
    pub note: String,
    pub days: Vec<MetroDay>,
    pub lines: Vec<MetroLine>,
    pub stops: Vec<MetroStop>,
    /// The word on the terminal: the next week, or what is left of this one.
    pub terminal: String,
    /// How many tasks run on to it.
    pub terminal_count: String,
    /// The tag beside a stop waiting on the person's answer.
    pub waiting: String,
    /// The legend's words for a stop, an interchange, a waiting stop, a line's end and the run to the terminal.
    pub keys: [String; 5],
}

impl MetroView {
    pub fn is_empty(&self) -> bool {
        self.stops.is_empty()
    }
}

impl Journal<'_> {
    /// `open` holds the tasks the week's latest summary says are left.
    pub(super) fn metro(&self, monday: Date, records: &[&WorkRecord], open: &HashSet<&str>, current: bool, ready: bool) -> MetroView {
        let pick = |korean: &'static str, english: &'static str| self.language.pick(korean, english);
        // Each record's work, day by day: when it began that day, how long the agent worked, and what it left.
        struct Visit<'r> {
            record: &'r WorkRecord,
            day: usize,
            at: f64,
            minutes: i64,
            pull: Option<u32>,
        }
        let week_end = self.start_of(monday.checked_add(7.days()).unwrap_or(monday));
        let mut days = Vec::new();
        let mut visits: Vec<Visit> = Vec::new();
        for offset in 0..7 {
            let date = monday.checked_add(offset.days()).unwrap_or(monday);
            let (from, to) = (self.start_of(date), self.start_of(date.tomorrow().unwrap_or(date)));
            let mut worked = SignedDuration::ZERO;
            let mut any = false;
            for record in records {
                let turns: Vec<_> = record.turns_between(from, to).collect();
                let Some(first) = turns.first() else { continue };
                any = true;
                let mut span = SignedDuration::ZERO;
                for turn in &turns {
                    span += match turn.finished_at {
                        Some(end) => turn.asked_at.duration_until(end),
                        // Work still going counts up to now, within the day.
                        None if self.running(record) && record.turns.last().is_some_and(|last| std::ptr::eq(last, *turn)) => turn.asked_at.duration_until(self.now.min(to)),
                        None => SignedDuration::ZERO,
                    }
                    .max(SignedDuration::ZERO);
                }
                worked += span;
                let hour = from.duration_until(first.asked_at).as_secs_f64() / 3600.0;
                visits.push(Visit {
                    record,
                    day: days.len(),
                    at: ((hour - FIRST_HOUR) / HOURS).clamp(0.0, 1.0),
                    minutes: span.as_mins(),
                    pull: record.pull_requests.iter().find(|pull| pull.at >= from && pull.at < to).map(|pull| pull.number),
                });
            }
            if any {
                let label = match self.language {
                    Language::Korean => format!("{} {}일", self.weekday_long(date.weekday()), date.day()),
                    Language::English => format!("{} {}", self.weekday(date.weekday()), date.day()),
                };
                days.push(MetroDay { label, total: self.duration(worked) });
            }
        }

        // Projects by their week's time; the first few get a colour, the rest share the grey line.
        let mut projects: Vec<(&str, i64)> = Vec::new();
        for visit in &visits {
            let project = visit.record.project();
            match projects.iter_mut().find(|(name, _)| *name == project) {
                Some((_, minutes)) => *minutes += visit.minutes,
                None => projects.push((project, visit.minutes)),
            }
        }
        projects.sort_by(|a, b| b.1.cmp(&a.1));
        let gathered = projects.len() > COLOURED_LINES + 1;
        let line_of = |project: &str| projects.iter().position(|(name, _)| *name == project).unwrap_or(0).min(if gathered { COLOURED_LINES } else { usize::MAX });
        let count = if gathered { COLOURED_LINES + 1 } else { projects.len() };

        let mut lines: Vec<MetroLine> = (0..count)
            .map(|index| {
                let on_line: Vec<&Visit> = visits.iter().filter(|visit| line_of(visit.record.project()) == index).collect();
                let mut keys: Vec<&str> = on_line.iter().map(|visit| visit.record.key.as_str()).collect();
                keys.sort_unstable();
                keys.dedup();
                let minutes: i64 = on_line.iter().map(|visit| visit.minutes).sum();
                let name = if gathered && index == COLOURED_LINES {
                    match self.language {
                        Language::Korean => format!("그 밖 {}개", projects.len() - COLOURED_LINES),
                        Language::English => format!("{} more", projects.len() - COLOURED_LINES),
                    }
                } else {
                    projects[index].0.to_owned()
                };
                let carried = keys
                    .iter()
                    .filter(|key| {
                        open.contains(**key)
                            || on_line.iter().any(|visit| visit.record.key == **key && matches!(self.callout_by(visit.record, week_end), Some(Callout::Asking | Callout::Running)))
                    })
                    .count();
                let time = self.duration(SignedDuration::from_mins(minutes));
                let legend = match self.language {
                    Language::Korean => format!("{name} · 작업 {}개 · {time}", keys.len()),
                    Language::English => format!("{name} · {} tasks · {time}", keys.len()),
                };
                MetroLine { name, colour: (index < COLOURED_LINES).then_some(index), lanes: Vec::new(), legend, carried }
            })
            .collect();

        // Each day, the line worked on longest takes the top lane; lines idle that day keep their week order below.
        for day in 0..days.len() {
            let mut order: Vec<(usize, i64)> = (0..count)
                .map(|line| (line, visits.iter().filter(|visit| visit.day == day && line_of(visit.record.project()) == line).map(|visit| visit.minutes).sum()))
                .collect();
            order.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            for (lane, (line, _)) in order.into_iter().enumerate() {
                lines[line].lanes.push(lane);
            }
        }

        // Stops: a busy day names only its longest few on each line, and the names take turns above and below.
        let mut stops: Vec<MetroStop> = visits
            .iter()
            .map(|visit| {
                let last_day = visits.iter().filter(|other| other.record.key == visit.record.key).map(|other| other.day).max() == Some(visit.day);
                MetroStop {
                    key: visit.record.key.clone(),
                    line: line_of(visit.record.project()),
                    day: visit.day,
                    at: visit.at,
                    minutes: visit.minutes,
                    title: visit.record.title.clone(),
                    pull: visit.pull.map(|number| format!("PR #{number}")),
                    asking: last_day && self.callout_by(visit.record, week_end) == Some(Callout::Asking),
                    name_above: None,
                }
            })
            .collect();
        stops.sort_by(|a, b| a.line.cmp(&b.line).then(a.day.cmp(&b.day)).then(a.at.total_cmp(&b.at)));
        for line in 0..count {
            let mut above = true;
            for day in 0..days.len() {
                let indices: Vec<usize> = (0..stops.len()).filter(|&index| stops[index].line == line && stops[index].day == day).collect();
                let mut longest = indices.clone();
                longest.sort_by(|&a, &b| stops[b].minutes.cmp(&stops[a].minutes).then(a.cmp(&b)));
                longest.truncate(NAMED_PER_DAY);
                for index in indices.into_iter().filter(|index| longest.contains(index)) {
                    stops[index].name_above = Some(above);
                    above = !above;
                }
            }
        }

        let carries_on = !current || ready;
        let open_count: usize = lines.iter().map(|line| line.carried).sum();
        MetroView {
            title: if current { pick("이번 주 노선도", "This week's map") } else { pick("그 주 노선도", "That week's map") }.to_owned(),
            note: pick(
                "프로젝트마다 노선, 작업마다 역이에요. 그날 가장 오래 일한 프로젝트가 맨 윗줄로 올라와요. 역을 누르면 그 작업이 열려요.",
                "A line per project, a stop per task. The project worked on longest each day takes the top line. Press a stop to open its task.",
            )
            .to_owned(),
            days,
            lines,
            stops,
            terminal: if carries_on { pick("다음 주", "Next week") } else { pick("남은 일", "Left") }.to_owned(),
            terminal_count: match self.language {
                Language::Korean => format!("{open_count}개"),
                Language::English => format!("{open_count} open"),
            },
            waiting: pick("답 기다림", "Waiting").to_owned(),
            keys: [
                pick("역 · 크기는 시간", "Stop · size is time").to_owned(),
                pick("PR 환승역", "Pull request").to_owned(),
                pick("답을 기다리는 일", "Waiting for an answer").to_owned(),
                pick("종점", "End").to_owned(),
                if carries_on { pick("다음 주로", "To next week") } else { pick("남은 일로", "Still open") }.to_owned(),
            ],
        }
    }
}
