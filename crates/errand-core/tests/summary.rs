//! Day summaries: what the AI is asked, what of its answer is kept, how they are stored, edited and reported.

use errand_core::journal::summary::{DaySummary, Kind, Open, Summaries};
use errand_core::journal::view::{Journal, Live};
use errand_core::journal::{Commit, PullRequest, WorkRecord, WorkTurn};
use errand_core::{Language, Provider};
use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;
use serde_json::json;
use std::collections::HashMap;

fn at(text: &str) -> Timestamp {
    text.parse().unwrap()
}

fn turn(asked: &str, finished: Option<&str>, ask: &str, result: &str) -> WorkTurn {
    WorkTurn { asked_at: at(asked), ask: ask.into(), finished_at: finished.map(at), result: Some(result.into()).filter(|r: &String| !r.is_empty()), aborted: false }
}

fn record(key: &str, provider: Provider, folder: &str, title: &str, turns: Vec<WorkTurn>) -> WorkRecord {
    WorkRecord { key: key.into(), provider, title: title.into(), folder: folder.into(), branch: None, url: None, turns, commits: vec![], pull_requests: vec![], files: vec![] }
}

/// Thursday the 12th and Friday the 13th of November 2026, in Seoul.
fn sample() -> Vec<WorkRecord> {
    let mut login = record("claude:a", Provider::Claude, "/code/shop-web", "로그인 화면 리팩터링", vec![turn("2026-11-13T00:12:00Z", Some("2026-11-13T00:40:00Z"), "훅으로 바꿔줘", "폼 상태를 옮겼어요. 푸시는 하지 않았어요.")]);
    login.commits = vec![Commit { sha: "a1f3c2e".into(), message: "분리".into(), at: at("2026-11-13T00:39:00Z") }, Commit { sha: "7be09d4".into(), message: "문구".into(), at: at("2026-11-13T00:40:00Z") }];
    let mut checks = record("codex:b", Provider::Codex, "/code/payments-api", "입력값 검증 추가", vec![turn("2026-11-13T01:05:00Z", Some("2026-11-13T01:31:00Z"), "금액 검증 붙여줘", "검증을 더했어요.")]);
    checks.pull_requests = vec![PullRequest { number: 48, url: "https://github.com/me/p/pull/48".into(), at: at("2026-11-13T01:30:00Z") }];
    let asking = record("codex:c", Provider::Codex, "/code/payments-api", "권한 확인 리팩터링", vec![turn("2026-11-13T06:40:00Z", None, "권한 확인을 모아줘", "")]);
    let thursday = record("claude:d", Provider::Claude, "/code/shop-web", "대시보드 차트 추가", vec![turn("2026-11-12T07:30:00Z", Some("2026-11-12T08:24:00Z"), "차트 넣어줘", "차트를 더했어요.")]);
    vec![thursday, login, checks, asking]
}

fn journal<'a>(records: &'a [WorkRecord], live: &'a HashMap<String, Live>, summaries: &'a Summaries, zone: &'a TimeZone, now: &str) -> Journal<'a> {
    Journal { records, live, translations: Box::leak(Box::default()), summaries, now: at(now), zone, language: Language::Korean }
}

fn friday() -> Date {
    "2026-11-13".parse().unwrap()
}

#[test]
fn automatic_requests_wait_for_a_saved_onboarding_choice() {
    use errand_core::{model::{AppState, Effect, Msg, WidgetPreferences, update}, reset_history::ResetHistory, sessions::SessionRecords, tasks};
    let zone = errand_core::format::local_zone();
    let today = Timestamp::now().to_zoned(zone.clone()).date();
    let now = today.at(9, 0, 0, 0).to_zoned(zone.clone()).unwrap().timestamp();
    let yesterday = today.yesterday().unwrap().at(12, 0, 0, 0).to_zoned(zone).unwrap().timestamp();
    let work = record("claude:setup", Provider::Claude, "/code/shop-web", "차트", vec![WorkTurn {
        asked_at: yesterday, ask: "차트를 넣어줘".into(), finished_at: Some(yesterday + jiff::SignedDuration::from_mins(10)),
        result: Some("차트를 추가했어요.".into()), aborted: false,
    }]);
    let mut state = AppState::new(ResetHistory::new(), SessionRecords::new(now), WidgetPreferences::default(), "한결".into());
    state.journal.language = Some(Language::Korean);
    state.journal.open = true;
    assert!(!state.journal.summaries.auto);
    assert!(!update(&mut state, Msg::JournalScanned(vec![work.clone()]), now).iter().any(|effect| matches!(effect, Effect::Summarize { .. })));

    // A legacy saved preference cannot start requests before the new setup screen is dealt with.
    state.tasks.settings.summaries = true;
    update(&mut state, Msg::Tasks(tasks::Msg::BeginOnboarding), now);
    assert!(!update(&mut state, Msg::JournalScanned(vec![work.clone()]), now).iter().any(|effect| matches!(effect, Effect::Summarize { .. })));
    let choice = tasks::AiSettings { summaries: true, onboarding_complete: true, ..state.tasks.settings.clone() };
    update(&mut state, Msg::Tasks(tasks::Msg::SettingsSaved { settings: choice.clone(), result: Err("disk full".into()) }), now);
    assert!(!state.journal.summaries.auto);
    update(&mut state, Msg::Tasks(tasks::Msg::SettingsSaved { settings: choice, result: Ok(()) }), now);
    assert!(update(&mut state, Msg::JournalScanned(vec![work]), now).iter().any(|effect| matches!(effect, Effect::Summarize { .. })));
}

fn answer() -> serde_json::Value {
    json!({
        "done": [
            {"task": "claude:a", "text": "로그인 폼 상태를 훅으로 옮겼어요", "report": "로그인 폼 상태를 훅으로 분리", "kind": "refactor"},
            {"task": "codex:b", "text": "결제 금액 검증을 더했어요", "report": "결제 금액 검증 추가", "kind": "feature"},
            {"task": "unknown", "text": "만들어 낸 줄", "report": "없는 작업", "kind": "feature"}
        ],
        "left": [
            {"task": "codex:c", "text": "관리자 예외 규칙 답을 기다려요", "report": "관리자 예외 규칙 확인 필요", "state": "asking"},
            {"task": "claude:a", "text": "푸시하지 않았어요", "report": "로그인 변경 푸시", "state": "todo"}
        ]
    })
}

#[test]
fn a_day_is_asked_about_by_task_and_only_named_tasks_are_kept() {
    let (records, live, summaries, zone) = (sample(), HashMap::new(), Summaries::default(), TimeZone::get("Asia/Seoul").unwrap());
    let journal = journal(&records, &live, &summaries, &zone, "2026-11-13T07:10:00Z");
    let input = journal.summary_input(friday()).unwrap();
    assert_eq!(input.tasks.len(), 3, "Thursday's task is not Friday's");
    assert_eq!(input.finished(), 2);
    let asking = input.tasks.iter().find(|task| task.key == "codex:c").unwrap();
    assert_eq!(asking.state, "running", "an unanswered request this recent counts as running");
    assert_eq!(input.tasks.iter().find(|task| task.key == "claude:a").unwrap().minutes, 28);

    let summary = DaySummary::from_answer(&answer(), &input, at("2026-11-13T09:40:00Z"), Provider::Codex).unwrap();
    assert_eq!(summary.done.len(), 2, "a line about a task the request did not name is dropped");
    assert_eq!(summary.done[0].project, "shop-web", "the project comes from the record, not the answer");
    assert_eq!(summary.done[0].kind, Kind::Refactor);
    assert_eq!(summary.left[0].open, Open::Asking);
    assert_eq!(summary.left[0].title, "권한 확인 리팩터링");
    assert!(DaySummary::from_answer(&json!({"done": [], "left": []}), &input, at("2026-11-13T09:40:00Z"), Provider::Codex).is_err());
    assert!(DaySummary::from_answer(&json!({"left": []}), &input, at("2026-11-13T09:40:00Z"), Provider::Codex).is_err());
}

#[test]
fn summaries_are_stored_edited_and_reported() {
    let (records, live, summaries, zone) = (sample(), HashMap::new(), Summaries::default(), TimeZone::get("Asia/Seoul").unwrap());
    let journal = journal(&records, &live, &summaries, &zone, "2026-11-13T09:50:00Z");
    let input = journal.summary_input(friday()).unwrap();
    let summary = DaySummary::from_answer(&answer(), &input, at("2026-11-13T09:40:00Z"), Provider::Codex).unwrap();
    assert_eq!(DaySummary::from_json(&summary.to_json()), Some(summary.clone()));

    let text = summary.edit_text();
    assert!(text.starts_with("[한 일]\nshop-web · 로그인 폼 상태를 훅으로 옮겼어요\n"));
    assert_eq!(summary.with_edit(&text).done, summary.done, "an untouched edit keeps every line");
    let changed = summary.with_edit(&text.replace("결제 금액 검증을 더했어요", "결제 금액과 통화 검증을 더했어요").replace("[남은 일]\n", "[남은 일]\n- 새 일 · 문서 고치기\n"));
    assert!(changed.edited);
    assert_eq!(changed.done[1].text, "결제 금액과 통화 검증을 더했어요");
    assert_eq!(changed.done[1].report, "결제 금액과 통화 검증을 더했어요", "changed words become the report line too");
    assert_eq!(changed.done[1].key.as_deref(), Some("codex:b"));
    assert_eq!((changed.left[0].title.as_str(), changed.left[0].open, changed.left[0].key.as_deref()), ("새 일", Open::Todo, None), "a new line is new");
    assert_eq!(changed.left[1].open, Open::Asking, "the lines below keep their own state");

    let report = journal.daily_report(&summary);
    assert_eq!(
        report,
        "11월 13일 (금) 업무 보고\n\n[한 일]\n- shop-web: 로그인 폼 상태를 훅으로 분리 (커밋 2)\n- payments-api: 결제 금액 검증 추가 (PR #48)\n\n[진행 중]\n- 권한 확인 리팩터링: 관리자 예외 규칙 확인 필요\n\n[다음에 할 일]\n- 로그인 화면 리팩터링: 로그인 변경 푸시\n"
    );
}

#[test]
fn finished_days_are_summed_up_once_and_again_when_more_work_ends() {
    let (records, live, zone) = (sample(), HashMap::new(), TimeZone::get("Asia/Seoul").unwrap());
    let mut summaries = Summaries { auto: true, ..Summaries::default() };
    // Friday 16:10 in Seoul: Thursday is due; today waits for the evening.
    let thursday: Date = "2026-11-12".parse().unwrap();
    assert_eq!(journal(&records, &live, &summaries, &zone, "2026-11-13T07:10:00Z").next_to_summarize(&summaries), Some(thursday));
    // Friday 19:00: today first.
    assert_eq!(journal(&records, &live, &summaries, &zone, "2026-11-13T10:00:00Z").next_to_summarize(&summaries), Some(friday()));

    let made = |summaries: &Summaries, date: Date, now: &str| {
        let journal = journal(&records, &live, summaries, &zone, now);
        let input = journal.summary_input(date).unwrap();
        let answer = json!({"done": input.tasks.iter().filter(|task| task.state == "done").map(|task| json!({"task": task.key, "text": "했어요", "report": "함", "kind": "fix"})).collect::<Vec<_>>(), "left": []});
        DaySummary::from_answer(&answer, &input, at(now), Provider::Claude).unwrap()
    };
    let summary = made(&summaries, thursday, "2026-11-13T07:10:00Z");
    summaries.days.insert(thursday, summary);
    assert_eq!(journal(&records, &live, &summaries, &zone, "2026-11-13T07:10:00Z").next_to_summarize(&summaries), None, "nothing else is due before the evening");

    // More of Thursday's work ending makes its summary out of date, unless the person edited it.
    let mut later = records.clone();
    later[0].turns.push(turn("2026-11-12T09:00:00Z", Some("2026-11-12T09:20:00Z"), "범례도", "범례를 더했어요."));
    assert_eq!(journal(&later, &live, &summaries, &zone, "2026-11-13T07:10:00Z").next_to_summarize(&summaries), Some(thursday));
    summaries.days.get_mut(&thursday).unwrap().edited = true;
    assert_eq!(journal(&later, &live, &summaries, &zone, "2026-11-13T07:10:00Z").next_to_summarize(&summaries), None);

    // Off, or already making one, or the last try failed for that day: nothing on its own.
    let off = Summaries { auto: false, ..Summaries::default() };
    assert_eq!(journal(&records, &live, &off, &zone, "2026-11-13T07:10:00Z").next_to_summarize(&off), None);
    let failed = Summaries { auto: true, failed: [(thursday, "x".to_owned())].into_iter().collect(), ..Summaries::default() };
    assert_eq!(journal(&records, &live, &failed, &zone, "2026-11-13T07:10:00Z").next_to_summarize(&failed), None);
}

mod flow {
    use super::*;
    use errand_core::model::{AppState, Effect, Msg, WidgetPreferences, update};
    use errand_core::reset_history::ResetHistory;
    use errand_core::sessions::SessionRecords;

    fn state(now: Timestamp) -> AppState {
        let mut state = AppState::new(ResetHistory::new(), SessionRecords::new(now), WidgetPreferences::default(), "한결".to_owned());
        state.journal.open = true;
        state.journal.language = Some(Language::Korean);
        state.journal.summaries.auto = true;
        state
    }

    #[test]
    fn the_window_sums_up_finished_days_one_at_a_time_and_keeps_them() {
        // The day before today in whatever zone the tests run in, so the rule's own clock agrees.
        let now = Timestamp::now();
        let zone = errand_core::format::local_zone();
        let today = now.to_zoned(zone.clone()).date();
        let yesterday = today.yesterday().unwrap();
        let noon = yesterday.at(12, 0, 0, 0).to_zoned(zone).unwrap().timestamp();
        let finished = |key: &str| record(key, Provider::Claude, "/code/shop-web", "차트 추가", vec![WorkTurn { asked_at: noon, ask: "차트 넣어줘".into(), finished_at: Some(noon + jiff::SignedDuration::from_mins(20)), result: Some("차트를 더했어요.".into()), aborted: false }]);
        let mut state = state(now);
        let effects = update(&mut state, Msg::JournalScanned(vec![finished("claude:x")]), now);
        let input = match effects.as_slice() {
            [Effect::Summarize { input, .. }] => input.clone(),
            other => panic!("expected one summary request, got {other:?}"),
        };
        assert_eq!(input.date, yesterday);
        assert_eq!(state.journal.summaries.making, Some(yesterday));
        assert!(update(&mut state, Msg::JournalSummarize(yesterday), now).is_empty(), "one at a time");

        let summary = DaySummary::from_answer(&json!({"done": [{"task": "claude:x", "text": "차트를 더했어요", "report": "차트 추가", "kind": "feature"}], "left": []}), &input, now, Provider::Codex).unwrap();
        let effects = update(&mut state, Msg::JournalSummarized { date: yesterday, result: Ok(summary.clone()) }, now);
        assert_eq!(effects, [Effect::SaveSummary(summary.clone())]);
        assert_eq!(state.journal.summaries.days.get(&yesterday), Some(&summary));
        assert_eq!(state.journal.summaries.making, None);

        // An edit is kept as the person wrote it.
        let effects = update(&mut state, Msg::JournalSummaryEdited { date: yesterday, text: "[한 일]\nshop-web · 차트와 범례를 더했어요\n".into() }, now);
        assert!(matches!(effects.as_slice(), [Effect::SaveSummary(edited)] if edited.edited && edited.done[0].text == "차트와 범례를 더했어요"));

        // A failed day is not asked about again on its own; the person can try again.
        let mut state = super::flow::state(now);
        update(&mut state, Msg::JournalScanned(vec![finished("claude:y")]), now);
        update(&mut state, Msg::JournalSummarized { date: yesterday, result: Err("x / y".into()) }, now);
        assert!(update(&mut state, Msg::JournalScanned(vec![finished("claude:y")]), now).is_empty());
        assert!(matches!(update(&mut state, Msg::JournalSummarize(yesterday), now).as_slice(), [Effect::Summarize { .. }]));

        // Turned off in the settings, nothing is summed up on its own.
        let mut state = super::flow::state(now);
        state.journal.summaries.auto = false;
        assert!(update(&mut state, Msg::JournalScanned(vec![finished("claude:z")]), now).is_empty());
    }
}

mod morning {
    use super::*;
    use errand_core::model::{AppState, Effect, Msg, WidgetPreferences, update};
    use errand_core::reset_history::ResetHistory;
    use errand_core::sessions::SessionRecords;
    use errand_core::view::{ViewContext, nudge};

    #[test]
    fn the_pet_tells_about_the_last_day_once_each_morning() {
        let zone = errand_core::format::local_zone();
        let today = Timestamp::now().to_zoned(zone.clone()).date();
        let at = |date: Date, hour: i8| date.at(hour, 0, 0, 0).to_zoned(zone.clone()).unwrap().timestamp();
        let yesterday = today.yesterday().unwrap();
        let finished = record("claude:x", Provider::Claude, "/code/shop-web", "차트 추가", vec![WorkTurn { asked_at: at(yesterday, 14), ask: "차트 넣어줘".into(), finished_at: Some(at(yesterday, 15)), result: Some("차트를 더했어요.".into()), aborted: false }]);
        let mut state = AppState::new(ResetHistory::new(), SessionRecords::new(at(today, 7)), WidgetPreferences::default(), "한결".to_owned());
        state.journal.language = Some(Language::Korean);
        state.journal.summaries.auto = true;

        // Before the morning, nothing; from 8, the window being closed, the last week is read first.
        assert!(!update(&mut state, Msg::SessionsScanned(vec![]), at(today, 7)).iter().any(|effect| matches!(effect, Effect::ScanJournal(_))));
        let morning = at(today, 9);
        assert!(update(&mut state, Msg::SessionsScanned(vec![]), morning).iter().any(|effect| matches!(effect, Effect::ScanJournal(_))));
        // Yesterday has finished work and no summary: it is summed up before the bubble shows.
        let effects = update(&mut state, Msg::JournalScanned(vec![finished.clone()]), morning);
        let input = match effects.as_slice() {
            [Effect::Summarize { input, .. }] => input.clone(),
            other => panic!("expected a summary request, got {other:?}"),
        };
        assert_eq!(input.date, yesterday);
        assert_eq!(state.journal.nudge, None);
        let summary = DaySummary::from_answer(&json!({"done": [{"task": "claude:x", "text": "차트를 더했어요", "report": "차트 추가", "kind": "feature"}], "left": []}), &input, morning, Provider::Codex).unwrap();
        update(&mut state, Msg::JournalSummarized { date: yesterday, result: Ok(summary) }, morning);
        assert_eq!(state.journal.nudge, Some(yesterday));
        let context = ViewContext { now: morning, zone: zone.clone(), computer: "Mac", language: Language::Korean };
        let bubble = nudge(&state, &context).unwrap();
        assert_eq!((bubble.title.as_str(), bubble.note.as_str(), bubble.answer), ("어제 정리 · 끝낸 일 1개", "차트를 더했어요", None));

        // Not now: gone until tomorrow, and remembered.
        assert_eq!(update(&mut state, Msg::NudgeLater, morning), [Effect::SaveNudged(today)]);
        assert_eq!(state.journal.nudge, None);
        assert!(update(&mut state, Msg::SessionsScanned(vec![]), at(today, 11)).is_empty());

        // Opening it shows that day in the window.
        state.journal.nudged = None;
        // Records read two hours ago are read again before the bubble comes back.
        assert!(update(&mut state, Msg::SessionsScanned(vec![]), at(today, 11)).iter().any(|effect| matches!(effect, Effect::ScanJournal(_))));
        update(&mut state, Msg::JournalScanned(vec![finished]), at(today, 11));
        assert_eq!(state.journal.nudge, Some(yesterday));
        let effects = update(&mut state, Msg::NudgeSee, at(today, 11));
        assert!(effects.contains(&Effect::SaveNudged(today)) && effects.contains(&Effect::ShowDashboard));
        assert_eq!((state.journal.place.tab, state.journal.place.back), (errand_core::journal::view::JournalTab::Day, 1));
    }
}

/// What QA found in the summaries: retries, edits, past days and the bubble.
mod qa {
    use super::*;
    use errand_core::model::{AppState, Effect, Msg, WidgetPreferences, update};
    use errand_core::reset_history::ResetHistory;
    use errand_core::sessions::SessionRecords;

    fn state(now: Timestamp) -> AppState {
        let mut state = AppState::new(ResetHistory::new(), SessionRecords::new(now), WidgetPreferences::default(), "한결".to_owned());
        state.journal.open = true;
        state.journal.language = Some(Language::Korean);
        state.journal.summaries.auto = true;
        state
    }

    #[test]
    fn failed_days_are_not_tried_again_in_turn() {
        let zone = errand_core::format::local_zone();
        let now = Timestamp::now();
        let today = now.to_zoned(zone.clone()).date();
        let day = |back: i64| today.checked_sub(jiff::ToSpan::days(back)).unwrap();
        let noon = |date: Date| date.at(12, 0, 0, 0).to_zoned(zone.clone()).unwrap().timestamp();
        let finished = |key: &str, date: Date| record(key, Provider::Claude, "/code/shop-web", "차트", vec![WorkTurn { asked_at: noon(date), ask: "차트".into(), finished_at: Some(noon(date) + jiff::SignedDuration::from_mins(5)), result: Some("했어요.".into()), aborted: false }]);
        let mut state = state(now);
        let records = vec![finished("claude:a", day(1)), finished("claude:b", day(2))];
        let mut tried = Vec::new();
        let mut effects = update(&mut state, Msg::JournalScanned(records.clone()), now);
        while let Some(date) = effects.iter().find_map(|effect| match effect { Effect::Summarize { input, .. } => Some(input.date), _ => None }) {
            tried.push(date);
            assert!(tried.len() <= 2, "tried again: {tried:?}");
            effects = update(&mut state, Msg::JournalSummarized { date, result: Err("x / y".into()) }, now);
        }
        assert_eq!(tried, [day(1), day(2)]);
        assert!(!update(&mut state, Msg::JournalScanned(records), now).iter().any(|effect| matches!(effect, Effect::Summarize { .. })));
    }

    #[test]
    fn a_past_day_is_judged_by_its_own_work_and_new_requests_do_not_age_a_summary() {
        let zone = TimeZone::get("Asia/Seoul").unwrap();
        // Finished on Thursday, running again on Friday.
        let mut chart = record("claude:d", Provider::Claude, "/code/shop-web", "차트", vec![turn("2026-11-12T07:30:00Z", Some("2026-11-12T08:24:00Z"), "차트", "차트를 더했어요.")]);
        chart.turns.push(turn("2026-11-13T06:50:00Z", None, "범례도", ""));
        let records = vec![chart.clone()];
        let live = HashMap::from([("claude:d".to_owned(), Live { state: errand_core::sessions::SessionState::Running, question: None })]);
        let summaries = Summaries::default();
        let journal = journal(&records, &live, &summaries, &zone, "2026-11-13T07:10:00Z");
        let thursday: Date = "2026-11-12".parse().unwrap();
        let input = journal.summary_input(thursday).expect("Thursday's finished work counts");
        assert_eq!(input.tasks[0].state, "done");
        // A request only started does not change a day's fingerprint; its end does.
        let before = journal.day_basis(friday());
        let mut more = records.clone();
        more[0].turns.push(turn("2026-11-13T07:00:00Z", None, "하나 더", ""));
        assert_eq!(super::journal(&more, &live, &summaries, &zone, "2026-11-13T07:10:00Z").day_basis(friday()), before);
        more[0].turns.last_mut().unwrap().finished_at = Some(at("2026-11-13T07:05:00Z"));
        assert_ne!(super::journal(&more, &live, &summaries, &zone, "2026-11-13T07:10:00Z").day_basis(friday()), before);
    }

    #[test]
    fn a_deleted_or_added_line_does_not_move_tasks() {
        let (records, live, summaries, zone) = (sample(), HashMap::new(), Summaries::default(), TimeZone::get("Asia/Seoul").unwrap());
        let journal = journal(&records, &live, &summaries, &zone, "2026-11-13T09:50:00Z");
        let input = journal.summary_input(friday()).unwrap();
        let summary = DaySummary::from_answer(&answer(), &input, at("2026-11-13T09:40:00Z"), Provider::Codex).unwrap();
        let without_first = summary.edit_text().replace("shop-web · 로그인 폼 상태를 훅으로 옮겼어요\n", "");
        let edited = summary.with_edit(&without_first);
        assert_eq!(edited.done.len(), 1);
        assert_eq!((edited.done[0].key.as_deref(), edited.done[0].kind), (Some("codex:b"), Kind::Feature));
        let added = summary.edit_text().replace("[한 일]\n", "[한 일]\n문서 · 설명서를 고쳤어요\n").replace("결제 금액 검증을 더했어요", "결제 금액 검증을 넣었어요");
        let edited = summary.with_edit(&added);
        assert_eq!(edited.done.iter().map(|item| item.key.as_deref()).collect::<Vec<_>>(), [None, Some("claude:a"), Some("codex:b")]);
    }

    #[test]
    fn edits_and_deletes_win_over_a_summary_still_being_made() {
        let zone = errand_core::format::local_zone();
        let now = Timestamp::now();
        let yesterday = now.to_zoned(zone.clone()).date().yesterday().unwrap();
        let noon = yesterday.at(12, 0, 0, 0).to_zoned(zone.clone()).unwrap().timestamp();
        let finished = record("claude:x", Provider::Claude, "/code/shop-web", "차트", vec![WorkTurn { asked_at: noon, ask: "차트".into(), finished_at: Some(noon + jiff::SignedDuration::from_mins(5)), result: Some("했어요.".into()), aborted: false }]);
        let mut state = state(now);
        let input = match update(&mut state, Msg::JournalScanned(vec![finished.clone()]), now).as_slice() {
            [Effect::Summarize { input, .. }] => input.clone(),
            other => panic!("expected a summary request, got {other:?}"),
        };
        let made = DaySummary::from_answer(&json!({"done": [{"task": "claude:x", "text": "차트를 더했어요", "report": "차트 추가", "kind": "feature"}], "left": []}), &input, now, Provider::Codex).unwrap();
        // A summary already kept is edited while it is being made again: the edit stays.
        state.journal.summaries.days.insert(yesterday, made.clone());
        update(&mut state, Msg::JournalSummaryEdited { date: yesterday, text: "[한 일]\nshop-web · 고친 문장\n".into() }, now);
        assert!(update(&mut state, Msg::JournalSummarized { date: yesterday, result: Ok(made.clone()) }, now).iter().all(|effect| !matches!(effect, Effect::SaveSummary(_))));
        assert_eq!(state.journal.summaries.days[&yesterday].done[0].text, "고친 문장");
        // Deleted while one is being made: it does not come back, and the bubble goes.
        state.journal.summaries.making = Some(yesterday);
        state.journal.nudge = Some(yesterday);
        update(&mut state, Msg::JournalSummariesDeleted(Ok(())), now);
        assert_eq!(state.journal.nudge, None);
        update(&mut state, Msg::JournalSummarized { date: yesterday, result: Ok(made) }, now);
        assert!(!state.journal.summaries.days.contains_key(&yesterday) || state.journal.summaries.making.is_some());
    }

    #[test]
    fn the_morning_bubble_waits_for_an_out_of_date_summary_to_be_made_again() {
        let zone = errand_core::format::local_zone();
        let today = Timestamp::now().to_zoned(zone.clone()).date();
        let at = |date: Date, hour: i8| date.at(hour, 0, 0, 0).to_zoned(zone.clone()).unwrap().timestamp();
        let yesterday = today.yesterday().unwrap();
        let finished = record("claude:x", Provider::Claude, "/code/shop-web", "차트", vec![WorkTurn { asked_at: at(yesterday, 14), ask: "차트".into(), finished_at: Some(at(yesterday, 15)), result: Some("했어요.".into()), aborted: false }]);
        let morning = at(today, 9);
        let mut state = AppState::new(ResetHistory::new(), SessionRecords::new(morning), WidgetPreferences::default(), "한결".to_owned());
        state.journal.language = Some(Language::Korean);
        state.journal.summaries.auto = true;
        let done = errand_core::journal::summary::Done { key: Some("claude:x".into()), project: "shop-web".into(), text: "차트를 더했어요".into(), report: "차트 추가".into(), kind: Kind::Feature };
        let old = DaySummary { date: yesterday, made_at: morning, by: Provider::Codex, language: Language::Korean, done: vec![done], left: vec![], basis: "old".into(), edited: false };
        state.journal.summaries.days.insert(yesterday, old);
        state.journal.nudge = Some(yesterday);
        let effects = update(&mut state, Msg::JournalScanned(vec![finished]), morning);
        assert!(effects.iter().any(|effect| matches!(effect, Effect::Summarize { input, .. } if input.date == yesterday)), "{effects:?}");
        assert_eq!(state.journal.nudge, None, "the bubble waits for the new summary");
    }

    #[test]
    fn the_window_reads_its_full_records_after_a_shorter_read() {
        let now = Timestamp::now();
        let mut state = state(now);
        state.journal.open = false;
        state.journal.scanning = true;
        update(&mut state, Msg::ShowDashboard, now);
        let effects = update(&mut state, Msg::JournalScanned(vec![]), now);
        assert!(effects.iter().any(|effect| matches!(effect, Effect::ScanJournal(_))), "{effects:?}");
    }
}
