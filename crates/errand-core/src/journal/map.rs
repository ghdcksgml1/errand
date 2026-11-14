//! A week as one map: a line for each project, thick where an agent worked on it. The desktop window draws the
//! same map for a week and, opened up on one of its days, for that day; a month is its weeks, small. Work that
//! overlapped on one line runs on a track beside it, a pull request is a ringed end, work waiting on the person
//! runs on dotted to now, and a line's colour belongs to its project from week to week.

use super::summary::{Kind, Open};
use super::view::{Callout, Journal, Stat, StatPart, duration_parts, minutes_text, stat_names};
use super::{WorkRecord, WorkTurn};
use crate::{Language, Provider};
use jiff::civil::Date;
use jiff::{SignedDuration, Timestamp, ToSpan};
use std::collections::{HashMap, HashSet};

/// Lines with a colour of their own; a fifth line, or all the rest together, is grey.
pub const COLOURED: usize = 4;
/// The minutes one piece of work must end before the next begins for the two to share a track.
const GAP: f64 = 6.0;
/// The weeks the year strip shows, from the week that holds the first of January.
const YEAR_WEEKS: usize = 53;

/// How a piece of work stands at the end of its day.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapState {
    Done,
    Running,
    /// Waiting on the person's answer.
    Asking,
    /// Finished as asked, with something the day's summary says is left.
    Open,
}

/// One task's work on one day.
#[derive(Clone, Debug, PartialEq)]
pub struct MapTask {
    pub key: String,
    pub provider: Provider,
    pub title: String,
    pub lane: usize,
    /// 0 on the line itself; the others run beside it while work on the line overlaps.
    pub track: usize,
    /// Days from the map's Monday.
    pub day: usize,
    /// Minutes from that day's midnight.
    pub start: f64,
    pub end: f64,
    /// The stretches an agent worked. Between them the task was the person's to answer.
    pub work: Vec<(f64, f64)>,
    /// For work waiting on the person today: now, where the wait has reached.
    pub waiting: Option<f64>,
    pub state: MapState,
    /// The agents' working time that day, and the same in words; empty where none was measured.
    pub minutes: i64,
    /// The same to the second, so that parts of a week add up to what the week's date line says.
    pub seconds: i64,
    pub worked: String,
    /// The day's last request was answered and nothing about the task was live.
    pub finished: bool,
    pub commits: usize,
    /// The day's first pull request, and how many there were.
    pub pull: Option<u32>,
    pub pulls: usize,
    /// What kind of work the day's summary calls it; none before the day is summed up.
    pub kind: Option<Kind>,
    /// The summary's sentence for it, or its title before there is one.
    pub line: String,
    /// Why it is left, for work that is.
    pub why: String,
    /// When: "09:12 – 09:40 · 24분", or since when while it runs.
    pub time: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MapLane {
    pub name: String,
    /// Its colour, by slot; none for the grey line.
    pub colour: Option<usize>,
    /// Its week: "작업 7 · 3시간 16분".
    pub note: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MapDay {
    pub date: Date,
    /// "월", "Mon".
    pub label: String,
    pub number: i8,
    /// The day's working time; empty for a day without work.
    pub total: String,
    pub today: bool,
    pub future: bool,
    /// Days back from today, to open it.
    pub back: i64,
    /// The heading for its summary: "오늘 정리", "목요일 정리".
    pub title: String,
    /// Each line's own day: "작업 2 · 1시간 46분", or that it had none.
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MapView {
    pub monday: Date,
    /// Weeks back from this week; negative for a week still to come.
    pub back: i64,
    pub lanes: Vec<MapLane>,
    /// A week with a single project draws its branches as the lines: the project, and how the lines are split.
    pub split: Option<String>,
    pub tasks: Vec<MapTask>,
    /// Monday to Sunday.
    pub days: Vec<MapDay>,
    pub first_hour: i8,
    pub last_hour: i8,
    /// The day and minute of now, while the map holds today.
    pub now: Option<(usize, f64)>,
    pub now_text: String,
    /// Tasks still open at the end, which run on past the week.
    pub carried: usize,
    /// "다음 주" and "3개", where the open work runs to.
    pub carried_words: (String, String),
    /// What stands in for a map without work.
    pub empty: String,
    /// "11/9 – 15".
    pub range: String,
    /// "작업 23 · 13시간 8분"; empty for a week without work.
    pub tally: String,
    /// The week in a line: its longest task's sentence and how many more there were.
    pub top: String,
}

/// The tasks of one kind, as indexes into the map's tasks.
#[derive(Clone, Debug, PartialEq)]
pub struct KindRow {
    pub name: String,
    pub tasks: Vec<usize>,
}

/// A stretch of work around a 24-hour dial, in minutes from midnight: the week it is from, and its task there.
#[derive(Clone, Debug, PartialEq)]
pub struct ClockArc {
    pub week: usize,
    pub task: usize,
    pub from: f64,
    pub to: f64,
}

/// When the work was asked for, around the clock.
#[derive(Clone, Debug, PartialEq)]
pub struct Clock {
    pub arcs: Vec<ClockArc>,
    /// The hour with the most work: "15시", "15:00". Empty without work.
    pub peak: String,
    /// Morning and afternoon, each with its working time.
    pub halves: Vec<(String, String)>,
}

/// One week of the year strip.
#[derive(Clone, Debug, PartialEq)]
pub struct YearCell {
    /// Weeks back from this week; none for a week still to come.
    pub back: Option<i64>,
    /// 0 for no work, up to 5 for the most.
    pub level: u8,
    pub tip: String,
}

/// The year as its weeks, to see how much was asked when and to go to any of them.
#[derive(Clone, Debug, PartialEq)]
pub struct YearView {
    pub label: String,
    pub cells: Vec<YearCell>,
    /// Where each month's name goes, by the cell it starts at.
    pub months: Vec<(usize, String)>,
    /// This week's cell.
    pub current: usize,
}

impl MapView {
    /// The kinds of work among one day's tasks, or the whole week's: the most common first, and tasks whose
    /// day is not summed up yet last.
    pub fn kinds(&self, day: Option<usize>, language: Language) -> Vec<KindRow> {
        let mut rows: Vec<(Option<Kind>, Vec<usize>)> = Vec::new();
        for (index, task) in self.tasks.iter().enumerate() {
            if day.is_some_and(|day| day != task.day) {
                continue;
            }
            match rows.iter_mut().find(|row| row.0 == task.kind) {
                Some(row) => row.1.push(index),
                None => rows.push((task.kind, vec![index])),
            }
        }
        rows.sort_by(|a, b| a.0.is_none().cmp(&b.0.is_none()).then(b.1.len().cmp(&a.1.len())).then(a.0.cmp(&b.0)));
        rows.into_iter().map(|(kind, tasks)| KindRow { name: kind.map_or(language.pick("정리 전", "Not summed up"), |kind| kind.name(language)).to_owned(), tasks }).collect()
    }

    /// The working stretches of one day, or of every day laid over each other, around the clock.
    pub fn clock(&self, day: Option<usize>, language: Language) -> Clock {
        clock(std::slice::from_ref(self), day, language)
    }

    /// The date line's counts for some of the map's tasks: those of one day, or of the whole week, that `keep`
    /// passes. Which counts there are goes by all of that day's or week's tasks, so the line keeps its shape
    /// while what it counts grows or narrows.
    pub fn stats_of(&self, day: Option<usize>, keep: &dyn Fn(usize) -> bool, language: Language) -> Vec<Stat> {
        let all: Vec<usize> = (0..self.tasks.len()).filter(|index| day.is_none_or(|day| day == self.tasks[*index].day)).collect();
        // A task that ran on several days is one task, and stands as its last day left it.
        let mut last: HashMap<&str, usize> = HashMap::new();
        for index in &all {
            last.insert(self.tasks[*index].key.as_str(), *index);
        }
        let kept: Vec<usize> = all.iter().copied().filter(|index| keep(*index)).collect();
        let plain = |text: String| vec![StatPart { text, small: false }];
        let names = stat_names(language);
        let finished = last.values().filter(|index| keep(**index) && self.tasks[**index].finished).count();
        let mut stats = vec![Stat { label: names[0].into(), parts: plain(finished.to_string()), asking: false }];
        let asking = |index: &&usize| self.tasks[**index].state == MapState::Asking;
        if last.values().any(|index| asking(&index)) {
            stats.push(Stat { label: names[1].into(), parts: plain(last.values().filter(|index| keep(**index)).filter(asking).count().to_string()), asking: true });
        }
        let seconds: i64 = kept.iter().map(|index| self.tasks[*index].seconds).sum();
        stats.push(Stat { label: names[2].into(), parts: duration_parts(seconds / 60, language), asking: false });
        if all.iter().any(|index| self.tasks[*index].commits + self.tasks[*index].pulls > 0) {
            let (commits, pulls) = kept.iter().fold((0, 0), |sum, index| (sum.0 + self.tasks[*index].commits, sum.1 + self.tasks[*index].pulls));
            stats.push(Stat { label: names[3].into(), parts: plain(format!("{commits} · {pulls}")), asking: false });
        }
        stats
    }
}

/// The working stretches of these weeks around one clock: one day of the first, or every day of all of them.
pub fn clock(weeks: &[MapView], day: Option<usize>, language: Language) -> Clock {
    clock_of(weeks, day, &|_, _| true, language)
}

/// The same for the tasks `keep` passes, each named by its week on screen and its place among that week's tasks.
pub fn clock_of(weeks: &[MapView], day: Option<usize>, keep: &dyn Fn(usize, usize) -> bool, language: Language) -> Clock {
    let mut hours = [0.0f64; 24];
    let mut arcs = Vec::new();
    for (week, map) in weeks.iter().enumerate() {
        for (index, task) in map.tasks.iter().enumerate() {
            if day.is_some_and(|day| day != task.day) || !keep(week, index) {
                continue;
            }
            for (from, to) in &task.work {
                arcs.push(ClockArc { week, task: index, from: *from, to: *to });
                let mut at = *from;
                while at < *to {
                    let hour = ((at / 60.0).floor() as usize).min(23);
                    let next = (((hour + 1) * 60) as f64).min(*to).max(at + 0.001);
                    hours[hour] += next - at;
                    at = next;
                }
            }
        }
    }
    let total: f64 = hours.iter().sum();
    if total <= 0.0 {
        return Clock { arcs, peak: String::new(), halves: Vec::new() };
    }
    let peak = (0..24).max_by(|a, b| hours[*a].total_cmp(&hours[*b]).then(b.cmp(a))).unwrap_or(0);
    let morning: f64 = hours[..12].iter().sum();
    // A half without work says so, where a span in words is never less than a minute.
    let half = |minutes: f64| if minutes < 0.5 { language.pick("0분", "0m").to_owned() } else { minutes_text(minutes.round() as i64, language) };
    Clock {
        arcs,
        peak: match language {
            Language::Korean => format!("{peak}시"),
            Language::English => format!("{peak}:00"),
        },
        halves: vec![(language.pick("오전", "AM").to_owned(), half(morning)), (language.pick("오후", "PM").to_owned(), half(total - morning))],
    }
}

impl Journal<'_> {
    /// Every project in the records, oldest first. A project's place here picks its colour, so the colour
    /// stays its own from week to week.
    fn projects_by_age(&self) -> Vec<&str> {
        let mut first: Vec<(&str, Timestamp)> = Vec::new();
        for record in self.records {
            let Some(at) = record.started_at() else { continue };
            match first.iter_mut().find(|(name, _)| *name == record.project()) {
                Some(entry) => entry.1 = entry.1.min(at),
                None => first.push((record.project(), at)),
            }
        }
        first.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(b.0)));
        first.into_iter().map(|(name, _)| name).collect()
    }

    fn this_monday(&self) -> Date {
        let today = self.today();
        today.checked_sub((today.weekday().to_monday_zero_offset() as i64).days()).unwrap_or(today)
    }

    /// "작업 7 · 3시간 16분", without the time where none was measured.
    fn tally(&self, tasks: usize, minutes: i64) -> String {
        let count = format!("{} {tasks}", self.language.pick("작업", "Tasks"));
        if minutes > 0 { format!("{count} · {}", minutes_text(minutes, self.language)) } else { count }
    }

    /// The week that begins on `monday` as a map.
    pub fn map(&self, monday: Date) -> MapView {
        let today = self.today();
        let pick = |korean: &'static str, english: &'static str| self.language.pick(korean, english);
        let dates: Vec<Date> = (0..7i64).map(|offset| monday.checked_add(offset.days()).unwrap_or(monday)).collect();
        let bounds: Vec<(Timestamp, Timestamp)> = dates.iter().map(|date| (self.start_of(*date), self.start_of(date.tomorrow().unwrap_or(*date)))).collect();
        let (from, to) = (bounds[0].0, bounds[6].1);
        let records: Vec<&WorkRecord> = self.records.iter().filter(|record| record.turns_between(from, to).next().is_some()).collect();
        let worked = |record: &WorkRecord, from: Timestamp, to: Timestamp| -> i64 { record.turns_between(from, to).filter_map(WorkTurn::duration).map(|span| span.as_mins()).sum() };
        let worked_seconds = |record: &WorkRecord, from: Timestamp, to: Timestamp| -> i64 { record.turns_between(from, to).filter_map(WorkTurn::duration).map(|span| span.as_secs()).sum() };

        // The lines: the week's projects, or one project's branches, the longest worked first.
        let mut projects: Vec<&str> = Vec::new();
        for record in &records {
            if !projects.contains(&record.project()) {
                projects.push(record.project());
            }
        }
        let branches: HashSet<Option<&str>> = records.iter().map(|record| record.branch.as_deref()).collect();
        let split = (projects.len() == 1 && branches.len() > 1).then(|| projects[0].to_owned());
        let line_of = |record: &WorkRecord| -> String {
            match &split {
                Some(project) => record.branch.clone().unwrap_or_else(|| project.clone()),
                None => record.project().to_owned(),
            }
        };
        let mut lines: Vec<(String, i64)> = Vec::new();
        for record in &records {
            let (name, minutes) = (line_of(record), worked(record, from, to));
            match lines.iter_mut().find(|line| line.0 == name) {
                Some(line) => line.1 += minutes,
                None => lines.push((name, minutes)),
            }
        }
        lines.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let gathered = lines.len() > COLOURED + 1;
        let lane_count = if gathered { COLOURED + 1 } else { lines.len() };
        let age = self.projects_by_age();
        let mut taken = [false; COLOURED];
        let colours: Vec<Option<usize>> = (0..lane_count)
            .map(|index| {
                if index >= COLOURED {
                    return None;
                }
                // A project keeps the colour its age gives it; where two in one week want the same, the later
                // line takes the next one free.
                let wanted = if split.is_some() { index } else { age.iter().position(|project| *project == lines[index].0).unwrap_or(index) % COLOURED };
                let slot = (0..COLOURED).map(|step| (wanted + step) % COLOURED).find(|slot| !taken[*slot]).unwrap_or(wanted);
                taken[slot] = true;
                Some(slot)
            })
            .collect();

        let mut tasks: Vec<MapTask> = Vec::new();
        for (day, date) in dates.iter().enumerate() {
            let (day_from, day_to) = bounds[day];
            let minute = |at: Timestamp| day_from.duration_until(at).as_secs_f64() / 60.0;
            let now = (*date == today).then(|| minute(self.now));
            let summary = self.summaries.days.get(date);
            for record in &records {
                let turns: Vec<&WorkTurn> = record.turns_between(day_from, day_to).collect();
                let Some(first) = turns.first() else { continue };
                // The live state belongs to the day that holds the task's latest request.
                let latest = record.turns.last().is_some_and(|turn| turn.asked_at >= day_from && turn.asked_at < day_to);
                let callout = if latest { self.callout(record) } else { None };
                let start = minute(first.asked_at);
                let work: Vec<(f64, f64)> = turns
                    .iter()
                    .enumerate()
                    .map(|(index, turn)| {
                        let asked = minute(turn.asked_at);
                        let until = match turn.finished_at {
                            Some(finished) => minute(finished).min(1440.0),
                            None if callout == Some(Callout::Running) && index + 1 == turns.len() => now.unwrap_or(asked),
                            None => asked,
                        };
                        (asked, until.max(asked))
                    })
                    .collect();
                let end = work.iter().map(|stretch| stretch.1).fold(start, f64::max);
                let key = Some(record.key.as_str());
                let left = summary.and_then(|summary| summary.left.iter().find(|item| item.key.as_deref() == key));
                let done = summary.and_then(|summary| summary.done.iter().find(|item| item.key.as_deref() == key));
                let state = match callout {
                    Some(Callout::Asking) => MapState::Asking,
                    Some(Callout::Running) => MapState::Running,
                    _ if left.is_some_and(|item| item.open == Open::Todo) => MapState::Open,
                    _ => MapState::Done,
                };
                let why = match (left, state) {
                    (Some(item), _) => item.text.clone(),
                    (None, MapState::Asking) => pick("답을 기다려요", "Waiting for your answer").to_owned(),
                    (None, MapState::Running) => pick("진행 중이에요", "Still running").to_owned(),
                    _ => String::new(),
                };
                let name = line_of(record);
                tasks.push(MapTask {
                    key: record.key.clone(),
                    provider: record.provider,
                    title: record.title.clone(),
                    lane: lines.iter().position(|line| line.0 == name).unwrap_or(0).min(lane_count.saturating_sub(1)),
                    track: 0,
                    day,
                    start,
                    end,
                    waiting: if state == MapState::Asking { now.filter(|now| *now > end) } else { None },
                    work,
                    state,
                    minutes: worked(record, day_from, day_to),
                    seconds: worked_seconds(record, day_from, day_to),
                    worked: Some(worked(record, day_from, day_to)).filter(|minutes| *minutes > 0).map_or_else(String::new, |minutes| minutes_text(minutes, self.language)),
                    finished: self.finished_on(record, day_from, day_to),
                    commits: record.commits.iter().filter(|commit| commit.at >= day_from && commit.at < day_to).count(),
                    pull: record.pull_requests.iter().find(|pull| pull.at >= day_from && pull.at < day_to).map(|pull| pull.number),
                    pulls: record.pull_requests.iter().filter(|pull| pull.at >= day_from && pull.at < day_to).count(),
                    kind: done.map(|item| item.kind),
                    line: done.map_or_else(|| record.title.clone(), |item| item.text.clone()),
                    why,
                    time: self.task_time(record, day_from, day_to),
                });
            }
        }
        tasks.sort_by(|a, b| a.day.cmp(&b.day).then(a.start.total_cmp(&b.start)).then(a.key.cmp(&b.key)));
        // Work that overlaps on one line moves to a track beside it.
        let mut ends: HashMap<(usize, usize), Vec<f64>> = HashMap::new();
        for task in &mut tasks {
            let ends = ends.entry((task.lane, task.day)).or_default();
            let reach = task.waiting.unwrap_or(task.end);
            let track = ends.iter().position(|end| *end + GAP <= task.start).unwrap_or(ends.len());
            if track == ends.len() {
                ends.push(reach);
            } else {
                ends[track] = reach;
            }
            task.track = track;
        }

        let lane_tally = |lane: usize, day: Option<usize>| {
            let mine: Vec<&MapTask> = tasks.iter().filter(|task| task.lane == lane && day.is_none_or(|day| day == task.day)).collect();
            (mine.len(), mine.iter().map(|task| task.minutes).sum::<i64>())
        };
        let lanes: Vec<MapLane> = (0..lane_count)
            .map(|index| {
                let name = if gathered && index == COLOURED {
                    match self.language {
                        Language::Korean => format!("그 밖 {}개", lines.len() - COLOURED),
                        Language::English => format!("{} more", lines.len() - COLOURED),
                    }
                } else {
                    lines[index].0.clone()
                };
                let (count, minutes) = lane_tally(index, None);
                MapLane { name, colour: colours[index], note: self.tally(count, minutes) }
            })
            .collect();
        let days: Vec<MapDay> = dates
            .iter()
            .enumerate()
            .map(|(index, date)| {
                let minutes: i64 = tasks.iter().filter(|task| task.day == index).map(|task| task.minutes).sum();
                let is_today = *date == today;
                MapDay {
                    date: *date,
                    label: self.weekday(date.weekday()).to_owned(),
                    number: date.day(),
                    total: if minutes > 0 { minutes_text(minutes, self.language) } else { String::new() },
                    today: is_today,
                    future: *date > today,
                    back: date.until(today).map(|span| span.get_days() as i64).unwrap_or(0),
                    title: match (self.language, is_today) {
                        (Language::Korean, true) => "오늘 정리".to_owned(),
                        (Language::Korean, false) => format!("{} 정리", self.weekday_long(date.weekday())),
                        (Language::English, true) => "Today's summary".to_owned(),
                        (Language::English, false) => format!("{}'s summary", self.weekday_long(date.weekday())),
                    },
                    notes: (0..lane_count)
                        .map(|lane| match lane_tally(lane, Some(index)) {
                            (0, _) => pick("이날은 없음", "None that day").to_owned(),
                            (count, minutes) => self.tally(count, minutes),
                        })
                        .collect(),
                }
            })
            .collect();

        let (mut first, mut last) = (8.0f64, 20.0f64);
        for task in &tasks {
            first = first.min((task.start / 60.0).floor());
            last = last.max((task.waiting.unwrap_or(task.end) / 60.0).ceil());
        }
        let now = dates.iter().position(|date| *date == today).map(|day| (day, bounds[day].0.duration_until(self.now).as_secs_f64() / 60.0));
        if let Some((_, minute)) = now {
            last = last.max((minute / 60.0).ceil());
        }
        let this = self.this_monday();
        let back = monday.until(this).map(|span| span.get_days() as i64 / 7).unwrap_or(0);
        let open: HashSet<&str> = tasks.iter().filter(|task| task.state != MapState::Done).map(|task| task.key.as_str()).collect();
        let sunday = dates[6];
        let minutes: i64 = tasks.iter().map(|task| task.minutes).sum();
        let top = tasks.iter().max_by_key(|task| task.minutes).map_or_else(String::new, |task| match (self.language, tasks.len() - 1) {
            (_, 0) => task.line.clone(),
            (Language::Korean, more) => format!("{} 외 {more}개", task.line),
            (Language::English, more) => format!("{} and {more} more", task.line),
        });
        MapView {
            monday,
            back,
            lanes,
            split: split.map(|project| match self.language {
                Language::Korean => format!("{project} · 브랜치별"),
                Language::English => format!("{project} · by branch"),
            }),
            days,
            first_hour: (first.max(0.0) as i8).min(8),
            last_hour: (last.min(24.0) as i8).max(20),
            now,
            now_text: format!("{} {}", pick("지금", "Now"), self.clock(self.now)),
            carried: open.len(),
            carried_words: (
                pick("다음 주", "Next week").to_owned(),
                match self.language {
                    Language::Korean => format!("{}개", open.len()),
                    Language::English => open.len().to_string(),
                },
            ),
            empty: if monday > this { pick("아직 오지 않은 주예요.", "This week has not come yet.") } else { pick("이 주에는 맡긴 일이 없어요.", "Nothing was asked this week.") }.to_owned(),
            range: if monday.month() == sunday.month() {
                format!("{}/{} – {}", monday.month(), monday.day(), sunday.day())
            } else {
                format!("{}/{} – {}/{}", monday.month(), monday.day(), sunday.month(), sunday.day())
            },
            tally: if tasks.is_empty() { String::new() } else { self.tally(tasks.len(), minutes) },
            top,
            tasks,
        }
    }

    /// The weeks of the month `back` months ago, each as a map. A week belongs to the month its Thursday is in.
    pub fn month_maps(&self, back: i64) -> Vec<MapView> {
        let today = self.today();
        let first = today.first_of_month().checked_sub(back.months()).unwrap_or(today);
        let mut monday = first.checked_sub((first.weekday().to_monday_zero_offset() as i64).days()).unwrap_or(first);
        let mut weeks = Vec::new();
        for _ in 0..6 {
            let thursday = monday.checked_add(3.days()).unwrap_or(monday);
            if (thursday.year(), thursday.month()) == (first.year(), first.month()) {
                weeks.push(self.map(monday));
            }
            monday = monday.checked_add(7.days()).unwrap_or(monday);
        }
        weeks
    }

    /// This year week by week: how much the agents worked in each.
    pub fn year(&self) -> YearView {
        let today = self.today();
        let this = self.this_monday();
        let new_year = jiff::civil::date(today.year(), 1, 1);
        let first = new_year.checked_sub((new_year.weekday().to_monday_zero_offset() as i64).days()).unwrap_or(new_year);
        let mut minutes = [0i64; YEAR_WEEKS];
        let mut counts = [0usize; YEAR_WEEKS];
        for record in self.records {
            let mut seen = [false; YEAR_WEEKS];
            for turn in &record.turns {
                let Ok(span) = first.until(self.date(turn.asked_at)) else { continue };
                let days = span.get_days() as i64;
                if days < 0 || days >= (YEAR_WEEKS * 7) as i64 {
                    continue;
                }
                let week = (days / 7) as usize;
                minutes[week] += turn.duration().map_or(0, |span: SignedDuration| span.as_mins());
                if !std::mem::replace(&mut seen[week], true) {
                    counts[week] += 1;
                }
            }
        }
        let mut months = Vec::new();
        let mut month = 0i8;
        let cells = (0..YEAR_WEEKS)
            .map(|index| {
                let monday = first.checked_add((index as i64 * 7).days()).unwrap_or(first);
                let (thursday, sunday) = (monday.checked_add(3.days()).unwrap_or(monday), monday.checked_add(6.days()).unwrap_or(monday));
                if thursday.year() == today.year() && thursday.month() != month {
                    month = thursday.month();
                    months.push((index, self.month_name(month)));
                }
                let back = monday.until(this).map(|span| span.get_days() as i64 / 7).unwrap_or(0);
                let worked = minutes[index];
                let what = if monday > this {
                    self.language.pick("아직 오지 않은 주", "Still to come").to_owned()
                } else if counts[index] == 0 {
                    self.language.pick("기록 없음", "No records").to_owned()
                } else {
                    self.tally(counts[index], worked)
                };
                YearCell {
                    back: (monday <= this).then_some(back),
                    level: match worked {
                        _ if counts[index] == 0 => 0,
                        ..120 => 1,
                        120..240 => 2,
                        240..360 => 3,
                        360..480 => 4,
                        _ => 5,
                    },
                    tip: format!("{} – {} · {what}", self.month_day(monday), self.month_day(sunday)),
                }
            })
            .collect();
        YearView { label: today.year().to_string(), cells, months, current: first.until(this).map(|span| (span.get_days() / 7) as usize).unwrap_or(0) }
    }
}
