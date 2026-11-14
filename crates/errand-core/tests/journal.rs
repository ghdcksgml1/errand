//! Reading whole transcripts into work records.

use errand_core::Provider;
use errand_core::journal::{WorkRecord, read_claude, read_codex};
use jiff::{SignedDuration, Timestamp};

fn at(text: &str) -> Timestamp {
    text.parse().unwrap()
}

fn lines(lines: &[&str]) -> Vec<u8> {
    let mut data = lines.join("\n").into_bytes();
    data.push(b'\n');
    data
}

#[test]
fn claude_requests_answers_commits_and_files() {
    let data = lines(&[
        r#"{"type":"user","sessionId":"cli-1","cwd":"/Users/me/code/shop-web","gitBranch":"feature/login-hook","timestamp":"2026-11-13T00:12:00Z","message":{"role":"user","content":"로그인 화면을 훅 기반으로 바꿔줘"}}"#,
        r#"{"type":"ai-title","aiTitle":"로그인 화면 리팩터링","sessionId":"cli-1"}"#,
        r#"{"type":"assistant","timestamp":"2026-11-13T00:13:00Z","message":{"stop_reason":"tool_use","content":[{"type":"tool_use","name":"Edit","id":"t1"}]}}"#,
        r#"{"type":"user","timestamp":"2026-11-13T00:13:05Z","message":{"content":[{"type":"tool_result","tool_use_id":"t1"}]},"toolUseResult":{"filePath":"/Users/me/code/shop-web/src/Login.tsx","structuredPatch":[{"lines":[" a","-b","-c","+d"]}]}}"#,
        r#"{"type":"user","timestamp":"2026-11-13T00:14:00Z","message":{"content":[{"type":"tool_result","tool_use_id":"t2"}]},"toolUseResult":{"type":"create","filePath":"/Users/me/code/shop-web/src/useLoginForm.ts","content":"one\ntwo\nthree","structuredPatch":[]}}"#,
        r#"{"type":"assistant","timestamp":"2026-11-13T00:20:00Z","message":{"stop_reason":"end_turn","content":[{"type":"text","text":"폼 상태를 훅으로 옮겼어요."}]}}"#,
        r#"{"type":"user","timestamp":"2026-11-13T00:38:00Z","message":{"content":"커밋해줘"}}"#,
        r#"{"type":"user","timestamp":"2026-11-13T00:39:00Z","message":{"content":[{"type":"tool_result","tool_use_id":"t3"}]},"toolUseResult":{"stdout":"[feature/login-hook a1f3c2e] 로그인 폼 상태를 useLoginForm으로 분리\n 2 files changed","gitOperation":{"commit":{"sha":"a1f3c2e","kind":"committed"}}}}"#,
        r#"{"type":"user","timestamp":"2026-11-13T00:39:30Z","message":{"content":[{"type":"tool_result","tool_use_id":"t4"}]},"toolUseResult":{"stdout":"https://github.com/me/shop-web/pull/48\n"}}"#,
        r#"{"type":"user","isSidechain":true,"timestamp":"2026-11-13T00:39:40Z","message":{"content":"sub-agent prompt"}}"#,
        r#"{"type":"assistant","timestamp":"2026-11-13T00:40:00Z","message":{"stop_reason":"end_turn","content":[{"type":"text","text":"커밋 2개를 남겼어요."}]}}"#,
    ]);
    let transcript = read_claude(&data);
    assert_eq!(transcript.id.as_deref(), Some("cli-1"));
    assert_eq!(transcript.title.as_deref(), Some("로그인 화면 리팩터링"));
    assert_eq!(transcript.branch.as_deref(), Some("feature/login-hook"));
    assert_eq!(transcript.turns.len(), 2, "the side chain's message is not a request");
    assert_eq!(transcript.turns[0].result.as_deref(), Some("폼 상태를 훅으로 옮겼어요."));
    assert_eq!(transcript.turns[0].duration(), Some(SignedDuration::from_mins(8)));
    assert_eq!(transcript.commits.len(), 1);
    assert_eq!((transcript.commits[0].sha.as_str(), transcript.commits[0].message.as_str()), ("a1f3c2e", "로그인 폼 상태를 useLoginForm으로 분리"));
    assert_eq!(transcript.pull_requests.iter().map(|pull| pull.number).collect::<Vec<_>>(), [48]);
    let files: Vec<_> = transcript.files.iter().map(|file| (file.path.rsplit('/').next().unwrap(), file.added, file.removed, file.created)).collect();
    assert_eq!(files, [("Login.tsx", 1, 2, false), ("useLoginForm.ts", 3, 0, true)]);

    let record = WorkRecord::from_transcript("claude:cli-1".into(), Provider::Claude, transcript, None, None);
    assert_eq!(record.project(), "shop-web");
    assert_eq!(record.started_at(), Some(at("2026-11-13T00:12:00Z")));
    assert_eq!(record.ended_at(), Some(at("2026-11-13T00:40:00Z")));
    assert_eq!(record.working_time(), SignedDuration::from_mins(10));
}

#[test]
fn claude_interruptions_and_automated_prompts() {
    let data = lines(&[
        r#"{"type":"user","timestamp":"2026-11-13T01:00:00Z","message":{"content":"테스트 고쳐줘"}}"#,
        r#"{"type":"user","timestamp":"2026-11-13T01:01:00Z","message":{"content":"[Request interrupted by user]"}}"#,
        r#"{"type":"user","timestamp":"2026-11-13T01:02:00Z","message":{"content":"<task-notification>done</task-notification>"}}"#,
        r#"{"type":"user","timestamp":"2026-11-13T01:03:00Z","message":{"content":"다시 해줘"}}"#,
        r#"{"type":"system","subtype":"turn_duration","timestamp":"2026-11-13T01:09:00Z"}"#,
    ]);
    let transcript = read_claude(&data);
    let turns: Vec<_> = transcript.turns.iter().map(|turn| (turn.ask.as_str(), turn.aborted, turn.finished_at)).collect();
    assert_eq!(turns, [("테스트 고쳐줘", true, Some(at("2026-11-13T01:01:00Z"))), ("다시 해줘", false, Some(at("2026-11-13T01:09:00Z")))]);
    // Without a title of its own, the record is named by its first request.
    let record = WorkRecord::from_transcript("claude:x".into(), Provider::Claude, transcript, None, None);
    assert_eq!(record.title, "테스트 고쳐줘");
}

#[test]
fn codex_turns_patches_and_printed_commits() {
    let data = lines(&[
        r#"{"type":"session_meta","timestamp":"2026-11-13T01:00:00Z","payload":{"id":"thread-1","cwd":"/Users/me/code/payments-api","git":{"branch":"main"}}}"#,
        r#"{"type":"event_msg","timestamp":"2026-11-13T01:05:00Z","payload":{"type":"item_completed","item":{"type":"UserMessage","content":[{"type":"text","text":"결제 요청에 금액 검증 붙여줘"}]}}}"#,
        r#"{"type":"event_msg","timestamp":"2026-11-13T01:05:01Z","payload":{"type":"user_message","message":"결제 요청에 금액 검증 붙여줘"}}"#,
        r#"{"type":"response_item","timestamp":"2026-11-13T01:10:00Z","payload":{"type":"custom_tool_call","name":"apply_patch","input":"*** Begin Patch\n*** Update File: src/validate.rs\n@@\n-old\n+new\n+more\n*** Add File: tests/validate.rs\n+one\n+two\n*** End Patch"}}"#,
        r#"{"type":"response_item","timestamp":"2026-11-13T01:20:00Z","payload":{"type":"custom_tool_call_output","output":[{"type":"input_text","text":"[main 9c0ffee] 결제 금액과 통화 검증\n 2 files changed"}]}}"#,
        r#"{"type":"event_msg","timestamp":"2026-11-13T01:31:00Z","payload":{"type":"task_complete","last_agent_message":"금액과 통화 검증을 더했어요."}}"#,
    ]);
    let transcript = read_codex(&data);
    assert_eq!(transcript.id.as_deref(), Some("thread-1"));
    assert_eq!(transcript.branch.as_deref(), Some("main"));
    assert_eq!(transcript.turns.len(), 1, "the same request logged twice is one request");
    assert_eq!(transcript.turns[0].result.as_deref(), Some("금액과 통화 검증을 더했어요."));
    assert_eq!(transcript.commits.iter().map(|commit| (commit.sha.as_str(), commit.message.as_str())).collect::<Vec<_>>(), [("9c0ffee", "결제 금액과 통화 검증")]);
    let files: Vec<_> = transcript.files.iter().map(|file| (file.path.as_str(), file.added, file.removed, file.created)).collect();
    assert_eq!(files, [("src/validate.rs", 2, 1, false), ("tests/validate.rs", 2, 0, true)]);
}

mod views {
    use errand_core::journal::view::{Callout, Draft, Journal, JournalPlace, JournalTab, Live};
    use errand_core::journal::{Commit, PullRequest, WorkRecord, WorkTurn};
    use errand_core::sessions::SessionState;
    use errand_core::{Language, Provider};
    use jiff::Timestamp;
    use jiff::tz::TimeZone;
    use std::collections::HashMap;

    fn at(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    fn turn(asked: &str, finished: Option<&str>, result: &str) -> WorkTurn {
        WorkTurn { asked_at: at(asked), ask: "요청".into(), finished_at: finished.map(at), result: Some(result.into()).filter(|r: &String| !r.is_empty()), aborted: false }
    }

    fn record(key: &str, provider: Provider, folder: &str, title: &str, turns: Vec<WorkTurn>) -> WorkRecord {
        WorkRecord { key: key.into(), provider, title: title.into(), folder: folder.into(), branch: None, url: None, turns, commits: vec![], pull_requests: vec![], files: vec![] }
    }

    fn sample() -> Vec<WorkRecord> {
        let mut login = record("claude:a", Provider::Claude, "/code/shop-web", "로그인 화면 리팩터링", vec![turn("2026-11-13T00:12:00Z", Some("2026-11-13T00:40:00Z"), "폼 상태를 옮겼어요.")]);
        login.commits = vec![Commit { sha: "a1f3c2e".into(), message: "분리".into(), at: at("2026-11-13T00:39:00Z") }];
        let mut checks = record("codex:b", Provider::Codex, "/code/payments-api", "입력값 검증 추가", vec![turn("2026-11-13T01:05:00Z", Some("2026-11-13T01:31:00Z"), "검증을 더했어요.")]);
        checks.pull_requests = vec![PullRequest { number: 48, url: "https://github.com/me/p/pull/48".into(), at: at("2026-11-13T01:30:00Z") }];
        let running = record("claude:c", Provider::Claude, "/code/deploy-config", "배포 설정 정리", vec![turn("2026-11-13T05:02:00Z", None, "")]);
        let older = record("claude:d", Provider::Claude, "/code/shop-web", "대시보드 차트 추가", vec![turn("2026-11-12T07:30:00Z", Some("2026-11-12T08:24:00Z"), "차트를 더했어요.")]);
        vec![older, login, checks, running]
    }

    fn journal<'a>(records: &'a [WorkRecord], live: &'a HashMap<String, Live>, zone: &'a TimeZone) -> Journal<'a> {
        // 16:10 in Seoul on Friday 13 November 2026.
        Journal { records, live, translations: Box::leak(Box::default()), summaries: Box::leak(Box::default()), now: at("2026-11-13T07:10:00Z"), zone, language: Language::Korean }
    }

    #[test]
    fn a_day_shows_its_tasks_newest_first_with_live_states() {
        let records = sample();
        let live = HashMap::from([("claude:c".to_owned(), Live { state: SessionState::Running, question: None })]);
        let zone = TimeZone::get("Asia/Seoul").unwrap();
        let day = journal(&records, &live, &zone).day(&JournalPlace::default());
        assert_eq!((day.masthead.big.as_str(), day.masthead.top.as_str(), day.masthead.bottom.as_str()), ("13", "금요일", "2026년 11월"));
        assert_eq!(day.tasks.iter().map(|task| task.title.as_str()).collect::<Vec<_>>(), ["배포 설정 정리", "입력값 검증 추가", "로그인 화면 리팩터링"]);
        assert_eq!(day.tasks[0].callout, Some(Callout::Running));
        assert_eq!(day.tasks[0].meta, "deploy-config · 14:02부터");
        assert_eq!(day.tasks[2].meta, "shop-web · 09:12 – 09:40 · 28분 · 커밋 1");
        assert_eq!(day.tasks[1].meta, "payments-api · 10:05 – 10:31 · 26분 · PR #48");
        // The date line counts instead of pills: finished work, AI time, then commits and pull requests.
        assert!(day.masthead.pills.is_empty());
        let stats: Vec<(String, String)> = day.masthead.stats.iter().map(|stat| (stat.label.clone(), stat.parts.iter().map(|part| part.text.as_str()).collect())).collect();
        assert_eq!(stats, [("끝낸 일".to_owned(), "2".to_owned()), ("AI 작업".to_owned(), "54분".to_owned()), ("커밋 · PR".to_owned(), "1 · 1".to_owned())]);
        assert_eq!((day.axis.first, day.axis.last), (8, 18));
        assert!(day.axis.now.is_some() && day.masthead.can_go_forward == false);
        assert!(day.bars.iter().all(|bar| bar.start < bar.end && bar.lane == 0 && bar.waiting.is_none()));
        // Finished work and no summary yet: before the evening, the card offers to make one now.
        assert!(matches!(&day.draft, Draft::Offer(note, _) if note == "끝낸 작업 2개를 아직 정리하지 않았어요."));

        let yesterday = journal(&records, &live, &zone).day(&JournalPlace { back: 1, ..Default::default() });
        assert_eq!((yesterday.masthead.big.as_str(), yesterday.tasks.len(), yesterday.masthead.can_go_forward), ("12", 1, true));
        assert!(matches!(&yesterday.draft, Draft::Offer(note, _) if note == "끝낸 작업 1개를 아직 정리하지 않았어요."));

        // Work waiting on the person's answer ends where the agent stopped, and the wait runs on to now.
        let asking = HashMap::from([("claude:c".to_owned(), Live { state: SessionState::Asking, question: Some("어느 브랜치에?".into()) })]);
        let day = journal(&records, &asking, &zone).day(&JournalPlace::default());
        let bar = day.bars.iter().find(|bar| bar.key == "claude:c").unwrap();
        assert!(bar.waiting.is_some_and(|until| until > bar.end));
        assert_eq!(day.masthead.stats[1].label, "답변 필요");
        assert!(day.masthead.stats[1].asking);
    }

    #[test]
    fn a_week_a_month_and_a_report() {
        let records = sample();
        let live = HashMap::new();
        let zone = TimeZone::get("Asia/Seoul").unwrap();
        let journal = journal(&records, &live, &zone);
        let week = journal.week(&JournalPlace { tab: JournalTab::Week, ..Default::default() });
        assert_eq!((week.masthead.big.as_str(), week.masthead.top.as_str(), week.masthead.bottom.as_str()), ("9–15", "11월", "이번 주"));
        // Days with work only, oldest first; before a summary, a day reads as its tasks.
        assert_eq!(week.days.iter().map(|day| (day.day.as_str(), day.label.as_str())).collect::<Vec<_>>(), [("목", "11월 12일"), ("금", "11월 13일")]);
        assert!(week.days[1].today);
        assert_eq!(week.days[1].note.as_deref(), Some("정리 전"));
        assert_eq!(week.days[1].lines.iter().map(|line| line.text.as_str()).collect::<Vec<_>>(), ["로그인 화면 리팩터링", "입력값 검증 추가", "배포 설정 정리"]);
        assert!(week.report.is_empty() && week.carried.is_empty() && week.kinds.is_empty(), "nothing summed up yet");
        assert_eq!(week.recap.as_ref().map(|recap| recap.button.as_str()), Some("지금까지 보기"), "Friday 16:10 is before the recap is ready");
        let recap = journal.recap(&JournalPlace { tab: JournalTab::Week, recap: Some(1), ..Default::default() });
        assert_eq!(recap.position, "2 / 5", "no kind card without summaries");
        assert!(matches!(&recap.cards[1], errand_core::journal::view::RecapCard::Finished { count: 3, .. }));

        let month = journal.month(&JournalPlace { tab: JournalTab::Month, ..Default::default() });
        assert_eq!(month.cells.len(), 35, "November 2026 starts on a Sunday and needs five weeks");
        assert_eq!(month.full_rows, 2);
        let today = month.cells.iter().find(|cell| cell.today).unwrap();
        assert_eq!((today.day, today.lines.len()), (Some(13), 3));

        let report = journal.report(&JournalPlace { tab: JournalTab::Report, ..Default::default() });
        assert_eq!(report.masthead.big, "4");
        assert_eq!(report.heatmap.len(), 53 * 7);
        assert_eq!(report.weeks.len(), 12);
        assert_eq!(report.weeks.last().map(|week| (week.claude, week.codex)), Some((3, 1)));
        assert_eq!(report.hours.iter().sum::<usize>(), 4);
        assert_eq!(report.since, "11월 12일부터 기록");

        let detail = journal.detail("claude:a").unwrap();
        assert_eq!(detail.time, "11월 13일 09:12 – 09:40 · 28분");
        assert_eq!(detail.turns[0].commits, ["a1f3c2e"]);
        assert_eq!(detail.open_label, "Claude에서 열기");

        let log = journal.log(&JournalPlace::default());
        assert!(log.starts_with("## 11월 13일 금요일\n"));
        assert!(log.contains("### shop-web\n- 로그인 화면 리팩터링: 폼 상태를 옮겼어요. (커밋 1)\n"));
        assert!(log.contains("- 입력값 검증 추가: 검증을 더했어요. (PR #48)"));
    }

    #[test]
    fn a_week_as_a_metro_map() {
        let records = sample();
        let live = HashMap::from([("claude:c".to_owned(), Live { state: SessionState::Running, question: None })]);
        let zone = TimeZone::get("Asia/Seoul").unwrap();
        let week = journal(&records, &live, &zone).week(&JournalPlace { tab: JournalTab::Week, ..Default::default() });
        let metro = &week.metro;
        assert_eq!(metro.title, "이번 주 노선도");
        assert_eq!(metro.days.iter().map(|day| (day.label.as_str(), day.total.as_str())).collect::<Vec<_>>(), [("목요일 12일", "54분"), ("금요일 13일", "3시간 2분")]);
        // Lines by the week's time; the running task counts up to now.
        assert_eq!(metro.lines.iter().map(|line| (line.name.as_str(), line.colour)).collect::<Vec<_>>(), [("deploy-config", Some(0)), ("shop-web", Some(1)), ("payments-api", Some(2))]);
        assert_eq!(metro.lines[1].legend, "shop-web · 작업 2개 · 1시간 22분");
        // Each day the busiest line takes the top lane, so shop-web and deploy-config cross between Thursday and Friday.
        assert_eq!(metro.lines.iter().map(|line| line.lanes.clone()).collect::<Vec<_>>(), [vec![1, 0], vec![0, 1], vec![2, 2]]);
        // Names take turns above and below along a line; the pull request and the open task are marked.
        let shop: Vec<Option<bool>> = metro.stops.iter().filter(|stop| stop.line == 1).map(|stop| stop.name_above).collect();
        assert_eq!(shop, [Some(true), Some(false)]);
        let checks = metro.stops.iter().find(|stop| stop.key == "codex:b").unwrap();
        assert_eq!((checks.pull.as_deref(), checks.minutes, checks.day), (Some("PR #48"), 26, 1));
        let deploy = metro.stops.iter().find(|stop| stop.key == "claude:c").unwrap();
        assert!((deploy.at - (14.0 + 2.0 / 60.0 - 8.0) / 12.0).abs() < 1e-6);
        assert_eq!((metro.lines[0].carried, metro.lines[1].carried), (1, 0));
        assert_eq!(metro.terminal, "남은 일", "Friday 16:10 is before the week is done");
        let asking = HashMap::from([("claude:c".to_owned(), Live { state: SessionState::Asking, question: None })]);
        let week = journal(&records, &asking, &zone).week(&JournalPlace { tab: JournalTab::Week, ..Default::default() });
        assert!(week.metro.stops.iter().find(|stop| stop.key == "claude:c").unwrap().asking);

        // Past four colours the rest share a grey line, and a busy day names only its two longest stops per line.
        let mut many = sample();
        for (index, project) in ["docs", "infra", "mobile"].iter().enumerate() {
            many.push(record(&format!("codex:p{index}"), Provider::Codex, &format!("/code/{project}"), project, vec![turn("2026-11-11T01:00:00Z", Some("2026-11-11T01:10:00Z"), "끝냈어요.")]));
        }
        for (index, minutes) in [5, 40, 20].iter().enumerate() {
            let end = format!("2026-11-11T0{}:{minutes:02}:00Z", 2 + index);
            many.push(record(&format!("claude:s{index}"), Provider::Claude, "/code/shop-web", "작은 일", vec![turn(&format!("2026-11-11T0{}:00:00Z", 2 + index), Some(&end), "끝냈어요.")]));
        }
        let week = journal(&many, &HashMap::new(), &zone).week(&JournalPlace { tab: JournalTab::Week, ..Default::default() });
        let lines = &week.metro.lines;
        assert_eq!(lines.len(), 5);
        assert_eq!((lines[4].name.as_str(), lines[4].colour), ("그 밖 2개", None));
        assert!(week.metro.stops.iter().all(|stop| stop.line < 5));
        let wednesday_shop: Vec<(i64, Option<bool>)> = week.metro.stops.iter().filter(|stop| stop.day == 0 && stop.key.starts_with("claude:s")).map(|stop| (stop.minutes, stop.name_above)).collect();
        assert_eq!(wednesday_shop, [(5, None), (40, Some(true)), (20, Some(false))]);
    }

    #[test]
    fn answers_in_another_language_can_be_translated_and_shown_as_written() {
        use errand_core::journal::view::{Translate, Translations, foreign};
        assert!(foreign("I installed it. The build is now in your Applications folder.", Language::Korean));
        assert!(!foreign("main `bc8e741`을 점검했고, 수정이 필요한 보안 문제 3건을 재현했습니다. [tasks.rs:100](/Users/me/code/errand/crates/tasks.rs)", Language::Korean));
        assert!(!foreign("`44e4ed3`", Language::Korean), "a hash alone is no language");
        assert!(foreign("폼 상태를 훅으로 옮기고 테스트를 고쳤어요.", Language::English));
        assert!(!foreign("Moved the form state into a hook.", Language::English));

        let english = "Moved the form state into a hook and fixed the tests.";
        let records = vec![record("claude:e", Provider::Claude, "/code/shop-web", "로그인 훅", vec![turn("2026-11-13T02:00:00Z", Some("2026-11-13T02:30:00Z"), english), turn("2026-11-13T03:00:00Z", Some("2026-11-13T03:10:00Z"), "테스트를 고쳤어요.")])];
        let (live, zone) = (HashMap::new(), TimeZone::get("Asia/Seoul").unwrap());
        let mut translations = Translations::default();
        let view = |translations: &Translations| Journal { records: &records, live: &live, translations, summaries: Box::leak(Box::default()), now: at("2026-11-13T07:10:00Z"), zone: &zone, language: Language::Korean }.detail("claude:e").unwrap();

        // Only the English answer is offered, and nothing changes until it comes back.
        assert_eq!(view(&translations).translate, Some(Translate::Offer(vec![english.to_owned()])));
        translations.working = Some("claude:e".into());
        assert_eq!(view(&translations).translate, Some(Translate::Working));
        translations.working = None;
        translations.failed = Some(("claude:e".into(), "번역하지 못했어요. / Could not translate.".into()));
        assert_eq!(view(&translations).translate, Some(Translate::Failed("번역하지 못했어요.".into(), vec![english.to_owned()])));
        translations.failed = None;
        translations.texts.insert(english.to_owned(), "폼 상태를 훅으로 옮기고 테스트를 고쳤어요.".to_owned());
        let shown = view(&translations);
        assert_eq!(shown.translate, Some(Translate::Shown));
        assert_eq!(shown.turns[0].result, "폼 상태를 훅으로 옮기고 테스트를 고쳤어요.");
        assert_eq!(shown.turns[1].result, "테스트를 고쳤어요.");
        translations.original = true;
        let original = view(&translations);
        assert_eq!(original.translate, Some(Translate::Original));
        assert_eq!(original.turns[0].result, english);

        // A task whose answers are all in the window's language offers nothing.
        let sample = sample();
        assert_eq!(journal(&sample, &live, &zone).detail("claude:a").unwrap().translate, None);
    }
}

#[test]
fn requests_lose_what_the_app_put_in_front_and_worktrees_count_as_their_repository() {
    let data = lines(&[
        r#"{"type":"user","cwd":"/Users/me/code/shop-web/.claude/worktrees/login-hook-1a2b3c","timestamp":"2026-11-13T00:12:00Z","message":{"content":"<artifact-view-context artifact=\"x\">\n{\"context\":{}}\n</artifact-view-context>\n<artifact-content-authored-by-others/>\n2번이 좋아"}}"#,
        r#"{"type":"user","timestamp":"2026-11-13T00:20:00Z","message":{"content":"<system-reminder>only this</system-reminder>"}}"#,
    ]);
    let transcript = read_claude(&data);
    assert_eq!(transcript.turns.iter().map(|turn| turn.ask.as_str()).collect::<Vec<_>>(), ["2번이 좋아"]);
    let record = WorkRecord::from_transcript("k".into(), Provider::Claude, transcript, None, None);
    assert_eq!(record.project(), "shop-web");
}

#[test]
fn an_answer_after_the_thinking_closed_the_turn_still_counts() {
    let data = lines(&[
        r#"{"type":"user","timestamp":"2026-11-13T00:00:00Z","message":{"content":"설명해줘"}}"#,
        r#"{"type":"assistant","timestamp":"2026-11-13T00:01:00Z","message":{"stop_reason":"end_turn","content":[{"type":"thinking","thinking":"…"}]}}"#,
        r#"{"type":"assistant","timestamp":"2026-11-13T00:01:05Z","message":{"stop_reason":"end_turn","content":[{"type":"text","text":"이렇게 했어요."}]}}"#,
        r#"{"type":"user","timestamp":"2026-11-13T00:05:00Z","message":{"content":"고마워"}}"#,
        r#"{"type":"assistant","timestamp":"2026-11-13T00:05:10Z","message":{"stop_reason":"stop_sequence","content":[{"type":"text","text":"천만에요."}]}}"#,
    ]);
    let transcript = read_claude(&data);
    let turns: Vec<_> = transcript.turns.iter().map(|turn| (turn.result.as_deref(), turn.finished_at)).collect();
    assert_eq!(turns, [(Some("이렇게 했어요."), Some(at("2026-11-13T00:01:05Z"))), (Some("천만에요."), Some(at("2026-11-13T00:05:10Z")))]);
}

#[test]
fn old_transcripts_without_end_marks_still_have_answers() {
    let data = lines(&[
        r#"{"type":"user","timestamp":"2026-04-19T07:12:00Z","message":{"content":"docker-compose로 말아줘"}}"#,
        r#"{"type":"assistant","timestamp":"2026-04-19T07:13:00Z","message":{"content":[{"type":"tool_use","name":"Write"}]}}"#,
        r#"{"type":"assistant","timestamp":"2026-04-19T07:15:00Z","message":{"content":[{"type":"text","text":"compose 파일을 만들었어요."}]}}"#,
        r#"{"type":"user","timestamp":"2026-04-19T09:00:00Z","message":{"content":"왜 css가 안 되지?"}}"#,
        r#"{"type":"assistant","timestamp":"2026-04-19T09:02:00Z","message":{"content":[{"type":"text","text":"경로가 틀렸어요."}]}}"#,
    ]);
    let transcript = read_claude(&data);
    let turns: Vec<_> = transcript.turns.iter().map(|turn| (turn.result.as_deref(), turn.finished_at)).collect();
    assert_eq!(turns, [(Some("compose 파일을 만들었어요."), Some(at("2026-04-19T07:15:00Z"))), (Some("경로가 틀렸어요."), None)]);
}
