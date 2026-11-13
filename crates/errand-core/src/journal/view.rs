//! What the desktop window shows for a day, a week, a month, a report or one task, worked out from the work
//! records with every word already in the person's language. The platform only draws it.

use super::WorkRecord;
use crate::sessions::SessionState;
use crate::sessions::text::preview;
use crate::{Language, Provider};
use jiff::civil::{Date, Weekday};
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp, ToSpan};
use std::collections::{BTreeMap, HashMap};

/// How long a request with no answer, in a session the widget does not show, still counts as running.
const UNANSWERED: SignedDuration = SignedDuration::from_hours(2);

/// The window's four views.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum JournalTab {
    #[default]
    Day,
    Week,
    Month,
    Report,
}

impl JournalTab {
    pub const ALL: [JournalTab; 4] = [JournalTab::Day, JournalTab::Week, JournalTab::Month, JournalTab::Report];

    pub fn name(self, language: Language) -> &'static str {
        match self {
            JournalTab::Day => language.pick("하루", "Day"),
            JournalTab::Week => language.pick("주", "Week"),
            JournalTab::Month => language.pick("월", "Month"),
            JournalTab::Report => language.pick("리포트", "Report"),
        }
    }
}

/// How far back a report looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReportSpan {
    #[default]
    Weeks12,
    Months6,
    Year,
}

impl ReportSpan {
    pub const ALL: [ReportSpan; 3] = [ReportSpan::Weeks12, ReportSpan::Months6, ReportSpan::Year];

    pub fn name(self, language: Language) -> &'static str {
        match self {
            ReportSpan::Weeks12 => language.pick("12주", "12 weeks"),
            ReportSpan::Months6 => language.pick("6개월", "6 months"),
            ReportSpan::Year => language.pick("1년", "1 year"),
        }
    }

    fn weeks(self) -> i64 {
        match self {
            ReportSpan::Weeks12 => 12,
            ReportSpan::Months6 => 26,
            ReportSpan::Year => 52,
        }
    }
}

/// What the window is looking at.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JournalPlace {
    pub tab: JournalTab,
    /// Steps back from the present: days, weeks or months by tab. Zero is today, this week, this month.
    pub back: i64,
    pub project: Option<String>,
    pub span: ReportSpan,
    /// The task whose detail is open.
    pub open: Option<String>,
}

/// The widget's live state for a task, where it shows one.
#[derive(Clone, Debug, PartialEq)]
pub struct Live {
    pub state: SessionState,
    pub question: Option<String>,
}

/// Everything the views read.
pub struct Journal<'a> {
    pub records: &'a [WorkRecord],
    pub live: &'a HashMap<String, Live>,
    pub now: Timestamp,
    pub zone: &'a TimeZone,
    pub language: Language,
}

/// A live state worth calling out on a record, as the widget's chips do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Callout {
    Asking,
    New,
    Running,
}

impl Callout {
    fn of(state: SessionState) -> Option<Callout> {
        match state {
            SessionState::Asking => Some(Callout::Asking),
            SessionState::New => Some(Callout::New),
            SessionState::Running | SessionState::Stalled => Some(Callout::Running),
            _ => None,
        }
    }

    pub fn word(self, language: Language) -> &'static str {
        match self {
            Callout::Asking => language.pick("답변 필요", "Needs answer"),
            Callout::New => language.pick("새 결과", "New result"),
            Callout::Running => language.pick("진행 중", "Running"),
        }
    }

    pub fn state(self) -> SessionState {
        match self {
            Callout::Asking => SessionState::Asking,
            Callout::New => SessionState::New,
            Callout::Running => SessionState::Running,
        }
    }
}

/// A count in the header: a call-out in its state's colours, or a plain tally with its icon.
#[derive(Clone, Debug, PartialEq)]
pub struct Pill {
    pub callout: Option<Callout>,
    pub icon: &'static str,
    pub text: String,
}

/// The big number and the two lines beside it.
#[derive(Clone, Debug, PartialEq)]
pub struct Masthead {
    pub big: String,
    pub top: String,
    pub bottom: String,
    pub pills: Vec<Pill>,
    /// Forward goes nowhere past the present.
    pub can_go_forward: bool,
}

/// A stretch of work on a time axis, as fractions of the axis.
#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    pub key: String,
    pub provider: Provider,
    pub start: f64,
    pub end: f64,
    pub lane: usize,
    pub label: String,
    pub callout: Option<Callout>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HourAxis {
    pub first: i8,
    pub last: i8,
    /// Where now is, while the axis shows today.
    pub now: Option<f64>,
    pub now_text: String,
}

/// A task as a card.
#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub key: String,
    pub provider: Provider,
    pub title: String,
    pub ask: String,
    pub line: String,
    pub callout: Option<Callout>,
    pub time: String,
    pub project: String,
    pub marks: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DayView {
    pub masthead: Masthead,
    pub axis: HourAxis,
    pub bars: Vec<Bar>,
    pub lanes: usize,
    /// Every project that day with its count, after "all".
    pub filters: Vec<(Option<String>, String, usize)>,
    pub cards: Vec<Card>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WeekRow {
    pub label: String,
    pub today: bool,
    pub bars: Vec<Bar>,
    pub lanes: usize,
    pub now: Option<f64>,
}

/// A task as a line in its project's card.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub key: String,
    pub provider: Provider,
    pub title: String,
    pub callout: Option<Callout>,
    pub pull_request: Option<String>,
    pub when: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectCard {
    pub name: String,
    pub count: usize,
    pub marks: String,
    pub lines: Vec<Line>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WeekView {
    pub masthead: Masthead,
    pub axis: HourAxis,
    pub rows: Vec<WeekRow>,
    pub projects: Vec<ProjectCard>,
}

/// How a task shows in a calendar day: a dot in its service's colour, or a ring in a live state's.
#[derive(Clone, Debug, PartialEq)]
pub struct DayLine {
    pub key: String,
    pub title: String,
    pub provider: Provider,
    pub callout: Option<Callout>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CalendarCell {
    pub day: Option<i8>,
    pub today: bool,
    pub future: bool,
    pub lines: Vec<DayLine>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectSplit {
    pub name: String,
    pub count: usize,
    pub codex: usize,
    pub claude: usize,
    pub marks: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PullRequestRow {
    pub number: String,
    pub title: String,
    pub when: String,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MonthView {
    pub masthead: Masthead,
    pub weekdays: [&'static str; 7],
    /// Whole weeks, Sunday first.
    pub cells: Vec<CalendarCell>,
    /// Weeks that have begun get room; the ones still to come stay short.
    pub full_rows: usize,
    pub projects: Vec<ProjectSplit>,
    pub pull_requests: Vec<PullRequestRow>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WeekBar {
    pub codex: usize,
    pub claude: usize,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReportView {
    pub masthead: Masthead,
    /// A year of days, Sunday to Saturday in each column: a level from 0 to 5, -1 before the first record,
    /// none for days still to come.
    pub heatmap: Vec<Option<i8>>,
    pub heatmap_months: Vec<(usize, String)>,
    pub since: String,
    pub weeks: Vec<WeekBar>,
    pub projects: Vec<ProjectSplit>,
    /// Requests by the hour they were made, from `first_hour`.
    pub hours: Vec<usize>,
    pub first_hour: i8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TurnRow {
    pub time: String,
    pub ask: String,
    pub result: String,
    pub commits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FileRow {
    pub path: String,
    pub diff: String,
    pub created: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DetailView {
    pub key: String,
    pub provider: Provider,
    pub title: String,
    pub project: String,
    pub branch: Option<String>,
    pub time: String,
    pub url: Option<String>,
    pub open_label: String,
    pub pills: Vec<String>,
    pub turns: Vec<TurnRow>,
    pub commits: Vec<(String, String, String)>,
    pub files: Vec<FileRow>,
}

impl Journal<'_> {
    fn date(&self, at: Timestamp) -> Date {
        at.to_zoned(self.zone.clone()).date()
    }

    fn today(&self) -> Date {
        self.date(self.now)
    }

    fn start_of(&self, date: Date) -> Timestamp {
        date.to_zoned(self.zone.clone()).map(|zoned| zoned.timestamp()).unwrap_or(self.now)
    }

    fn clock(&self, at: Timestamp) -> String {
        at.to_zoned(self.zone.clone()).strftime("%H:%M").to_string()
    }

    fn callout(&self, record: &WorkRecord) -> Option<Callout> {
        match self.live.get(&record.key) {
            Some(live) => Callout::of(live.state),
            None => self.running(record).then_some(Callout::Running),
        }
    }

    /// Still at work: as the widget says, or, for a session it does not show, a request this recent with no answer
    /// yet. An older unanswered request stopped without a word.
    fn running(&self, record: &WorkRecord) -> bool {
        match self.live.get(&record.key) {
            Some(live) => matches!(Callout::of(live.state), Some(Callout::Running | Callout::Asking)),
            None => record.turns.last().is_some_and(|turn| turn.finished_at.is_none() && turn.asked_at.duration_until(self.now) < UNANSWERED),
        }
    }

    fn weekday(&self, weekday: Weekday) -> &'static str {
        let index = weekday.to_sunday_zero_offset() as usize;
        match self.language {
            Language::Korean => ["일", "월", "화", "수", "목", "금", "토"][index],
            Language::English => ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][index],
        }
    }

    fn weekday_long(&self, weekday: Weekday) -> String {
        match self.language {
            Language::Korean => format!("{}요일", self.weekday(weekday)),
            Language::English => ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"][weekday.to_sunday_zero_offset() as usize].to_owned(),
        }
    }

    fn month_name(&self, month: i8) -> String {
        match self.language {
            Language::Korean => format!("{month}월"),
            Language::English => ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][(month - 1) as usize].to_owned(),
        }
    }

    fn month_day(&self, date: Date) -> String {
        match self.language {
            Language::Korean => format!("{}월 {}일", date.month(), date.day()),
            Language::English => format!("{} {}", self.month_name(date.month()), date.day()),
        }
    }

    fn duration(&self, span: SignedDuration) -> String {
        let minutes = span.as_mins().max(1);
        let (hours, rest) = (minutes / 60, minutes % 60);
        match (self.language, hours, rest) {
            (Language::Korean, 0, _) => format!("{rest}분"),
            (Language::Korean, _, 0) => format!("{hours}시간"),
            (Language::Korean, _, _) => format!("{hours}시간 {rest}분"),
            (Language::English, 0, _) => format!("{rest}m"),
            (Language::English, _, 0) => format!("{hours}h"),
            (Language::English, _, _) => format!("{hours}h {rest}m"),
        }
    }

    fn marks(&self, commits: usize, pulls: &[u32]) -> String {
        let mut parts = Vec::new();
        if commits > 0 {
            parts.push(format!("{} {commits}", self.language.pick("커밋", "Commits")));
        }
        match pulls {
            [] => {}
            [number] => parts.push(format!("PR #{number}")),
            more => parts.push(format!("PR {}", more.len())),
        }
        parts.join(" · ")
    }

    /// The records with a request in `[from, to)`, and those turns.
    fn within(&self, from: Timestamp, to: Timestamp) -> Vec<&WorkRecord> {
        self.records.iter().filter(|record| record.turns_between(from, to).next().is_some()).collect()
    }

    fn tally_pills(&self, records: &[&WorkRecord], from: Timestamp, to: Timestamp, with_count: bool) -> Vec<Pill> {
        let mut pills = Vec::new();
        for callout in [Callout::Asking, Callout::New, Callout::Running] {
            let count = records.iter().filter(|record| self.callout(record) == Some(callout)).count();
            if count > 0 {
                pills.push(Pill { callout: Some(callout), icon: callout.state().icon(), text: format!("{} {count}", callout.word(self.language)) });
            }
        }
        if with_count {
            pills.push(Pill { callout: None, icon: "list-checks", text: format!("{} {}", self.language.pick("작업", "Tasks"), records.len()) });
        }
        let commits: usize = records.iter().map(|record| record.commits.iter().filter(|commit| commit.at >= from && commit.at < to).count()).sum();
        let pulls: usize = records.iter().map(|record| record.pull_requests.iter().filter(|pull| pull.at >= from && pull.at < to).count()).sum();
        pills.push(Pill { callout: None, icon: "git-commit-horizontal", text: format!("{} {commits}", self.language.pick("커밋", "Commits")) });
        pills.push(Pill { callout: None, icon: "git-pull-request", text: format!("PR {pulls}") });
        pills
    }

    /// An hour axis wide enough for the work shown, never narrower than 9 to 18.
    fn axis(&self, spans: &[(Timestamp, Timestamp)], day: Date) -> HourAxis {
        let start = self.start_of(day);
        let hour = |at: Timestamp| start.duration_until(at).as_secs_f64() / 3600.0;
        let mut first = 9.0f64;
        let mut last = 18.0f64;
        for (from, to) in spans {
            first = first.min(hour(*from).floor());
            last = last.max(hour(*to).ceil());
        }
        let first = (first.max(0.0) as i8).min(8);
        let last = (last.min(24.0) as i8).max(first + 9).min(24);
        let (now, now_text) = if day == self.today() {
            let place = (hour(self.now) - first as f64) / (last - first) as f64;
            (Some(place.clamp(0.0, 1.0)), format!("{} {}", self.language.pick("지금", "Now"), self.clock(self.now)))
        } else {
            (None, String::new())
        };
        HourAxis { first, last, now, now_text }
    }

    /// Bars for each record's work within the day, packed into as few lanes as never overlap.
    fn bars(&self, records: &[&WorkRecord], day: Date, axis: &HourAxis) -> (Vec<Bar>, usize) {
        let from = self.start_of(day);
        let to = self.start_of(day.tomorrow().unwrap_or(day));
        let place = |at: Timestamp| {
            let hours = from.duration_until(at).as_secs_f64() / 3600.0;
            ((hours - axis.first as f64) / (axis.last - axis.first) as f64).clamp(0.0, 1.0)
        };
        let mut bars: Vec<Bar> = Vec::new();
        let mut lane_ends: Vec<f64> = Vec::new();
        let mut spans: Vec<(&WorkRecord, Timestamp, Timestamp)> = records
            .iter()
            .filter_map(|record| {
                let turns: Vec<_> = record.turns_between(from, to).collect();
                let begin = turns.first()?.asked_at;
                let last_ask = turns.last()?.asked_at;
                let end = turns.iter().filter_map(|turn| turn.finished_at).max().unwrap_or(begin).max(last_ask);
                let running = turns.last().is_some_and(|turn| turn.finished_at.is_none()) && self.running(record);
                Some((*record, begin, if running { self.now.min(to) } else { end.max(begin) }))
            })
            .collect();
        spans.sort_by_key(|(_, begin, _)| *begin);
        for (record, begin, end) in spans {
            let (start, end) = (place(begin), place(end).max(place(begin) + 0.004));
            let lane = lane_ends.iter().position(|ends| *ends <= start).unwrap_or(lane_ends.len());
            if lane == lane_ends.len() {
                lane_ends.push(end);
            } else {
                lane_ends[lane] = end;
            }
            bars.push(Bar { key: record.key.clone(), provider: record.provider, start, end, lane, label: preview(&record.title, 24), callout: self.callout(record) });
        }
        (bars, lane_ends.len().max(1))
    }

    fn card(&self, record: &WorkRecord, from: Timestamp, to: Timestamp) -> Card {
        let turns: Vec<_> = record.turns_between(from, to).collect();
        let last = turns.last().copied().or(record.turns.last());
        let callout = self.callout(record);
        let unanswered = last.is_some_and(|turn| turn.finished_at.is_none());
        let running = unanswered && self.running(record);
        let line = match (callout, last) {
            (Some(Callout::Asking), _) => self.live.get(&record.key).and_then(|live| live.question.clone()).unwrap_or_default(),
            (_, Some(turn)) if running => format!("{} · {}", self.language.pick("작업 중", "Working"), self.duration(turn.asked_at.duration_until(self.now))),
            (_, Some(_)) if unanswered => self.language.pick("답이 기록되지 않았어요", "No answer recorded").to_owned(),
            (_, Some(turn)) => turn.result.as_deref().map(|result| preview(result.lines().find(|line| !line.trim().is_empty()).unwrap_or(""), 90)).unwrap_or_default(),
            _ => String::new(),
        };
        let first = turns.first().map(|turn| turn.asked_at).unwrap_or(from);
        let finished = turns.iter().filter_map(|turn| turn.finished_at).max();
        let worked: SignedDuration = turns.iter().filter_map(|turn| turn.duration()).fold(SignedDuration::ZERO, |total, span| total + span);
        let time = match finished {
            _ if running => format!("{}{}", self.clock(first), self.language.pick("부터", " on")),
            Some(end) if !unanswered => format!("{} – {} · {}", self.clock(first), self.clock(end), self.duration(worked)),
            _ => self.clock(first),
        };
        let commits = record.commits.iter().filter(|commit| commit.at >= from && commit.at < to).count();
        let pulls: Vec<u32> = record.pull_requests.iter().filter(|pull| pull.at >= from && pull.at < to).map(|pull| pull.number).collect();
        Card {
            key: record.key.clone(),
            provider: record.provider,
            title: record.title.clone(),
            ask: last.map(|turn| preview(&turn.ask, 80)).unwrap_or_default(),
            line,
            callout,
            time,
            project: record.project().to_owned(),
            marks: self.marks(commits, &pulls),
        }
    }

    pub fn day(&self, place: &JournalPlace) -> DayView {
        let day = self.today().checked_sub(place.back.days()).unwrap_or(self.today());
        let from = self.start_of(day);
        let to = self.start_of(day.tomorrow().unwrap_or(day));
        let records = self.within(from, to);
        let spans: Vec<(Timestamp, Timestamp)> = records
            .iter()
            .filter_map(|record| {
                let turns: Vec<_> = record.turns_between(from, to).collect();
                Some((turns.first()?.asked_at, turns.iter().filter_map(|turn| turn.finished_at).max().unwrap_or(self.now.min(to))))
            })
            .collect();
        let axis = self.axis(&spans, day);
        let (bars, lanes) = self.bars(&records, day, &axis);
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for record in &records {
            *counts.entry(record.project().to_owned()).or_default() += 1;
        }
        let mut filters = vec![(None, self.language.pick("전체", "All").to_owned(), records.len())];
        filters.extend(counts.into_iter().map(|(name, count)| (Some(name.clone()), name, count)));
        let mut shown: Vec<&WorkRecord> = records.iter().copied().filter(|record| place.project.as_deref().is_none_or(|project| record.project() == project)).collect();
        shown.sort_by_key(|record| std::cmp::Reverse(record.turns_between(from, to).last().map(|turn| turn.asked_at)));
        DayView {
            masthead: Masthead {
                big: day.day().to_string(),
                top: self.weekday_long(day.weekday()),
                bottom: match self.language {
                    Language::Korean => format!("{}년 {}월", day.year(), day.month()),
                    Language::English => format!("{} {}", self.month_name(day.month()), day.year()),
                },
                pills: self.tally_pills(&records, from, to, false),
                can_go_forward: place.back > 0,
            },
            axis,
            bars,
            lanes,
            filters,
            cards: shown.into_iter().map(|record| self.card(record, from, to)).collect(),
        }
    }

    fn monday(&self, back: i64) -> Date {
        let today = self.today();
        let monday = today.checked_sub((today.weekday().to_monday_zero_offset() as i64).days()).unwrap_or(today);
        monday.checked_sub((back * 7).days()).unwrap_or(monday)
    }

    pub fn week(&self, place: &JournalPlace) -> WeekView {
        let monday = self.monday(place.back);
        let sunday = monday.checked_add(6.days()).unwrap_or(monday);
        let from = self.start_of(monday);
        let to = self.start_of(sunday.tomorrow().unwrap_or(sunday));
        let records = self.within(from, to);
        let mut spans = Vec::new();
        for offset in 0..7 {
            let day = monday.checked_add(offset.days()).unwrap_or(monday);
            let (day_from, day_to) = (self.start_of(day), self.start_of(day.tomorrow().unwrap_or(day)));
            for record in &records {
                let turns: Vec<_> = record.turns_between(day_from, day_to).collect();
                if let Some(first) = turns.first() {
                    // Every day is laid on the same hours, so their spans are moved to the first day's clock.
                    let shift = SignedDuration::from_hours(24 * offset);
                    let end = turns.iter().filter_map(|turn| turn.finished_at).max().unwrap_or(first.asked_at);
                    spans.push((first.asked_at - shift, end - shift));
                }
            }
        }
        let axis = HourAxis { now: None, now_text: String::new(), ..self.axis(&spans, monday) };
        let mut rows = Vec::new();
        for offset in 0..7 {
            let day = monday.checked_add(offset.days()).unwrap_or(monday);
            let today = day == self.today();
            let day_axis = HourAxis { now: None, now_text: String::new(), ..axis.clone() };
            let (bars, lanes) = self.bars(&records, day, &day_axis);
            let now = today.then(|| {
                let hours = self.start_of(day).duration_until(self.now).as_secs_f64() / 3600.0;
                ((hours - axis.first as f64) / (axis.last - axis.first) as f64).clamp(0.0, 1.0)
            });
            rows.push(WeekRow { label: format!("{} {}", self.weekday(day.weekday()), day.day()), today, bars, lanes, now });
        }
        let mut by_project: BTreeMap<String, Vec<&WorkRecord>> = BTreeMap::new();
        for record in &records {
            by_project.entry(record.project().to_owned()).or_default().push(record);
        }
        let mut projects: Vec<ProjectCard> = by_project
            .into_iter()
            .map(|(name, mut list)| {
                list.sort_by_key(|record| std::cmp::Reverse(record.turns_between(from, to).last().map(|turn| turn.asked_at)));
                let commits = list.iter().map(|record| record.commits.iter().filter(|commit| commit.at >= from && commit.at < to).count()).sum();
                let pulls: Vec<u32> = list.iter().flat_map(|record| record.pull_requests.iter().filter(|pull| pull.at >= from && pull.at < to).map(|pull| pull.number)).collect();
                let lines = list
                    .iter()
                    .map(|record| {
                        let last = record.turns_between(from, to).last().map(|turn| turn.asked_at).unwrap_or(from);
                        let date = self.date(last);
                        Line {
                            key: record.key.clone(),
                            provider: record.provider,
                            title: record.title.clone(),
                            callout: self.callout(record),
                            pull_request: record.pull_requests.iter().find(|pull| pull.at >= from && pull.at < to).map(|pull| format!("PR #{}", pull.number)),
                            when: format!("{} {}", self.weekday(date.weekday()), self.clock(last)),
                        }
                    })
                    .collect();
                ProjectCard { count: list.len(), marks: self.marks(commits, &pulls), name, lines }
            })
            .collect();
        projects.sort_by_key(|project| std::cmp::Reverse(project.count));
        let current = place.back == 0;
        WeekView {
            masthead: Masthead {
                big: format!("{}–{}", monday.day(), sunday.day()),
                top: if monday.month() == sunday.month() {
                    self.month_name(monday.month())
                } else {
                    format!("{}–{}", self.month_name(monday.month()), self.month_name(sunday.month()))
                },
                bottom: if current { self.language.pick("이번 주", "This week").to_owned() } else { monday.year().to_string() },
                pills: self.tally_pills(&records, from, to, true),
                can_go_forward: place.back > 0,
            },
            axis,
            rows,
            projects,
        }
    }

    fn split(&self, records: &[&WorkRecord], from: Timestamp, to: Timestamp, limit: usize) -> Vec<ProjectSplit> {
        let mut by_project: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
        for record in records {
            let entry = by_project.entry(record.project().to_owned()).or_default();
            match record.provider {
                Provider::Codex => entry.0 += 1,
                Provider::Claude => entry.1 += 1,
            }
            entry.2 += record.commits.iter().filter(|commit| commit.at >= from && commit.at < to).count();
        }
        let mut list: Vec<ProjectSplit> = by_project
            .into_iter()
            .map(|(name, (codex, claude, commits))| ProjectSplit { name, count: codex + claude, codex, claude, marks: self.marks(commits, &[]) })
            .collect();
        list.sort_by_key(|project| std::cmp::Reverse(project.count));
        list.truncate(limit);
        list
    }

    pub fn month(&self, place: &JournalPlace) -> MonthView {
        let today = self.today();
        let mut first = today.first_of_month();
        for _ in 0..place.back {
            first = first.checked_sub(1.month()).unwrap_or(first).first_of_month();
        }
        let next = first.checked_add(1.month()).unwrap_or(first);
        let (from, to) = (self.start_of(first), self.start_of(next));
        let records = self.within(from, to);
        let lead = first.weekday().to_sunday_zero_offset() as i64;
        let days = first.days_in_month() as i64;
        let weeks = (lead + days + 6) / 7;
        let mut cells = Vec::new();
        let mut full_rows = 0;
        for week in 0..weeks {
            let mut started = false;
            for weekday in 0..7 {
                let index = week * 7 + weekday - lead;
                if index < 0 || index >= days {
                    cells.push(CalendarCell { day: None, today: false, future: false, lines: Vec::new() });
                    continue;
                }
                let date = first.checked_add(index.days()).unwrap_or(first);
                started |= date <= today;
                let (day_from, day_to) = (self.start_of(date), self.start_of(date.tomorrow().unwrap_or(date)));
                let mut lines: Vec<(Timestamp, DayLine)> = records
                    .iter()
                    .filter_map(|record| {
                        let first_turn = record.turns_between(day_from, day_to).next()?;
                        Some((first_turn.asked_at, DayLine { key: record.key.clone(), title: record.title.clone(), provider: record.provider, callout: self.callout(record).filter(|callout| *callout != Callout::New) }))
                    })
                    .collect();
                lines.sort_by_key(|(at, _)| *at);
                cells.push(CalendarCell { day: Some(date.day()), today: date == today, future: date > today, lines: lines.into_iter().map(|(_, line)| line).collect() });
            }
            if started {
                full_rows += 1;
            }
        }
        let mut pull_requests: Vec<PullRequestRow> = records
            .iter()
            .flat_map(|record| {
                record.pull_requests.iter().filter(|pull| pull.at >= from && pull.at < to).map(|pull| PullRequestRow {
                    number: format!("#{}", pull.number),
                    title: record.title.clone(),
                    when: match self.language {
                        Language::Korean => format!("{}일", self.date(pull.at).day()),
                        Language::English => self.month_day(self.date(pull.at)),
                    },
                    url: pull.url.clone(),
                })
            })
            .collect();
        pull_requests.reverse();
        MonthView {
            masthead: Masthead {
                big: match self.language {
                    Language::Korean => first.month().to_string(),
                    Language::English => self.month_name(first.month()),
                },
                top: self.language.pick("월", "").to_owned(),
                bottom: first.year().to_string() + self.language.pick("년", ""),
                pills: self.tally_pills(&records, from, to, true),
                can_go_forward: place.back > 0,
            },
            weekdays: match self.language {
                Language::Korean => ["일", "월", "화", "수", "목", "금", "토"],
                Language::English => ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"],
            },
            cells,
            full_rows: full_rows.max(1),
            projects: self.split(&records, from, to, 4),
            pull_requests,
        }
    }

    pub fn report(&self, place: &JournalPlace) -> ReportView {
        let today = self.today();
        let sunday = today.checked_sub((today.weekday().to_sunday_zero_offset() as i64).days()).unwrap_or(today);
        let span_start = sunday.checked_sub(((place.span.weeks() - 1) * 7).days()).unwrap_or(sunday);
        let (from, to) = (self.start_of(span_start), self.start_of(today.tomorrow().unwrap_or(today)));
        let records = self.within(from, to);
        let requests: Vec<Timestamp> = self.records.iter().flat_map(|record| record.turns.iter().map(|turn| turn.asked_at)).collect();
        let first_record = requests.iter().min().map(|at| self.date(*at));
        // The heat map is always a year, ending with this week.
        let grid_start = sunday.checked_sub((52 * 7).days()).unwrap_or(sunday);
        let mut per_day: HashMap<Date, usize> = HashMap::new();
        for at in &requests {
            *per_day.entry(self.date(*at)).or_default() += 1;
        }
        let mut heatmap = Vec::new();
        let mut heatmap_months = Vec::new();
        let mut last_month = 0;
        for week in 0..53 {
            let week_start = grid_start.checked_add((week * 7).days()).unwrap_or(grid_start);
            if week_start.month() != last_month {
                heatmap_months.push((week as usize, self.month_name(week_start.month())));
                last_month = week_start.month();
            }
            for day in 0..7 {
                let date = week_start.checked_add(day.days()).unwrap_or(week_start);
                heatmap.push(if date > today {
                    None
                } else if first_record.is_none_or(|first| date < first) {
                    Some(-1)
                } else {
                    Some(per_day.get(&date).copied().unwrap_or(0).min(5) as i8)
                });
            }
        }
        let mut weeks = Vec::new();
        for week in 0..place.span.weeks() {
            let week_start = span_start.checked_add((week * 7).days()).unwrap_or(span_start);
            let (week_from, week_to) = (self.start_of(week_start), self.start_of(week_start.checked_add(7.days()).unwrap_or(week_start)));
            let in_week = self.within(week_from, week_to);
            weeks.push(WeekBar {
                codex: in_week.iter().filter(|record| record.provider == Provider::Codex).count(),
                claude: in_week.iter().filter(|record| record.provider == Provider::Claude).count(),
                label: format!("{}/{}", week_start.month(), week_start.day()),
            });
        }
        let first_hour = 6;
        let mut hours = vec![0; 18];
        for record in &records {
            for turn in record.turns_between(from, to) {
                let hour = turn.asked_at.to_zoned(self.zone.clone()).hour();
                if hour >= first_hour && ((hour - first_hour) as usize) < hours.len() {
                    hours[(hour - first_hour) as usize] += 1;
                }
            }
        }
        let worked = records.iter().map(|record| record.turns_between(from, to).filter_map(|turn| turn.duration()).fold(SignedDuration::ZERO, |total, span| total + span)).fold(SignedDuration::ZERO, |total, span| total + span);
        let commits: usize = records.iter().map(|record| record.commits.iter().filter(|commit| commit.at >= from).count()).sum();
        let pulls: usize = records.iter().map(|record| record.pull_requests.iter().filter(|pull| pull.at >= from).count()).sum();
        ReportView {
            masthead: Masthead {
                big: records.len().to_string(),
                top: self.language.pick("작업", "tasks").to_owned(),
                bottom: format!("{} – {}", self.month_day(span_start), self.month_day(today)),
                pills: vec![
                    Pill { callout: None, icon: "git-commit-horizontal", text: format!("{} {commits}", self.language.pick("커밋", "Commits")) },
                    Pill { callout: None, icon: "git-pull-request", text: format!("PR {pulls}") },
                    Pill { callout: None, icon: "clock", text: format!("{} {}", self.language.pick("AI 작업", "AI work"), self.duration(worked)) },
                ],
                can_go_forward: false,
            },
            heatmap,
            heatmap_months,
            since: first_record.map(|first| format!("{}{}", self.month_day(first), self.language.pick("부터 기록", " onwards"))).unwrap_or_default(),
            weeks,
            projects: self.split(&records, from, to, 5),
            hours,
            first_hour,
        }
    }

    pub fn detail(&self, key: &str) -> Option<DetailView> {
        let record = self.records.iter().find(|record| record.key == key)?;
        let start = record.started_at()?;
        let end = record.ended_at();
        let day = self.date(start);
        let time = match end {
            Some(end) => format!("{} {} – {} · {}", self.month_day(day), self.clock(start), self.clock(end), self.duration(record.working_time())),
            None => format!("{} {}{}", self.month_day(day), self.clock(start), self.language.pick("부터", " on")),
        };
        let turns = record
            .turns
            .iter()
            .enumerate()
            .map(|(index, turn)| {
                let until = record.turns.get(index + 1).map(|next| next.asked_at).unwrap_or(Timestamp::MAX);
                TurnRow {
                    time: self.clock(turn.asked_at),
                    ask: preview(&turn.ask, 160),
                    result: turn.result.as_deref().map(|result| preview(result, 220)).unwrap_or_else(|| {
                        if turn.aborted { self.language.pick("중단했어요.", "Stopped.").to_owned() } else if turn.finished_at.is_none() { self.language.pick("작업 중이에요.", "Still working.").to_owned() } else { String::new() }
                    }),
                    commits: record.commits.iter().filter(|commit| commit.at >= turn.asked_at && commit.at < until).map(|commit| commit.sha.clone()).collect(),
                }
            })
            .collect();
        let prefix = format!("{}/", record.folder.trim_end_matches('/'));
        let files = record
            .files
            .iter()
            .map(|file| FileRow {
                path: file.path.strip_prefix(&prefix).unwrap_or(&file.path).to_owned(),
                diff: match (file.added, file.removed) {
                    (added, 0) => format!("+{added}"),
                    (0, removed) => format!("−{removed}"),
                    (added, removed) => format!("+{added} −{removed}"),
                },
                created: file.created,
            })
            .collect();
        let count = |word: (&'static str, &'static str), n: usize| format!("{} {n}", self.language.pick(word.0, word.1));
        Some(DetailView {
            key: record.key.clone(),
            provider: record.provider,
            title: record.title.clone(),
            project: record.project().to_owned(),
            branch: record.branch.clone(),
            time,
            url: record.url.clone(),
            open_label: match record.provider {
                Provider::Codex => self.language.pick("Codex에서 열기", "Open in Codex"),
                Provider::Claude => self.language.pick("Claude에서 열기", "Open in Claude"),
            }
            .to_owned(),
            pills: vec![
                count(("요청", "Requests"), record.turns.len()),
                count(("커밋", "Commits"), record.commits.len()),
                count(("파일", "Files"), record.files.len()),
            ],
            turns,
            commits: record.commits.iter().map(|commit| (commit.sha.clone(), commit.message.clone(), self.clock(commit.at))).collect(),
            files,
        })
    }

    /// The day, week or month on screen as a plain log, project by project, for pasting into a report.
    pub fn log(&self, place: &JournalPlace) -> String {
        let (heading, from, to) = match place.tab {
            JournalTab::Day | JournalTab::Report => {
                let day = self.today().checked_sub(place.back.days()).unwrap_or(self.today());
                (format!("{} {}", self.month_day(day), self.weekday_long(day.weekday())), self.start_of(day), self.start_of(day.tomorrow().unwrap_or(day)))
            }
            JournalTab::Week => {
                let monday = self.monday(place.back);
                let next = monday.checked_add(7.days()).unwrap_or(monday);
                (format!("{} – {}", self.month_day(monday), self.month_day(next.yesterday().unwrap_or(next))), self.start_of(monday), self.start_of(next))
            }
            JournalTab::Month => {
                let mut first = self.today().first_of_month();
                for _ in 0..place.back {
                    first = first.checked_sub(1.month()).unwrap_or(first).first_of_month();
                }
                let next = first.checked_add(1.month()).unwrap_or(first);
                (format!("{} {}", first.year(), self.month_name(first.month())), self.start_of(first), self.start_of(next))
            }
        };
        let records = self.within(from, to);
        let mut by_project: BTreeMap<String, Vec<&WorkRecord>> = BTreeMap::new();
        for record in &records {
            by_project.entry(record.project().to_owned()).or_default().push(record);
        }
        let mut text = format!("## {heading}\n");
        for (project, list) in by_project {
            text.push_str(&format!("\n### {project}\n"));
            for record in list {
                let commits = record.commits.iter().filter(|commit| commit.at >= from && commit.at < to).count();
                let pulls: Vec<u32> = record.pull_requests.iter().filter(|pull| pull.at >= from && pull.at < to).map(|pull| pull.number).collect();
                let marks = self.marks(commits, &pulls);
                let result = record.turns_between(from, to).last().and_then(|turn| turn.result.as_deref()).map(|result| preview(result.lines().find(|line| !line.trim().is_empty()).unwrap_or(""), 120));
                let mut line = format!("- {}", record.title);
                if let Some(result) = result.filter(|result| !result.is_empty()) {
                    line.push_str(&format!(": {result}"));
                }
                if !marks.is_empty() {
                    line.push_str(&format!(" ({marks})"));
                }
                text.push_str(&line);
                text.push('\n');
            }
        }
        text
    }
}

/// The view on screen.
#[derive(Clone, Debug, PartialEq)]
pub enum Body {
    Day(DayView),
    Week(WeekView),
    Month(MonthView),
    Report(ReportView),
}

/// The words the window draws besides the records themselves.
#[derive(Clone, Debug, PartialEq)]
pub struct Words {
    pub copy: &'static str,
    pub search: &'static str,
    pub back: &'static str,
    pub forward: &'static str,
    pub by_hour: &'static str,
    pub by_weekday: &'static str,
    pub projects: &'static str,
    pub pull_requests: &'static str,
    pub history: &'static str,
    pub history_span: &'static str,
    pub weekly: &'static str,
    pub hours: &'static str,
    pub flow: &'static str,
    pub commits: &'static str,
    pub files: &'static str,
    pub new_file: &'static str,
    pub close: &'static str,
    pub copy_summary: &'static str,
    pub fewer: &'static str,
    pub more: &'static str,
    pub loading: &'static str,
    pub empty: &'static str,
    pub more_lines: &'static str,
}

impl Words {
    pub fn new(language: Language) -> Words {
        let pick = |korean, english| language.pick(korean, english);
        Words {
            copy: pick("일지로 복사", "Copy as log"),
            search: pick("기록 검색", "Search records"),
            back: pick("이전", "Previous"),
            forward: pick("다음", "Next"),
            by_hour: pick("시간대별", "By hour"),
            by_weekday: pick("요일별", "By day"),
            projects: pick("프로젝트", "Projects"),
            pull_requests: pick("만든 PR", "Pull requests"),
            history: pick("작업 기록", "Work history"),
            history_span: pick("지난 1년", "Past year"),
            weekly: pick("주별 작업", "Tasks per week"),
            hours: pick("요청한 시각", "When you asked"),
            flow: pick("흐름", "Flow"),
            commits: pick("커밋", "Commits"),
            files: pick("바뀐 파일", "Changed files"),
            new_file: pick("새 파일", "New"),
            close: pick("닫기", "Close"),
            copy_summary: pick("요약 복사", "Copy summary"),
            fewer: pick("적음", "Less"),
            more: pick("많음", "More"),
            loading: pick("기록을 읽는 중…", "Reading records…"),
            empty: pick("이 기간에 맡긴 작업이 없어요", "No tasks in this period"),
            more_lines: pick("외 {}개", "+{} more"),
        }
    }
}

/// A service's remaining allowance as the title bar's small battery.
#[derive(Clone, Debug, PartialEq)]
pub struct MiniBattery {
    pub provider: Provider,
    pub level: Option<f64>,
    pub percent: String,
    pub low: bool,
}

/// Everything the window draws.
#[derive(Clone, Debug, PartialEq)]
pub struct Screen {
    pub place: JournalPlace,
    pub tabs: Vec<(JournalTab, &'static str)>,
    pub spans: Vec<(ReportSpan, &'static str)>,
    pub usage: Vec<MiniBattery>,
    pub body: Body,
    pub detail: Option<DetailView>,
    pub log: String,
    /// No records have been read yet.
    pub loading: bool,
    pub words: Words,
    /// The person's character, as the widget draws it.
    pub avatar: String,
}

impl Journal<'_> {
    pub fn screen(&self, place: &JournalPlace, usage: Vec<MiniBattery>, loading: bool, avatar: String) -> Screen {
        let body = match place.tab {
            JournalTab::Day => Body::Day(self.day(place)),
            JournalTab::Week => Body::Week(self.week(place)),
            JournalTab::Month => Body::Month(self.month(place)),
            JournalTab::Report => Body::Report(self.report(place)),
        };
        Screen {
            place: place.clone(),
            tabs: JournalTab::ALL.iter().map(|tab| (*tab, tab.name(self.language))).collect(),
            spans: ReportSpan::ALL.iter().map(|span| (*span, span.name(self.language))).collect(),
            usage,
            body,
            detail: place.open.as_deref().and_then(|key| self.detail(key)),
            log: self.log(place),
            loading,
            words: Words::new(self.language),
            avatar,
        }
    }
}
