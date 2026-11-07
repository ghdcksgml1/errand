//! `--preview <file.png>`: draws the screens from fixed data, without real accounts. The fixtures are the
//! ones the Objective-C build drew, with English stand-ins for the sample tasks.

use crate::platform::Platform;
use errand_core::format::{self, timestamp_from_seconds};
use errand_core::model::{AccountInfo, AppState, Msg, Profile, WidgetPreferences, update};
use errand_core::ports::RoomService;
use errand_core::rooms::{self, Invite, Role, RoomError, RoomMember, RoomSummary, RoomsScreen};
use errand_core::reset_history::ResetHistory;
use errand_core::sessions::{SessionRecords, SessionRow, TaskCounts, VisibleSession};
use errand_core::update::Release;
use errand_core::usage::{Reading, UsageError, UsageWindow, claude, codex};
use errand_core::{Language, PerProvider, Provider, Timestamp};
use jiff::SignedDuration;
use serde_json::json;

/// 2026-10-15T06:00:00Z, the moment the fixtures were read.
const FETCHED: f64 = 1_792_044_000.0;

/// Fixture words in the preview's language.
fn words(language: Language, korean: &'static str, english: &'static str) -> &'static str {
    match language {
        Language::Korean => korean,
        Language::English => english,
    }
}

pub fn state(arguments: &[String], now: Timestamp, language: Language) -> AppState {
    let has = |flag: &str| arguments.iter().any(|argument| argument == flag);
    let fetched = timestamp_from_seconds(FETCHED).expect("a fixed time");
    let nickname = words(language, "내 닉네임", "My nickname").to_owned();
    let mut state = AppState::new(ResetHistory::new(), SessionRecords::new(now), WidgetPreferences::default(), nickname.clone());
    let weekly = json!({"usedPercent": 28, "windowDurationMins": 10080, "resetsAt": 1792368000});
    let mut codex_response = json!({"rateLimits": {"primary": weekly}});
    let mut claude_limits = json!({"five_hour": {"used_percentage": 35.4, "resets_at": 1792054800},
                                   "seven_day": {"used_percentage": 58, "resets_at": 1792368000}});
    if has("--preview-multiple") {
        codex_response = json!({"rateLimits": {"primary": {"usedPercent": 90, "windowDurationMins": 300, "resetsAt": 1792054800}, "secondary": weekly}});
        claude_limits = json!({"five_hour": {"used_percentage": 81, "resets_at": 1792054800},
                               "seven_day": {"used_percentage": 58, "resets_at": 1792368000}});
    }
    let codex_reading = Reading::from(codex::windows_from_response(&codex_response));
    let mut claude_reading = Reading { windows: claude::windows_from_rate_limits(&claude_limits), reported_at: Some(fetched) };
    if has("--preview-multiple") {
        claude_reading.windows.extend(claude::window("limits/Fable", 88, 1792368000.0));
    }
    for (provider, reading) in [(Provider::Codex, codex_reading), (Provider::Claude, claude_reading)] {
        update(&mut state, Msg::UsageRead { provider, result: Ok(reading) }, fetched);
    }
    if has("--preview-reset") {
        // A reset seen just now on every limit: the reading before had its reset half a minute ago, and the limit
        // has moved on to next week.
        state.history = ResetHistory::new();
        let next = format::seconds(now) + 604_800.0;
        for provider in Provider::ALL {
            let windows = match provider {
                Provider::Codex => codex::windows_from_response(&json!({"rateLimits": {"primary": {"usedPercent": 0, "windowDurationMins": 10080, "resetsAt": next}}})),
                Provider::Claude => claude::windows_from_rate_limits(&json!({"seven_day": {"used_percentage": 0, "resets_at": next}})),
            };
            let before: Vec<UsageWindow> = windows.iter().map(|window| UsageWindow { resets_at: Some(format::seconds(now) - 30.0), ..window.clone() }).collect();
            state.history.record(provider, &before, now - SignedDuration::from_mins(1));
            update(&mut state, Msg::UsageRead { provider, result: Ok(windows.into()) }, now);
        }
    }
    if has("--preview-errors") {
        update(&mut state, Msg::UsageRead { provider: Provider::Codex, result: Err(UsageError::CodexClosed) }, fetched);
        state.usage[Provider::Claude] = Default::default();
        update(&mut state, Msg::UsageRead { provider: Provider::Claude, result: Err(UsageError::ClaudeNotHeard) }, fetched);
    }
    let account = || AccountInfo { email: Some("me@example.com".to_owned()), running: true };
    state.accounts = PerProvider { codex: account(), claude: account() };
    if has("--preview-update") {
        update(&mut state, Msg::ReleaseRead(Some(Release { version: "9.9.9".to_owned(), download: Some("https://example.com/Errand.zip".to_owned()) })), now);
    }
    // Someone who does not use Codex.
    state.widget.hidden_services.codex = has("--preview-hide-codex");
    // A stand-in, unless the preview asks for this computer's own nickname and character. Set before the rooms,
    // so the person's report carries the character they would send.
    state.profile = if has("--preview-profile") { Platform::current(language).profile() } else { Profile { avatar: nickname.clone(), name: nickname } };
    // The cards come before the rooms, so the person's row in a room counts what their own task header does.
    if has("--preview-tray") {
        add_sessions(&mut state, arguments, now, language);
    }
    // The sample rooms: `--preview-rooms` lists them beside the dock. With `--preview-room` the person is in the
    // late-night room and their team's, the late-night one open under the profile; `--preview-private-room`
    // opens the team's instead.
    let sample = errand_io::rooms::SampleRooms::new();
    state.rooms.sample = sample.sample();
    let in_rooms = has("--preview-room") || has("--preview-private-room");
    if has("--preview-rooms") || in_rooms {
        state.rooms.list = sample.rooms(now).unwrap_or_default();
        state.rooms.open = has("--preview-rooms");
    }
    if in_rooms {
        for room in ["late-night", "team"] {
            let mut report = rooms::presence_report(&state, state.rooms.visibility(room));
            // Without cards of their own, the person shows a few, so their row has pills too.
            if report.tasks == TaskCounts::default() {
                report.tasks = TaskCounts { new: 1, running: 1, seen: 2 };
            }
            update(&mut state, Msg::JoinRoom(room.to_owned()), now);
            // The person came in a while ago, so their own row counts up too.
            let _ = sample.sync(room, &report, now - SignedDuration::from_secs(754));
            update(&mut state, Msg::RoomRead(room.to_owned(), sample.sync(room, &report, now)), now);
        }
        // Read again, the person now counted in the rooms they are in.
        state.rooms.list = sample.rooms(now).unwrap_or_default();
        state.rooms.expanded = Some(if has("--preview-private-room") { "team" } else { "late-night" }.to_owned());
    }
    // The steps of making and joining a private room, in the bubble in place of the list.
    let team = || state.rooms.list.iter().find(|room| room.id == "team").cloned().map(|room| RoomSummary { role: None, ..room });
    let code = "K7QFM2XA9P".to_owned();
    let screen = if has("--preview-create") {
        Some(RoomsScreen::Create { sending: false, failure: None })
    } else if has("--preview-invite") {
        let invite = Invite { code: code.clone(), expires_at: now + SignedDuration::from_hours(24 * 7), uses_left: 10 };
        Some(RoomsScreen::Invite { room: "team".to_owned(), invite: Some(invite), loading: false, failure: None, copied: has("--preview-copied") })
    } else if has("--preview-members") {
        // The owner, two here now, and two who were here days ago, kept by the nickname they had.
        let someone = |id: &str, name: &str, present: bool, working: bool, days: i64| RoomMember {
            id: id.to_owned(),
            name: Some(name.to_owned()),
            avatar: Some(errand_core::avatar::Avatar::from_seed(name).code()),
            role: Role::Member,
            me: false,
            present,
            working,
            last_seen: Some(now - SignedDuration::from_hours(24 * days)),
        };
        let owner = RoomMember { role: Role::Owner, me: true, ..someone("me", &state.profile.name, true, false, 0) };
        let members = vec![owner, someone("1", "민지", true, true, 0), someone("2", "지훈", true, false, 0), someone("3", "서연", false, false, 2), someone("4", "도윤", false, false, 9)];
        Some(RoomsScreen::Members { room: "team".to_owned(), members: Some(members), loading: false, failure: None })
    } else if has("--preview-code") || has("--preview-link") {
        Some(RoomsScreen::Code { code: code.clone(), checking: false, room: team(), failure: None, from_link: has("--preview-link"), accepting: false })
    } else if has("--preview-code-error") {
        Some(RoomsScreen::Code { code: "K7QFM2XA9Q".to_owned(), checking: false, room: None, failure: Some(RoomError::InviteNotFound), from_link: false, accepting: false })
    } else {
        None
    };
    if let Some(screen) = screen {
        state.rooms.screen = screen;
    }
    state
}

fn card(key: &str, provider: Provider, title: &str, prompt: &str, asked: Timestamp, finished: Option<Timestamp>, seen: bool) -> VisibleSession {
    VisibleSession {
        row: SessionRow {
            key: key.to_owned(),
            provider,
            title: title.to_owned(),
            prompt: prompt.to_owned(),
            prompt_at: asked,
            running: finished.is_none(),
            aborted: false,
            finished_at: finished,
            result: None,
            action: None,
            folder: String::new(),
            url: String::new(),
            seen_in_app: false,
            stalled: false,
            parent: None,
            on_screen: false,
        },
        seen,
    }
}

/// The task cards the Objective-C previews stacked: one new result with a next step, running work and a seen
/// answer; or a crowd of them.
pub fn add_sessions(state: &mut AppState, arguments: &[String], now: Timestamp, language: Language) {
    let has = |flag: &str| arguments.iter().any(|argument| argument == flag);
    let ago = |seconds: i64| now - SignedDuration::from_secs(seconds);
    let say = |korean, english| words(language, korean, english);
    let mut shown = vec![
        card("claude:preview-1", Provider::Claude, say("로그인 화면 리팩터링", "Refactor the sign-in screen"), say("커밋해줘", "Commit it"), ago(1500), Some(ago(420)), false),
        card(
            "codex:preview-2",
            Provider::Codex,
            say("API 캐시 정리", "Clean up the API cache"),
            say("API 응답 캐시는 꺼줄래? 트래픽이 많지 않아서 아직은 필요 없을 것 같아", "Can you turn off the API response cache? Traffic is low, so we don't need it yet"),
            ago(260),
            None,
            false,
        ),
        card("claude:preview-3", Provider::Claude, say("배포 설정 정리", "Tidy up the deploy settings"), say("develop에 커밋하고 푸시해줘", "Commit and push to develop"), ago(5400), Some(ago(5300)), true),
    ];
    shown[0].row.result = Some(say("커밋 2건 완료 (1a2b3c4, 5d6e7f8), 푸시는 안 함", "Made 2 commits (1a2b3c4, 5d6e7f8), not pushed").to_owned());
    shown[0].row.action = Some(say("푸시할지 정해주세요", "Decide whether to push").to_owned());
    shown[0].row.folder = "web-app".to_owned();
    shown[1].row.folder = "api-server".to_owned();
    shown[2].row.result = Some(say("README 안내를 develop에 커밋하고 푸시했어요 (9f8e7d6)", "Committed and pushed the README notes to develop (9f8e7d6)").to_owned());
    shown[2].row.folder = "docs-site".to_owned();
    if has("--preview-crowded") || has("--preview-many") {
        // A crowded stack: one running request and finished ones that were already opened.
        let titles = match language {
            Language::Korean => ["develop 파이프라인 확인", "CI 캐시 설정 정리", "대시보드 차트 추가", "입력값 검증 보강", "권한 검사 리팩터링", "리뷰 의견 반영", "배포 설정 정리",
                                 "업데이트 서버 점검", "로그인 화면 리팩터링", "API 캐시 정리"],
            Language::English => ["Check the develop pipeline", "Tidy the CI cache settings", "Add a dashboard chart", "Harden input validation", "Refactor permission checks",
                                  "Apply review comments", "Tidy up the deploy settings", "Check the update server", "Refactor the sign-in screen", "Clean up the API cache"],
        };
        let prompt = say("CI 이미지를 빌드와 같은 Node 이미지로 맞춰서 다시 해줄래?", "Can you redo it with the CI image on the build's Node image?");
        let result = say("lint 이미지를 빌드와 같은 Node 이미지로 맞췄어요", "Moved the lint image to the build's Node image");
        let count = if has("--preview-many") { titles.len() } else { 5 };
        shown = (0..count)
            .map(|index| {
                let provider = if index % 2 == 1 { Provider::Claude } else { Provider::Codex };
                let step = index as i64 + 1;
                let mut session = card(&format!("preview:{index}"), provider, titles[index], prompt, ago(300 * step), Some(ago(280 * step)), index > 0);
                session.row.running = index == 0;
                session.row.result = Some(result.to_owned());
                session.row.folder = "api-server".to_owned();
                session
            })
            .collect();
    }
    if has("--preview-sub-tasks") {
        // Work the first session started: a result to look at, one running, and one already read.
        let mut sub_tasks = [
            card("claude:preview-1a", Provider::Claude, say("입력값 검증 추가", "Add input checks"), say("검증 붙여줘", "Add the checks"), ago(900), Some(ago(120)), false),
            card("claude:preview-1b", Provider::Claude, say("로그인 테스트 고치기", "Fix the sign-in tests"), say("테스트 고쳐줘", "Fix the tests"), ago(800), None, false),
            card("claude:preview-1c", Provider::Claude, say("사용 설명서 고치기", "Update the guide"), say("설명서 고쳐줘", "Update the guide"), ago(700), Some(ago(600)), true),
        ];
        for session in &mut sub_tasks {
            session.row.parent = Some("claude:preview-1".to_owned());
        }
        shown.splice(1..1, sub_tasks);
    }
    // As a scan would: a hidden service's tasks are left out.
    shown.retain(|session| state.shows(session.row.provider));
    state.sessions.shown = shown;
    if has("--preview-pending") {
        update(state, Msg::ToggleComplete("claude:preview-1".to_owned()), now);
    }
}
