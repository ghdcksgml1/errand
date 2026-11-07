//! Work rooms: what the person tells a room, how its tabs and people read, and joining, opening and leaving.

use errand_core::avatar::Avatar;
use errand_core::avatar::Part;
use errand_core::format::timestamp_from_seconds;
use errand_core::layout::{DockShape, MEMBER_ROWS};
use errand_core::model::{AppState, Effect, Msg, WidgetPreferences, update};
use errand_core::reset_history::ResetHistory;
use errand_core::rooms::{self, Face, Identity, Member, Presence, PresenceReport, Room, RoomError, RoomSummary, Visibility};
use errand_core::sessions::{SessionRecords, SessionRow, SessionState, TaskCounts, VisibleSession};
use errand_core::view::{self, ViewContext};
use errand_core::{Language, Provider, TimeZone, Timestamp, theme};
use jiff::tz::offset;

/// 2026-10-15 15:00 in Seoul.
const NOW: f64 = 1_792_044_000.0;

fn at(seconds: f64) -> Timestamp {
    timestamp_from_seconds(seconds).unwrap()
}
fn seoul() -> TimeZone {
    TimeZone::fixed(offset(9))
}
fn context() -> ViewContext {
    ViewContext { now: at(NOW), zone: seoul(), computer: "Mac", language: Language::Korean }
}
fn state() -> AppState {
    AppState::new(ResetHistory::new(), SessionRecords::new(at(NOW)), WidgetPreferences::default(), " 한결 ".to_owned())
}

/// A task card on screen, its request and answer as private as they get.
fn card(key: &str, running: bool, stalled: bool, seen: bool) -> VisibleSession {
    VisibleSession {
        row: SessionRow {
            key: key.to_owned(),
            provider: Provider::Claude,
            title: "비밀 프로젝트 출시 준비".to_owned(),
            prompt: "고객 명단 정리해줘".to_owned(),
            prompt_at: at(NOW - 7200.0),
            running,
            aborted: false,
            finished_at: (!running).then(|| at(NOW - 600.0)),
            result: Some("명단 정리 끝".to_owned()),
            action: None,
            folder: "secret-launch".to_owned(),
            url: String::new(),
            seen_in_app: false,
            stalled,
            parent: None,
            on_screen: false,
        },
        seen,
    }
}

fn tasks(new: usize, running: usize, seen: usize) -> TaskCounts {
    TaskCounts { new, running, seen }
}

fn member(name: &str, tasks: TaskCounts, quiet_minutes: f64, me: bool) -> Member {
    Member { id: name.to_owned(), name: name.to_owned(), avatar: None, tasks, last_active: at(NOW - quiet_minutes * 60.0), joined_at: None, me }
}

/// This install's own seed, as the platform first draws one.
const SEED: &str = "5f0c2b8e-61d4-4c3a-9a57-0e1f2d3c4b5a";

#[test]
fn a_room_hears_what_the_folded_task_header_counts_and_the_nickname_only_when_private() {
    let mut state = state();
    state.profile.avatar = SEED.to_owned();
    state.sessions.shown = vec![
        card("claude:new", false, false, false),
        card("claude:stalled", true, true, false),
        card("claude:running", true, false, false),
        card("claude:seen", false, false, true),
        card("claude:checked", false, false, true),
    ];
    // Checked off and waiting to clear: the header no longer counts it.
    state.sessions.pending.insert("claude:checked".to_owned(), at(NOW + 5.0));
    let public = rooms::presence_report(&state, Visibility::Public);
    let character = Avatar::from_seed(SEED);
    assert_eq!(public, PresenceReport { identity: Identity::Anonymous, avatar: character, tasks: tasks(2, 1, 1) }, "a public room never hears the name");
    let header = view::dock(&state, &context()).header.expect("the task header");
    let pills: Vec<(SessionState, usize)> = header.pills.iter().map(|pill| (pill.state, pill.count)).collect();
    assert_eq!(pills, [(SessionState::New, 2), (SessionState::Running, 1), (SessionState::Seen, 1)], "the very counts on the person's own header");
    let private = rooms::presence_report(&state, Visibility::Private);
    assert_eq!((&private.identity, private.avatar), (&Identity::Profile("한결".to_owned()), character), "the same face in every room");
    // Rooms hear the character's parts, never the install's own seed it was drawn from.
    let code = private.avatar.code();
    assert!(code.starts_with("avatar:") && !code.contains(SEED) && Avatar::parse(&code) == Some(character), "{code}");
    state.profile.avatar = character.step(Part::Eyes, 1).code();
    assert_eq!(rooms::presence_report(&state, Visibility::Public).avatar, character.step(Part::Eyes, 1), "a picked character is sent as picked");
    // The report's whole shape: nothing about titles, requests, answers or folders can be in it, and the
    // character is only the official parts.
    let PresenceReport { identity: _, avatar: Avatar { shape: _, top: _, eyes: _, mouth: _, outfit: _, blush: _, clothes: _ }, tasks: TaskCounts { new: _, running: _, seen: _ } } =
        private;
}

#[test]
fn a_private_room_hears_a_nickname_it_takes() {
    let mut state = state();
    let nickname = |state: &AppState| rooms::presence_report(state, Visibility::Private).identity;
    state.profile.name = "  Hang\u{7}yeol Kim, Software Engineer at Errand  ".to_owned();
    assert_eq!(nickname(&state), Identity::Profile("Hangyeol Kim, Softwa".to_owned()), "no control characters, twenty at most");
    state.profile.name = "가나다라마바사아자차카타파하거너더러마 바사".to_owned();
    assert_eq!(nickname(&state), Identity::Profile("가나다라마바사아자차카타파하거너더러마".to_owned()), "a cut that ends on a space drops it");
    state.profile.name = " \t ".to_owned();
    assert_eq!(nickname(&state), Identity::Anonymous, "without a name, the room gives an alias");
}

#[test]
fn members_read_as_working_resting_or_away() {
    let now = at(NOW);
    assert_eq!(member("a", tasks(0, 2, 0), 0.0, false).presence(now), Presence::Working);
    assert_eq!(member("b", tasks(3, 0, 1), 5.0, false).presence(now), Presence::Resting, "answers to check are not work under way");
    assert_eq!(member("c", tasks(0, 0, 0), 59.0, false).presence(now), Presence::Resting);
    assert_eq!(member("d", tasks(1, 0, 0), 61.0, false).presence(now), Presence::Away);
    assert_eq!(member("e", tasks(0, 1, 0), 90.0, false).presence(now), Presence::Working, "a request running is work, however quiet");
}

fn late_night(members: Vec<Member>) -> Room {
    Room { id: "late-night".to_owned(), name: "고독한 야근방".to_owned(), tagline: "불 꺼진 사무실".to_owned(), visibility: Visibility::Public, members }
}

fn summary(id: &str, name: &str, visibility: Visibility) -> RoomSummary {
    let faces = vec![Face { name: "졸린 수달".to_owned(), avatar: None }, Face { name: "새벽감자".to_owned(), avatar: Some(Avatar::from_seed("새벽감자").code()) }];
    RoomSummary { id: id.to_owned(), name: name.to_owned(), tagline: String::new(), visibility, members: 7, working: 4, faces }
}

fn sync(room: &str, visibility: Visibility) -> Effect {
    Effect::SyncRoom { room: room.to_owned(), visibility }
}

/// A row under the open tab: name, the "나" badge, its pills as (state, count), and whether it is faded.
type Row = (String, Option<&'static str>, Vec<(SessionState, usize)>, bool);

fn rows(state: &AppState) -> Vec<Row> {
    let rooms = view::dock(state, &context()).rooms.expect("the room tabs");
    rooms.members.into_iter().map(|row| (row.name, row.me, row.pills.iter().map(|pill| (pill.state, pill.count)).collect(), row.faded)).collect()
}

/// Each tab: whether it is locked, its name, how many are in the room once known, and whether it is open.
fn tabs(state: &AppState) -> Vec<(bool, String, Option<usize>, bool)> {
    let rooms = view::dock(state, &context()).rooms.expect("the room tabs");
    rooms.tabs.into_iter().map(|tab| (tab.private, tab.name, tab.people, tab.open)).collect()
}

#[test]
fn several_rooms_at_once_each_a_tab_under_the_profile() {
    let mut state = state();
    assert_eq!(update(&mut state, Msg::ShowRooms, at(NOW)), [Effect::FetchRooms]);
    assert!(state.rooms.open);
    assert_eq!(view::rooms(&state, &context()).status.as_deref(), Some("방 목록을 불러오는 중…"));
    assert!(view::dock(&state, &context()).rooms.is_none(), "no tabs before joining a room");
    update(&mut state, Msg::RoomsRead(Ok(vec![summary("late-night", "고독한 야근방", Visibility::Public), summary("team", "우리 팀 야근방", Visibility::Private)])), at(NOW));
    let list = view::rooms(&state, &context());
    assert_eq!((list.joined_heading, list.others_heading), (None, None), "no headings before joining a room");
    let card = &list.list[0];
    assert_eq!((list.title, card.people.as_str(), card.working.count, card.visibility.text.as_str()), ("작업방", "7명", 4, "익명 참여"));
    assert_eq!((list.list[1].visibility.text.as_str(), card.button, card.joined), ("프로필 공개", "들어가기", false));
    assert_eq!((card.faces[0].initial.as_str(), card.faces[0].avatar), ("졸", None), "without a character, a grey face with a first letter");
    assert_eq!(card.faces[1].avatar, Some(Avatar::from_seed("새벽감자")), "with one, their own character, in a public room too");
    assert!(update(&mut state, Msg::SyncRoom, at(NOW)).is_empty(), "nothing to sync outside a room");

    assert_eq!(update(&mut state, Msg::JoinRoom("late-night".to_owned()), at(NOW)), [sync("late-night", Visibility::Public), Effect::SaveRooms], "kept for the next launch");
    assert!(update(&mut state, Msg::JoinRoom("late-night".to_owned()), at(NOW)).is_empty(), "already in");
    assert_eq!(tabs(&state), [(false, "고독한 야근방".to_owned(), None, true)], "a room just joined opens its tab, named from the list");
    let joining = view::dock(&state, &context()).rooms.unwrap();
    assert_eq!((joining.status.as_deref(), joining.rows()), (Some("들어가는 중…"), 1));
    let list = view::rooms(&state, &context());
    let card = &list.list[0];
    assert_eq!((card.joined, card.button, card.badge), (true, "나가기", Some("참여 중")), "the list offers a way out of a room the person is in");
    assert_eq!((list.joined_heading.as_deref(), list.others_heading), (Some("참여 중  1/3"), Some("다른 방")));
    assert_eq!(list.list[1].badge, None);

    let room = late_night(vec![
        member("커밋요정", tasks(0, 0, 3), 30.0, false),
        member("졸린 수달", tasks(1, 1, 2), 0.0, true),
        member("새벽감자", tasks(0, 2, 0), 0.0, false),
        member("핫픽스맨", tasks(2, 1, 0), 0.0, false),
    ]);
    update(&mut state, Msg::RoomRead("late-night".to_owned(), Ok(room.clone())), at(NOW));
    assert_eq!(tabs(&state), [(false, "고독한 야근방".to_owned(), Some(4), true)], "the tab counts who is in the room");
    assert_eq!(
        rows(&state),
        [
            ("졸린 수달".to_owned(), Some("나"), vec![(SessionState::New, 1), (SessionState::Running, 1), (SessionState::Seen, 2)], false),
            ("새벽감자".to_owned(), None, vec![(SessionState::Running, 2)], false),
            ("핫픽스맨".to_owned(), None, vec![(SessionState::New, 2), (SessionState::Running, 1)], false),
            ("커밋요정".to_owned(), None, vec![(SessionState::Seen, 3)], false),
        ],
        "the person first, then whoever is working, the busiest first: a face, a name and the folded task header's pills"
    );
    let dock = view::dock(&state, &context());
    assert_eq!(dock.shape(), DockShape { header: false, batteries: 2, tabs: true, rows: 4 });
    let open = dock.rooms.as_ref().unwrap();
    assert_eq!(open.tabs[0].accessibility_label, "고독한 야근방, 익명 참여, 4명");
    assert_eq!(open.tabs[0].tooltip, "고독한 야근방 · 불 꺼진 사무실\n눌러서 접기");
    let pill = &open.members[0].pills[0];
    assert_eq!((pill.icon, pill.color, pill.words.as_str()), ("circle-check", theme::session_state(SessionState::New), "확인할 것 1개"), "drawn as the header draws its pills");

    assert_eq!(
        update(&mut state, Msg::JoinRoom("team".to_owned()), at(NOW)),
        [sync("team", Visibility::Private), Effect::SaveRooms],
        "joining another room keeps the first; a private room hears the profile"
    );
    assert_eq!(
        tabs(&state),
        [(false, "고독한 야근방".to_owned(), Some(4), false), (true, "우리 팀 야근방".to_owned(), None, true)],
        "a private room's tab is locked"
    );
    assert_eq!(update(&mut state, Msg::SyncRoom, at(NOW)), [sync("late-night", Visibility::Public), sync("team", Visibility::Private)]);

    update(&mut state, Msg::ExpandRoom("late-night".to_owned()), at(NOW));
    assert_eq!(rows(&state).len(), 4, "a tab shows its own room's people");
    update(&mut state, Msg::ExpandRoom("late-night".to_owned()), at(NOW));
    assert!(rows(&state).is_empty() && tabs(&state).iter().all(|(_, _, _, open)| !open), "a click on the open tab folds it");
    assert_eq!(view::dock(&state, &context()).shape().rows, 0);
    update(&mut state, Msg::ExpandRoom("team".to_owned()), at(NOW));
    update(&mut state, Msg::ExpandRoom("nowhere".to_owned()), at(NOW));
    assert_eq!(state.rooms.expanded.as_deref(), Some("team"), "only a room the person is in opens, and nothing else folds for it");

    update(&mut state, Msg::CloseRooms, at(NOW));
    assert!(!state.rooms.open);
    assert_eq!(
        update(&mut state, Msg::ShowRooms, at(NOW)),
        [Effect::FetchRooms, sync("late-night", Visibility::Public), sync("team", Visibility::Private)],
        "opening the list tells every room the latest"
    );
    update(&mut state, Msg::ToggleDetails, at(NOW));
    assert!(state.details_open && !state.rooms.open, "one bubble beside the dock at a time");

    assert_eq!(update(&mut state, Msg::LeaveRoom("team".to_owned()), at(NOW)), [Effect::LeaveRoom("team".to_owned()), Effect::FetchRooms, Effect::SaveRooms]);
    assert_eq!(tabs(&state), [(false, "고독한 야근방".to_owned(), Some(4), false)], "leaving the open room folds the rooms");
    update(&mut state, Msg::RoomRead("team".to_owned(), Ok(room)), at(NOW));
    assert_eq!(tabs(&state).len(), 1, "an answer from a room since left is dropped");
    assert!(update(&mut state, Msg::LeaveRoom("team".to_owned()), at(NOW)).is_empty());
    update(&mut state, Msg::LeaveRoom("late-night".to_owned()), at(NOW));
    assert!(view::dock(&state, &context()).rooms.is_none(), "out of every room, the tabs go");

    update(&mut state, Msg::RoomsRead(Err(RoomError::Unreachable)), at(NOW));
    assert_eq!(view::rooms(&state, &context()).status, None, "a list once read stays up");
    state.rooms.list.clear();
    assert_eq!(view::rooms(&state, &context()).status.as_deref(), Some("작업방 서버에 연결하지 못했어요."));
}

#[test]
fn the_dock_opens_the_list_of_rooms_and_closes_it_again() {
    let mut state = state();
    update(&mut state, Msg::JoinRoom("late-night".to_owned()), at(NOW));
    update(&mut state, Msg::ToggleDetails, at(NOW));
    assert_eq!(
        update(&mut state, Msg::ToggleRooms, at(NOW)),
        [Effect::FetchRooms, sync("late-night", Visibility::Public)],
        "opening reads the list afresh and tells the room the latest counts"
    );
    assert!(state.rooms.open && !state.details_open, "the list takes the details' place");
    assert!(view::dock(&state, &context()).rooms_listed, "the way to the rooms knows its list is open");
    assert_eq!(update(&mut state, Msg::ToggleRooms, at(NOW)), [], "the same press closes it, with nothing to ask");
    assert!(!state.rooms.open && !view::dock(&state, &context()).rooms_listed);
}

#[test]
fn a_fourth_room_waits_until_the_person_leaves_one() {
    let mut state = state();
    let list = ["late-night", "team", "dawn", "deadline"].map(|room| summary(room, room, Visibility::Public));
    update(&mut state, Msg::RoomsRead(Ok(list.to_vec())), at(NOW));
    let shut = |state: &AppState| view::rooms(state, &context()).list.into_iter().map(|card| card.shut).collect::<Vec<_>>();
    update(&mut state, Msg::JoinRoom("late-night".to_owned()), at(NOW));
    update(&mut state, Msg::JoinRoom("team".to_owned()), at(NOW));
    assert_eq!(shut(&state), [None, None, None, None], "under the limit every way in is open");
    assert_eq!(view::rooms(&state, &context()).subtitle, Language::Korean.rooms_subtitle());

    assert_eq!(update(&mut state, Msg::JoinRoom("dawn".to_owned()), at(NOW)), [sync("dawn", Visibility::Public), Effect::SaveRooms]);
    let full = "방은 3개까지 들어갈 수 있어요. 다른 방을 나가면 들어가요.";
    assert_eq!(view::rooms(&state, &context()).subtitle, full, "the list says why no more rooms open");
    assert_eq!(shut(&state), [None, None, None, Some(full.to_owned())], "the rooms the person is in can still be left");

    assert_eq!(update(&mut state, Msg::JoinRoom("deadline".to_owned()), at(NOW)), [], "the fourth room is not asked");
    assert_eq!((state.rooms.joined.len(), state.rooms.expanded.as_deref()), (3, Some("dawn")), "and what was open stays open");
    assert!(update(&mut state, Msg::SyncRoom, at(NOW)).len() == 3, "the three rooms carry on as before");

    update(&mut state, Msg::LeaveRoom("team".to_owned()), at(NOW));
    assert_eq!(shut(&state), [None, None, None, None]);
    assert_eq!(update(&mut state, Msg::JoinRoom("deadline".to_owned()), at(NOW)), [sync("deadline", Visibility::Public), Effect::SaveRooms], "leaving one makes way");
}

#[test]
fn rooms_an_older_errand_joined_past_the_limit_stay_until_left() {
    let mut state = state();
    let rooms = ["late-night", "team", "dawn", "deadline", "quiet"].map(|room| (room.to_owned(), Visibility::Public));
    update(&mut state, Msg::RoomsRemembered(rooms.to_vec()), at(NOW));
    assert_eq!(tabs(&state).len(), 5, "nobody is put out of a room on updating");
    assert_eq!(update(&mut state, Msg::JoinRoom("sixth".to_owned()), at(NOW)), []);
    update(&mut state, Msg::LeaveRoom("quiet".to_owned()), at(NOW));
    update(&mut state, Msg::LeaveRoom("deadline".to_owned()), at(NOW));
    assert_eq!(update(&mut state, Msg::JoinRoom("sixth".to_owned()), at(NOW)), [], "three is still as many as Errand joins");
    update(&mut state, Msg::LeaveRoom("dawn".to_owned()), at(NOW));
    assert_eq!(update(&mut state, Msg::JoinRoom("sixth".to_owned()), at(NOW)), [sync("sixth", Visibility::Public), Effect::SaveRooms]);
}

#[test]
fn a_busy_room_shows_six_rows_and_counts_the_rest() {
    let mut state = state();
    update(&mut state, Msg::JoinRoom("late-night".to_owned()), at(NOW));
    let crowd = |count: usize| late_night((0..count).map(|index| member(&format!("수달{index}"), tasks(0, index % 2, 1), 0.0, index == 0)).collect());
    update(&mut state, Msg::RoomRead("late-night".to_owned(), Ok(crowd(10))), at(NOW));
    let rooms = view::dock(&state, &context()).rooms.unwrap();
    assert_eq!((rooms.members.len(), rooms.more.as_deref(), rooms.rows()), (5, Some("외 5명"), MEMBER_ROWS));
    assert_eq!(rooms.tabs[0].people, Some(10), "the tab counts everyone, rows or not");
    update(&mut state, Msg::RoomRead("late-night".to_owned(), Ok(crowd(MEMBER_ROWS))), at(NOW));
    let rooms = view::dock(&state, &context()).rooms.unwrap();
    assert_eq!((rooms.members.len(), rooms.more), (MEMBER_ROWS, None), "a room that fits shows everyone");
}

#[test]
fn a_room_errand_knows_nothing_of_is_joined_anonymously() {
    let mut state = state();
    assert_eq!(update(&mut state, Msg::JoinRoom("unlisted".to_owned()), at(NOW)), [sync("unlisted", Visibility::Public), Effect::SaveRooms]);
    assert_eq!(tabs(&state), [(false, "들어가는 중…".to_owned(), None, true)]);
    update(&mut state, Msg::RoomRead("unlisted".to_owned(), Err(RoomError::Unreachable)), at(NOW));
    assert_eq!(view::dock(&state, &context()).rooms.unwrap().status.as_deref(), Some("작업방 서버에 연결하지 못했어요."));
}

#[test]
fn everyone_shows_their_own_character_in_public_and_private_rooms_alike() {
    let mut state = state();
    let picked = "avatar:dome,bear,dots,smile,tee,blush,F97316";
    let with = |avatar: Option<&str>, member: Member| Member { avatar: avatar.map(str::to_owned), ..member };
    let people = vec![
        with(Some(picked), member("졸린 수달", tasks(0, 1, 0), 0.0, true)),
        with(None, member("바쁜 너구리", tasks(0, 2, 0), 0.0, false)),
        // A room should never send a seed; if one did, it would not stand in for a character.
        with(Some(SEED), member("조용한 부엉이", tasks(0, 1, 0), 0.0, false)),
        with(Some("avatar:blob,bear,dots,smile,tee,blush,F97316"), member("성실한 펭귄", tasks(0, 1, 0), 0.0, false)),
    ];
    update(&mut state, Msg::JoinRoom("late-night".to_owned()), at(NOW));
    update(&mut state, Msg::RoomRead("late-night".to_owned(), Ok(late_night(people))), at(NOW));
    let faces = |state: &AppState| view::dock(state, &context()).rooms.unwrap().members.into_iter().map(|row| (row.face.initial, row.face.avatar)).collect::<Vec<_>>();
    let expected = [("졸".to_owned(), Avatar::parse(picked)), ("바".to_owned(), None), ("성".to_owned(), None), ("조".to_owned(), None)];
    assert_eq!(faces(&state), expected, "the person's own character as the room sent it, and a grey face without one");
    state.rooms.joined[0].room.as_mut().unwrap().visibility = Visibility::Private;
    assert_eq!(faces(&state), expected, "the same in a private room");
}

#[test]
fn a_room_that_is_gone_closes_its_tab_and_one_that_turns_the_person_away_says_why() {
    let mut state = state();
    let late = || late_night(vec![member("졸린 수달", tasks(0, 1, 0), 0.0, true), member("새벽감자", tasks(0, 2, 0), 0.0, false)]);
    update(&mut state, Msg::JoinRoom("late-night".to_owned()), at(NOW));
    update(&mut state, Msg::JoinRoom("team".to_owned()), at(NOW));
    update(&mut state, Msg::RoomRead("late-night".to_owned(), Ok(late())), at(NOW));
    update(&mut state, Msg::ExpandRoom("late-night".to_owned()), at(NOW));
    let status = |state: &AppState| view::dock(state, &context()).rooms.unwrap().status;

    for failure in [RoomError::Unreachable, RoomError::RateLimited] {
        update(&mut state, Msg::RoomRead("late-night".to_owned(), Err(failure)), at(NOW));
        assert_eq!((rows(&state).len(), status(&state)), (2, None), "{failure:?}: out of reach a while, the room as it last answered stays");
    }
    let turned_away = [
        (RoomError::Full, "방이 꽉 찼어요. 자리가 나면 들어가요."),
        (RoomError::TooManyRooms, "방에 너무 많이 들어가 있어요. 다른 방을 나가면 들어가요."),
        (RoomError::ClientTooOld, "작업방을 쓰려면 Errand를 업데이트하세요."),
        (RoomError::Unauthorized, "작업방 서버가 이 Mac을 알아보지 못했어요."),
        (RoomError::Rejected, "작업방 서버가 요청을 받지 않았어요."),
    ];
    for (failure, words) in turned_away {
        update(&mut state, Msg::RoomRead("late-night".to_owned(), Ok(late())), at(NOW));
        update(&mut state, Msg::RoomRead("late-night".to_owned(), Err(failure)), at(NOW));
        assert_eq!((rows(&state).len(), status(&state).as_deref()), (0, Some(words)), "{failure:?}: no longer in the room, the reason in place of its people");
        assert_eq!(tabs(&state)[0], (false, "고독한 야근방".to_owned(), None, true), "the tab stays, by name but counting no one, and the next sync tries again");
    }
    update(&mut state, Msg::RoomRead("late-night".to_owned(), Ok(late())), at(NOW));
    assert_eq!(rows(&state).len(), 2, "back in once the room lets the person in");

    assert_eq!(update(&mut state, Msg::RoomRead("late-night".to_owned(), Err(RoomError::NotFound)), at(NOW)), [Effect::SaveRooms], "forgotten for the next launch too");
    assert_eq!(tabs(&state), [(false, "들어가는 중…".to_owned(), None, false)], "a room that is gone closes its tab, folding its people");
    assert_eq!(state.rooms.expanded, None);
    update(&mut state, Msg::ShowRooms, at(NOW));
    assert_eq!(
        update(&mut state, Msg::RoomRead("team".to_owned(), Err(RoomError::NotFound)), at(NOW)),
        [Effect::FetchRooms, Effect::SaveRooms],
        "with the list open, it is read again"
    );
    assert!(view::dock(&state, &context()).rooms.is_none());

    update(&mut state, Msg::RoomsRead(Err(RoomError::ClientTooOld)), at(NOW));
    assert_eq!(view::rooms(&state, &context()).status.as_deref(), Some("작업방을 쓰려면 Errand를 업데이트하세요."));
}

#[test]
fn the_rooms_joined_come_back_on_the_next_launch_each_as_visible_as_it_was() {
    let mut state = state();
    update(&mut state, Msg::RoomsRead(Ok(vec![summary("late-night", "고독한 야근방", Visibility::Public), summary("team", "우리 팀 야근방", Visibility::Private)])), at(NOW));
    update(&mut state, Msg::JoinRoom("late-night".to_owned()), at(NOW));
    update(&mut state, Msg::JoinRoom("team".to_owned()), at(NOW));
    let saved = state.rooms.remembered();
    assert_eq!(saved, [("late-night".to_owned(), Visibility::Public), ("team".to_owned(), Visibility::Private)]);

    // The next launch: no list read yet, nothing answered.
    let mut state = self::state();
    let mut remembered = saved.clone();
    remembered.extend([("late-night".to_owned(), Visibility::Public), ("../devices".to_owned(), Visibility::Public), ("".to_owned(), Visibility::Private)]);
    assert_eq!(
        update(&mut state, Msg::RoomsRemembered(remembered), at(NOW)),
        [sync("late-night", Visibility::Public), sync("team", Visibility::Private), Effect::SaveRooms],
        "each room joined once, a private one hearing the nickname from the start; nothing that is not a room ID"
    );
    assert_eq!(state.rooms.remembered(), saved);
    assert_eq!(state.rooms.expanded, None, "the tabs come back folded");
    assert_eq!(tabs(&state).iter().map(|(private, _, people, open)| (*private, *people, *open)).collect::<Vec<_>>(), [(false, None, false), (true, None, false)]);

    // Answers that say nothing new keep the file as it is; learning a room's visibility updates it.
    let team = Room { id: "team".to_owned(), name: "우리 팀 야근방".to_owned(), tagline: String::new(), visibility: Visibility::Private, members: vec![] };
    assert!(update(&mut state, Msg::RoomRead("team".to_owned(), Ok(team.clone())), at(NOW)).is_empty());
    assert!(update(&mut state, Msg::SyncRoom, at(NOW)).iter().all(|effect| *effect != Effect::SaveRooms));
    assert_eq!(
        update(&mut state, Msg::RoomRead("late-night".to_owned(), Ok(Room { visibility: Visibility::Private, ..late_night(vec![]) })), at(NOW)),
        [Effect::SaveRooms]
    );
    assert_eq!(state.rooms.remembered()[0], ("late-night".to_owned(), Visibility::Private));
}

#[test]
fn each_row_counts_how_long_they_have_been_in_the_room() {
    let mut state = state();
    let came = |seconds: f64, member: Member| Member { joined_at: Some(at(NOW - seconds)), ..member };
    let people = vec![
        came(42.0, member("졸린 수달", tasks(1, 1, 0), 0.0, true)),
        came(3723.0, member("새벽감자", tasks(0, 2, 0), 0.0, false)),
        // Its clock a little ahead of Errand's, the server says this one came in a moment from now.
        came(-2.5, member("핫픽스맨", tasks(0, 1, 0), 0.0, false)),
        member("커밋요정", tasks(0, 0, 3), 0.0, false),
    ];
    update(&mut state, Msg::JoinRoom("late-night".to_owned()), at(NOW));
    update(&mut state, Msg::RoomRead("late-night".to_owned(), Ok(late_night(people))), at(NOW));
    let times = |state: &AppState, context: &ViewContext| {
        let rows = view::dock(state, context).rooms.unwrap().members;
        rows.into_iter().map(|row| (row.name, row.elapsed, row.accessibility_label)).collect::<Vec<_>>()
    };
    assert_eq!(
        times(&state, &context()),
        [
            ("졸린 수달".to_owned(), Some("0:42".to_owned()), "졸린 수달 · 확인할 것 1개 · 진행 중 1개 · 들어온 지 42초".to_owned()),
            ("새벽감자".to_owned(), Some("1:02:03".to_owned()), "새벽감자 · 진행 중 2개 · 들어온 지 1시간 2분 3초".to_owned()),
            ("핫픽스맨".to_owned(), Some("0:00".to_owned()), "핫픽스맨 · 진행 중 1개 · 들어온 지 0초".to_owned()),
            ("커밋요정".to_owned(), None, "커밋요정 · 확인함 3개".to_owned()),
        ],
        "worked out from when each came in, never below zero, and no time where the room gives none"
    );
    let later = ViewContext { now: at(NOW + 61.0), language: Language::English, ..context() };
    assert_eq!(times(&state, &later)[0].1.as_deref(), Some("1:43"), "a minute on, without hearing from the room");
    assert!(times(&state, &later)[0].2.ends_with("Here for 1 min 43 s"), "{}", times(&state, &later)[0].2);
    assert_eq!(view::dock(&state, &context()).rooms.unwrap().members[0].tooltip, "졸린 수달 · 확인할 것 1개 · 진행 중 1개", "the tooltip stays still");
}

#[test]
fn the_rooms_clock_ticks_only_while_someones_time_shows() {
    let mut state = state();
    let tick = |effects: &[Effect]| effects.iter().filter(|effect| **effect == Effect::TickRoomClock).count();
    let timed = late_night(vec![Member { joined_at: Some(at(NOW - 42.0)), ..member("졸린 수달", tasks(0, 1, 0), 0.0, true) }]);
    let untimed = late_night(vec![member("졸린 수달", tasks(0, 1, 0), 0.0, true)]);

    update(&mut state, Msg::JoinRoom("late-night".to_owned()), at(NOW));
    assert_eq!(tick(&update(&mut state, Msg::RoomRead("late-night".to_owned(), Ok(untimed.clone())), at(NOW))), 0, "no time from the room, nothing to count");
    assert_eq!(tick(&update(&mut state, Msg::RoomRead("late-night".to_owned(), Ok(timed.clone())), at(NOW))), 1, "the first answer that says when starts it");
    assert_eq!(tick(&update(&mut state, Msg::SyncRoom, at(NOW))), 0, "one tick at a time");
    assert_eq!(tick(&update(&mut state, Msg::RoomClock, at(NOW + 1.0))), 1, "each tick asks for the next while the times show");

    update(&mut state, Msg::ExpandRoom("late-night".to_owned()), at(NOW + 1.0));
    assert_eq!(tick(&update(&mut state, Msg::RoomClock, at(NOW + 2.0))), 0, "folded, the last tick asks for no other");
    assert_eq!(tick(&update(&mut state, Msg::ExpandRoom("late-night".to_owned()), at(NOW + 2.0))), 1, "open again, it starts again");
    update(&mut state, Msg::SetWidgetHidden(true), at(NOW + 2.0));
    assert_eq!(tick(&update(&mut state, Msg::RoomClock, at(NOW + 3.0))), 0, "nor while the widget is hidden");
    assert_eq!(tick(&update(&mut state, Msg::SetWidgetHidden(false), at(NOW + 3.0))), 1);
    update(&mut state, Msg::RoomRead("late-night".to_owned(), Err(RoomError::Full)), at(NOW + 3.0));
    assert_eq!(tick(&update(&mut state, Msg::RoomClock, at(NOW + 4.0))), 0, "nor once the room has turned the person away");
    update(&mut state, Msg::RoomRead("late-night".to_owned(), Ok(timed)), at(NOW + 4.0));
    update(&mut state, Msg::LeaveRoom("late-night".to_owned()), at(NOW + 4.0));
    assert_eq!(tick(&update(&mut state, Msg::RoomClock, at(NOW + 5.0))), 0, "nor out of the room");
    assert!(!state.rooms.clock);
}

#[test]
fn the_list_says_when_its_rooms_are_sample_data() {
    let mut state = state();
    assert_eq!(view::rooms(&state, &context()).footer, "방에는 작업 개수 세 가지와 캐릭터, 비공개 방에는 닉네임까지만 보내요. 무슨 작업인지는 공유되지 않아요.");
    state.rooms.sample = true;
    assert_eq!(view::rooms(&state, &context()).footer, "예시 데이터로 보고 있어요. 무슨 작업인지는 어떤 방에도 공유되지 않아요.");
}

#[test]
fn rooms_read_in_english() {
    let mut state = state();
    update(&mut state, Msg::JoinRoom("late-night".to_owned()), at(NOW));
    let people = vec![member("", tasks(2, 0, 1), 3.0, true), member("새벽감자", tasks(0, 0, 0), 90.0, false), member("커밋요정", tasks(0, 0, 0), 20.0, false)];
    update(&mut state, Msg::RoomRead("late-night".to_owned(), Ok(late_night(people))), at(NOW));
    let english = ViewContext { language: Language::English, ..context() };
    let rooms = view::dock(&state, &english).rooms.unwrap();
    let rows: Vec<(&str, Option<&str>, usize, bool, &str)> =
        rooms.members.iter().map(|row| (row.name.as_str(), row.me, row.pills.len(), row.faded, row.tooltip.as_str())).collect();
    assert_eq!(
        rows,
        [
            ("You", None, 2, false, "You · 2 to check · 1 seen"),
            ("커밋요정", None, 0, false, "커밋요정 · On a break"),
            ("새벽감자", None, 0, true, "새벽감자 · Away")
        ],
        "without a name the person is just You; whoever is away reads faded"
    );
    assert_eq!(
        (rooms.tabs[0].accessibility_label.as_str(), rooms.tabs[0].tooltip.as_str()),
        ("고독한 야근방, Anonymous, 3 people", "고독한 야근방 · 불 꺼진 사무실\nClick to fold")
    );
    update(&mut state, Msg::RoomsRead(Ok(vec![summary("late-night", "고독한 야근방", Visibility::Public)])), at(NOW));
    let list = view::rooms(&state, &english);
    assert_eq!((list.title, list.list[0].visibility.text.as_str(), list.list[0].button), ("Work rooms", "Anonymous", "Leave"));
    assert_eq!(Language::English.rooms_full(), "You can be in 3 rooms at once. Leave one to join another.");
}
