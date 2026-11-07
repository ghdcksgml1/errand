//! Ported from the rules in Tests/FloatingPetsTests.m and Tests/AccountRemovalTests.m that need no window:
//! what the batteries, header, cards and menu say, and how checking off, hovering and refreshing behave.
//! With B gone, a third account cannot be represented at all, so the removal checks become checks that
//! exactly Codex and Claude appear.

use errand_core::avatar::{Avatar, Part};
use errand_core::format::timestamp_from_seconds;
use errand_core::model::{Alert, AppState, Attention, COMPLETION_DELAY, Effect, Msg, Notice, OpenError, Profile, WidgetPreferences, update};
use errand_core::rooms::Visibility;
use errand_core::reset_history::ResetHistory;
use errand_core::sessions::text::PICTURE;
use errand_core::sessions::{Question, SessionRecords, SessionRow, SessionState};
use errand_core::usage::{Mood, Reading, Span, UsageError, UsageWindow, WindowName};
use errand_core::view::{self, MenuAction, MenuEntry, ViewContext};
use errand_core::{Language, Provider, TimeZone, Timestamp, theme};
use jiff::SignedDuration;
use jiff::tz::offset;

const NOW: f64 = 1_792_044_000.0;

fn at(seconds: f64) -> Timestamp {
    timestamp_from_seconds(seconds).unwrap()
}
fn context() -> ViewContext {
    ViewContext { now: at(NOW), zone: TimeZone::fixed(offset(9)), computer: "Mac", language: Language::Korean }
}
fn english() -> ViewContext {
    ViewContext { language: Language::English, ..context() }
}
fn state() -> AppState {
    AppState::new(ResetHistory::new(), SessionRecords::new(at(NOW)), WidgetPreferences::default(), "한결".to_owned())
}
const WEEK: i64 = 10080;
fn window(identifier: &str, minutes: i64, remaining: Option<i64>, resets_at: Option<f64>) -> UsageWindow {
    UsageWindow { identifier: identifier.to_owned(), name: WindowName::span(Span::Minutes(minutes)), remaining_percent: remaining, resets_at }
}
/// 2026-10-19T00:00:00Z, 09:00 in Seoul.
const WEEKLY_RESET: f64 = 1_792_368_000.0;

fn read(state: &mut AppState, provider: Provider, result: Result<Vec<UsageWindow>, UsageError>) -> Vec<Effect> {
    update(state, Msg::UsageRead { provider, result: result.map(Reading::from) }, at(NOW))
}

#[test]
fn a_refresh_reads_both_services_once_until_both_answer() {
    let mut state = state();
    assert_eq!(
        update(&mut state, Msg::RefreshUsage, at(NOW)),
        [Effect::ReadProfile, Effect::FetchUsage(Provider::Codex), Effect::FetchUsage(Provider::Claude)],
        "automatic polling reads Codex and Claude, nothing else"
    );
    assert!(state.loading());
    assert!(update(&mut state, Msg::RefreshUsage, at(NOW)).is_empty(), "no second read while one is in flight");
    let effects = read(&mut state, Provider::Codex, Ok(vec![window("codex/primary", WEEK, Some(72), Some(WEEKLY_RESET))]));
    assert_eq!(effects, [Effect::SaveResetHistory], "a first deadline is recorded as the reset baseline");
    assert!(state.loading(), "still waiting for Claude");
    read(&mut state, Provider::Claude, Err(UsageError::CodexClosed));
    assert!(!state.loading());
    assert_eq!(state.usage[Provider::Codex].fetched_at, Some(at(NOW)));
}

#[test]
fn batteries_say_what_is_left_and_when_it_refills() {
    let mut state = state();
    read(&mut state, Provider::Codex, Ok(vec![window("codex/primary", WEEK, Some(72), Some(WEEKLY_RESET))]));
    let codex = view::battery(&state, Provider::Codex, &context());
    assert_eq!((codex.percent.as_str(), codex.detail.as_str()), ("72%", "주간 · 10/19 09:00 초기화"));
    assert_eq!((codex.level, codex.color, codex.low), (Some(0.72), theme::HEALTHY, false));
    assert!(codex.tooltip.starts_with("Codex · 주간 72% 남음 · 다음 10/19 09:00 초기화 · Mac 시간대 기준"));

    read(&mut state, Provider::Codex, Err(UsageError::CodexNoAnswer("account/rateLimits/read".to_owned())));
    let stale = view::battery(&state, Provider::Codex, &context());
    assert_eq!(stale.detail, "이전 값 · 10/19 09:00 초기화", "a failed read keeps the previous value, marked as such");
    assert_eq!((stale.color, stale.detail_color, stale.percent_color), (theme::UNKNOWN, theme::WARNING, theme::MUTED));

    read(&mut state, Provider::Claude, Err(UsageError::ClaudeNotHeard));
    let failed = view::battery(&state, Provider::Claude, &context());
    assert_eq!((failed.percent.as_str(), failed.detail.as_str(), failed.level), ("확인 불가", "조회 실패 · 초기화 확인 불가", None));

    read(&mut state, Provider::Claude, Ok(vec![window("five_hour", 300, Some(64), Some(1792054800.0)), window("seven_day", WEEK, Some(12), Some(WEEKLY_RESET))]));
    let low = view::battery(&state, Provider::Claude, &context());
    assert_eq!((low.percent.as_str(), low.detail.as_str()), ("12%", "주간 · 10/19 09:00 초기화"), "the battery follows the limit that runs out first");
    assert!(low.low && low.percent_color == theme::LOW && low.color == theme::LOW && low.accessibility_value == 12);

    read(&mut state, Provider::Claude, Ok(vec![window("five_hour", 300, Some(64), None), window("weekly", WEEK, None, None)]));
    let partial = view::battery(&state, Provider::Claude, &context());
    assert_eq!((partial.percent.as_str(), partial.detail.as_str()), ("64% ?", "5시간 · 초기화 확인 불가"));

    let mut waiting = self::state();
    update(&mut waiting, Msg::RefreshUsage, at(NOW));
    let loading = view::battery(&waiting, Provider::Codex, &context());
    assert_eq!((loading.percent.as_str(), loading.detail.as_str()), ("확인 중", "초기화 확인 중"));
}

#[test]
fn only_a_press_of_refresh_asks_claude_code() {
    let read = [Effect::ReadProfile, Effect::FetchUsage(Provider::Codex), Effect::FetchUsage(Provider::Claude)];
    assert_eq!(update(&mut state(), Msg::RefreshUsage, at(NOW)), read, "the timer and the launch only read what is on record");
    let mut pressed = state();
    assert_eq!(update(&mut pressed, Msg::AskUsage, at(NOW)), [Effect::ReadProfile, Effect::FetchUsage(Provider::Codex), Effect::AskClaudeCode]);
    assert!(pressed.loading(), "until Claude Code answers");
    assert!(update(&mut pressed, Msg::AskUsage, at(NOW)).is_empty(), "no second question while one is in flight");
    read_all(&mut pressed);
    assert!(!pressed.loading());

    let mut without_claude = state();
    without_claude.widget.hidden_services.claude = true;
    assert_eq!(update(&mut without_claude, Msg::AskUsage, at(NOW)), [Effect::ReadProfile, Effect::FetchUsage(Provider::Codex)], "a hidden service is not asked");
}

fn read_all(state: &mut AppState) {
    for provider in Provider::ALL {
        read(state, provider, Ok(Vec::new()));
    }
}

#[test]
fn claudes_card_says_when_claude_code_spoke_and_not_when_errand_looked() {
    let mut state = state();
    let windows = vec![window("seven_day", WEEK, Some(42), Some(WEEKLY_RESET))];
    update(&mut state, Msg::UsageRead { provider: Provider::Claude, result: Ok(Reading { windows, reported_at: Some(at(NOW - 3600.0)) }) }, at(NOW));
    read(&mut state, Provider::Codex, Ok(vec![window("codex/primary", WEEK, Some(72), Some(WEEKLY_RESET))]));
    let details = view::details(&state, &context());
    assert_eq!(details.cards[0].data_status, "최신 사용량 · 조회 10/15 15:00");
    assert_eq!(details.cards[1].data_status, "Claude Code 기준 10/15 14:00", "reading the record just now says nothing about how fresh it is");
    assert_eq!(view::details(&state, &english()).cards[1].data_status, "From Claude Code, 10/15 14:00");
    read(&mut state, Provider::Claude, Err(UsageError::ClaudeNotHeard));
    assert_eq!(view::details(&state, &context()).cards[1].data_status, "갱신 실패 · 이전 값 · 조회 10/15 15:00", "a failed read says so first");
}

#[test]
fn an_empty_claude_card_says_where_its_usage_comes_from() {
    let mut state = state();
    read(&mut state, Provider::Codex, Ok(Vec::new()));
    read(&mut state, Provider::Claude, Ok(Vec::new()));
    let details = view::details(&state, &context());
    assert_eq!(details.cards[0].empty.as_ref().unwrap().1, "해당 계정으로 로그인한 뒤\n다시 확인해주세요.");
    assert_eq!(details.cards[1].empty.as_ref().unwrap().1, "새로고침을 누르거나 터미널에서\nClaude Code를 쓰면 표시돼요.", "signing in again would not help");
}

#[test]
fn a_rollover_reads_as_a_fresh_refill_for_a_minute() {
    let mut state = state();
    update(&mut state, Msg::UsageRead { provider: Provider::Codex, result: Ok(vec![window("codex/primary", WEEK, Some(3), Some(NOW - 30.0))].into()) }, at(NOW - 60.0));
    update(&mut state, Msg::UsageRead { provider: Provider::Codex, result: Ok(vec![window("codex/primary", WEEK, Some(100), Some(NOW + 604_800.0))].into()) }, at(NOW));
    assert_eq!(state.mood(Provider::Codex, at(NOW + 10.0)), Mood::Reset);
    assert_eq!(state.mood(Provider::Codex, at(NOW + 61.0)), Mood::Healthy);
    let card = view::account_card(&state, Provider::Codex, &ViewContext { now: at(NOW + 10.0), ..context() });
    assert!(card.quotas[0].last_reset.starts_with("초기화 감지   "), "{:?}", card.quotas[0]);
    assert_eq!(view::dashboard(&state, &ViewContext { now: at(NOW + 10.0), ..context() }).moods[0], "새 한도가 열렸어요");
}

fn row(key: &str, provider: Provider, prompt_ago: f64, finished_ago: Option<f64>) -> SessionRow {
    SessionRow {
        key: key.to_owned(),
        provider,
        title: format!("{key} 작업"),
        prompt: "README 정리하고 커밋해줘".to_owned(),
        prompt_at: at(NOW - prompt_ago),
        running: finished_ago.is_none(),
        aborted: false,
        finished_at: finished_ago.map(|ago| at(NOW - ago)),
        result: finished_ago.map(|_| "커밋했어요".to_owned()),
        action: None,
        folder: "app".to_owned(),
        url: format!("app://{key}"),
        seen_in_app: false,
        stalled: false,
        parent: None,
        on_screen: false,
        question: None,
    }
}

fn keys(state: &AppState) -> Vec<&str> {
    state.sessions.shown.iter().map(|session| session.row.key.as_str()).collect()
}

fn scanned(state: &mut AppState, rows: Vec<SessionRow>) {
    assert!(matches!(update(state, Msg::ScanSessions, at(NOW)).as_slice(), [Effect::ScanSessions(_)]));
    update(state, Msg::SessionsScanned(rows), at(NOW));
}

#[test]
fn the_header_counts_add_up_to_the_stack() {
    let mut state = state();
    let mut seen = row("codex:seen", Provider::Codex, 400.0, Some(390.0));
    seen.seen_in_app = true;
    scanned(&mut state, vec![row("claude:new", Provider::Claude, 100.0, Some(90.0)), row("codex:running", Provider::Codex, 200.0, None), seen]);
    let header = view::dock(&state, &context()).header.expect("tasks put a header on the dock");
    assert_eq!(header.title, "내가 시킨 작업 3");
    assert_eq!(header.pills.iter().map(|pill| pill.words.as_str()).collect::<Vec<_>>(), ["확인할 것 1개", "진행 중 1개", "확인함 1개"]);
    assert!(header.pills[1].spinning && header.pills[1].color == theme::RUNNING);
    assert_eq!(header.accessibility_label, "내가 시킨 작업 3개, 확인할 것 1개, 진행 중 1개, 확인함 1개. 접기");
    update(&mut state, Msg::ToggleComplete("claude:new".to_owned()), at(NOW));
    let header = view::dock(&state, &context()).header.unwrap();
    assert_eq!(header.title, "내가 시킨 작업 2", "a checked card no longer counts");
    assert_eq!(update(&mut state, Msg::ToggleTray, at(NOW)), [Effect::SaveWidgetPreferences]);
    let header = view::dock(&state, &context()).header.unwrap();
    assert!(header.collapsed && header.toggle == "펼치기" && !state.tray_visible());
    assert!(view::dock(&self::state(), &context()).header.is_none(), "without tasks the dock shows only the batteries");
}

#[test]
fn a_checked_card_waits_five_seconds_and_can_be_taken_back() {
    let mut state = state();
    scanned(&mut state, vec![row("claude:a", Provider::Claude, 100.0, Some(90.0)), row("codex:b", Provider::Codex, 200.0, Some(190.0))]);
    let effects = update(&mut state, Msg::ToggleComplete("claude:a".to_owned()), at(NOW));
    assert_eq!(effects, [Effect::ScheduleCompletion { key: "claude:a".to_owned(), after: COMPLETION_DELAY }]);
    assert_eq!(COMPLETION_DELAY, SignedDuration::from_secs(5));
    let card = &view::tray(&state, &ViewContext { now: at(NOW + 2.0), ..context() }, 560.0).cards[0];
    assert_eq!((card.state, card.chip), (SessionState::Done, "완료"));
    assert!((card.countdown.unwrap().fraction - 0.6).abs() < 1e-9, "the countdown drains with the wait");

    assert!(update(&mut state, Msg::ToggleComplete("claude:a".to_owned()), at(NOW + 1.0)).is_empty(), "되돌리기 takes it back");
    assert!(update(&mut state, Msg::CompletionDue("claude:a".to_owned()), at(NOW + 5.0)).is_empty(), "the old timer finds nothing to clear");

    update(&mut state, Msg::ToggleComplete("claude:a".to_owned()), at(NOW + 2.0));
    assert!(update(&mut state, Msg::CompletionDue("claude:a".to_owned()), at(NOW + 5.0)).is_empty(), "an earlier check's timer cannot clear a later check early");
    assert_eq!(update(&mut state, Msg::CompletionDue("claude:a".to_owned()), at(NOW + 7.0)), [Effect::FadeCard("claude:a".to_owned())]);
    assert_eq!(update(&mut state, Msg::CardFaded("claude:a".to_owned()), at(NOW + 7.2)), [Effect::SaveSessionRecords]);
    assert_eq!(keys(&state), ["codex:b"], "after the wait the card is completed and gone");
    assert!(update(&mut state, Msg::CardFaded("codex:b".to_owned()), at(NOW + 8.0)).is_empty(), "a fade without a check completes nothing");
}

#[test]
fn new_cards_wait_while_the_pointer_rests_on_the_stack() {
    let mut state = state();
    scanned(&mut state, vec![row("codex:t-1", Provider::Codex, 300.0, Some(200.0)), row("codex:t-2", Provider::Codex, 600.0, Some(500.0))]);
    update(&mut state, Msg::PointerOnCards(true), at(NOW));
    let mut changed = row("codex:t-1", Provider::Codex, 300.0, Some(200.0));
    changed.result = Some("로그인 고쳤어요".to_owned());
    scanned(&mut state, vec![row("codex:t-new", Provider::Codex, 10.0, None), changed, row("codex:t-2", Provider::Codex, 600.0, Some(500.0))]);
    assert_eq!(keys(&state), ["codex:t-1", "codex:t-2"], "nothing slides in under the pointer");
    assert_eq!(state.sessions.shown[0].row.result.as_deref(), Some("로그인 고쳤어요"), "cards already shown update in place");
    update(&mut state, Msg::PointerOnCards(false), at(NOW));
    assert_eq!(keys(&state), ["codex:t-new", "codex:t-1", "codex:t-2"], "the new card appears once the pointer leaves");
}

#[test]
fn opening_a_card_marks_it_seen_without_moving_it() {
    let mut state = state();
    scanned(&mut state, vec![row("claude:local_1", Provider::Claude, 100.0, Some(90.0)), row("codex:t-1", Provider::Codex, 200.0, Some(190.0))]);
    let effects = update(&mut state, Msg::OpenSession("claude:local_1".to_owned()), at(NOW));
    assert_eq!(effects, [Effect::SaveSessionRecords, Effect::OpenUrl("app://claude:local_1".to_owned())]);
    assert_eq!(keys(&state), ["claude:local_1", "codex:t-1"]);
    assert_eq!(SessionState::of(&state.sessions.shown[0]), SessionState::Seen);
    assert!(update(&mut state, Msg::OpenSession("missing".to_owned()), at(NOW)).is_empty());
}

#[test]
fn cards_say_what_was_asked_and_what_came_back() {
    let mut state = state();
    let mut finished = row("claude:local_1", Provider::Claude, 1500.0, Some(420.0));
    finished.action = Some("푸시 여부 결정".to_owned());
    let mut running = row("codex:t-2", Provider::Codex, 260.0, None);
    running.prompt = String::new();
    scanned(&mut state, vec![finished, running]);
    let tray = view::tray(&state, &context(), 560.0);
    let (first, second) = (&tray.cards[0], &tray.cards[1]);
    assert_eq!(first.key, "codex:t-2", "newest request first");
    assert_eq!((first.prompt.as_str(), first.prompt_missing), ("요청 내용을 읽지 못했어요", true));
    assert_eq!(first.detail.as_ref().unwrap().text, "작업 중 · 4분째");
    assert_eq!((first.when.as_str(), first.chip), ("4분 전 요청", "진행 중"));
    assert_eq!(second.prompt, "“README 정리하고 커밋해줘”");
    assert_eq!((second.action.as_deref(), second.height), (Some("푸시 여부 결정"), 136.0), "a next step adds a line");
    assert_eq!(second.detail.as_ref().unwrap().tooltip, "커밋했어요\n할 일: 푸시 여부 결정");
    assert_eq!(second.when, "7분 전 응답");
    assert_eq!(second.accessibility_label, "Claude, claude:local_1 작업, 새 결과, 요청: README 정리하고 커밋해줘, 커밋했어요");

    let crowded: Vec<SessionRow> = (0..10).map(|index| row(&format!("codex:{index}"), Provider::Codex, 300.0 * (index + 1) as f64, Some(280.0))).collect();
    scanned(&mut state, crowded);
    let tray = view::tray(&state, &context(), 560.0);
    assert!(tray.fold.more_bar && tray.cards.iter().all(|card| card.compact), "many sessions fold and leave the rest for the more bar");
    assert!(tray.cards[9].tooltip.ends_with("· 클릭해서 열기") && tray.cards[9].detail.is_none());
}

#[test]
fn the_menu_and_details_list_codex_and_claude_only() {
    let mut state = state();
    state.accounts.claude.email = Some("person@example.com".to_owned());
    state.accounts.codex.running = true;
    let entries = view::menu(&state, true, &context());
    let titles: Vec<&str> = entries
        .iter()
        .filter_map(|entry| match entry {
            MenuEntry::Item(item) => Some(item.title.as_str()),
            MenuEntry::Separator => None,
        })
        .collect();
    assert_eq!(
        titles,
        [
            "Codex 열기",
            "사용량 조회 중…",
            "Claude 열기 · person@example.com",
            "사용량 조회 중…",
            "사용량 새로고침",
            "계정 창 열기",
            "작업방",
            "플로팅 위젯 숨기기",
            "플로팅 위젯 위치 초기화",
            "Codex 표시",
            "Claude 표시",
            "Errand 종료"
        ]
    );
    let launches: Vec<MenuAction> = entries
        .iter()
        .filter_map(|entry| match entry {
            MenuEntry::Item(item) => item.action.filter(|action| matches!(action, MenuAction::OpenApp(_))),
            MenuEntry::Separator => None,
        })
        .collect();
    assert_eq!(launches, [MenuAction::OpenApp(Provider::Codex), MenuAction::OpenApp(Provider::Claude)]);
    let details = view::details(&state, &context());
    assert_eq!(details.cards.iter().map(|card| card.provider).collect::<Vec<_>>(), Provider::ALL);
    assert_eq!((details.cards[0].status.as_str(), details.cards[0].open_title), ("Codex 로그인 필요 · 실행 중", "창 보기"));
    assert_eq!(details.cards[1].empty.as_ref().unwrap().0, "사용량 정보가 없어요");
    assert_eq!(view::status_icon(&state, Language::Korean).tooltip, "Codex 확인 중 · Claude 확인 중");
}

#[test]
fn opening_an_app_reports_back_in_the_window() {
    let mut state = state();
    assert_eq!(update(&mut state, Msg::OpenApp(Provider::Claude), at(NOW)), [Effect::OpenApp(Provider::Claude)]);
    assert!(state.launching && state.notice == Notice::Opening(Provider::Claude));
    assert_eq!(view::dashboard(&state, &context()).message.text, "Claude 창을 여는 중…");
    assert!(update(&mut state, Msg::OpenApp(Provider::Codex), at(NOW)).is_empty(), "one launch at a time");
    update(&mut state, Msg::AppOpened { provider: Provider::Claude, result: Ok(()) }, at(NOW));
    assert_eq!(view::dashboard(&state, &context()).message.text, "Claude 창을 열었습니다.");
    update(&mut state, Msg::OpenApp(Provider::Codex), at(NOW));
    let missing = OpenError::NotInstalled(Provider::Codex);
    let effects = update(&mut state, Msg::AppOpened { provider: Provider::Codex, result: Err(missing.clone()) }, at(NOW));
    assert_eq!(effects[0], Effect::ShowAlert(Alert::OpenFailed(missing.clone())));
    assert_eq!(Language::Korean.alert(&Alert::OpenFailed(missing)).0, "계정 창을 열지 못했습니다");
    let message = view::dashboard(&state, &context()).message;
    assert_eq!((message.text.as_str(), message.tone), ("Applications 폴더에 Codex 또는 ChatGPT 앱을 설치하세요.", view::Tone::Error));
    assert!(!state.launching);
}

#[test]
fn hiding_the_widget_closes_its_details_and_is_remembered() {
    let mut state = state();
    update(&mut state, Msg::ToggleDetails, at(NOW));
    assert!(state.details_open);
    assert_eq!(update(&mut state, Msg::SetWidgetHidden(true), at(NOW)), [Effect::SaveWidgetPreferences]);
    assert!(state.widget.hidden && !state.details_open);
}

#[test]
fn the_profile_shows_the_character_its_seed_draws() {
    let mut state = state();
    state.profile.avatar = "8C2B4E0A-1F3D-4E5A-9B6C-7D8E9F0A1B2C".to_owned();
    let profile = view::dock(&state, &context()).profile;
    assert_eq!(profile.avatar, Avatar::from_seed("8C2B4E0A-1F3D-4E5A-9B6C-7D8E9F0A1B2C"));
    assert_eq!(profile.accessibility_label, "프로필 한결");
}

#[test]
fn the_shuffle_asks_for_a_whole_new_character() {
    let mut state = state();
    let before = state.profile.clone();
    assert_eq!(update(&mut state, Msg::ShuffleAvatar, at(NOW)), [Effect::NewAvatarSeed]);
    assert_eq!(state.profile, before, "the new character shows once its seed comes back with the profile");
}

#[test]
fn the_character_editor_and_the_details_take_turns() {
    let mut state = state();
    update(&mut state, Msg::ToggleDetails, at(NOW));
    update(&mut state, Msg::ToggleAvatarEditor, at(NOW));
    assert!(state.avatar_editor_open && !state.details_open, "the editor opens and the details make way");
    update(&mut state, Msg::ToggleDetails, at(NOW));
    assert!(state.details_open && !state.avatar_editor_open, "and the other way round");
    update(&mut state, Msg::ToggleAvatarEditor, at(NOW));
    update(&mut state, Msg::SetWidgetHidden(true), at(NOW));
    assert!(!state.avatar_editor_open, "hiding the widget closes the editor");
}

#[test]
fn a_part_picked_in_the_editor_is_kept_and_shown() {
    let mut state = state();
    let picked = Avatar::from_seed("한결").step(Part::Shape, 1);
    assert_eq!(update(&mut state, Msg::PickAvatar(picked), at(NOW)), [Effect::SaveAvatar(picked.code())]);
    assert_eq!(state.profile.avatar, picked.code());
    assert_eq!(view::dock(&state, &context()).profile.avatar, picked);
    let editor = view::avatar_editor(&state, Language::Korean);
    let body = &editor.parts[0];
    assert_eq!((body.label, body.choice, body.next_label.as_str()), ("몸", "세모", "몸 다음"));
    assert_eq!((body.previous, body.next), (picked.step(Part::Shape, -1), picked.step(Part::Shape, 1)));
    assert_eq!(editor.clothes.len(), 12);
    assert_eq!(editor.clothes.iter().filter(|clothes| clothes.chosen).map(|clothes| clothes.colour).collect::<Vec<_>>(), [picked.clothes]);
    assert!(editor.clothes.iter().all(|clothes| clothes.avatar == Avatar { clothes: clothes.colour, ..picked }));
    assert_eq!(view::avatar_editor(&state, Language::English).parts[0].next_label, "Next body");
}

#[test]
fn a_nickname_typed_in_the_editor_is_kept_and_an_empty_one_brings_back_the_usual_name() {
    let mut state = state();
    state.profile = Profile { name: "한결".to_owned(), avatar: "한결".to_owned() };
    assert_eq!(view::avatar_editor(&state, Language::Korean).nickname, "한결", "the editor shows the name the dock does");
    assert_eq!(
        update(&mut state, Msg::RenameProfile("  야근\u{7}요정 다람쥐 아저씨 김한결입니다요오 ".to_owned()), at(NOW)),
        [Effect::SaveNickname(Some("야근요정 다람쥐 아저씨 김한결입니다요".to_owned())), Effect::ReadProfile],
        "kept as a private room would hear it: no control characters or outer spaces, twenty at most"
    );
    assert_eq!(state.profile.name, "한결", "the dock shows it once the profile is read again with it");
    assert!(update(&mut state, Msg::RenameProfile(" 한결 ".to_owned()), at(NOW)).is_empty(), "the same name again keeps nothing");
    assert_eq!(update(&mut state, Msg::RenameProfile("  ".to_owned()), at(NOW)), [Effect::SaveNickname(None), Effect::ReadProfile]);
}

#[test]
fn a_new_nickname_reaches_the_private_rooms_at_once() {
    let mut state = state();
    state.profile = Profile { name: "한결".to_owned(), avatar: "한결".to_owned() };
    update(&mut state, Msg::RoomsRemembered(vec![("late-night".to_owned(), Visibility::Public), ("team".to_owned(), Visibility::Private)]), at(NOW));
    let renamed = update(&mut state, Msg::ProfileRead(Profile { name: "다람쥐".to_owned(), avatar: "한결".to_owned() }), at(NOW + 1.0));
    assert_eq!(renamed, [Effect::SyncRoom { room: "team".to_owned(), visibility: Visibility::Private }], "a public room never hears the name");
    let same = state.profile.clone();
    assert!(update(&mut state, Msg::ProfileRead(same), at(NOW + 2.0)).is_empty(), "the same profile read again sends nothing");
}

#[test]
fn english_reads_the_same_state_in_english() {
    let mut state = state();
    read(&mut state, Provider::Codex, Ok(vec![window("codex/primary", WEEK, Some(72), Some(WEEKLY_RESET))]));
    let codex = view::battery(&state, Provider::Codex, &english());
    assert_eq!((codex.percent.as_str(), codex.detail.as_str()), ("72%", "Weekly · resets 10/19 09:00"));
    assert!(codex.tooltip.starts_with("Codex · Weekly 72% left · resets 10/19 09:00 · Mac time zone"), "{}", codex.tooltip);
    read(&mut state, Provider::Claude, Err(UsageError::ClaudeNotHeard));
    let failed = view::battery(&state, Provider::Claude, &english());
    assert_eq!((failed.percent.as_str(), failed.detail.as_str()), ("Unknown", "Check failed · reset unknown"));
    assert!(failed.tooltip.ends_with("Press refresh to get it from Claude Code.\nIf you use it in Terminal, python3 scripts/connect_claude.py\nkeeps it up to date."));
    let details = view::details(&state, &english());
    assert_eq!(details.cards[0].quotas[0].next_reset, "Next reset   10/19 09:00");
    assert_eq!(details.cards[0].quotas[0].last_reset, "Reset seen   Not recorded yet");
    assert_eq!(details.cards[1].empty.as_ref().unwrap().0, "Couldn't get usage");
    assert_eq!(details.cards[1].status, "Sign in to Claude Code");

    let mut seen = row("codex:seen", Provider::Codex, 400.0, Some(390.0));
    seen.seen_in_app = true;
    let mut untitled = row("claude:untitled", Provider::Claude, 100.0, Some(90.0));
    untitled.title = String::new();
    untitled.prompt = format!("이 화면 봐줘 {PICTURE}");
    scanned(&mut state, vec![untitled, row("codex:running", Provider::Codex, 200.0, None), seen]);
    let header = view::dock(&state, &english()).header.unwrap();
    assert_eq!(header.title, "My tasks 3");
    assert_eq!(header.pills.iter().map(|pill| pill.words.as_str()).collect::<Vec<_>>(), ["1 to check", "1 running", "1 seen"]);
    assert_eq!((header.toggle, header.accessibility_label.as_str()), ("Hide", "My tasks, 3, 1 to check, 1 running, 1 seen. Hide"));
    let tray = view::tray(&state, &english(), 560.0);
    assert_eq!((tray.cards[0].title.as_str(), tray.cards[0].prompt.as_str()), ("Claude session", "“이 화면 봐줘 [Image]”"));
    assert_eq!((tray.cards[0].chip, tray.cards[0].when.as_str()), ("New", "answered 1 min ago"));
    assert_eq!((tray.cards[1].detail.as_ref().unwrap().text.as_str(), tray.cards[1].when.as_str()), ("Working · 3 min", "asked 3 min ago"));
    let korean = view::tray(&state, &context(), 560.0);
    assert_eq!((korean.cards[0].title.as_str(), korean.cards[0].prompt.as_str()), ("Claude 세션", "“이 화면 봐줘 [이미지]”"));

    let titles: Vec<String> = view::menu(&state, false, &english())
        .into_iter()
        .filter_map(|entry| match entry {
            MenuEntry::Item(item) => Some(item.title),
            MenuEntry::Separator => None,
        })
        .collect();
    assert_eq!(
        titles,
        [
            "Open Codex",
            "Weekly 72% left",
            "Open Claude",
            "Couldn't check usage",
            "Refresh Usage",
            "Open Usage Window",
            "Work Rooms",
            "Show Floating Widget",
            "Reset Widget Position",
            "Show Codex",
            "Show Claude",
            "Quit Errand"
        ]
    );
    update(&mut state, Msg::OpenApp(Provider::Claude), at(NOW));
    assert_eq!(view::dashboard(&state, &english()).message.text, "Opening Claude…");
    assert_eq!(view::status_icon(&state, Language::English).tooltip, "Codex 72% left · Claude checking");
}

#[test]
fn the_language_follows_the_first_preference_errand_speaks() {
    assert_eq!(Language::preferred(["ko-KR", "en-US"]), Language::Korean);
    assert_eq!(Language::preferred(["ja-JP", "en_GB", "ko"]), Language::English);
    assert_eq!(Language::preferred(["fr-FR"]), Language::English, "English when none is spoken");
    assert_eq!(Language::preferred(Vec::<String>::new()), Language::English);
    assert_eq!(Language::from_tag("KO"), Some(Language::Korean));
    assert_eq!(Language::from_tag("korean"), None);
    assert_eq!(Language::ALL.map(Language::tag), ["ko", "en"]);
}

#[test]
fn a_hidden_service_drops_out_everywhere_but_one_always_stays() {
    let mut state = state();
    assert_eq!(update(&mut state, Msg::ToggleService(Provider::Codex), at(NOW)), [Effect::SaveWidgetPreferences]);
    assert!(!state.shows(Provider::Codex) && state.widget.hidden_services.codex);
    assert_eq!(
        update(&mut state, Msg::RefreshUsage, at(NOW)),
        [Effect::ReadProfile, Effect::FetchUsage(Provider::Claude)],
        "a hidden service's usage is not read"
    );
    read(&mut state, Provider::Claude, Ok(vec![window("seven_day", WEEK, Some(42), Some(WEEKLY_RESET))]));
    assert!(update(&mut state, Msg::ToggleService(Provider::Claude), at(NOW)).is_empty(), "the last service shown stays");
    assert!(state.shows(Provider::Claude));

    scanned(&mut state, vec![row("codex:t-1", Provider::Codex, 100.0, None), row("claude:local_1", Provider::Claude, 200.0, Some(150.0))]);
    assert_eq!(keys(&state), ["claude:local_1"], "its tasks are left out");
    let dock = view::dock(&state, &context());
    assert_eq!(dock.batteries.iter().map(|battery| battery.provider).collect::<Vec<_>>(), [Provider::Claude]);
    assert_eq!(view::details(&state, &context()).cards.len(), 1);
    let dashboard = view::dashboard(&state, &context());
    assert_eq!((dashboard.cards.len(), dashboard.moods.len(), dashboard.title.as_str()), (1, 1, "Claude 사용량"));
    assert_eq!(dashboard.message.text, "Claude의 사용량을 확인합니다.");
    assert_eq!(view::status_icon(&state, Language::Korean).levels, [Some(42)]);
    let items: Vec<(String, bool, bool)> = view::menu(&state, true, &context())
        .into_iter()
        .filter_map(|entry| match entry {
            MenuEntry::Item(item) => Some((item.title, item.checked, item.enabled)),
            MenuEntry::Separator => None,
        })
        .collect();
    assert!(!items.iter().any(|(title, _, _)| title.starts_with("Codex 열기")), "{items:?}");
    assert!(items.contains(&("Codex 표시".to_owned(), false, true)), "{items:?}");
    assert!(items.contains(&("Claude 표시".to_owned(), true, false)), "the last service's switch is off limits: {items:?}");

    assert_eq!(
        update(&mut state, Msg::ToggleService(Provider::Codex), at(NOW)),
        [Effect::SaveWidgetPreferences, Effect::FetchUsage(Provider::Codex)],
        "shown again, it is read at once"
    );
    assert_eq!(keys(&state), ["codex:t-1", "claude:local_1"], "and its tasks come back");
    assert_eq!(view::dashboard(&state, &english()).title, "Codex and Claude usage");
}

fn on_screen(prompt_ago: f64, finished_ago: Option<f64>) -> SessionRow {
    SessionRow { on_screen: true, ..row("claude:local_1", Provider::Claude, prompt_ago, finished_ago) }
}

fn read_session(state: &mut AppState, front: Option<Provider>, idle_seconds: i64) -> Vec<Effect> {
    update(state, Msg::AttentionRead(Attention { front, idle: SignedDuration::from_secs(idle_seconds) }), at(NOW))
}

#[test]
fn an_answer_in_the_session_on_screen_is_read_once_the_person_acts_with_the_app_in_front() {
    let mut state = state();
    let other = row("claude:local_2", Provider::Claude, 90.0, Some(50.0));
    let scan = vec![on_screen(100.0, Some(60.0)), other.clone()];
    assert_eq!(update(&mut state, Msg::SessionsScanned(scan.clone()), at(NOW)), [Effect::ReadAttention], "an unread answer on screen asks where the person is");

    assert!(read_session(&mut state, Some(Provider::Codex), 5).is_empty(), "another app is in front");
    assert!(read_session(&mut state, None, 5).is_empty());
    assert!(read_session(&mut state, Some(Provider::Claude), 61).is_empty(), "nothing touched since the answer: the person may have walked away");
    assert_eq!(SessionState::of(&state.sessions.shown[1]), SessionState::New);

    assert_eq!(read_session(&mut state, Some(Provider::Claude), 59), [Effect::SaveSessionRecords]);
    let by_key = |state: &AppState, key: &str| state.sessions.shown.iter().find(|session| session.row.key == key).map(SessionState::of);
    assert_eq!(by_key(&state, "claude:local_1"), Some(SessionState::Seen));
    assert_eq!(by_key(&state, "claude:local_2"), Some(SessionState::New), "only the session on screen");

    assert!(update(&mut state, Msg::SessionsScanned(scan), at(NOW)).is_empty(), "read, it asks no more");
    let left = vec![row("claude:local_1", Provider::Claude, 100.0, Some(60.0)), SessionRow { on_screen: true, ..other }];
    assert_eq!(update(&mut state, Msg::SessionsScanned(left), at(NOW)), [Effect::ReadAttention]);
    assert_eq!(by_key(&state, "claude:local_1"), Some(SessionState::Seen), "it stays read after the person moves on");
    assert!(read_session(&mut state, Some(Provider::Claude), 0).len() == 1);

    let next = vec![on_screen(100.0, Some(-10.0))];
    assert_eq!(update(&mut state, Msg::SessionsScanned(next), at(NOW)), [Effect::ReadAttention], "a later answer is new again");
    assert_eq!(by_key(&state, "claude:local_1"), Some(SessionState::New));
}

#[test]
fn nothing_is_asked_while_the_session_on_screen_has_nothing_unread() {
    let mut state = state();
    assert!(update(&mut state, Msg::SessionsScanned(vec![on_screen(100.0, None)]), at(NOW)).is_empty(), "still running");
    let read = SessionRow { seen_in_app: true, ..on_screen(100.0, Some(60.0)) };
    assert!(update(&mut state, Msg::SessionsScanned(vec![read]), at(NOW)).is_empty(), "already read in the app");
    let unread = row("claude:local_1", Provider::Claude, 100.0, Some(60.0));
    assert!(update(&mut state, Msg::SessionsScanned(vec![unread]), at(NOW)).is_empty(), "not on screen");
    assert!(read_session(&mut state, Some(Provider::Claude), 0).is_empty(), "an answer to an old question changes nothing");
}

fn side(key: &str, prompt_ago: f64, finished_ago: Option<f64>) -> SessionRow {
    SessionRow { parent: Some("claude:local_p".to_owned()), ..row(key, Provider::Claude, prompt_ago, finished_ago) }
}

#[test]
fn sub_tasks_are_lines_in_their_card_and_stay_when_it_folds() {
    let mut state = state();
    scanned(&mut state, vec![row("claude:local_p", Provider::Claude, 900.0, Some(800.0)), side("claude:local_k", 300.0, None), row("codex:t", Provider::Codex, 600.0, None)]);
    assert_eq!(state.sessions.counts().running, 2, "the task header still counts each session");
    let tray = view::tray(&state, &context(), 560.0);
    let keys: Vec<&str> = tray.cards.iter().map(|card| card.key.as_str()).collect();
    assert_eq!(keys, ["claude:local_p", "codex:t"], "a sub-task has no card of its own, and its request places its card");
    let (card, line) = (&tray.cards[0], &tray.cards[0].sub_tasks[0]);
    assert_eq!((line.key.as_str(), line.chip, line.height), ("claude:local_k", "진행 중", 32.0));
    assert_eq!(line.accessibility_label, "하위 작업, Claude, claude:local_k 작업, 진행 중, 요청: README 정리하고 커밋해줘");
    assert_eq!((card.height, card.total_height()), (118.0, 154.0));
    assert_eq!(tray.fold.content_height, 154.0 + 8.0 + 118.0);

    let tight = view::tray(&state, &context(), 200.0);
    assert!(tight.cards[0].compact && tight.cards[1].compact, "{:?}", tight.fold);
    assert_eq!((tight.cards[0].sub_tasks.len(), tight.cards[0].total_height()), (1, 98.0), "a folded card keeps its lines");
}

#[test]
fn checking_a_card_clears_the_lines_already_read_and_the_others_become_cards() {
    let mut state = state();
    let read = SessionRow { seen_in_app: true, ..side("claude:local_read", 300.0, Some(200.0)) };
    let rows = vec![row("claude:local_p", Provider::Claude, 900.0, Some(800.0)), read, side("claude:local_new", 250.0, Some(100.0)), side("claude:local_run", 200.0, None)];
    scanned(&mut state, rows.clone());
    update(&mut state, Msg::ToggleComplete("claude:local_p".to_owned()), at(NOW));
    assert_eq!(view::tray(&state, &context(), 560.0).cards.len(), 1, "the lines stay in the card until it clears");
    let effects = update(&mut state, Msg::CardFaded("claude:local_p".to_owned()), at(NOW));
    assert_eq!(effects, [Effect::SaveSessionRecords]);
    assert_eq!(keys(&state), ["claude:local_run", "claude:local_new"], "what is left to look at stands alone");
    update(&mut state, Msg::SessionsScanned(rows), at(NOW));
    assert_eq!(keys(&state), ["claude:local_run", "claude:local_new"], "the next scan brings neither the card nor the read line back");
    let tray = view::tray(&state, &context(), 560.0);
    assert!(tray.cards.len() == 2 && tray.cards.iter().all(|card| card.sub_tasks.is_empty()));
}

#[test]
fn a_line_checks_off_by_itself() {
    let mut state = state();
    scanned(&mut state, vec![row("claude:local_p", Provider::Claude, 900.0, Some(800.0)), side("claude:local_k", 300.0, Some(200.0))]);
    let effects = update(&mut state, Msg::ToggleComplete("claude:local_k".to_owned()), at(NOW));
    assert_eq!(effects, [Effect::ScheduleCompletion { key: "claude:local_k".to_owned(), after: COMPLETION_DELAY }]);
    let tray = view::tray(&state, &context(), 560.0);
    let (card, line) = (&tray.cards[0], &tray.cards[0].sub_tasks[0]);
    assert!(card.countdown.is_none() && line.countdown.is_some() && line.state == SessionState::Done);
    assert_eq!(update(&mut state, Msg::CompletionDue("claude:local_k".to_owned()), at(NOW) + COMPLETION_DELAY), [Effect::FadeCard("claude:local_k".to_owned())]);
    update(&mut state, Msg::CardFaded("claude:local_k".to_owned()), at(NOW));
    assert_eq!(keys(&state), ["claude:local_p"]);
    assert!(view::tray(&state, &context(), 560.0).cards[0].sub_tasks.is_empty());
}

fn asking() -> SessionRow {
    let question = Question { id: "toolu_q".to_owned(), asked_at: at(NOW - 120.0), questions: vec!["어느 안으로 할까요?".to_owned(), "색은요?".to_owned()] };
    SessionRow { question: Some(question), ..row("claude:local_q", Provider::Claude, 300.0, None) }
}

#[test]
fn a_card_waiting_on_a_question_says_what_it_asks_and_the_header_counts_it_apart() {
    let mut state = state();
    scanned(&mut state, vec![asking(), row("codex:t", Provider::Codex, 200.0, Some(100.0)), row("codex:r", Provider::Codex, 100.0, None)]);
    let tray = view::tray(&state, &context(), 560.0);
    let card = tray.cards.iter().find(|card| card.key == "claude:local_q").unwrap();
    assert_eq!((card.state, card.chip, card.when.as_str()), (SessionState::Asking, "답변 필요", "2분 전 질문"));
    let detail = card.detail.as_ref().unwrap();
    assert_eq!((detail.text.as_str(), detail.icon, detail.color), ("질문 · 어느 안으로 할까요?", "message-circle-question-mark", theme::ASKING));
    assert_eq!(detail.tooltip, "질문 · 어느 안으로 할까요?\n질문 · 색은요?", "the tooltip has every question");
    assert!(card.accessibility_label.contains("답변 필요") && card.accessibility_label.ends_with("질문 · 어느 안으로 할까요?"));
    assert_eq!(theme::card_style(SessionState::Asking, false).signal, theme::ASKING);

    let header = view::dock(&state, &context()).header.expect("the task header");
    let pills: Vec<(&str, theme::Rgb)> = header.pills.iter().map(|pill| (pill.words.as_str(), pill.color)).collect();
    assert_eq!(pills, [("답변 필요 1개", theme::ASKING), ("확인할 것 1개", theme::NEW_RESULT), ("진행 중 1개", theme::RUNNING)], "questions first, apart from the answers");
    assert_eq!(state.sessions.counts().new, 2, "rooms hear a question among the answers to check");

    let folded = view::tray(&state, &context(), 150.0);
    let small = folded.cards.iter().find(|card| card.key == "claude:local_q").unwrap();
    assert!(small.compact && small.tooltip.contains("질문 · 어느 안으로 할까요?") && small.accessibility_label.contains("질문 ·"), "{small:?}");
    assert!(view::rooms(&state, &context()).list.is_empty());
}

#[test]
fn a_plan_waiting_for_approval_says_so() {
    let mut state = state();
    let mut plan = asking();
    plan.question.as_mut().unwrap().questions.clear();
    scanned(&mut state, vec![plan]);
    let tray = view::tray(&state, &context(), 560.0);
    assert_eq!(tray.cards[0].detail.as_ref().unwrap().text, "계획을 확인해 주세요");
    let english = view::tray(&state, &english(), 560.0);
    assert_eq!((english.cards[0].chip, english.cards[0].when.as_str()), ("Needs answer", "asked you 2 min ago"));
}
