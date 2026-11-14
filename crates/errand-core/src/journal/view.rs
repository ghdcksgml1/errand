//! What the desktop window shows for a day, a week, a month, a report or one task, worked out from the work
//! records with every word already in the person's language. The platform only draws it.

use super::WorkRecord;
use crate::sessions::SessionState;
use crate::sessions::text::preview;
use crate::{Language, Provider};
use jiff::civil::{Date, Weekday};
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp, ToSpan};
use std::collections::{BTreeMap, HashMap, HashSet};

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
    /// The views the window offers: a report's figures follow whichever of these is on screen.
    pub const SHOWN: [JournalTab; 3] = [JournalTab::Day, JournalTab::Week, JournalTab::Month];

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
    /// The recap card on screen, over the week.
    pub recap: Option<usize>,
}

/// The widget's live state for a task, where it shows one.
#[derive(Clone, Debug, PartialEq)]
pub struct Live {
    pub state: SessionState,
    pub question: Option<String>,
}

/// Answers translated into the person's language, kept only while Errand runs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Translations {
    /// Each answer as the panel shows it, by the words it was written in.
    pub texts: HashMap<String, String>,
    /// The task whose answers are being translated.
    pub working: Option<String>,
    /// The task whose answers could not be translated, and why.
    pub failed: Option<(String, String)>,
    /// The person asked to see the answers as they were written.
    pub original: bool,
}

/// Everything the views read.
pub struct Journal<'a> {
    pub records: &'a [WorkRecord],
    pub live: &'a HashMap<String, Live>,
    pub translations: &'a Translations,
    pub summaries: &'a super::summary::Summaries,
    pub now: Timestamp,
    pub zone: &'a TimeZone,
    pub language: Language,
}

/// Whether `text` reads as another language than `language`: Korean letters against Latin ones, leaving out
/// anything in backticks. Short bits such as a hash or a file name count as neither.
pub fn foreign(text: &str, language: Language) -> bool {
    let (mut hangul, mut latin, mut code) = (0usize, 0usize, false);
    for c in text.chars() {
        match c {
            '`' => code = !code,
            _ if code => {}
            '\u{AC00}'..='\u{D7A3}' | '\u{3131}'..='\u{318E}' => hangul += 1,
            _ if c.is_ascii_alphabetic() => latin += 1,
            _ => {}
        }
    }
    // A Korean syllable carries about as much as three Latin letters.
    match language {
        Language::Korean => latin >= 12 && hangul * 3 < latin,
        Language::English => hangul >= 6 && hangul * 3 >= latin,
    }
}

/// The panel's offer to translate a task's answers.
#[derive(Clone, Debug, PartialEq)]
pub enum Translate {
    /// Some answers are in another language; these are the ones not translated yet.
    Offer(Vec<String>),
    Working,
    /// The last try failed: why, and the answers to try again.
    Failed(String, Vec<String>),
    /// The translations are on screen; the button shows the originals.
    Shown,
    /// The originals are on screen; the button shows the translations.
    Original,
}

/// The panel's words for its translate button.
pub fn translate_label(translate: &Translate, language: Language) -> &'static str {
    match translate {
        Translate::Offer(_) => language.pick("번역", "Translate"),
        Translate::Working => language.pick("번역 중…", "Translating…"),
        Translate::Failed(..) => language.pick("다시 번역", "Translate again"),
        Translate::Shown => language.pick("원문 보기", "Show original"),
        Translate::Original => language.pick("번역 보기", "Show translation"),
    }
}

/// A span of minutes in words: "1시간 22분", "45m". Never less than a minute.
pub(crate) fn minutes_text(minutes: i64, language: Language) -> String {
    let minutes = minutes.max(1);
    let (hours, rest) = (minutes / 60, minutes % 60);
    match (language, hours, rest) {
        (Language::Korean, 0, _) => format!("{rest}분"),
        (Language::Korean, _, 0) => format!("{hours}시간"),
        (Language::Korean, _, _) => format!("{hours}시간 {rest}분"),
        (Language::English, 0, _) => format!("{rest}m"),
        (Language::English, _, 0) => format!("{hours}h"),
        (Language::English, _, _) => format!("{hours}h {rest}m"),
    }
}

/// Minutes as figures: hours and minutes large, their units small.
pub(crate) fn duration_parts(minutes: i64, language: Language) -> Vec<StatPart> {
    let minutes = minutes.max(0);
    let (hours, rest) = (minutes / 60, minutes % 60);
    let (h, m) = (language.pick("시간", "h"), language.pick("분", "m"));
    let part = |text: String, small: bool| StatPart { text, small };
    match hours {
        0 => vec![part(rest.to_string(), false), part(m.into(), true)],
        _ => vec![part(hours.to_string(), false), part(h.into(), true), part(format!(" {rest}"), false), part(m.into(), true)],
    }
}

/// The names of the date line's counts: what was finished, what waits for an answer, the agents' time, and
/// the commits and pull requests.
pub(crate) fn stat_names(language: Language) -> [&'static str; 4] {
    [language.pick("끝낸 일", "Done"), language.pick("답변 필요", "Needs answer"), language.pick("AI 작업", "AI time"), language.pick("커밋 · PR", "Commits · PRs")]
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

/// One piece of a count's figure: the number large, its unit small.
#[derive(Clone, Debug, PartialEq)]
pub struct StatPart {
    pub text: String,
    pub small: bool,
}

/// A count at the end of the date line: a label over a large figure.
#[derive(Clone, Debug, PartialEq)]
pub struct Stat {
    pub label: String,
    pub parts: Vec<StatPart>,
    /// Drawn in the answer-needed colour.
    pub asking: bool,
}

/// The big number and the two lines beside it.
#[derive(Clone, Debug, PartialEq)]
pub struct Masthead {
    pub big: String,
    pub top: String,
    pub bottom: String,
    pub pills: Vec<Pill>,
    /// Counts drawn large instead of the pills and the character, on the day and the week.
    pub stats: Vec<Stat>,
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
    /// For work waiting on the person's answer: how far the wait runs, to now.
    pub waiting: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HourAxis {
    pub first: i8,
    pub last: i8,
    /// Where now is, while the axis shows today.
    pub now: Option<f64>,
    pub now_text: String,
}

/// A task as a row in the day's list.
#[derive(Clone, Debug, PartialEq)]
pub struct TaskRow {
    pub key: String,
    pub provider: Provider,
    pub title: String,
    /// Project, time and marks.
    pub meta: String,
    pub callout: Option<Callout>,
}

/// A finished piece of work in a summary, with what it left behind.
#[derive(Clone, Debug, PartialEq)]
pub struct DraftLine {
    pub key: Option<String>,
    pub project: String,
    pub text: String,
    pub mark: String,
}

/// Something a summary says is left.
#[derive(Clone, Debug, PartialEq)]
pub struct LeftLine {
    pub key: Option<String>,
    pub title: String,
    pub text: String,
    pub open: super::summary::Open,
}

/// A day's summary as the character hands it over.
#[derive(Clone, Debug, PartialEq)]
pub struct DraftCard {
    pub date: Date,
    pub title: String,
    pub chip: String,
    pub note: String,
    pub done: Vec<DraftLine>,
    pub left: Vec<LeftLine>,
    /// More work ended after it was made: the words, and whether it can be made again now.
    pub stale: Option<(String, bool)>,
    pub footer: String,
    /// The day as a report to paste.
    pub report: String,
}

/// Where the day's summary stands.
#[derive(Clone, Debug, PartialEq)]
pub enum Draft {
    /// Nothing finished that day yet.
    Nothing(String),
    /// Finished work with no summary: a note, and the button that makes one.
    Offer(String, String),
    Making(String),
    /// Why it failed, and a button to try again when that can help.
    Failed(String, Option<String>),
    Ready(Box<DraftCard>),
}

/// The same day a month earlier, when there was work then.
#[derive(Clone, Debug, PartialEq)]
pub struct BackThen {
    pub label: String,
    pub text: String,
    /// Days back from the day on screen, to go there.
    pub days: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DayView {
    pub date: Date,
    pub masthead: Masthead,
    pub axis: HourAxis,
    pub bars: Vec<Bar>,
    pub lanes: usize,
    pub draft: Draft,
    pub tasks: Vec<TaskRow>,
    pub back_then: Option<BackThen>,
}

/// One day of the week as the stitched page shows it: its summary's lines, or its tasks before one is made.
#[derive(Clone, Debug, PartialEq)]
pub struct WeekDay {
    pub date: Date,
    pub day: String,
    pub label: String,
    pub lines: Vec<DraftLine>,
    /// Lines left out to keep the day short.
    pub more: usize,
    /// "오늘 초안", "고침" or "정리 전": how far the day's summary is.
    pub note: Option<String>,
    /// The day's work across 08–19, as fractions, in its service's colour.
    pub strip: Vec<(f64, f64, Provider)>,
    pub today: bool,
}

/// The way into the week's recap cards.
#[derive(Clone, Debug, PartialEq)]
pub struct RecapEntry {
    pub title: String,
    pub note: String,
    pub button: String,
    pub count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WeekView {
    pub masthead: Masthead,
    pub monday: Date,
    pub title: String,
    pub days: Vec<WeekDay>,
    pub recap: Option<RecapEntry>,
    pub carried_title: String,
    pub carried: Vec<LeftLine>,
    pub kinds: Vec<(String, usize)>,
    /// The week as a report to paste; empty when no day is summed up.
    pub report: String,
    /// The week as a metro map of its projects and tasks.
    pub metro: super::metro::MetroView,
}

/// One card of a week's recap.
#[derive(Clone, Debug, PartialEq)]
pub enum RecapCard {
    Cover { label: String, title: String, span: String, chips: Vec<String> },
    Finished { label: String, count: usize, versus: String, waiting: usize, note: String },
    Kind { label: String, top: String, of: String, kinds: Vec<(String, usize)> },
    Busiest { label: String, day: String, note: String, days: Vec<(String, f64, bool)> },
    Longest { label: String, provider: Provider, project: String, title: String, time: Vec<StatPart>, span: String, bar: Option<(f64, f64)> },
    Carried { label: String, count: usize, items: Vec<(String, String, bool)>, save: String },
}

/// A week's recap: its cards and which one is on screen.
#[derive(Clone, Debug, PartialEq)]
pub struct RecapView {
    pub cards: Vec<RecapCard>,
    pub index: usize,
    pub position: String,
    pub save_one: String,
    pub save_all: String,
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
    /// None when every answer is already in the person's language.
    pub translate: Option<Translate>,
    pub commits: Vec<(String, String, String)>,
    pub files: Vec<FileRow>,
}

impl Journal<'_> {
    pub(crate) fn date(&self, at: Timestamp) -> Date {
        at.to_zoned(self.zone.clone()).date()
    }

    pub(crate) fn today(&self) -> Date {
        self.date(self.now)
    }

    pub(crate) fn start_of(&self, date: Date) -> Timestamp {
        date.to_zoned(self.zone.clone()).map(|zoned| zoned.timestamp()).unwrap_or(self.now)
    }

    pub(crate) fn clock(&self, at: Timestamp) -> String {
        at.to_zoned(self.zone.clone()).strftime("%H:%M").to_string()
    }

    pub(crate) fn callout(&self, record: &WorkRecord) -> Option<Callout> {
        match self.live.get(&record.key) {
            Some(live) => Callout::of(live.state),
            None => self.running(record).then_some(Callout::Running),
        }
    }

    /// The live state for a period that holds the task's latest request. A task worked on again after `to` was
    /// at rest at the end of that period, whatever it is doing now.
    pub(crate) fn callout_by(&self, record: &WorkRecord, to: Timestamp) -> Option<Callout> {
        if record.turns.last().is_some_and(|turn| turn.asked_at < to) { self.callout(record) } else { None }
    }

    /// Still at work: as the widget says, or, for a session it does not show, a request this recent with no answer
    /// yet. An older unanswered request stopped without a word.
    pub(crate) fn running(&self, record: &WorkRecord) -> bool {
        match self.live.get(&record.key) {
            Some(live) => matches!(Callout::of(live.state), Some(Callout::Running | Callout::Asking)),
            None => record.turns.last().is_some_and(|turn| turn.finished_at.is_none() && turn.asked_at.duration_until(self.now) < UNANSWERED),
        }
    }

    pub(crate) fn weekday(&self, weekday: Weekday) -> &'static str {
        let index = weekday.to_sunday_zero_offset() as usize;
        match self.language {
            Language::Korean => ["일", "월", "화", "수", "목", "금", "토"][index],
            Language::English => ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][index],
        }
    }

    pub(crate) fn weekday_long(&self, weekday: Weekday) -> String {
        match self.language {
            Language::Korean => format!("{}요일", self.weekday(weekday)),
            Language::English => ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"][weekday.to_sunday_zero_offset() as usize].to_owned(),
        }
    }

    pub(crate) fn month_name(&self, month: i8) -> String {
        match self.language {
            Language::Korean => format!("{month}월"),
            Language::English => ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][(month - 1) as usize].to_owned(),
        }
    }

    pub(crate) fn month_day(&self, date: Date) -> String {
        match self.language {
            Language::Korean => format!("{}월 {}일", date.month(), date.day()),
            Language::English => format!("{} {}", self.month_name(date.month()), date.day()),
        }
    }

    pub(crate) fn duration(&self, span: SignedDuration) -> String {
        minutes_text(span.as_mins(), self.language)
    }

    pub(crate) fn marks(&self, commits: usize, pulls: &[u32]) -> String {
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
        // A count of nothing says nothing.
        if commits > 0 {
            pills.push(Pill { callout: None, icon: "git-commit-horizontal", text: format!("{} {commits}", self.language.pick("커밋", "Commits")) });
        }
        if pulls > 0 {
            pills.push(Pill { callout: None, icon: "git-pull-request", text: format!("PR {pulls}") });
        }
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
        let mut spans: Vec<(&WorkRecord, Timestamp, Timestamp, Option<Timestamp>)> = records
            .iter()
            .filter_map(|record| {
                let turns: Vec<_> = record.turns_between(from, to).collect();
                let begin = turns.first()?.asked_at;
                let last_ask = turns.last()?.asked_at;
                let end = turns.iter().filter_map(|turn| turn.finished_at).max().unwrap_or(begin).max(last_ask);
                let running = turns.last().is_some_and(|turn| turn.finished_at.is_none()) && self.running(record);
                // Work waiting on the person ends where the agent last did something; the wait runs on to now.
                let asking = self.callout(record) == Some(Callout::Asking);
                let last_mark = record.commits.iter().filter(|commit| commit.at >= begin && commit.at < to).map(|commit| commit.at).max().unwrap_or(begin);
                let end = if asking { end.max(last_mark) } else if running { self.now.min(to) } else { end.max(begin) };
                Some((*record, begin, end, asking.then(|| self.now.min(to))))
            })
            .collect();
        spans.sort_by_key(|(_, begin, _, _)| *begin);
        for (record, begin, end, waiting) in spans {
            let (start, end) = (place(begin), place(end).max(place(begin) + 0.012));
            let waiting = waiting.map(place).filter(|until| *until > end);
            let reach = waiting.unwrap_or(end);
            let lane = lane_ends.iter().position(|ends| *ends <= start).unwrap_or(lane_ends.len());
            if lane == lane_ends.len() {
                lane_ends.push(reach);
            } else {
                lane_ends[lane] = reach;
            }
            bars.push(Bar { key: record.key.clone(), provider: record.provider, start, end, lane, label: preview(&record.title, 24), callout: self.callout(record), waiting });
        }
        (bars, lane_ends.len().max(1))
    }

    /// A task's time that day: from and to with its working time, or since when while it runs.
    pub(crate) fn task_time(&self, record: &WorkRecord, from: Timestamp, to: Timestamp) -> String {
        let turns: Vec<_> = record.turns_between(from, to).collect();
        let first = turns.first().map(|turn| turn.asked_at).unwrap_or(from);
        let unanswered = turns.last().is_some_and(|turn| turn.finished_at.is_none());
        let finished = turns.iter().filter_map(|turn| turn.finished_at).max();
        let worked: SignedDuration = turns.iter().filter_map(|turn| turn.duration()).fold(SignedDuration::ZERO, |total, span| total + span);
        match finished {
            _ if unanswered && self.running(record) => format!("{}{}", self.clock(first), self.language.pick("부터", " on")),
            Some(end) if !unanswered => format!("{} – {} · {}", self.clock(first), self.clock(end), self.duration(worked)),
            _ => self.clock(first),
        }
    }

    fn task_row(&self, record: &WorkRecord, from: Timestamp, to: Timestamp) -> TaskRow {
        let commits = record.commits.iter().filter(|commit| commit.at >= from && commit.at < to).count();
        let pulls: Vec<u32> = record.pull_requests.iter().filter(|pull| pull.at >= from && pull.at < to).map(|pull| pull.number).collect();
        let meta = [record.project().to_owned(), self.task_time(record, from, to), self.marks(commits, &pulls)].into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" · ");
        TaskRow { key: record.key.clone(), provider: record.provider, title: record.title.clone(), meta, callout: self.callout(record) }
    }

    /// Finished that day: the day's last request answered, and nothing live about it.
    pub(crate) fn finished_on(&self, record: &WorkRecord, from: Timestamp, to: Timestamp) -> bool {
        let last = record.turns_between(from, to).last();
        last.is_some_and(|turn| turn.finished_at.is_some()) && !matches!(self.callout_by(record, to), Some(Callout::Running | Callout::Asking))
    }

    /// A span as figures: hours and minutes large, their units small.
    fn duration_parts(&self, span: SignedDuration) -> Vec<StatPart> {
        duration_parts(span.as_mins(), self.language)
    }

    /// The counts on the day's and the week's date lines.
    fn stats(&self, records: &[&WorkRecord], from: Timestamp, to: Timestamp, versus: Option<SignedDuration>) -> Vec<Stat> {
        let plain = |text: String| vec![StatPart { text, small: false }];
        let finished = records.iter().filter(|record| self.finished_on(record, from, to)).count();
        let asking = records.iter().filter(|record| self.callout_by(record, to) == Some(Callout::Asking)).count();
        let worked: SignedDuration = records.iter().flat_map(|record| record.turns_between(from, to)).filter_map(|turn| turn.duration()).fold(SignedDuration::ZERO, |total, span| total + span);
        let commits: usize = records.iter().map(|record| record.commits.iter().filter(|commit| commit.at >= from && commit.at < to).count()).sum();
        let pulls: usize = records.iter().map(|record| record.pull_requests.iter().filter(|pull| pull.at >= from && pull.at < to).count()).sum();
        let names = stat_names(self.language);
        let mut stats = vec![Stat { label: names[0].into(), parts: plain(finished.to_string()), asking: false }];
        if asking > 0 {
            stats.push(Stat { label: names[1].into(), parts: plain(asking.to_string()), asking: true });
        }
        let mut label = names[2].to_owned();
        if let Some(before) = versus {
            let difference = (worked - before).as_mins();
            if difference.abs() >= 30 {
                let span = self.duration(SignedDuration::from_mins(difference.abs()));
                label = match (self.language, difference > 0) {
                    (Language::Korean, true) => format!("{label} · 지난주보다 {span} 더"),
                    (Language::Korean, false) => format!("{label} · 지난주보다 {span} 덜"),
                    (Language::English, true) => format!("{label} · {span} more than last week"),
                    (Language::English, false) => format!("{label} · {span} less than last week"),
                };
            }
        }
        stats.push(Stat { label, parts: self.duration_parts(worked), asking: false });
        if commits + pulls > 0 {
            stats.push(Stat { label: names[3].into(), parts: plain(format!("{commits} · {pulls}")), asking: false });
        }
        stats
    }

    /// The day's summary as it stands, or what stands in for it.
    fn draft(&self, date: Date, records: &[&WorkRecord], from: Timestamp, to: Timestamp) -> Draft {
        let summaries = self.summaries;
        let today = date == self.today();
        let making = summaries.making == Some(date);
        if let Some(summary) = summaries.days.get(&date) {
            let pick = |korean: &'static str, english: &'static str| self.language.pick(korean, english);
            let made = self.clock(summary.made_at);
            let chip = match (summary.edited, today) {
                (true, _) => format!("{} · {made}", pick("고침", "Edited")),
                (false, true) => format!("{} · {made}", pick("초안", "Draft")),
                (false, false) => format!("{made} {}", pick("정리", "")).trim().to_owned(),
            };
            let stale = if making {
                Some((pick("다시 정리하는 중…", "Summing up again…").to_owned(), false))
            } else if summary.basis != self.day_basis(date) {
                Some((pick("끝난 작업이 더 있어요", "More work has ended since").to_owned(), true))
            } else {
                None
            };
            let by = match (summary.by, self.language) {
                (Provider::Codex, Language::Korean) => "Codex 요약",
                (Provider::Claude, Language::Korean) => "Claude 요약",
                (Provider::Codex, Language::English) => "Codex summary",
                (Provider::Claude, Language::English) => "Claude summary",
            };
            return Draft::Ready(Box::new(DraftCard {
                date,
                title: if today { pick("오늘 정리", "Today's summary") } else { pick("이날 정리", "That day's summary") }.to_owned(),
                chip,
                note: match self.language {
                    Language::Korean => format!("작업 {}개 기준 · 고친 내용은 주간 정리와 보고서에 반영돼요.", records.len()),
                    Language::English => format!("From {} tasks · edits carry into the week and the reports.", records.len()),
                },
                done: summary.done.iter().map(|item| DraftLine { key: item.key.clone(), project: item.project.clone(), text: item.text.clone(), mark: self.summary_marks(date, item.key.as_deref()) }).collect(),
                left: summary.left.iter().map(|item| LeftLine { key: item.key.clone(), title: item.title.clone(), text: item.text.clone(), open: item.open }).collect(),
                stale,
                footer: format!("{by} · {}", pick("이 Mac에만 저장", "Kept on this Mac")),
                report: self.daily_report(summary),
            }));
        }
        let finished = records.iter().filter(|record| self.finished_on(record, from, to)).count();
        if making {
            return Draft::Making(self.language.pick("정리하는 중…", "Summing up…").into());
        }
        if summaries.unreadable {
            return Draft::Failed(self.language.pick("저장된 요약을 읽지 못했어요. 폴더 권한을 확인해 주세요.", "Could not read saved summaries. Check folder permissions.").into(), None);
        }
        if let Some(why) = summaries.failed.get(&date) {
            let why = match why.split_once(" / ") {
                Some((korean, english)) => if self.language == Language::Korean { korean } else { english },
                None => why.as_str(),
            };
            return Draft::Failed(why.to_owned(), Some(self.language.pick("다시 시도", "Try again").into()));
        }
        if finished == 0 {
            let note = if today {
                self.language.pick("아직 끝낸 작업이 없어요.", "Nothing finished yet.")
            } else {
                self.language.pick("이날은 끝낸 작업이 없어요.", "Nothing was finished that day.")
            };
            return Draft::Nothing(note.into());
        }
        let note = match (self.language, today && summaries.auto) {
            (Language::Korean, true) => format!("끝낸 작업 {finished}개 · 저녁 6시에 자동 정리"),
            (Language::Korean, false) => format!("끝낸 작업 {finished}개를 아직 정리하지 않았어요."),
            (Language::English, true) => format!("{finished} done · summed up at 6 pm"),
            (Language::English, false) => format!("{finished} done, not summed up yet."),
        };
        Draft::Offer(note, self.language.pick("지금 정리하기", "Sum up now").into())
    }

    /// The same date a month before, when there was work.
    fn back_then(&self, date: Date) -> Option<BackThen> {
        let then = date.checked_sub(1.month()).ok()?;
        let records = self.day_records_on(then);
        let first = records.first()?;
        let text = match self.summaries.days.get(&then).and_then(|summary| summary.done.first()) {
            Some(line) => line.text.clone(),
            None if records.len() > 1 => match self.language {
                Language::Korean => format!("{} 외 {}개", first.title, records.len() - 1),
                Language::English => format!("{} and {} more", first.title, records.len() - 1),
            },
            None => first.title.clone(),
        };
        Some(BackThen {
            label: format!("{} · {}", self.language.pick("지난달 오늘", "A month ago"), self.month_day(then)),
            text,
            days: then.until(date).ok()?.get_days() as i64,
        })
    }

    fn day_records_on(&self, date: Date) -> Vec<&WorkRecord> {
        let (from, to) = (self.start_of(date), self.start_of(date.tomorrow().unwrap_or(date)));
        let mut records = self.within(from, to);
        records.sort_by_key(|record| record.turns_between(from, to).next().map(|turn| turn.asked_at));
        records
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
        let mut shown: Vec<&WorkRecord> = records.clone();
        shown.sort_by_key(|record| std::cmp::Reverse(record.turns_between(from, to).last().map(|turn| turn.asked_at)));
        DayView {
            date: day,
            masthead: Masthead {
                big: day.day().to_string(),
                top: self.weekday_long(day.weekday()),
                bottom: match self.language {
                    Language::Korean => format!("{}년 {}월", day.year(), day.month()),
                    Language::English => format!("{} {}", self.month_name(day.month()), day.year()),
                },
                pills: Vec::new(),
                stats: self.stats(&records, from, to, None),
                can_go_forward: place.back > 0,
            },
            axis,
            bars,
            lanes,
            draft: self.draft(day, &records, from, to),
            tasks: shown.into_iter().map(|record| self.task_row(record, from, to)).collect(),
            back_then: self.back_then(day),
        }
    }

    fn monday(&self, back: i64) -> Date {
        let today = self.today();
        let monday = today.checked_sub((today.weekday().to_monday_zero_offset() as i64).days()).unwrap_or(today);
        monday.checked_sub((back * 7).days()).unwrap_or(monday)
    }

    /// A week's bounds and its records.
    fn week_records(&self, monday: Date) -> (Timestamp, Timestamp, Vec<&WorkRecord>) {
        let sunday = monday.checked_add(6.days()).unwrap_or(monday);
        let (from, to) = (self.start_of(monday), self.start_of(sunday.tomorrow().unwrap_or(sunday)));
        (from, to, self.within(from, to))
    }

    fn worked(&self, records: &[&WorkRecord], from: Timestamp, to: Timestamp) -> SignedDuration {
        records.iter().flat_map(|record| record.turns_between(from, to)).filter_map(|turn| turn.duration()).fold(SignedDuration::ZERO, |total, span| total + span)
    }

    /// The week's recap is ready from Friday 18:00, and for any week before this one.
    fn recap_ready(&self, monday: Date) -> bool {
        let friday = monday.checked_add(4.days()).unwrap_or(monday);
        let evening = friday.at(super::summary::EVENING, 0, 0, 0).to_zoned(self.zone.clone()).map(|zoned| zoned.timestamp()).unwrap_or(self.now);
        self.now >= evening
    }

    /// "11월 둘째 주", or "Week 2 of November".
    fn week_name(&self, monday: Date) -> String {
        let ordinal = (monday.day() as usize - 1) / 7;
        match self.language {
            Language::Korean => format!("{} {} 주", self.month_name(monday.month()), ["첫째", "둘째", "셋째", "넷째", "다섯째"][ordinal.min(4)]),
            Language::English => format!("Week {} of {}", ordinal + 1, self.month_name(monday.month())),
        }
    }

    pub fn week(&self, place: &JournalPlace) -> WeekView {
        let monday = self.monday(place.back);
        let sunday = monday.checked_add(6.days()).unwrap_or(monday);
        let (from, to, records) = self.week_records(monday);
        let last_monday = monday.checked_sub(7.days()).unwrap_or(monday);
        let (last_from, last_to, last_records) = self.week_records(last_monday);
        let before = self.worked(&last_records, last_from, last_to);
        let pick = |korean: &'static str, english: &'static str| self.language.pick(korean, english);
        let mut days = Vec::new();
        let mut summaries = Vec::new();
        for offset in 0..7 {
            let date = monday.checked_add(offset.days()).unwrap_or(monday);
            let (day_from, day_to) = (self.start_of(date), self.start_of(date.tomorrow().unwrap_or(date)));
            let mut list: Vec<&WorkRecord> = records.iter().copied().filter(|record| record.turns_between(day_from, day_to).next().is_some()).collect();
            if list.is_empty() {
                continue;
            }
            list.sort_by_key(|record| record.turns_between(day_from, day_to).next().map(|turn| turn.asked_at));
            let place_of = |at: Timestamp| (day_from.duration_until(at).as_secs_f64() / 3600.0 - 8.0) / 11.0;
            let strip = list
                .iter()
                .filter_map(|record| {
                    let turns: Vec<_> = record.turns_between(day_from, day_to).collect();
                    let begin = turns.first()?.asked_at;
                    let end = turns.iter().filter_map(|turn| turn.finished_at).max().unwrap_or(begin);
                    Some((place_of(begin).clamp(0.0, 1.0), place_of(end).clamp(0.0, 1.0).max(place_of(begin).clamp(0.0, 1.0) + 0.012), record.provider))
                })
                .collect();
            let today = date == self.today();
            let summary = self.summaries.days.get(&date);
            let (mut lines, note): (Vec<DraftLine>, Option<String>) = match summary {
                Some(summary) => {
                    summaries.push(summary);
                    let lines = summary.done.iter().map(|item| DraftLine { key: item.key.clone(), project: item.project.clone(), text: item.text.clone(), mark: self.summary_marks(date, item.key.as_deref()) }).collect();
                    let note = if summary.edited { Some(pick("고침", "Edited").to_owned()) } else if today { Some(pick("오늘 초안", "Today's draft").to_owned()) } else { None };
                    (lines, note)
                }
                None => {
                    let lines = list
                        .iter()
                        .map(|record| {
                            let commits = record.commits.iter().filter(|commit| commit.at >= day_from && commit.at < day_to).count();
                            let pulls: Vec<u32> = record.pull_requests.iter().filter(|pull| pull.at >= day_from && pull.at < day_to).map(|pull| pull.number).collect();
                            DraftLine { key: Some(record.key.clone()), project: record.project().to_owned(), text: record.title.clone(), mark: self.marks(commits, &pulls) }
                        })
                        .collect();
                    (lines, Some(pick("정리 전", "Not summed up").to_owned()))
                }
            };
            let more = lines.len().saturating_sub(4);
            lines.truncate(4);
            days.push(WeekDay { date, day: self.weekday(date.weekday()).to_owned(), label: self.month_day(date), lines, more, note, strip, today });
        }
        // What is left is what the latest summed-up day says is left.
        let carried: Vec<LeftLine> = summaries
            .iter()
            .max_by_key(|summary| summary.date)
            .map(|summary| summary.left.iter().map(|item| LeftLine { key: item.key.clone(), title: item.title.clone(), text: item.text.clone(), open: item.open }).collect())
            .unwrap_or_default();
        let mut kind_counts: BTreeMap<super::summary::Kind, usize> = BTreeMap::new();
        for summary in &summaries {
            for item in &summary.done {
                *kind_counts.entry(item.kind).or_default() += 1;
            }
        }
        let mut kinds: Vec<(String, usize)> = kind_counts.into_iter().map(|(kind, count)| (kind.name(self.language).to_owned(), count)).collect();
        kinds.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
        let ready = self.recap_ready(monday);
        let open: HashSet<&str> = carried.iter().filter_map(|line| line.key.as_deref()).collect();
        let metro = self.metro(monday, &records, &open, place.back == 0, ready);
        let finished = records.iter().filter(|record| self.finished_on(record, from, to)).count();
        let cards = if finished > 0 { self.recap(place).cards.len() } else { 0 };
        let recap = (finished > 0).then(|| RecapEntry {
            title: match self.language {
                Language::Korean => format!("{} 회고 카드\n{cards}장", if place.back == 0 { "이번 주" } else { "이 주" }),
                Language::English => format!("This week's recap\nin {cards} cards"),
            },
            note: if ready { pick("금요일 18:00 기준", "As of Friday 18:00") } else { pick("금요일 18:00 완성 예정", "Ready Friday 18:00") }.to_owned(),
            button: if ready { pick("넘겨 보기", "Flip through") } else { pick("지금까지 보기", "See so far") }.to_owned(),
            count: finished,
        });
        let current = place.back == 0;
        WeekView {
            masthead: Masthead {
                big: format!("{}–{}", monday.day(), sunday.day()),
                top: if monday.month() == sunday.month() {
                    self.month_name(monday.month())
                } else {
                    format!("{}–{}", self.month_name(monday.month()), self.month_name(sunday.month()))
                },
                bottom: if current { pick("이번 주", "This week").to_owned() } else { monday.year().to_string() },
                pills: Vec::new(),
                stats: self.stats(&records, from, to, (!last_records.is_empty()).then_some(before)),
                can_go_forward: place.back > 0,
            },
            monday,
            title: if current { pick("이번 주 정리", "This week") } else { pick("이 주 정리", "That week") }.to_owned(),
            days,
            recap,
            carried_title: if current && !ready { pick("남은 일", "Left") } else { pick("다음 주로 넘어가는 일", "Carried to next week") }.to_owned(),
            carried,
            kinds,
            report: if summaries.is_empty() { String::new() } else { self.weekly_report(monday, &summaries) },
            metro,
        }
    }

    /// The week on screen as six cards: the cover, what got done, what kind of work, the busiest day, the
    /// longest task and what carries over. The kind card needs summaries and is left out without them.
    pub fn recap(&self, place: &JournalPlace) -> RecapView {
        let monday = self.monday(place.back);
        let sunday = monday.checked_add(6.days()).unwrap_or(monday);
        let (from, to, records) = self.week_records(monday);
        let pick = |korean: &'static str, english: &'static str| self.language.pick(korean, english);
        let this = if place.back == 0 { pick("이번 주", "This week") } else { pick("이 주", "That week") };
        let projects: std::collections::BTreeSet<&str> = records.iter().map(|record| record.project()).collect();
        let mut cards = vec![RecapCard::Cover {
            label: match self.language {
                Language::Korean => format!("{this} 회고"),
                Language::English => format!("{this} in review"),
            },
            title: self.week_name(monday),
            span: match self.language {
                Language::Korean => format!("{}일 – {}일", monday.day(), sunday.day()),
                Language::English => format!("{} – {}", self.month_day(monday), self.month_day(sunday)),
            },
            chips: vec![
                format!("{} {}", pick("작업", "Tasks"), records.len()),
                format!("{} {}", pick("프로젝트", "Projects"), projects.len()),
            ],
        }];
        let finished = records.iter().filter(|record| self.finished_on(record, from, to)).count();
        let waiting = records.iter().filter(|record| self.callout(record) == Some(Callout::Asking)).count();
        let (last_from, last_to, last_records) = self.week_records(monday.checked_sub(7.days()).unwrap_or(monday));
        let last_finished = last_records.iter().filter(|record| self.finished_on(record, last_from, last_to)).count();
        let versus = match (self.language, finished.cmp(&last_finished)) {
            (_, _) if last_records.is_empty() => String::new(),
            (Language::Korean, std::cmp::Ordering::Greater) => format!("지난주보다 {}개 많아요", finished - last_finished),
            (Language::Korean, std::cmp::Ordering::Less) => format!("지난주보다 {}개 적어요", last_finished - finished),
            (Language::Korean, std::cmp::Ordering::Equal) => "지난주와 같아요".to_owned(),
            (Language::English, std::cmp::Ordering::Greater) => format!("{} more than last week", finished - last_finished),
            (Language::English, std::cmp::Ordering::Less) => format!("{} fewer than last week", last_finished - finished),
            (Language::English, std::cmp::Ordering::Equal) => "Same as last week".to_owned(),
        };
        cards.push(RecapCard::Finished {
            label: match self.language {
                Language::Korean => format!("{this} 끝낸 일"),
                Language::English => format!("Finished {}", this.to_lowercase()),
            },
            count: finished,
            versus,
            waiting,
            note: match (self.language, waiting) {
                (_, 0) => String::new(),
                (Language::Korean, 1) => "1개는 답변 필요".to_owned(),
                (Language::Korean, n) => format!("{n}개는 답변 필요"),
                (Language::English, 1) => "One is waiting on your answer".to_owned(),
                (Language::English, n) => format!("{n} are waiting on your answer"),
            },
        });
        let mut kind_counts: BTreeMap<super::summary::Kind, usize> = BTreeMap::new();
        for offset in 0..7 {
            if let Some(summary) = monday.checked_add(offset.days()).ok().and_then(|date| self.summaries.days.get(&date)) {
                for item in &summary.done {
                    *kind_counts.entry(item.kind).or_default() += 1;
                }
            }
        }
        if !kind_counts.is_empty() {
            let total: usize = kind_counts.values().sum();
            let mut kinds: Vec<(String, usize)> = kind_counts.into_iter().map(|(kind, count)| (kind.name(self.language).to_owned(), count)).collect();
            kinds.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
            kinds.truncate(5);
            cards.push(RecapCard::Kind {
                label: pick("가장 많이 한 일", "Most of the work").to_owned(),
                top: kinds[0].0.clone(),
                of: match self.language {
                    Language::Korean => format!("{total}개 중 {}개", kinds[0].1),
                    Language::English => format!("{} of {total}", kinds[0].1),
                },
                kinds,
            });
        }
        // The busiest weekday by AI time; the weekend only when there was work on it.
        let mut per_day = Vec::new();
        for offset in 0..7 {
            let date = monday.checked_add(offset.days()).unwrap_or(monday);
            let (day_from, day_to) = (self.start_of(date), self.start_of(date.tomorrow().unwrap_or(date)));
            let list: Vec<&WorkRecord> = records.iter().copied().filter(|record| record.turns_between(day_from, day_to).next().is_some()).collect();
            per_day.push((date, self.worked(&list, day_from, day_to), list.len()));
        }
        if per_day[5].2 + per_day[6].2 == 0 {
            per_day.truncate(5);
        }
        if let Some((busiest, worked, count)) = per_day.iter().copied().max_by_key(|(_, worked, _)| *worked).filter(|(_, worked, _)| worked.is_positive()) {
            let peak = worked.as_secs_f64();
            cards.push(RecapCard::Busiest {
                label: pick("가장 바빴던 날", "The busiest day").to_owned(),
                day: self.weekday_long(busiest.weekday()),
                note: match self.language {
                    Language::Korean => format!("AI 작업 {} · 작업 {count}", self.duration(worked)),
                    Language::English => format!("{} of AI work · {count} tasks", self.duration(worked)),
                },
                days: per_day.iter().map(|(date, worked, _)| (self.weekday(date.weekday()).to_owned(), worked.as_secs_f64() / peak, *date == busiest)).collect(),
            });
        }
        if let Some((record, worked)) = records.iter().map(|record| (*record, self.worked(&[record], from, to))).max_by_key(|(_, worked)| *worked).filter(|(_, worked)| worked.is_positive()) {
            let turns: Vec<_> = record.turns_between(from, to).collect();
            let begin = turns.first().map(|turn| turn.asked_at).unwrap_or(from);
            let end = turns.iter().filter_map(|turn| turn.finished_at).max().unwrap_or(begin);
            let same_day = self.date(begin) == self.date(end);
            let day_from = self.start_of(self.date(begin));
            let place_of = |at: Timestamp| ((day_from.duration_until(at).as_secs_f64() / 3600.0 - 8.0) / 11.0).clamp(0.0, 1.0);
            cards.push(RecapCard::Longest {
                label: pick("가장 오래 맡긴 작업", "The longest task").to_owned(),
                provider: record.provider,
                project: record.project().to_owned(),
                title: record.title.clone(),
                time: self.duration_parts(worked),
                span: if same_day {
                    format!("{} {} – {}", self.weekday(self.date(begin).weekday()), self.clock(begin), self.clock(end))
                } else {
                    format!("{} – {}", self.weekday(self.date(begin).weekday()), self.weekday(self.date(end).weekday()))
                },
                bar: same_day.then(|| (place_of(begin), place_of(end).max(place_of(begin) + 0.02))),
            });
        }
        let latest = (0..7).rev().filter_map(|offset| monday.checked_add(offset.days()).ok()).find_map(|date| self.summaries.days.get(&date));
        let items: Vec<(String, String, bool)> = latest.map(|summary| summary.left.iter().map(|item| (item.title.clone(), item.text.clone(), item.open == super::summary::Open::Asking)).collect()).unwrap_or_default();
        cards.push(RecapCard::Carried {
            label: pick("다음 주로 넘어가는 일", "Carried to next week").to_owned(),
            count: items.len(),
            items,
            save: pick("이미지로 저장", "Save as image").to_owned(),
        });
        let index = place.recap.unwrap_or(0).min(cards.len() - 1);
        let count = cards.len();
        RecapView {
            position: format!("{} / {}", index + 1, cards.len()),
            cards,
            index,
            save_one: pick("이 카드 저장", "Save this card").to_owned(),
            save_all: match self.language {
                Language::Korean => format!("{}장을 한 장으로 저장", ["", "한", "두", "세", "네", "다섯", "여섯"].get(count).copied().unwrap_or("모든")),
                Language::English => "Save all as one image".to_owned(),
            },
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
                stats: Vec::new(),
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
        // Five shades by the person's own spread of busy days, so a heavy user's map is not all one colour.
        let mut busy: Vec<usize> = per_day.values().copied().filter(|count| *count > 0).collect();
        busy.sort_unstable();
        let cut = |share: f64| busy.get(((busy.len() as f64 - 1.0) * share).round() as usize).copied().unwrap_or(0);
        let steps = [cut(0.2), cut(0.4), cut(0.6), cut(0.8)];
        let level = |count: usize| if count == 0 { 0 } else { 1 + steps.iter().filter(|step| count > **step).count() as i8 };
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
                    Some(level(per_day.get(&date).copied().unwrap_or(0)))
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
                pills: [
                    (commits > 0).then(|| Pill { callout: None, icon: "git-commit-horizontal", text: format!("{} {commits}", self.language.pick("커밋", "Commits")) }),
                    (pulls > 0).then(|| Pill { callout: None, icon: "git-pull-request", text: format!("PR {pulls}") }),
                    worked.is_positive().then(|| Pill { callout: None, icon: "clock", text: format!("{} {}", self.language.pick("AI 작업", "AI work"), self.duration(worked)) }),
                ]
                .into_iter()
                .flatten()
                .collect(),
                stats: Vec::new(),
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
        // Answers in another language, and the ones of them not translated yet.
        let answers: Vec<String> = record.turns.iter().filter_map(|turn| turn.result.as_deref()).map(|result| preview(result, 220)).filter(|answer| foreign(answer, self.language)).collect();
        let waiting: Vec<String> = answers.iter().filter(|answer| !self.translations.texts.contains_key(*answer)).cloned().collect();
        let translations = self.translations;
        let translate = if answers.is_empty() {
            None
        } else if translations.working.as_deref() == Some(key) {
            Some(Translate::Working)
        } else if let Some((_, why)) = translations.failed.as_ref().filter(|(failed, _)| failed == key) {
            // The CLI's messages come in both languages, Korean first.
            let why = match (why.split_once(" / "), self.language) {
                (Some((korean, _)), Language::Korean) => korean,
                (Some((_, english)), Language::English) => english,
                (None, _) => why.as_str(),
            };
            Some(Translate::Failed(why.to_owned(), waiting.clone()))
        } else if !waiting.is_empty() {
            Some(Translate::Offer(waiting.clone()))
        } else if translations.original {
            Some(Translate::Original)
        } else {
            Some(Translate::Shown)
        };
        let shown = |answer: String| match translations.texts.get(&answer) {
            Some(translated) if !translations.original => preview(translated, 220),
            _ => answer,
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
                    result: turn.result.as_deref().map(|result| shown(preview(result, 220))).unwrap_or_else(|| {
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
            pills: [(("요청", "Requests"), record.turns.len()), (("커밋", "Commits"), record.commits.len()), (("파일", "Files"), record.files.len())]
                .into_iter()
                .filter(|(_, n)| *n > 0)
                .map(|(word, n)| count(word, n))
                .collect(),
            turns,
            translate,
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

/// What the page draws under the date line: the map of the week on screen, or of the week that holds the day on
/// screen, or each week of the month, and what goes with it.
#[derive(Clone, Debug, PartialEq)]
pub struct MapScreen {
    pub weeks: Vec<super::map::MapView>,
    /// The day the map is opened on, from its Monday.
    pub day: Option<usize>,
    /// Each day's summary as it stands, for the days with work, by the day's place from Monday.
    pub drafts: Vec<(usize, Draft)>,
    pub year: super::map::YearView,
    /// The heading over a month's weeks: "11월 정리". Empty on a day or a week.
    pub title: String,
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
    pub edit: &'static str,
    pub copy_report: &'static str,
    pub redo: &'static str,
    pub left: &'static str,
    pub tasks_today: &'static str,
    pub tasks_that_day: &'static str,
    pub waiting: &'static str,
    pub copy_week: &'static str,
    pub week_summary: &'static str,
    pub week_note: &'static str,
    pub carried: &'static str,
    pub kinds: &'static str,
    pub no_summary: &'static str,
    pub tasks: &'static str,
    pub peak: &'static str,
    pub whole_week: &'static str,
    pub nothing_that_day: &'static str,
    pub replay: &'static str,
    pub stop: &'static str,
    pub selected: &'static str,
    pub nothing_selected: &'static str,
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
            edit: pick("고치기", "Edit"),
            copy_report: pick("보고서로 복사", "Copy as report"),
            redo: pick("다시 정리", "Sum up again"),
            left: pick("남은 일", "Left"),
            tasks_today: pick("오늘 맡긴 일", "Today's tasks"),
            tasks_that_day: pick("이날 맡긴 일", "Tasks that day"),
            waiting: pick("답변 대기 시간", "Waiting for an answer"),
            copy_week: pick("주간 보고로 복사", "Copy weekly report"),
            week_summary: pick("이번 주 정리", "This week"),
            week_note: pick("날짜별 정리예요. 줄을 누르면 그 작업이 열려요.", "Each day's summary in order. Press a line to open its task."),
            carried: pick("다음 주로 넘어가는 일", "Carried to next week"),
            kinds: pick("일의 종류", "Kinds of work"),
            no_summary: pick("정리 전", "Not summed up"),
            tasks: pick("작업", "Tasks"),
            peak: pick("가장 많이 맡긴 때", "Busiest hour"),
            whole_week: pick("한 주", "Week"),
            nothing_that_day: pick("이날은 맡긴 일이 없어요.", "Nothing was asked that day."),
            replay: pick("다시 보기", "Replay"),
            stop: pick("멈춤", "Stop"),
            selected: pick("선택한 구간", "Selection"),
            nothing_selected: pick("이 구간에는 맡긴 일이 없어요.", "Nothing was asked in this stretch."),
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
    /// The map and its cards, for whichever view is on screen.
    pub map: MapScreen,
    pub detail: Option<DetailView>,
    /// The week's recap cards, while they are open.
    pub recap: Option<RecapView>,
    pub log: String,
    /// No records have been read yet.
    pub loading: bool,
    pub words: Words,
    /// The language every word above is in.
    pub language: Language,
    /// The person's character, as the widget draws it.
    pub avatar: String,
}

impl Journal<'_> {
    fn map_screen(&self, place: &JournalPlace) -> MapScreen {
        let today = self.today();
        let (weeks, day) = match place.tab {
            JournalTab::Day => {
                let date = today.checked_sub(place.back.days()).unwrap_or(today);
                let offset = date.weekday().to_monday_zero_offset() as i64;
                (vec![self.map(date.checked_sub(offset.days()).unwrap_or(date))], Some(offset as usize))
            }
            JournalTab::Week => (vec![self.map(self.monday(place.back))], None),
            JournalTab::Month | JournalTab::Report => (self.month_maps(place.back), None),
        };
        let drafts = match (place.tab, weeks.first()) {
            (JournalTab::Day | JournalTab::Week, Some(map)) => map
                .days
                .iter()
                .enumerate()
                .filter(|(index, _)| match day {
                    Some(day) => day == *index,
                    None => map.tasks.iter().any(|task| task.day == *index),
                })
                .map(|(index, head)| {
                    let (from, to) = (self.start_of(head.date), self.start_of(head.date.tomorrow().unwrap_or(head.date)));
                    (index, self.draft(head.date, &self.within(from, to), from, to))
                })
                .collect(),
            _ => Vec::new(),
        };
        let title = match (place.tab, weeks.first()) {
            (JournalTab::Month | JournalTab::Report, Some(first)) => {
                let month = self.month_name(first.days[3].date.month());
                match self.language {
                    Language::Korean => format!("{month} 정리"),
                    Language::English => format!("{month} summary"),
                }
            }
            _ => String::new(),
        };
        MapScreen { weeks, day, drafts, year: self.year(), title }
    }

    pub fn screen(&self, place: &JournalPlace, usage: Vec<MiniBattery>, loading: bool, avatar: String) -> Screen {
        let body = match place.tab {
            JournalTab::Day => Body::Day(self.day(place)),
            JournalTab::Week => Body::Week(self.week(place)),
            JournalTab::Month => Body::Month(self.month(place)),
            JournalTab::Report => Body::Report(self.report(place)),
        };
        Screen {
            place: place.clone(),
            tabs: JournalTab::SHOWN.iter().map(|tab| (*tab, tab.name(self.language))).collect(),
            spans: ReportSpan::ALL.iter().map(|span| (*span, span.name(self.language))).collect(),
            usage,
            body,
            map: self.map_screen(place),
            detail: place.open.as_deref().and_then(|key| self.detail(key)),
            recap: (place.tab == JournalTab::Week && place.recap.is_some()).then(|| self.recap(place)),
            log: self.log(place),
            loading,
            words: Words::new(self.language),
            language: self.language,
            avatar,
        }
    }
}
