//! Private rooms: making one, its invite link, coming in by a code or a link, and leaving or turning people
//! out for good.

use errand_core::format::timestamp_from_seconds;
use errand_core::model::{AppState, Confirm, Effect, Msg, WidgetPreferences, update};
use errand_core::reset_history::ResetHistory;
use errand_core::rooms::{self, Invite, Member, Role, Room, RoomError, RoomSummary, RoomsScreen, Visibility};
use errand_core::sessions::{SessionRecords, TaskCounts};
use errand_core::view::{self, RoomsScreenVm, ViewContext};
use errand_core::{Language, TimeZone, Timestamp};
use jiff::tz::offset;

/// 2026-10-15 15:00 in Seoul.
const NOW: f64 = 1_792_044_000.0;

fn at(seconds: f64) -> Timestamp {
    timestamp_from_seconds(seconds).unwrap()
}
fn context() -> ViewContext {
    ViewContext { now: at(NOW), zone: TimeZone::fixed(offset(9)), computer: "Mac", language: Language::Korean }
}
fn state() -> AppState {
    let mut state = AppState::new(ResetHistory::new(), SessionRecords::new(at(NOW)), WidgetPreferences::default(), "한결".to_owned());
    update(&mut state, Msg::ShowRooms, at(NOW));
    state
}

fn private(id: &str, name: &str, role: Option<Role>) -> RoomSummary {
    RoomSummary { id: id.to_owned(), name: name.to_owned(), tagline: String::new(), visibility: Visibility::Private, members: 0, working: 0, faces: vec![], role }
}

fn invite(code: &str, days: f64, uses: usize) -> Invite {
    Invite { code: code.to_owned(), expires_at: at(NOW + days * 86_400.0), uses_left: uses }
}

fn screen(state: &AppState) -> RoomsScreenVm {
    view::rooms(state, &context()).screen
}

#[test]
fn codes_read_however_they_are_typed_and_links_carry_them() {
    assert_eq!(rooms::invite_code("K7QF·M2XA·9P").as_deref(), Some("K7QFM2XA9P"));
    assert_eq!(rooms::invite_code(" k7qf-m2xa-9p ").as_deref(), Some("K7QFM2XA9P"), "any case, any separator");
    assert_eq!(rooms::invite_code("K7QF M2XA 9O").as_deref(), Some("K7QFM2XA90"), "an O is the zero it looks like");
    assert_eq!(rooms::invite_code("IL00000000").as_deref(), Some("1100000000"));
    assert_eq!(rooms::invite_code("K7QFM2XA9"), None, "nine characters are not a code");
    assert_eq!(rooms::invite_code("K7QFM2XA9U"), None, "U is not in Crockford's alphabet");
    assert_eq!(rooms::grouped("K7QFM2XA9P", '·'), "K7QF·M2XA·9P");
    assert_eq!(invite("K7QFM2XA9P", 7.0, 10).link(), "https://errand-backend.vercel.app/join#K7QF-M2XA-9P", "the code after a #, so no server hears it");
    assert_eq!(rooms::invite_from_link("errand://invite/K7QFM2XA9P").as_deref(), Some("K7QFM2XA9P"));
    assert_eq!(rooms::invite_from_link("errand://invite/k7qf-m2xa-9p/").as_deref(), Some("K7QFM2XA9P"));
    assert_eq!(rooms::invite_from_link("errand://rooms/K7QFM2XA9P"), None);
    assert_eq!(rooms::invite_from_link("https://example.com/invite/K7QFM2XA9P"), None);
}

#[test]
fn a_room_is_made_its_maker_goes_in_and_gets_its_link() {
    let mut state = state();
    assert!(update(&mut state, Msg::ShowCreateRoom, at(NOW)).is_empty());
    assert!(matches!(screen(&state), RoomsScreenVm::Create(_)));
    assert!(update(&mut state, Msg::CreateRoom { name: "   ".to_owned(), tagline: String::new() }, at(NOW)).is_empty(), "a name is needed");
    let long = "가".repeat(31);
    assert!(update(&mut state, Msg::CreateRoom { name: long, tagline: String::new() }, at(NOW)).is_empty(), "thirty characters at most");
    assert_eq!(
        update(&mut state, Msg::CreateRoom { name: " 우리 팀\u{7} 야근방 ".to_owned(), tagline: "  같이 야근해요 ".to_owned() }, at(NOW)),
        [Effect::CreateRoom { name: "우리 팀 야근방".to_owned(), tagline: "같이 야근해요".to_owned() }],
        "cleaned as the server keeps it"
    );
    assert!(update(&mut state, Msg::CreateRoom { name: "또".to_owned(), tagline: String::new() }, at(NOW)).is_empty(), "not twice while sending");
    let RoomsScreenVm::Create(create) = screen(&state) else { panic!("still naming it") };
    assert_eq!((create.sending, create.button), (true, "만드는 중…"));

    let effects = update(&mut state, Msg::RoomCreated(Ok(private("p-team", "우리 팀 야근방", Some(Role::Owner)))), at(NOW));
    assert!(effects.contains(&Effect::SyncRoom { room: "p-team".to_owned(), visibility: Visibility::Private }), "the maker goes in at once, by profile");
    assert!(effects.contains(&Effect::ReadInvites("p-team".to_owned())));
    assert!(state.rooms.is_joined("p-team") && state.rooms.list[0].id == "p-team");
    assert_eq!(update(&mut state, Msg::InvitesRead("p-team".to_owned(), Ok(vec![])), at(NOW)), [Effect::MakeInvite { room: "p-team".to_owned(), replacing: None }], "no link yet: one is made");
    update(&mut state, Msg::InviteMade("p-team".to_owned(), Ok(invite("K7QFM2XA9P", 7.0, 10))), at(NOW));
    let RoomsScreenVm::Invite(shown) = screen(&state) else { panic!("the link") };
    assert_eq!((shown.code.as_deref(), shown.left.as_str()), (Some("K7QF·M2XA·9P"), "7일 남음 · 10번 더 쓸 수 있어요"));
    assert_eq!(shown.subtitle, "우리 팀 야근방에 들어올 수 있는 링크예요. 친구에게 보내 주세요.");

    assert_eq!(update(&mut state, Msg::CopyInviteLink, at(NOW)), [Effect::Copy("https://errand-backend.vercel.app/join#K7QF-M2XA-9P".to_owned())]);
    let RoomsScreenVm::Invite(shown) = screen(&state) else { panic!("the link") };
    assert_eq!((shown.copied, shown.copy_link), (true, "복사했어요"));
    assert_eq!(update(&mut state, Msg::CopyInviteCode, at(NOW)), [Effect::Copy("K7QF-M2XA-9P".to_owned())]);
    assert_eq!(
        update(&mut state, Msg::RenewInvite("p-team".to_owned()), at(NOW)),
        [Effect::MakeInvite { room: "p-team".to_owned(), replacing: Some("K7QFM2XA9P".to_owned()) }],
        "a new link, the old one gone first"
    );
}

#[test]
fn too_many_rooms_made_says_why_and_keeps_the_form() {
    let mut state = state();
    update(&mut state, Msg::ShowCreateRoom, at(NOW));
    update(&mut state, Msg::CreateRoom { name: "여섯 번째 방".to_owned(), tagline: String::new() }, at(NOW));
    update(&mut state, Msg::RoomCreated(Err(RoomError::TooManyOwned)), at(NOW));
    let RoomsScreenVm::Create(create) = screen(&state) else { panic!("still naming it") };
    assert_eq!((create.sending, create.failure.as_deref()), (false, Some("방은 5개까지 만들 수 있어요. 만든 방을 없애면 새로 만들 수 있어요.")));
}

#[test]
fn only_the_owner_sees_the_link_and_a_member_can_quit_from_the_list() {
    let mut state = state();
    let owned = private("p-own", "내 방", Some(Role::Owner));
    let joined = private("p-friend", "친구 방", Some(Role::Member));
    update(&mut state, Msg::RoomsRead(Ok(vec![owned, joined])), at(NOW));
    let list = view::rooms(&state, &context());
    assert_eq!((list.create, list.enter_code), ("방 만들기", "초대 코드 입력"));
    let card = |id: &str| list.list.iter().find(|card| card.id == id).cloned().unwrap();
    assert_eq!((card("p-own").invite, card("p-own").quit), (Some("초대"), None));
    assert_eq!((card("p-friend").invite, card("p-friend").quit), (None, Some("빠지기")), "a member not in it can drop it for good");
    assert!(update(&mut state, Msg::ShowInvite("p-friend".to_owned()), at(NOW)).is_empty(), "no link for someone else's room");
    assert_eq!(update(&mut state, Msg::ShowInvite("p-own".to_owned()), at(NOW)), [Effect::ReadInvites("p-own".to_owned())]);
}

#[test]
fn a_code_shows_its_room_before_anyone_goes_in() {
    let mut state = state();
    update(&mut state, Msg::ShowInviteCode, at(NOW));
    assert!(update(&mut state, Msg::CheckInvite("K7QF".to_owned()), at(NOW)).is_empty(), "a code too short is not sent");
    let RoomsScreenVm::Code(code) = screen(&state) else { panic!("the code") };
    assert_eq!(code.failure.as_deref(), Some("코드가 맞지 않거나 기한이 지났어요."));
    assert_eq!(update(&mut state, Msg::CheckInvite("k7qf-m2xa-9p".to_owned()), at(NOW)), [Effect::CheckInvite("K7QFM2XA9P".to_owned())]);
    assert!(update(&mut state, Msg::AcceptInvite, at(NOW)).is_empty(), "nothing to accept before the room is known");
    update(&mut state, Msg::InviteChecked("K7QFM2XA9P".to_owned(), Ok(private("p-team", "우리 팀 야근방", None))), at(NOW));
    let RoomsScreenVm::Code(code) = screen(&state) else { panic!("the code") };
    assert_eq!((code.room.as_ref().map(|room| room.name.as_str()), code.accept), (Some("우리 팀 야근방"), "들어가기"));
    assert!(!state.rooms.is_joined("p-team"), "seeing the room lets no one in");

    assert_eq!(update(&mut state, Msg::AcceptInvite, at(NOW)), [Effect::AcceptInvite("K7QFM2XA9P".to_owned())]);
    let effects = update(&mut state, Msg::InviteAccepted("K7QFM2XA9P".to_owned(), Ok(private("p-team", "우리 팀 야근방", Some(Role::Member)))), at(NOW));
    assert!(effects.contains(&Effect::SyncRoom { room: "p-team".to_owned(), visibility: Visibility::Private }), "accepted, the person goes in");
    assert_eq!((state.rooms.screen.clone(), state.rooms.expanded.as_deref()), (RoomsScreen::List, Some("p-team")));
}

#[test]
fn a_link_opens_the_rooms_on_the_room_it_invites_to() {
    let mut state = AppState::new(ResetHistory::new(), SessionRecords::new(at(NOW)), WidgetPreferences::default(), "한결".to_owned());
    assert!(update(&mut state, Msg::OpenLink("errand://settings".to_owned()), at(NOW)).is_empty(), "a link Errand does not know does nothing");
    let effects = update(&mut state, Msg::OpenLink("errand://invite/K7QF-M2XA-9P".to_owned()), at(NOW));
    assert!(state.rooms.open && effects.contains(&Effect::CheckInvite("K7QFM2XA9P".to_owned())));
    let RoomsScreenVm::Code(code) = screen(&state) else { panic!("the code") };
    assert_eq!((code.from_link, code.title), (true, "초대받은 방"));
    update(&mut state, Msg::InviteChecked("K7QFM2XA9P".to_owned(), Err(RoomError::InviteNotFound)), at(NOW));
    let RoomsScreenVm::Code(code) = screen(&state) else { panic!("the code") };
    assert_eq!(code.failure.as_deref(), Some("코드가 맞지 않거나 기한이 지났어요."));
    update(&mut state, Msg::CloseRooms, at(NOW));
    assert_eq!(state.rooms.screen, RoomsScreen::List, "closed, the rooms open on the list next time");
}

#[test]
fn leaving_for_good_and_turning_people_out_ask_first() {
    let mut state = state();
    update(&mut state, Msg::RoomsRead(Ok(vec![private("p-own", "내 방", Some(Role::Owner)), private("p-friend", "친구 방", Some(Role::Member))])), at(NOW));
    assert_eq!(
        update(&mut state, Msg::AskQuitRoom("p-own".to_owned()), at(NOW)),
        [Effect::Confirm(Confirm::QuitRoom { room: "p-own".to_owned(), name: "내 방".to_owned(), owner: true })]
    );
    let (title, _, go) = Language::Korean.confirm(&Confirm::QuitRoom { room: "p-own".to_owned(), name: "내 방".to_owned(), owner: true });
    assert_eq!((title.as_str(), go), ("내 방을(를) 없앨까요?", "방 없애기"));
    assert!(state.rooms.list.iter().any(|room| room.id == "p-own"), "nothing happens before the answer");
    assert_eq!(
        update(&mut state, Msg::Confirmed(Confirm::QuitRoom { room: "p-friend".to_owned(), name: "친구 방".to_owned(), owner: false }), at(NOW)),
        [Effect::QuitRoom("p-friend".to_owned())]
    );
    assert!(!state.rooms.list.iter().any(|room| room.id == "p-friend"), "gone from the list at once");

    // In their own room, the owner can turn someone out from their row; nobody else can, and never themselves.
    update(&mut state, Msg::JoinRoom("p-own".to_owned()), at(NOW));
    let someone = |name: &str, me: bool| Member {
        id: format!("id-{name}"),
        name: name.to_owned(),
        avatar: None,
        tasks: TaskCounts::default(),
        last_active: at(NOW),
        joined_at: None,
        me,
    };
    let room = Room {
        id: "p-own".to_owned(),
        name: "내 방".to_owned(),
        tagline: String::new(),
        visibility: Visibility::Private,
        members: vec![someone("한결", true), someone("민지", false)],
        role: Some(Role::Owner),
    };
    update(&mut state, Msg::RoomRead("p-own".to_owned(), Ok(room.clone())), at(NOW));
    let rows = view::dock(&state, &context()).rooms.unwrap().members;
    assert_eq!(rows.iter().map(|row| row.remove.is_some()).collect::<Vec<_>>(), [false, true]);
    assert!(rows[1].tooltip.ends_with("우클릭해서 내보내기"));
    let remove = rows[1].remove.clone().unwrap();
    assert_eq!(
        update(&mut state, Msg::AskRemoveMember { room: remove.room.clone(), member: remove.member.clone(), name: remove.name.clone() }, at(NOW)),
        [Effect::Confirm(Confirm::RemoveMember { room: "p-own".to_owned(), member: "id-민지".to_owned(), name: "민지".to_owned() })]
    );
    assert_eq!(
        update(&mut state, Msg::Confirmed(Confirm::RemoveMember { room: "p-own".to_owned(), member: "id-민지".to_owned(), name: "민지".to_owned() }), at(NOW)),
        [Effect::RemoveMember { room: "p-own".to_owned(), member: "id-민지".to_owned() }]
    );
    assert_eq!(view::dock(&state, &context()).rooms.unwrap().members.len(), 1, "their row goes at once");

    update(&mut state, Msg::RoomRead("p-own".to_owned(), Ok(Room { role: Some(Role::Member), ..room })), at(NOW));
    assert!(view::dock(&state, &context()).rooms.unwrap().members.iter().all(|row| row.remove.is_none()), "a member turns no one out");
}
