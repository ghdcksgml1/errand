//! The week as a map: its lines and their colours, tracks for overlapping work, how each piece of work stands,
//! and the cards and the year strip drawn from the same tasks.

use errand_core::journal::map::MapState;
use errand_core::journal::summary::{DaySummary, Done, Kind, Left, Open, Summaries};
use errand_core::journal::view::{Journal, JournalPlace, JournalTab, Live};
use errand_core::journal::{PullRequest, WorkRecord, WorkTurn};
use errand_core::sessions::SessionState;
use errand_core::{Language, Provider};
use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;
use std::collections::HashMap;

fn at(text: &str) -> Timestamp {
    text.parse().unwrap()
}

fn turn(asked: &str, finished: Option<&str>) -> WorkTurn {
    WorkTurn { asked_at: at(asked), ask: "해줘".into(), finished_at: finished.map(at), result: finished.map(|_| "했어요.".to_owned()), aborted: false }
}

fn record(key: &str, provider: Provider, folder: &str, title: &str, turns: Vec<WorkTurn>) -> WorkRecord {
    WorkRecord { key: key.into(), provider, title: title.into(), folder: folder.into(), branch: None, url: None, turns, commits: vec![], pull_requests: vec![], files: vec![] }
}

/// Friday the 13th of November 2026 in Seoul, with one task from the week before.
fn sample() -> Vec<WorkRecord> {
    let old = record("codex:o", Provider::Codex, "/code/payments-api", "정산 배치 재시도", vec![turn("2026-11-02T04:00:00Z", Some("2026-11-02T04:44:00Z"))]);
    let login = record("claude:a", Provider::Claude, "/code/shop-web", "로그인 화면 리팩터링", vec![turn("2026-11-13T00:12:00Z", Some("2026-11-13T00:25:00Z")), turn("2026-11-13T00:30:00Z", Some("2026-11-13T00:40:00Z"))]);
    let tests = record("claude:t", Provider::Claude, "/code/shop-web", "로그인 테스트 고치기", vec![turn("2026-11-13T00:20:00Z", Some("2026-11-13T00:50:00Z"))]);
    let mut checks = record("codex:b", Provider::Codex, "/code/payments-api", "입력값 검증 추가", vec![turn("2026-11-13T01:05:00Z", Some("2026-11-13T01:31:00Z"))]);
    checks.pull_requests = vec![PullRequest { number: 48, url: "https://github.com/me/p/pull/48".into(), at: at("2026-11-13T01:30:00Z") }];
    let deploy = record("claude:r", Provider::Claude, "/code/shop-web", "배포 설정 정리", vec![turn("2026-11-13T06:40:00Z", None)]);
    vec![old, login, tests, checks, deploy]
}

fn journal<'a>(records: &'a [WorkRecord], live: &'a HashMap<String, Live>, summaries: &'a Summaries, zone: &'a TimeZone) -> Journal<'a> {
    Journal { records, live, translations: Box::leak(Box::default()), summaries, now: at("2026-11-13T07:10:00Z"), zone, language: Language::Korean }
}

fn monday() -> Date {
    "2026-11-09".parse().unwrap()
}

#[test]
fn a_week_is_a_line_per_project_and_overlapping_work_takes_a_track() {
    let (records, live, summaries, zone) = (sample(), HashMap::new(), Summaries::default(), TimeZone::get("Asia/Seoul").unwrap());
    let map = journal(&records, &live, &summaries, &zone).map(monday());
    assert_eq!(map.lanes.iter().map(|lane| lane.name.as_str()).collect::<Vec<_>>(), ["shop-web", "payments-api"], "the project worked on longest comes first");
    assert_eq!(map.lanes[0].note, "작업 3 · 53분");
    assert_eq!(map.split, None);
    let task = |key: &str| map.tasks.iter().find(|task| task.key == key).unwrap();
    let login = task("claude:a");
    assert_eq!((login.day, login.lane, login.track), (4, 0, 0));
    assert_eq!(login.work, [(552.0, 565.0), (570.0, 580.0)], "09:12–09:25 and 09:30–09:40, in minutes of the day");
    assert_eq!((login.minutes, login.state), (23, MapState::Done));
    assert_eq!(task("claude:t").track, 1, "it began while the first was still going");
    assert_eq!((task("codex:b").lane, task("codex:b").track, task("codex:b").pull), (1, 0, Some(48)));
    // An unanswered request this recent is still running, and runs to now.
    let deploy = task("claude:r");
    assert_eq!((deploy.state, deploy.end), (MapState::Running, 970.0));
    assert_eq!(deploy.why, "진행 중이에요");
    assert_eq!(map.now, Some((4, 970.0)));
    assert_eq!((map.first_hour, map.last_hour), (8, 20));
    assert_eq!(map.days[4].total, "1시간 19분");
    assert!(map.days[4].today && map.days[5].future && map.days[3].back == 1);
    assert_eq!(map.days[4].notes, ["작업 3 · 53분", "작업 1 · 26분"]);
    assert_eq!((map.carried, map.range.as_str(), map.tally.as_str()), (1, "11/9 – 15", "작업 4 · 1시간 19분"));
    assert_eq!(map.back, 0);
}

#[test]
fn a_project_keeps_its_colour_from_week_to_week() {
    let (records, live, summaries, zone) = (sample(), HashMap::new(), Summaries::default(), TimeZone::get("Asia/Seoul").unwrap());
    let journal = journal(&records, &live, &summaries, &zone);
    let this = journal.map(monday());
    let last = journal.map("2026-11-02".parse().unwrap());
    // payments-api is the older project, so it has the first colour wherever its line sits.
    assert_eq!(this.lanes.iter().map(|lane| lane.colour).collect::<Vec<_>>(), [Some(1), Some(0)]);
    assert_eq!((last.lanes[0].name.as_str(), last.lanes[0].colour, last.back), ("payments-api", Some(0), 1));
}

#[test]
fn a_week_with_one_project_splits_into_its_branches() {
    let mut records = sample();
    records.retain(|record| record.project() == "shop-web");
    records[0].branch = Some("feature/login-hook".into());
    records[1].branch = Some("fix/tests".into());
    let (live, summaries, zone) = (HashMap::new(), Summaries::default(), TimeZone::get("Asia/Seoul").unwrap());
    let map = journal(&records, &live, &summaries, &zone).map(monday());
    assert_eq!(map.split.as_deref(), Some("shop-web · 브랜치별"));
    assert_eq!(map.lanes.iter().map(|lane| lane.name.as_str()).collect::<Vec<_>>(), ["fix/tests", "feature/login-hook", "shop-web"], "work without a branch keeps the project's name");
}

#[test]
fn live_states_and_the_days_summary_say_how_work_stands() {
    let records = sample();
    let live: HashMap<String, Live> = [("codex:b".to_owned(), Live { state: SessionState::Asking, question: None })].into();
    let friday: Date = "2026-11-13".parse().unwrap();
    let mut summaries = Summaries::default();
    summaries.days.insert(
        friday,
        DaySummary {
            date: friday,
            made_at: at("2026-11-13T07:00:00Z"),
            by: Provider::Codex,
            language: Language::Korean,
            done: vec![Done { key: Some("claude:a".into()), project: "shop-web".into(), text: "로그인 폼 상태를 훅으로 옮겼어요".into(), report: "로그인 폼 상태 분리".into(), kind: Kind::Refactor }],
            left: vec![Left { key: Some("claude:a".into()), title: "로그인 화면 리팩터링".into(), text: "푸시하지 않았어요".into(), report: "푸시".into(), open: Open::Todo }],
            basis: String::new(),
            edited: false,
        },
    );
    let zone = TimeZone::get("Asia/Seoul").unwrap();
    let map = journal(&records, &live, &summaries, &zone).map(monday());
    let task = |key: &str| map.tasks.iter().find(|task| task.key == key).unwrap();
    let login = task("claude:a");
    assert_eq!((login.state, login.kind, login.line.as_str(), login.why.as_str()), (MapState::Open, Some(Kind::Refactor), "로그인 폼 상태를 훅으로 옮겼어요", "푸시하지 않았어요"));
    let checks = task("codex:b");
    assert_eq!((checks.state, checks.end, checks.waiting), (MapState::Asking, 631.0, Some(970.0)), "the wait runs from its last answer to now");
    assert_eq!(map.carried, 3);

    let kinds = map.kinds(None, Language::Korean);
    assert_eq!(kinds.iter().map(|row| (row.name.as_str(), row.tasks.len())).collect::<Vec<_>>(), [("리팩터링", 1), ("정리 전", 3)]);
    assert_eq!(map.kinds(Some(3), Language::Korean), [], "Thursday had no work");
    let clock = map.clock(None, Language::Korean);
    assert_eq!(clock.peak, "9시");
    assert_eq!(clock.halves, [("오전".to_owned(), "1시간 19분".to_owned()), ("오후".to_owned(), "30분".to_owned())]);
    assert_eq!(clock.arcs.len(), 5);
}

#[test]
fn the_year_is_its_weeks_and_the_screen_carries_the_map() {
    let (records, live, summaries, zone) = (sample(), HashMap::new(), Summaries::default(), TimeZone::get("Asia/Seoul").unwrap());
    let journal = journal(&records, &live, &summaries, &zone);
    let year = journal.year();
    assert_eq!((year.label.as_str(), year.cells.len(), year.current), ("2026", 53, 45));
    assert_eq!(year.months.first(), Some(&(0, "1월".to_owned())), "the week of 29 December holds the first of January");
    assert_eq!((year.cells[45].back, year.cells[45].level), (Some(0), 1));
    assert_eq!(year.cells[45].tip, "11월 9일 – 11월 15일 · 작업 4 · 1시간 19분");
    assert_eq!((year.cells[44].back, year.cells[44].level), (Some(1), 1));
    assert_eq!((year.cells[43].level, year.cells[46].back), (0, None));

    let screen = |tab| journal.screen(&JournalPlace { tab, ..JournalPlace::default() }, Vec::new(), false, String::new());
    let week = screen(JournalTab::Week);
    assert_eq!(week.tabs.iter().map(|tab| tab.0).collect::<Vec<_>>(), [JournalTab::Day, JournalTab::Week, JournalTab::Month]);
    assert_eq!((week.map.weeks.len(), week.map.day, week.map.drafts.len()), (1, None, 1), "one draft, for the one day with work");
    let day = screen(JournalTab::Day);
    assert_eq!((day.map.day, day.map.drafts.iter().map(|draft| draft.0).collect::<Vec<_>>()), (Some(4), vec![4]));
    let month = screen(JournalTab::Month);
    assert_eq!(month.map.weeks.iter().map(|map| map.range.as_str()).collect::<Vec<_>>(), ["11/2 – 8", "11/9 – 15", "11/16 – 22", "11/23 – 29"]);
}

#[test]
fn the_counts_for_part_of_a_map_keep_the_date_lines_shape() {
    let records = sample();
    let live: HashMap<String, Live> = [("codex:b".to_owned(), Live { state: SessionState::Asking, question: None })].into();
    let (summaries, zone) = (Summaries::default(), TimeZone::get("Asia/Seoul").unwrap());
    let journal = journal(&records, &live, &summaries, &zone);
    let map = journal.map(monday());
    let figures = |stats: &[errand_core::journal::view::Stat]| stats.iter().map(|stat| stat.parts.iter().map(|part| part.text.as_str()).collect::<String>()).collect::<Vec<_>>();
    // All of it says what the date line says: on the day to the word, on the week but for what it adds to a name.
    let screen = |tab| journal.screen(&JournalPlace { tab, ..JournalPlace::default() }, Vec::new(), false, String::new());
    let errand_core::journal::view::Body::Day(day) = screen(JournalTab::Day).body else { panic!("the day's view") };
    assert_eq!(map.stats_of(Some(4), &|_| true, Language::Korean), day.masthead.stats);
    let errand_core::journal::view::Body::Week(week) = screen(JournalTab::Week).body else { panic!("the week's view") };
    let whole = map.stats_of(None, &|_| true, Language::Korean);
    assert_eq!(figures(&whole), figures(&week.masthead.stats));
    assert_eq!(figures(&whole), ["2", "1", "1시간 19분", "0 · 1"], "two finished, one waiting for an answer, one still running");
    // Part of it keeps every count, with only what that part holds.
    let early = map.stats_of(None, &|index| map.tasks[index].end <= 590.0, Language::Korean);
    assert_eq!(early.iter().map(|stat| stat.label.as_str()).collect::<Vec<_>>(), whole.iter().map(|stat| stat.label.as_str()).collect::<Vec<_>>());
    assert_eq!(figures(&early), ["2", "0", "53분", "0 · 0"]);
    assert_eq!(figures(&map.stats_of(None, &|_| false, Language::Korean)), ["0", "0", "0분", "0 · 0"]);
    // The clock for part of it counts only that part's hours.
    let clock = errand_core::journal::map::clock_of(std::slice::from_ref(&map), None, &|_, index| map.tasks[index].end <= 590.0, Language::Korean);
    assert_eq!((clock.arcs.len(), clock.peak.as_str()), (3, "9시"));
    assert_eq!(clock.halves, [("오전".to_owned(), "53분".to_owned()), ("오후".to_owned(), "0분".to_owned())], "a half without work says so");
}
