//! `--preview <file.png>`: draws the screens from fixed data, without real accounts. The fixtures are the
//! ones the Objective-C build drew, with English stand-ins for the sample tasks.

use crate::platform::Platform;
use errand_core::format::{self, timestamp_from_seconds};
use errand_core::model::{AccountInfo, AppState, Msg, Profile, WidgetPreferences, update};
use errand_core::ports::RoomService;
use errand_core::reset_history::ResetHistory;
use errand_core::rooms::{self, Invite, Role, RoomError, RoomMember, RoomSummary, RoomsScreen};
use errand_core::sessions::{Question, SessionRecords, SessionRow, TaskCounts, VisibleSession};
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
    if has("--preview-low") {
        // Claude's week nearly spent: 18% left.
        let low = json!({"five_hour": {"used_percentage": 35.4, "resets_at": 1792054800}, "seven_day": {"used_percentage": 82, "resets_at": 1792368000}});
        claude_reading.windows = claude::windows_from_rate_limits(&low);
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
    let compact = ["--preview-simple", "--preview-simple-open", "--preview-badge", "--preview-badge-open"].iter().any(|flag| has(flag));
    if has("--preview-tray") || compact {
        add_sessions(&mut state, arguments, now, language);
    }
    if has("--preview-task-form") || has("--preview-task-detail") || has("--preview-personal-task") {
        use errand_core::tasks::{self, Attachment};
        let attachments = vec![Attachment {
            id: "sample.png".into(),
            name: "메신저 확인 요청.png".into(),
            bytes: 184_320,
            image: false,
        }];
        if has("--preview-task-form") {
            tasks::update(
                &mut state.tasks,
                tasks::Msg::Drop(vec![tasks::Input::Png(vec![])]),
                now,
            );
            let token = state.tasks.draft.as_ref().unwrap().token;
            tasks::update(
                &mut state.tasks,
                tasks::Msg::Imported {
                    token,
                    result: Ok(attachments),
                },
                now,
            );
            tasks::update(
                &mut state.tasks,
                tasks::Msg::Title("온보딩 문서 수정안 확인하기".into()),
                now,
            );
            let draft = state.tasks.draft.as_mut().unwrap();
            draft.memo_suggestion = Some("금요일 회의 전에 의견 보내기".into());
            draft.generating = false;
        } else {
            state.tasks.records.push(tasks::Task {
                id: "manual:preview".into(),
                title: "온보딩 문서 수정안 확인하기".into(),
                memo: "금요일 회의 전에 의견 보내기".into(),
                attachments,
                created: now,
                done: false,
            });
        }
    }
    if has("--preview-task-detail") { state.tasks.selected = Some("manual:preview".into()); }

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
    add_journal(&mut state, arguments, now, language);
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
            question: None,
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
    if has("--preview-question") {
        // Claude asked something and waits for the answer.
        let asked = say("하위 작업은 얇은 카드 버전으로 쏙 들어가게, 디자인 시안먼저 보여줘", "Tuck sub-tasks in as thin cards, and show mock-ups first");
        let mut waiting = card("claude:preview-question", Provider::Claude, say("하위 작업 카드 시안", "Sub-task card mock-ups"), asked, ago(200), None, false);
        waiting.row.folder = "errand-public".to_owned();
        let question = say("하위 작업 카드를 어떤 시안으로 구현할까요?", "Which mock-up should the sub-task cards follow?");
        waiting.row.question = Some(Question { id: "toolu_preview".to_owned(), asked_at: ago(120), questions: vec![question.to_owned()] });
        shown.insert(0, waiting);
    }
    if has("--preview-calm") {
        // Nothing calls: the new result was opened, and only the running request is left to wait on.
        shown[0].seen = true;
    }
    if has("--preview-stalled") {
        // A request that has said nothing for over twenty minutes.
        let mut quiet = card("codex:preview-stalled", Provider::Codex, say("이미지 리사이즈 스크립트", "Image resize script"), say("썸네일 만드는 스크립트 짜줘", "Write the thumbnail script"), ago(2400), None, false);
        quiet.row.stalled = true;
        shown.insert(0, quiet);
    }
    if has("--preview-no-tasks") {
        shown.clear();
    }
    // As a scan would: a hidden service's tasks are left out.
    shown.retain(|session| state.shows(session.row.provider));
    state.sessions.shown = shown;
    if has("--preview-pending") {
        update(state, Msg::ToggleComplete("claude:preview-1".to_owned()), now);
    }
}

/// The desktop window's records: sample work around today, or with `--preview-journal-real` this Mac's own.
/// `--preview-journal-week`, `-month` and `-report` pick the view, `--preview-journal-detail` opens a task, and
/// `--preview-journal-back` looks one period back.
fn add_journal(state: &mut AppState, arguments: &[String], now: Timestamp, language: Language) {
    use errand_core::journal::view::JournalTab;
    let has = |flag: &str| arguments.iter().any(|argument| argument == flag);
    state.journal.records = if has("--preview-journal-real") {
        errand_io::journal_scan::JournalScanner::new(Platform::current(language).sessions.clone()).scan(now - errand_core::model::JOURNAL_REACH)
    } else {
        sample_journal(now, language)
    };
    state.journal.scanned_at = Some(now);
    state.journal.open = true;
    state.journal.place.tab = if has("--preview-journal-week") {
        JournalTab::Week
    } else if has("--preview-journal-month") {
        JournalTab::Month
    } else if has("--preview-journal-report") {
        JournalTab::Report
    } else {
        JournalTab::Day
    };
    if has("--preview-journal-back") {
        state.journal.place.back = 1;
    }
    if has("--preview-journal-detail") {
        let today = now.to_zoned(format::local_zone()).date();
        state.journal.place.open = state
            .journal
            .records
            .iter()
            .filter(|record| record.started_at().is_some_and(|start| start.to_zoned(format::local_zone()).date() == today) && !record.commits.is_empty())
            .chain(state.journal.records.iter())
            .next()
            .map(|record| record.key.clone());
    }
}

fn sample_journal(now: Timestamp, language: Language) -> Vec<errand_core::journal::WorkRecord> {
    use errand_core::journal::{Commit, FileChange, PullRequest, WorkRecord, WorkTurn};
    let zone = format::local_zone();
    let today = now.to_zoned(zone.clone()).date();
    let at = |days: i64, hour: i8, minute: i8| -> Timestamp {
        let date = today.checked_sub(jiff::Span::new().days(days)).unwrap_or(today);
        date.at(hour, minute, 0, 0).to_zoned(zone.clone()).map(|zoned| zoned.timestamp()).unwrap_or(now)
    };
    let say = |korean: &'static str, english: &'static str| words(language, korean, english).to_owned();
    let turn = |asked: Timestamp, minutes: Option<i64>, ask: String, result: String| WorkTurn {
        asked_at: asked,
        ask,
        finished_at: minutes.map(|minutes| asked + SignedDuration::from_mins(minutes)),
        result: (!result.is_empty()).then_some(result),
        aborted: false,
    };
    let record = |key: &str, provider: Provider, project: &str, title: String, turns: Vec<WorkTurn>| WorkRecord {
        key: key.to_owned(),
        provider,
        title,
        folder: format!("/Users/me/code/{project}"),
        branch: None,
        url: Some(format!("claude://code/continue?session={key}")),
        turns,
        commits: Vec::new(),
        pull_requests: Vec::new(),
        files: Vec::new(),
    };
    let mut login = record("sample:login", Provider::Claude, "shop-web", say("로그인 화면 리팩터링", "Refactor the sign-in screen"), vec![
        turn(at(0, 9, 12), Some(13), say("로그인 화면을 훅 기반으로 바꿔줘", "Move the sign-in screen onto hooks"), say("폼 상태와 검증을 useLoginForm 훅으로 옮겼어요. 파일 4개를 고쳤어요.", "Moved form state and checks into a useLoginForm hook, across four files.")),
        turn(at(0, 9, 25), Some(9), say("에러 문구도 하나로 맞춰줘", "Make the error messages match too"), say("흩어져 있던 에러 문구 5개를 messages.ts로 모았어요.", "Gathered five scattered error messages into messages.ts.")),
        // An answer in the other language, as agents sometimes write, so the panel offers to translate it.
        turn(at(0, 9, 38), Some(2), say("커밋해줘", "Commit it"), say("Left two commits, not pushed.", "커밋 2개를 남겼어요. 푸시는 하지 않았어요.")),
    ]);
    login.branch = Some("feature/login-hook".to_owned());
    login.commits = vec![
        Commit { sha: "a1f3c2e".to_owned(), message: say("로그인 폼 상태를 useLoginForm으로 분리", "Move sign-in form state into useLoginForm"), at: at(0, 9, 39) },
        Commit { sha: "7be09d4".to_owned(), message: say("로그인 에러 문구를 한곳에 모음", "Keep sign-in error messages in one place"), at: at(0, 9, 40) },
    ];
    login.files = vec![
        FileChange { path: "/Users/me/code/shop-web/src/pages/Login.tsx".to_owned(), added: 48, removed: 112, created: false },
        FileChange { path: "/Users/me/code/shop-web/src/hooks/useLoginForm.ts".to_owned(), added: 86, removed: 0, created: true },
        FileChange { path: "/Users/me/code/shop-web/src/i18n/messages.ts".to_owned(), added: 14, removed: 3, created: false },
    ];
    let mut checks = record("sample:checks", Provider::Codex, "payments-api", say("입력값 검증 추가", "Add input checks"), vec![
        turn(at(0, 10, 5), Some(26), say("결제 요청에 금액 검증 붙여줘", "Check the amount on payment requests"), say("금액과 통화 검증, 테스트 12개를 더했어요.", "Added amount and currency checks with twelve tests.")),
    ]);
    checks.commits = vec![Commit { sha: "9c0ffee".to_owned(), message: say("결제 금액과 통화 검증", "Check payment amount and currency"), at: at(0, 10, 30) }];
    checks.pull_requests = vec![PullRequest { number: 48, url: "https://github.com/me/payments-api/pull/48".to_owned(), at: at(0, 10, 31) }];
    let mut tests = record("sample:tests", Provider::Claude, "shop-web", say("로그인 테스트 고치기", "Fix the sign-in tests"), vec![
        turn(at(0, 11, 20), Some(32), say("실패하는 로그인 테스트 고쳐줘", "Fix the failing sign-in tests"), say("만료된 토큰 목업을 고쳐 테스트 3개를 통과시켰어요.", "Fixed the expired-token mock; three tests pass again.")),
    ]);
    tests.commits = vec![Commit { sha: "3c9e1b0".to_owned(), message: say("만료 토큰 목업 고침", "Fix the expired-token mock"), at: at(0, 11, 51) }];
    let deploy = record("sample:deploy", Provider::Claude, "deploy-config", say("배포 설정 정리", "Tidy the deploy settings"), vec![
        turn(now - SignedDuration::from_mins(40), None, say("develop에 커밋하고 푸시해줘", "Commit and push to develop"), String::new()),
    ]);
    let mut records = vec![login, checks, tests, deploy];
    // Days back, start, minutes, service, project, and the title in both languages.
    type Earlier = (i64, i8, i8, i64, Provider, &'static str, &'static str, &'static str);
    let earlier: [Earlier; 12] = [
        (1, 9, 40, 23, Provider::Codex, "deploy-config", "CI 캐시 설정 정리", "Tidy the CI cache"),
        (1, 13, 5, 12, Provider::Claude, "shop-web", "사용 설명서 고치기", "Update the guide"),
        (1, 14, 20, 16, Provider::Claude, "payments-api", "영수증 PDF 글꼴", "Receipt PDF fonts"),
        (1, 16, 30, 54, Provider::Claude, "shop-web", "대시보드 차트 추가", "Add a dashboard chart"),
        (2, 11, 0, 52, Provider::Claude, "payments-api", "웹훅 서명 검증", "Verify webhook signatures"),
        (2, 15, 10, 41, Provider::Codex, "shop-web", "상품 목록 무한 스크롤", "Infinite product list"),
        (3, 10, 20, 15, Provider::Codex, "shop-web", "결제 페이지 접근성 점검", "Checkout accessibility pass"),
        (3, 11, 50, 37, Provider::Claude, "payments-api", "환불 API 응답 정리", "Tidy refund responses"),
        (3, 17, 45, 18, Provider::Claude, "shop-web", "장바구니 수량 버그", "Cart quantity bug"),
        (8, 10, 30, 30, Provider::Claude, "shop-web", "이미지 지연 로딩", "Lazy-load images"),
        (9, 13, 15, 44, Provider::Codex, "payments-api", "정산 배치 재시도", "Retry settlement batches"),
        (16, 16, 40, 21, Provider::Claude, "deploy-config", "스테이징 도메인 연결", "Connect the staging domain"),
    ];
    for (index, (days, hour, minute, minutes, provider, project, korean, english)) in earlier.into_iter().enumerate() {
        let title = words(language, korean, english).to_owned();
        let mut item = record(&format!("sample:earlier-{index}"), provider, project, title.clone(), vec![turn(at(days, hour, minute), Some(minutes), title, say("마쳤어요.", "Done."))]);
        item.commits = vec![Commit { sha: format!("{:07x}", 0xabc000 + index), message: words(language, korean, english).to_owned(), at: at(days, hour, minute) + SignedDuration::from_mins(minutes) }];
        records.push(item);
    }
    records
}
