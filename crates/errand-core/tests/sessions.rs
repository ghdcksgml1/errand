//! Ported from the transcript and card-order parts of Tests/SessionTrackerTests.m. Reading the apps'
//! files and database is errand-io's, and tested there.

use errand_core::format::timestamp_from_seconds;
use errand_core::sessions::{
    ClaudeMeta, CodexThread, SessionRecords, SessionRow, SessionState, Turn, VisibleSession, claude_row, codex_row, families, fold_claude,
    fold_codex,
};
use errand_core::{Provider, Timestamp};
use serde_json::{Value, json};

const NOW: f64 = 1_792_044_000.0;

fn ago(seconds: f64) -> Timestamp {
    timestamp_from_seconds(NOW - seconds).unwrap()
}
fn stamp(at: Timestamp) -> String {
    at.strftime("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}
fn line(object: Value) -> String {
    format!("{object}\n")
}
fn codex_event(at: Timestamp, payload: Value) -> String {
    line(json!({"timestamp": stamp(at), "type": "event_msg", "payload": payload}))
}
fn codex_prompt(seconds_ago: f64, text: &str) -> String {
    codex_event(ago(seconds_ago), json!({"type": "task_started"}))
        + &codex_event(ago(seconds_ago - 0.1), json!({"type": "item_completed", "item": {"type": "UserMessage", "content": [{"type": "text", "text": text}]}}))
}
fn claude_user(seconds_ago: f64, content: Value, extra: Value) -> String {
    let mut object = json!({"type": "user", "isSidechain": false, "timestamp": stamp(ago(seconds_ago)), "message": {"role": "user", "content": content}});
    for (key, value) in extra.as_object().into_iter().flatten() {
        object[key] = value.clone();
    }
    line(object)
}
fn claude_assistant(seconds_ago: f64, text: &str, stop: &str, uuid: &str) -> String {
    line(json!({"type": "assistant", "isSidechain": false, "timestamp": stamp(ago(seconds_ago)), "uuid": uuid,
                "message": {"role": "assistant", "stop_reason": stop, "content": [{"type": "text", "text": text}]}}))
}
fn codex_turn(rollout: &str) -> Turn {
    let mut turn = Turn::default();
    fold_codex(&mut turn, rollout.as_bytes());
    turn
}
fn claude_turn(transcript: &str) -> Turn {
    let mut turn = Turn::default();
    fold_claude(&mut turn, transcript.as_bytes());
    turn
}

fn rollout() -> String {
    codex_prompt(600.0, "첫 요청")
        + &codex_event(ago(590.0), json!({"type": "task_complete", "last_agent_message": "**첫 결과**입니다."}))
        + &codex_prompt(60.0, "로그인 버그 고쳐줘")
}

#[test]
fn codex_turns_follow_start_complete_and_abort() {
    let turn = codex_turn(&rollout());
    assert!(turn.running && turn.prompt.as_deref() == Some("로그인 버그 고쳐줘") && turn.result.is_none(), "{turn:?}");
    let done = codex_turn(&(rollout() + &codex_event(ago(30.0), json!({"type": "task_complete", "last_agent_message": "수정했어요"}))));
    assert!(!done.running && done.result.as_deref() == Some("수정했어요") && done.finished_at == Some(ago(30.0)), "{done:?}");
    let aborted = codex_turn(&(rollout() + &codex_event(ago(30.0), json!({"type": "turn_aborted", "reason": "interrupted"}))));
    assert!(!aborted.running && aborted.aborted && aborted.result.is_none(), "an interrupted turn is not shown as a result");
    let torn = codex_turn(&(r#"{"timestamp":"x","payload":{"type":"task_st"#.to_owned() + "\n" + &rollout()));
    assert_eq!(torn.prompt.as_deref(), Some("로그인 버그 고쳐줘"), "a torn first line does not stop the remaining events");
}

#[test]
fn codex_turns_fold_across_chunks() {
    let whole = rollout();
    let split = whole.find("\n").unwrap() + 1;
    let mut turn = Turn::default();
    fold_codex(&mut turn, &whole.as_bytes()[..split]);
    fold_codex(&mut turn, &whole.as_bytes()[split..]);
    assert_eq!(turn, codex_turn(&whole));
}

fn transcript() -> String {
    [
        claude_user(900.0, json!("예전 요청"), json!({})),
        claude_assistant(890.0, "예전 답", "end_turn", "u-0"),
        claude_user(120.0, json!([{"type": "text", "text": "README 정리하고 커밋해줘"}]), json!({})),
        claude_assistant(110.0, "확인할게요", "tool_use", "u-a"),
        claude_user(100.0, json!([{"type": "tool_result", "content": "file"}]), json!({})),
        claude_user(99.0, json!("Caveat: meta"), json!({"isMeta": true})),
        claude_user(98.0, json!("subagent prompt"), json!({"isSidechain": true})),
        line(json!({"type": "assistant", "isSidechain": true, "timestamp": stamp(ago(97.0)),
                    "message": {"role": "assistant", "stop_reason": "end_turn", "content": [{"type": "text", "text": "sidechain answer"}]}})),
    ]
    .concat()
}

fn finished() -> String {
    transcript()
        + &claude_assistant(50.0, "커밋했어요 (abc123)", "end_turn", "u-1")
        + &line(json!({"type": "system", "subtype": "stop_hook_summary", "isSidechain": false, "timestamp": stamp(ago(49.0))}))
}

#[test]
fn claude_turns_keep_the_persons_prompt() {
    let turn = claude_turn(&transcript());
    assert!(turn.finished_at.is_none() && turn.prompt.as_deref() == Some("README 정리하고 커밋해줘"),
        "tool results, meta lines and the previous answer neither end nor replace the running prompt: {turn:?}");
    let done = claude_turn(&finished());
    assert!(!done.running && done.result.as_deref() == Some("커밋했어요 (abc123)") && done.finished_at == Some(ago(50.0)),
        "end_turn completes the turn at the answer's time: {done:?}");
    assert!(claude_turn(&(finished() + &claude_assistant(40.0, "마무리 중", "tool_use", "u-2"))).running,
        "work resumed by a stop hook counts as running again");
    let automated = claude_turn(&(finished() + &claude_user(20.0, json!("<side-sessions-event>done</side-sessions-event>"), json!({}))));
    assert!(automated.running && automated.prompt.as_deref() == Some("README 정리하고 커밋해줘"),
        "an automated event starts work but keeps the person's own last prompt");
    let compacted = claude_turn(&(finished() + &claude_user(20.0, json!("This session is being continued"), json!({"isCompactSummary": true}))));
    assert!(!compacted.running, "a compaction summary is not a new prompt");
    let interrupted = claude_turn(&(transcript() + &claude_user(10.0, json!([{"type": "text", "text": "[Request interrupted by user]"}]), json!({}))));
    assert!(!interrupted.running && interrupted.aborted, "a user interrupt ends the turn");
    let noted = claude_turn(&line(json!({"type": "last-prompt", "lastPrompt": "마지막 요청 기록"})));
    assert_eq!(noted.last_prompt.as_deref(), Some("마지막 요청 기록"));
}

// Emoji stay out of src/, where the no-symbol-glyph check for UI text looks.
#[test]
fn long_text_is_cut_at_a_whole_character() {
    use errand_core::sessions::text::preview;
    assert_eq!(preview("가나다라마", 5), "가나다라마");
    assert_eq!(preview("가나다라마바", 5), "가나다라마…");
    // A flag is four UTF-16 units; cutting inside it drops the whole flag.
    assert_eq!(preview("가나다🇰🇷", 4), "가나다…");
    assert_eq!(preview("ab👍🏽cd", 3), "ab…");
}

fn codex_thread() -> CodexThread {
    CodexThread {
        id: "t-user".to_owned(),
        rollout_path: "/tmp/rollout.jsonl".to_owned(),
        title: "로그인 수정".to_owned(),
        first_message: "첫 요청".to_owned(),
        cwd: "/Users/me/app".to_owned(),
        updated_at: ago(30.0),
    }
}

fn claude_meta() -> ClaudeMeta {
    let summary = r#"{"status_category":"completed","status_detail":"README 커밋 완료, 푸시 전","needs_action":"푸시 여부 결정"}"#;
    ClaudeMeta::from_json(&json!({
        "sessionId": "local_abc", "cliSessionId": "cli-1", "cwd": "/Users/me/Desktop/docs site", "title": "README 정리",
        "createdAt": (NOW - 1000.0) * 1000.0, "latestUserFrameAt": (NOW - 120.0) * 1000.0, "lastFocusedAt": (NOW - 115.0) * 1000.0,
        "lastAssistantUuid": "u-1", "postTurnSummaryFor": "u-1", "postTurnSummary": summary,
        "remoteMcpServersConfig": [{"name": "large"}]
    }))
    .unwrap()
}

#[test]
fn rows_carry_the_request_a_plain_result_and_a_deep_link() {
    let done = codex_turn(&(rollout() + &codex_event(ago(30.0), json!({"type": "task_complete", "last_agent_message": "## 결과\n로그인 **수정** 완료"}))));
    let codex = codex_row(&codex_thread(), &done, Some(ago(30.0)), false, ago(0.0));
    assert_eq!((codex.key.as_str(), codex.provider, codex.title.as_str()), ("codex:t-user", Provider::Codex, "로그인 수정"));
    assert_eq!((codex.prompt.as_str(), codex.result.as_deref()), ("로그인 버그 고쳐줘", Some("결과 로그인 수정 완료")));
    assert_eq!((codex.url.as_str(), codex.folder.as_str(), codex.seen_in_app), ("codex://threads/t-user", "app", false));

    let claude = claude_row("local_abc", &claude_meta(), &claude_turn(&finished()), ago(40.0), Some(ago(45.0)), ago(0.0)).unwrap();
    assert_eq!((claude.prompt.as_str(), claude.result.as_deref()), ("README 정리하고 커밋해줘", Some("README 커밋 완료, 푸시 전")),
        "the app's own summary describes the latest answer");
    assert_eq!(claude.action.as_deref(), Some("푸시 여부 결정"));
    assert_eq!(claude.url, "claude://code/continue?session=local_abc");
    assert!(!claude.seen_in_app, "focused before the answer arrived");
}

#[test]
fn an_outdated_summary_gives_way_to_the_transcripts_answer() {
    let mut meta = claude_meta();
    meta.post_turn_summary_for = Some("u-0".to_owned());
    meta.last_focused_at = Some(ago(10.0));
    let row = claude_row("local_abc", &meta, &claude_turn(&finished()), ago(40.0), None, ago(0.0)).unwrap();
    assert_eq!((row.result.as_deref(), row.action.as_deref()), (Some("커밋했어요 (abc123)"), None));
    assert!(row.seen_in_app, "focused after the answer");
    let mut nameless = ClaudeMeta::default();
    assert!(claude_row("local_x", &nameless, &Turn::default(), ago(5.0), None, ago(0.0)).is_none(), "a session with no ID cannot be opened");
    nameless.session_id = Some("local_x".to_owned());
    let bare = claude_row("local_x", &nameless, &Turn::default(), ago(5.0), None, ago(0.0)).unwrap();
    assert_eq!((bare.title.as_str(), bare.prompt.as_str(), bare.prompt_at), ("", "", ago(5.0)), "the view names an untitled session");
}

#[test]
fn running_work_silent_for_twenty_minutes_is_stalled() {
    let running = codex_turn(&codex_prompt(3000.0, "배포 고쳐줘"));
    assert!(codex_row(&codex_thread(), &running, Some(ago(1201.0)), false, ago(0.0)).stalled);
    assert!(!codex_row(&codex_thread(), &running, Some(ago(1199.0)), false, ago(0.0)).stalled);
    assert!(!codex_row(&codex_thread(), &running, None, false, ago(0.0)).stalled, "no file, no evidence of silence");
}

fn row(key: &str, prompt_seconds_ago: f64, finished_seconds_ago: Option<f64>) -> SessionRow {
    let provider = if key.starts_with("codex:") { Provider::Codex } else { Provider::Claude };
    SessionRow {
        key: key.to_owned(),
        provider,
        title: key.to_owned(),
        prompt: "요청".to_owned(),
        prompt_at: ago(prompt_seconds_ago),
        running: finished_seconds_ago.is_none(),
        aborted: false,
        finished_at: finished_seconds_ago.map(ago),
        result: None,
        action: None,
        folder: String::new(),
        url: String::new(),
        seen_in_app: false,
        stalled: false,
        parent: None,
        on_screen: false,
    }
}

fn keys(visible: &[VisibleSession]) -> Vec<&str> {
    visible.iter().map(|session| session.row.key.as_str()).collect()
}

#[test]
fn cards_stay_put_until_checked_off_and_come_back_with_a_new_request() {
    let mut records = SessionRecords::new(ago(0.0));
    let scan = || vec![row("claude:local_abc", 120.0, Some(50.0)), row("codex:t-user", 60.0, Some(30.0)), row("codex:t-old", 20.0 * 3600.0, Some(19.0 * 3600.0))];
    let visible = records.visible(scan());
    assert_eq!(keys(&visible), ["codex:t-user", "claude:local_abc"], "newest request first; work before the last 12 hours stays out");
    assert!(visible.iter().all(|session| !session.seen && SessionState::of(session) == SessionState::New));

    records.mark_seen("codex:t-user", ago(0.0));
    let visible = records.visible(scan());
    assert_eq!(keys(&visible), ["codex:t-user", "claude:local_abc"], "opening the top card marks it seen without moving it");
    assert!(visible[0].seen && !visible[1].seen);
    assert_eq!(SessionState::of(&visible[0]), SessionState::Seen);

    records.complete("codex:t-user", ago(0.0));
    assert_eq!(keys(&records.visible(scan())), ["claude:local_abc"], "only checking a card off removes it");

    let mut returned = scan();
    returned[1] = row("codex:t-user", -5.0, None);
    let visible = records.visible(returned);
    assert_eq!(keys(&visible), ["codex:t-user", "claude:local_abc"], "a new request after checking off brings the session back on top");
    assert_eq!(SessionState::of(&visible[0]), SessionState::Running);
}

#[test]
fn seen_needs_a_finished_answer_and_ties_order_by_key() {
    let mut records = SessionRecords::new(ago(0.0));
    let mut in_app = row("codex:b", 100.0, Some(90.0));
    in_app.seen_in_app = true;
    let early_mark = row("codex:a", 100.0, Some(10.0));
    records.mark_seen("codex:a", ago(20.0));
    let visible = records.visible(vec![in_app, early_mark]);
    assert_eq!(keys(&visible), ["codex:a", "codex:b"], "equal request times fall back to key order");
    assert!(!visible[0].seen, "opened before its latest answer arrived");
    assert!(visible[1].seen, "read in the app itself");
}

#[test]
fn checked_off_records_only_hold_times() {
    let mut records = SessionRecords::new(ago(0.0));
    records.complete("codex:t-user", ago(1.0));
    records.mark_seen("claude:local_abc", ago(1.0));
    let entries: Vec<(&str, bool, bool)> =
        records.entries().map(|(key, record)| (key, record.completed_at.is_some(), record.seen_at.is_some())).collect();
    assert_eq!(entries, [("claude:local_abc", false, true), ("codex:t-user", true, false)]);
    assert_eq!(records.floor("codex:t-user"), ago(1.0));
    assert_eq!(records.floor("codex:other"), ago(12.0 * 3600.0));
}

#[test]
fn a_side_session_names_the_card_it_shows_in_as_the_app_files_it() {
    let meta = |spawned: Value, detached: bool| ClaudeMeta::from_json(&json!({"sessionId": "local_kid", "spawnedFrom": spawned, "lineageDetached": detached})).unwrap();
    let side = json!({"sessionId": "local_parent", "sideSession": true});
    let row = claude_row("local_kid", &meta(side.clone(), false), &Turn::default(), ago(0.0), None, ago(0.0)).unwrap();
    assert_eq!(row.parent.as_deref(), Some("claude:local_parent"));
    assert_eq!(meta(side, true).parent_session, None, "the person moved it to the top level");
    assert_eq!(meta(json!({"sessionId": "local_parent", "taskId": "task"}), false).parent_session, None, "the app links side sessions only");
    assert_eq!(ClaudeMeta::from_json(&json!({"sessionId": "local_kid"})).unwrap().parent_session, None);
}

fn sub_task(key: &str, parent: &str, prompt_seconds_ago: f64) -> SessionRow {
    SessionRow { parent: Some(parent.to_owned()), ..row(key, prompt_seconds_ago, None) }
}

fn leaders(visible: &[VisibleSession]) -> Vec<(&str, Vec<&str>)> {
    families(visible).into_iter().map(|family| (family.head.row.key.as_str(), family.sub_tasks.iter().map(|session| session.row.key.as_str()).collect())).collect()
}

#[test]
fn sub_tasks_stay_with_their_card_and_the_newest_request_places_it() {
    let records = SessionRecords::new(ago(0.0));
    let scan = vec![
        row("claude:parent", 300.0, Some(200.0)),
        sub_task("claude:older", "claude:parent", 250.0),
        row("codex:other", 100.0, None),
        sub_task("claude:newer", "claude:parent", 50.0),
    ];
    let visible = records.visible(scan);
    assert_eq!(keys(&visible), ["claude:parent", "claude:newer", "claude:older", "codex:other"], "a request in a sub-task brings its whole card up");
    assert_eq!(leaders(&visible), [("claude:parent", vec!["claude:newer", "claude:older"]), ("codex:other", vec![])]);
}

#[test]
fn a_sub_task_leads_a_card_of_its_own_when_nothing_it_came_from_shows() {
    let mut records = SessionRecords::new(ago(0.0));
    let scan = || vec![row("claude:parent", 300.0, Some(200.0)), sub_task("claude:kid", "claude:parent", 100.0), sub_task("claude:grandkid", "claude:kid", 50.0)];
    let visible = records.visible(scan());
    assert_eq!(leaders(&visible), [("claude:parent", vec!["claude:grandkid", "claude:kid"])], "a sub-task's sub-task goes in the card at the top");
    records.complete("claude:parent", ago(0.0));
    let visible = records.visible(scan());
    assert_eq!(leaders(&visible), [("claude:kid", vec!["claude:grandkid"])], "with the parent cleared, the next one up leads");

    let looped = records.visible(vec![sub_task("claude:a", "claude:b", 20.0), sub_task("claude:b", "claude:a", 10.0)]);
    assert_eq!(leaders(&looped), [("claude:b", vec![]), ("claude:a", vec![])], "records that go round in a circle lose no card");
}
