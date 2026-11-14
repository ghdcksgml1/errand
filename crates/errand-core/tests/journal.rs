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
    use errand_core::journal::view::{Callout, Journal, JournalPlace, JournalTab, Live};
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
        Journal { records, live, translations: Box::leak(Box::default()), now: at("2026-11-13T07:10:00Z"), zone, language: Language::Korean }
    }

    #[test]
    fn a_day_shows_its_tasks_newest_first_with_live_states() {
        let records = sample();
        let live = HashMap::from([("claude:c".to_owned(), Live { state: SessionState::Running, question: None })]);
        let zone = TimeZone::get("Asia/Seoul").unwrap();
        let day = journal(&records, &live, &zone).day(&JournalPlace::default());
        assert_eq!((day.masthead.big.as_str(), day.masthead.top.as_str(), day.masthead.bottom.as_str()), ("13", "금요일", "2026년 11월"));
        assert_eq!(day.cards.iter().map(|card| card.title.as_str()).collect::<Vec<_>>(), ["배포 설정 정리", "입력값 검증 추가", "로그인 화면 리팩터링"]);
        assert_eq!(day.cards[0].callout, Some(Callout::Running));
        assert!(day.cards[0].line.starts_with("작업 중 · 2시간"));
        assert_eq!(day.cards[2].time, "09:12 – 09:40 · 28분");
        assert_eq!(day.cards[2].marks, "커밋 1");
        assert_eq!(day.cards[1].marks, "PR #48");
        let pills: Vec<_> = day.masthead.pills.iter().map(|pill| pill.text.as_str()).collect();
        assert_eq!(pills, ["진행 중 1", "커밋 1", "PR 1"]);
        assert_eq!(day.filters.iter().map(|(_, name, count)| (name.as_str(), *count)).collect::<Vec<_>>(), [("전체", 3), ("deploy-config", 1), ("payments-api", 1), ("shop-web", 1)]);
        assert_eq!((day.axis.first, day.axis.last), (8, 18));
        assert!(day.axis.now.is_some() && day.masthead.can_go_forward == false);
        assert!(day.bars.iter().all(|bar| bar.start < bar.end && bar.lane == 0));

        let filtered = journal(&records, &live, &zone).day(&JournalPlace { project: Some("shop-web".into()), ..Default::default() });
        assert_eq!(filtered.cards.len(), 1);
        let yesterday = journal(&records, &live, &zone).day(&JournalPlace { back: 1, ..Default::default() });
        assert_eq!((yesterday.masthead.big.as_str(), yesterday.cards.len(), yesterday.masthead.can_go_forward), ("12", 1, true));
    }

    #[test]
    fn a_week_a_month_and_a_report() {
        let records = sample();
        let live = HashMap::new();
        let zone = TimeZone::get("Asia/Seoul").unwrap();
        let journal = journal(&records, &live, &zone);
        let week = journal.week(&JournalPlace { tab: JournalTab::Week, ..Default::default() });
        assert_eq!((week.masthead.big.as_str(), week.masthead.top.as_str(), week.masthead.bottom.as_str()), ("9–15", "11월", "이번 주"));
        assert_eq!(week.rows.iter().map(|row| row.label.as_str()).collect::<Vec<_>>(), ["월 9", "화 10", "수 11", "목 12", "금 13", "토 14", "일 15"]);
        assert!(week.rows[4].today && week.rows[4].now.is_some());
        assert_eq!(week.projects[0].name, "shop-web");
        assert_eq!(week.projects[0].lines.iter().map(|line| line.when.as_str()).collect::<Vec<_>>(), ["금 09:12", "목 16:30"]);

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
        let view = |translations: &Translations| Journal { records: &records, live: &live, translations, now: at("2026-11-13T07:10:00Z"), zone: &zone, language: Language::Korean }.detail("claude:e").unwrap();

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
