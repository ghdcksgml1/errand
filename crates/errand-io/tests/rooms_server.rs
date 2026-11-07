//! The rooms server through fake networking: what each call sends, how answers and refusals read, registering
//! this install, and that its ID goes to the rooms server and nowhere else.

use errand_core::avatar::Avatar;
use errand_core::model::{AppState, WidgetPreferences};
use errand_core::ports::{HttpClient, HttpFailure, HttpMethod, HttpRequest, HttpResponse, RoomService};
use errand_core::reset_history::ResetHistory;
use errand_core::rooms::{self, Face, Identity, Member, PresenceReport, RoomError, RoomSummary, Visibility};
use errand_core::sessions::{SessionRecords, TaskCounts};
use errand_core::{Language, Timestamp};
use errand_io::api::rooms::{HttpRooms, RoomsServer, RoomsSource};
use errand_io::api::{Api, Endpoint, Service};
use errand_io::codex::CodexSource;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// This install's ID in these tests.
const ID: &str = "0b0d3f7e-1a2b-4c3d-8e9f-0123456789ab";
/// The seed this install first drew its character from.
const SEED: &str = "5f0c2b8e-61d4-4c3a-9a57-0e1f2d3c4b5a";
const SERVER: &str = "https://errand-backend.vercel.app";

#[derive(Default)]
struct FakeServer {
    replies: Mutex<VecDeque<Result<HttpResponse, HttpFailure>>>,
    requests: Mutex<Vec<HttpRequest>>,
}

impl FakeServer {
    fn replying(replies: Vec<Result<HttpResponse, HttpFailure>>) -> Arc<FakeServer> {
        Arc::new(FakeServer { replies: Mutex::new(replies.into()), requests: Mutex::default() })
    }
    fn requests(&self) -> Vec<HttpRequest> {
        self.requests.lock().unwrap().clone()
    }
    /// Each request as its method and address.
    fn calls(&self) -> Vec<(HttpMethod, String)> {
        self.requests().into_iter().map(|request| (request.method, request.url)).collect()
    }
}

impl HttpClient for FakeServer {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, HttpFailure> {
        self.requests.lock().unwrap().push(request.clone());
        self.replies.lock().unwrap().pop_front().unwrap_or(Err(HttpFailure))
    }
}

fn reply(status: u16, body: Value) -> Result<HttpResponse, HttpFailure> {
    Ok(HttpResponse { status, body: body.to_string().into_bytes(), retry_after: None })
}

fn raw(status: u16, body: &str) -> Result<HttpResponse, HttpFailure> {
    Ok(HttpResponse { status, body: body.as_bytes().to_vec(), retry_after: None })
}

fn waiting(retry_after: &str) -> Result<HttpResponse, HttpFailure> {
    Ok(HttpResponse { status: 429, body: json!({"error": "rate_limited"}).to_string().into_bytes(), retry_after: Some(retry_after.to_owned()) })
}

fn registered() -> Result<HttpResponse, HttpFailure> {
    raw(204, "")
}

fn api(network: &Arc<FakeServer>, server: RoomsServer) -> Arc<Api> {
    let home = PathBuf::from("/var/empty/errand-rooms-tests");
    let codex = CodexSource { executable: None, home: home.join(".codex"), directory: home.clone() };
    Arc::new(Api::new(network.clone(), home.join("claude-usage.plist"), codex).with_rooms_server(server))
}

fn client_at(network: &Arc<FakeServer>, server: RoomsServer, language: Language) -> HttpRooms {
    HttpRooms::new(api(network, server), || ID.to_owned(), language)
}

fn client(network: &Arc<FakeServer>) -> HttpRooms {
    client_at(network, RoomsServer::official(), Language::Korean)
}

fn at(text: &str) -> Timestamp {
    text.parse().unwrap()
}

fn now() -> Timestamp {
    at("2026-11-04T13:05:00Z")
}

fn report(identity: Identity) -> PresenceReport {
    PresenceReport { identity, avatar: Avatar::named(SEED), tasks: TaskCounts { new: 1, running: 2, seen: 3 } }
}

/// The server's answer to a report: the room and everyone in it.
fn late_night(visibility: &str) -> Value {
    json!({
        "id": "late-night", "name": "고독한 야근방", "tagline": "불 꺼진 사무실, 모니터만 켜진 사람들", "visibility": visibility,
        "members": [
            {"id": "6f1c0e4e-0b7a-4c1e-9d55-2a8f3c41b0de", "name": "졸린 수달", "avatar": "avatar:cloud,sprout,sleepy,round,hoodie,plain,14B8A6",
             "tasks": {"new": 0, "running": 2, "seen": 5}, "lastActive": "2026-11-04T13:02:11.077Z", "joinedAt": "2026-11-04T12:40:02.123Z", "me": true},
            // From a server that does not say when someone came in.
            {"id": "0d6a", "name": "바쁜 너구리", "avatar": null, "tasks": {"new": 1, "running": 0, "seen": 0}, "lastActive": "2026-11-04T11:40:00Z", "me": false},
        ],
    })
}

fn header<'a>(request: &'a HttpRequest, name: &str) -> Option<&'a str> {
    request.headers.iter().find(|(field, _)| field == name).map(|(_, value)| value.as_str())
}

fn body(request: &HttpRequest) -> Value {
    serde_json::from_slice(request.body.as_deref().expect("a body")).unwrap()
}

#[test]
fn every_call_carries_the_install_id_errands_version_and_language() {
    let network = FakeServer::replying(vec![registered(), reply(200, json!({"rooms": []})), reply(200, json!({"rooms": []}))]);
    let rooms = client_at(&network, RoomsServer::official(), Language::English);
    assert_eq!(rooms.rooms(now()), Ok(vec![]));
    assert_eq!(rooms.rooms(now()), Ok(vec![]));
    assert_eq!(
        network.calls(),
        [
            (HttpMethod::Post, format!("{SERVER}/v1/devices")),
            (HttpMethod::Get, format!("{SERVER}/v1/rooms")),
            (HttpMethod::Get, format!("{SERVER}/v1/rooms")),
        ],
        "the first call of a run registers this install, and only the first"
    );
    for request in network.requests() {
        assert_eq!(header(&request, "Authorization"), Some(format!("Bearer {ID}").as_str()));
        assert_eq!(header(&request, "User-Agent"), Some(format!("Errand/{}", env!("CARGO_PKG_VERSION")).as_str()));
        assert_eq!(header(&request, "Accept-Language"), Some("en"), "aliases come back in the language Errand speaks");
        assert_eq!((header(&request, "Content-Type"), &request.body), (None, &None), "nothing to say but who is asking");
    }
    let korean = FakeServer::replying(vec![registered(), reply(200, json!({"rooms": []}))]);
    client_at(&korean, RoomsServer::official(), Language::Korean).rooms(now()).unwrap();
    assert_eq!(header(&korean.requests()[1], "Accept-Language"), Some("ko"));
}

#[test]
fn a_room_hears_three_counts_and_the_character_and_a_private_one_the_nickname_too() {
    let network = FakeServer::replying(vec![registered(), reply(200, late_night("public")), reply(200, late_night("private"))]);
    let rooms = client(&network);
    rooms.sync("late-night", &report(Identity::Anonymous), now()).unwrap();
    rooms.sync("late-night", &report(Identity::Profile("한결".to_owned())), now()).unwrap();
    let requests = network.requests();
    let (public, private) = (&requests[1], &requests[2]);
    assert_eq!((public.method, public.url.as_str()), (HttpMethod::Put, "https://errand-backend.vercel.app/v1/rooms/late-night/presence"));
    assert_eq!(header(public, "Content-Type"), Some("application/json"));
    let character = Avatar::named(SEED).code();
    assert_eq!(body(public), json!({"avatar": character, "tasks": {"new": 1, "running": 2, "seen": 3}}), "no profile at all in a public room");
    assert_eq!(body(private), json!({"profile": {"nickname": "한결"}, "avatar": character, "tasks": {"new": 1, "running": 2, "seen": 3}}));
    for request in &requests[1..] {
        let sent = String::from_utf8(request.body.clone().unwrap()).unwrap();
        assert!(!sent.contains(SEED), "the character's parts, never the seed: {sent}");
    }
}

#[test]
fn the_character_sent_is_always_its_parts() {
    let network = FakeServer::replying(vec![registered(), reply(200, late_night("public")), reply(200, late_night("public"))]);
    let rooms = client(&network);
    let mut state = AppState::new(ResetHistory::new(), SessionRecords::new(now()), WidgetPreferences::default(), "한결".to_owned());
    for kept in [SEED.to_owned(), Avatar::from_seed("한결").code()] {
        state.profile.avatar = kept.clone();
        rooms.sync("late-night", &rooms::presence_report(&state, Visibility::Public), now()).unwrap();
        let sent = body(network.requests().last().unwrap());
        let code = sent["avatar"].as_str().unwrap();
        assert!(code.starts_with("avatar:") && Avatar::parse(code) == Some(Avatar::named(&kept)), "{kept} went as {code}");
        assert!(!code.contains(SEED));
    }
}

#[test]
fn answers_read_as_rooms_and_the_people_in_them() {
    let list = json!({"rooms": [
        {"id": "late-night", "name": "고독한 야근방", "tagline": "불 꺼진 사무실", "visibility": "public", "members": 7, "working": 4,
         "faces": [{"name": "졸린 수달", "avatar": "avatar:cloud,sprout,sleepy,round,hoodie,plain,14B8A6"}, {"name": "바쁜 너구리", "avatar": null}]},
        {"id": "team-x7k2m9q4w8e1", "name": "우리 팀", "visibility": "private", "members": 2, "working": 0, "faces": []},
        // Not a room Errand can join or ask after.
        {"id": "../devices", "name": "엉뚱한 방", "visibility": "public", "members": 1, "working": 0, "faces": []},
        {"name": "이름만 있는 방"},
    ]});
    let network = FakeServer::replying(vec![registered(), reply(200, list), reply(200, late_night("public"))]);
    let rooms = client(&network);
    let faces = vec![
        Face { name: "졸린 수달".to_owned(), avatar: Some("avatar:cloud,sprout,sleepy,round,hoodie,plain,14B8A6".to_owned()) },
        Face { name: "바쁜 너구리".to_owned(), avatar: None },
    ];
    assert_eq!(
        rooms.rooms(now()),
        Ok(vec![
            RoomSummary {
                id: "late-night".to_owned(),
                name: "고독한 야근방".to_owned(),
                tagline: "불 꺼진 사무실".to_owned(),
                visibility: Visibility::Public,
                members: 7,
                working: 4,
                faces,
                role: None
            },
            RoomSummary {
                id: "team-x7k2m9q4w8e1".to_owned(),
                name: "우리 팀".to_owned(),
                tagline: String::new(),
                visibility: Visibility::Private,
                members: 2,
                working: 0,
                faces: vec![],
                role: None
            },
        ])
    );
    let room = rooms.sync("late-night", &report(Identity::Anonymous), now()).unwrap();
    assert_eq!((room.id.as_str(), room.name.as_str(), room.visibility), ("late-night", "고독한 야근방", Visibility::Public));
    assert_eq!(
        room.members,
        [
            Member {
                id: "6f1c0e4e-0b7a-4c1e-9d55-2a8f3c41b0de".to_owned(),
                name: "졸린 수달".to_owned(),
                avatar: Some("avatar:cloud,sprout,sleepy,round,hoodie,plain,14B8A6".to_owned()),
                tasks: TaskCounts { new: 0, running: 2, seen: 5 },
                last_active: at("2026-11-04T13:02:11.077Z"),
                joined_at: Some(at("2026-11-04T12:40:02.123Z")),
                me: true,
            },
            Member {
                id: "0d6a".to_owned(),
                name: "바쁜 너구리".to_owned(),
                avatar: None,
                tasks: TaskCounts { new: 1, running: 0, seen: 0 },
                last_active: at("2026-11-04T11:40:00Z"),
                joined_at: None,
                me: false,
            },
        ]
    );
}

#[test]
fn when_someone_came_in_reads_only_from_a_time() {
    let mut answer = late_night("public");
    answer["members"][0]["joinedAt"] = json!("a while ago");
    answer["members"][1]["joinedAt"] = json!(1_792_000_000);
    let network = FakeServer::replying(vec![registered(), reply(200, answer)]);
    let room = client(&network).sync("late-night", &report(Identity::Anonymous), now()).unwrap();
    assert!(room.members.iter().all(|member| member.joined_at.is_none()), "no time in the room rather than a wrong one");
}

#[test]
fn an_answer_that_is_not_the_rooms_reads_as_no_answer() {
    let network = FakeServer::replying(vec![
        registered(),
        reply(200, json!({"id": "dawn", "name": "새벽 코딩방", "visibility": "public", "members": []})),
        raw(200, "<html>Vercel</html>"),
        reply(200, json!({"no": "rooms"})),
    ]);
    let rooms = client(&network);
    assert_eq!(rooms.sync("late-night", &report(Identity::Anonymous), now()), Err(RoomError::Unreachable), "an answer about another room");
    assert_eq!(rooms.sync("late-night", &report(Identity::Anonymous), now()), Err(RoomError::Unreachable));
    assert_eq!(rooms.rooms(now()), Err(RoomError::Unreachable));
}

#[test]
fn a_server_that_forgot_this_install_hears_it_register_again_and_the_call_once_more() {
    let network = FakeServer::replying(vec![registered(), reply(401, json!({"error": "unknown_device"})), registered(), reply(200, late_night("public"))]);
    let rooms = client(&network);
    assert!(rooms.sync("late-night", &report(Identity::Anonymous), now()).is_ok());
    let presence = "https://errand-backend.vercel.app/v1/rooms/late-night/presence".to_owned();
    let devices = format!("{SERVER}/v1/devices");
    assert_eq!(
        network.calls(),
        [(HttpMethod::Post, devices.clone()), (HttpMethod::Put, presence.clone()), (HttpMethod::Post, devices.clone()), (HttpMethod::Put, presence.clone())]
    );

    let stubborn = FakeServer::replying(vec![
        registered(),
        reply(401, json!({"error": "unknown_device"})),
        registered(),
        reply(401, json!({"error": "unknown_device"})),
        reply(200, late_night("public")),
    ]);
    let rooms = client(&stubborn);
    assert_eq!(rooms.sync("late-night", &report(Identity::Anonymous), now()), Err(RoomError::Unauthorized), "once more, and no more");
    assert_eq!(stubborn.requests().len(), 4);
    assert!(rooms.sync("late-night", &report(Identity::Anonymous), now()).is_ok(), "registered, the next call goes straight out");
    assert_eq!(stubborn.calls().last(), Some(&(HttpMethod::Put, presence)));
    assert_eq!(stubborn.requests().len(), 5);

    let unregistered = FakeServer::replying(vec![raw(500, "")]);
    assert_eq!(client(&unregistered).rooms(now()), Err(RoomError::Unreachable));
    assert_eq!(unregistered.calls(), [(HttpMethod::Post, devices)], "nothing else goes before the install is registered");
}

#[test]
fn refusals_read_as_what_the_app_does_about_them() {
    let cases = [
        (reply(404, json!({"error": "room_not_found"})), RoomError::NotFound),
        // No rooms server at the address: no tab closes for that.
        (reply(404, json!({"error": {"code": "DEPLOYMENT_NOT_FOUND"}})), RoomError::Unreachable),
        (raw(404, "<html>Not found</html>"), RoomError::Unreachable),
        (reply(409, json!({"error": "room_full"})), RoomError::Full),
        (reply(409, json!({"error": "too_many_rooms"})), RoomError::TooManyRooms),
        (reply(400, json!({"error": "invalid_report"})), RoomError::Rejected),
        (reply(413, json!({})), RoomError::Rejected),
        (reply(500, json!({})), RoomError::Unreachable),
        (reply(503, json!({})), RoomError::Unreachable),
        (Err(HttpFailure), RoomError::Unreachable),
    ];
    for (answer, expected) in cases {
        let network = FakeServer::replying(vec![registered(), answer.clone()]);
        assert_eq!(client(&network).sync("late-night", &report(Identity::Anonymous), now()), Err(expected), "{answer:?}");
    }
}

#[test]
fn a_version_the_server_no_longer_serves_sends_nothing_more_this_run() {
    let network = FakeServer::replying(vec![registered(), reply(426, json!({"error": "client_too_old"})), reply(200, json!({"rooms": []}))]);
    let rooms = client(&network);
    assert_eq!(rooms.sync("late-night", &report(Identity::Anonymous), now()), Err(RoomError::ClientTooOld));
    assert_eq!(rooms.rooms(now()), Err(RoomError::ClientTooOld));
    rooms.leave("late-night");
    assert_eq!(network.requests().len(), 2);
}

#[test]
fn a_rate_limit_holds_every_rooms_call_back_as_long_as_it_asks() {
    let network = FakeServer::replying(vec![registered(), waiting("120"), reply(200, json!({"rooms": []}))]);
    let rooms = client(&network);
    assert_eq!(rooms.sync("late-night", &report(Identity::Anonymous), now()), Err(RoomError::RateLimited));
    assert_eq!(rooms.rooms(now()), Err(RoomError::RateLimited), "the wait holds the list back too");
    assert_eq!(network.requests().len(), 2);

    let brief = FakeServer::replying(vec![registered(), waiting("0"), reply(200, json!({"rooms": []}))]);
    let rooms = client(&brief);
    assert_eq!(rooms.rooms(now()), Err(RoomError::RateLimited));
    assert_eq!(rooms.rooms(now()), Ok(vec![]), "a wait that is over holds nothing back");

    let registering = FakeServer::replying(vec![waiting("60")]);
    let rooms = client(&registering);
    assert_eq!(rooms.rooms(now()), Err(RoomError::RateLimited), "even registering waits");
    assert_eq!(rooms.rooms(now()), Err(RoomError::RateLimited));
    assert_eq!(registering.requests().len(), 1);
}

#[test]
fn leaving_tells_the_room() {
    let network = FakeServer::replying(vec![registered(), raw(204, ""), reply(404, json!({"error": "room_not_found"}))]);
    let rooms = client(&network);
    rooms.leave("late-night");
    rooms.leave("dawn");
    let presence = |room: &str| (HttpMethod::Delete, format!("{SERVER}/v1/rooms/{room}/presence"));
    assert_eq!(network.calls()[1..], [presence("late-night"), presence("dawn")]);
}

/// A call to the rooms server, addressed anywhere.
struct Addressed(&'static str);

impl Endpoint for Addressed {
    type Answer = u16;
    type Error = RoomError;
    const SERVICE: Service = Service::Rooms;

    fn request(&self) -> HttpRequest {
        let headers = vec![("Authorization".to_owned(), format!("Bearer {ID}"))];
        HttpRequest { method: HttpMethod::Get, url: self.0.to_owned(), headers, body: None, timeout: Duration::from_secs(1) }
    }

    fn answer(&self, response: HttpResponse) -> Result<u16, RoomError> {
        Ok(response.status)
    }
}

#[test]
fn the_install_id_goes_to_the_one_rooms_server_and_nowhere_else() {
    let network = FakeServer::replying(vec![raw(204, ""), raw(204, "")]);
    let local = RoomsServer::at("http://127.0.0.1:8787").unwrap();
    let api = api(&network, local.clone());
    let elsewhere = [
        "https://errand-backend.vercel.app/v1/rooms",
        "https://127.0.0.1:8787/v1/rooms",
        "http://127.0.0.1:8788/v1/rooms",
        "http://127.0.0.1.example.com:8787/v1/rooms",
        "http://127.0.0.1:8787@example.com/v1/rooms",
        "https://elsewhere.example/v1/rooms",
    ];
    for url in elsewhere {
        assert_eq!(api.send(&Addressed(url)), Err(RoomError::Unreachable), "{url}");
    }
    assert!(network.requests().is_empty(), "nothing addressed elsewhere leaves");
    assert_eq!(api.send(&Addressed("http://127.0.0.1:8787/v1/rooms")), Ok(204));

    // Every call the rooms make goes to that server.
    let network = FakeServer::replying(vec![registered(), reply(200, json!({"rooms": []}))]);
    client_at(&network, local, Language::Korean).rooms(now()).unwrap();
    assert!(network.requests().iter().all(|request| request.url.starts_with("http://127.0.0.1:8787/v1/")));
}

#[test]
fn the_rooms_are_errand_backends_unless_the_setting_says_otherwise() {
    let official = Some(RoomsSource::Server(RoomsServer::official()));
    assert_eq!(RoomsSource::from_setting(None), official, "without ERRAND_ROOMS_API, errand-backend");
    assert_eq!(RoomsSource::from_setting(Some("  ")), official, "an empty setting is no setting");
    assert_eq!(RoomsSource::from_setting(Some("https://errand-backend.vercel.app/")), official);
    assert_eq!(RoomsSource::from_setting(Some("sample")), Some(RoomsSource::Sample), "the sample rooms, to work offline");
    assert_eq!(RoomsSource::from_setting(Some(" Sample ")), Some(RoomsSource::Sample));
    let local = RoomsServer::at("http://127.0.0.1:3000").unwrap();
    assert_eq!(RoomsSource::from_setting(Some("http://127.0.0.1:3000")), Some(RoomsSource::Server(local)), "a server run on this computer");
    for unusable in ["http://localhost:3000", "samples", "errand-backend.vercel.app"] {
        assert_eq!(RoomsSource::from_setting(Some(unusable)), None, "{unusable}: no server this install's ID would go to");
    }

    // An Api told of no other server calls errand-backend; the network here is a stand-in.
    let network = FakeServer::replying(vec![registered(), reply(200, json!({"rooms": []}))]);
    let home = PathBuf::from("/var/empty/errand-rooms-tests");
    let codex = CodexSource { executable: None, home: home.join(".codex"), directory: home.clone() };
    let api = Arc::new(Api::new(network.clone(), home.join("claude-usage.plist"), codex));
    assert_eq!(api.rooms_server(), &RoomsServer::official());
    HttpRooms::new(api, || ID.to_owned(), Language::Korean).rooms(now()).unwrap();
    assert_eq!(network.calls(), [(HttpMethod::Post, format!("{SERVER}/v1/devices")), (HttpMethod::Get, format!("{SERVER}/v1/rooms"))]);
}

#[test]
fn the_rooms_server_is_reached_over_https_or_on_this_computer() {
    for address in ["https://errand-backend.vercel.app", "https://errand-backend.vercel.app/", " https://errand-rooms.vercel.app ", "https://localhost:8443"] {
        assert!(RoomsServer::at(address).is_some(), "{address}");
    }
    assert_eq!(RoomsServer::at("https://errand-backend.vercel.app/"), Some(RoomsServer::official()));
    assert_eq!(RoomsServer::at("https://errand-backend.vercel.app:443"), Some(RoomsServer::official()));
    assert_eq!(RoomsServer::at("http://127.0.0.1:3000/").map(|server| server.origin().url("/v1/rooms")).as_deref(), Some("http://127.0.0.1:3000/v1/rooms"));
    assert_eq!(RoomsServer::at("http://127.0.0.1").map(|server| server.origin().url("/v1/rooms")).as_deref(), Some("http://127.0.0.1/v1/rooms"));
    let refused = [
        "http://errand-backend.vercel.app",
        "http://localhost:3000",
        "http://192.168.0.7:3000",
        "http://127.0.0.1.example.com",
        "https://someone@errand-backend.vercel.app",
        "https://errand-backend.vercel.app/v1",
        "https://errand-backend.vercel.app?next=elsewhere",
        "https://errand-backend.vercel.app:http",
        "https://",
        "errand-backend.vercel.app",
        "ftp://errand-backend.vercel.app",
        "",
    ];
    for address in refused {
        assert_eq!(RoomsServer::at(address), None, "{address:?}");
    }
}

#[test]
fn the_install_id_is_read_when_first_needed_and_only_sent_when_it_is_errands() {
    let reads = Arc::new(AtomicUsize::new(0));
    let network = FakeServer::replying(vec![registered(), reply(200, json!({"rooms": []})), reply(200, json!({"rooms": []}))]);
    let counted = reads.clone();
    let rooms = HttpRooms::new(api(&network, RoomsServer::official()), move || {
        counted.fetch_add(1, Ordering::Relaxed);
        ID.to_owned()
    }, Language::Korean);
    assert_eq!(reads.load(Ordering::Relaxed), 0, "a run that never calls the rooms never reads it");
    rooms.rooms(now()).unwrap();
    rooms.rooms(now()).unwrap();
    assert_eq!(reads.load(Ordering::Relaxed), 1);

    let network = FakeServer::replying(vec![registered()]);
    let tampered = HttpRooms::new(api(&network, RoomsServer::official()), || "1234\r\nX-Other: header".to_owned(), Language::Korean);
    assert_eq!(tampered.rooms(now()), Err(RoomError::Rejected), "not an ID Errand made");
    assert!(network.requests().is_empty());
}

#[test]
fn only_a_room_id_the_server_makes_goes_into_an_address() {
    let network = FakeServer::replying(vec![registered(), reply(200, late_night("public"))]);
    let rooms = client(&network);
    for room in ["../devices", "late-night/../../v1/devices", "late night", "late-night?x=1", "", "방"] {
        assert_eq!(rooms.sync(room, &report(Identity::Anonymous), now()), Err(RoomError::NotFound), "{room:?}");
        rooms.leave(room);
    }
    assert!(network.requests().is_empty());
}

#[test]
fn no_refusal_or_error_says_the_install_id() {
    let network = FakeServer::replying(vec![registered(), reply(400, json!({"error": "invalid_report", "id": ID}))]);
    let failure = client(&network).sync("late-night", &report(Identity::Anonymous), now());
    assert_eq!(failure, Err(RoomError::Rejected));
    assert!(!format!("{failure:?}").contains(ID));
}

#[test]
fn private_rooms_are_made_invited_to_and_left_on_the_server() {
    let summary = |role: Value| json!({"id": "p-0123456789ab", "name": "우리 팀", "tagline": "", "visibility": "private", "members": 0, "working": 0, "faces": [], "role": role});
    let invite = json!({"code": "K7QFM2XA9P", "expiresAt": "2026-11-11T13:05:00Z", "usesLeft": 10});
    let network = FakeServer::replying(vec![
        registered(),
        reply(201, summary(json!("owner"))),
        reply(200, json!({"invites": [invite.clone(), {"code": "not a code", "expiresAt": "2026-11-11T13:05:00Z", "usesLeft": 3}]})),
        reply(201, invite.clone()),
        raw(204, ""),
        reply(200, summary(Value::Null)),
        reply(200, summary(json!("member"))),
        raw(204, ""),
        raw(204, ""),
        reply(404, json!({"error": "invite_not_found"})),
        reply(409, json!({"error": "too_many_owned_rooms"})),
        reply(409, json!({"error": "too_many_invites"})),
    ]);
    let rooms = client(&network);
    let made = rooms.create_room("우리 팀", "", now()).unwrap();
    assert_eq!((made.id.as_str(), made.role), ("p-0123456789ab", Some(rooms::Role::Owner)));
    let live = rooms.invites("p-0123456789ab", now()).unwrap();
    assert_eq!(live, [rooms::Invite { code: "K7QFM2XA9P".to_owned(), expires_at: at("2026-11-11T13:05:00Z"), uses_left: 10 }], "a code that does not read is left out");
    assert_eq!(rooms.make_invite("p-0123456789ab", now()).unwrap().code, "K7QFM2XA9P");
    rooms.delete_invite("p-0123456789ab", "K7QF·M2XA·9P").unwrap();
    assert_eq!(rooms.check_invite("k7qf-m2xa-9p", now()).unwrap().role, None);
    assert_eq!(rooms.accept_invite("K7QFM2XA9P", now()).unwrap().role, Some(rooms::Role::Member));
    rooms.quit_room("p-0123456789ab").unwrap();
    rooms.remove_member("p-0123456789ab", "6f9b1c2d-3e4f-4a5b-8c7d-9e0f1a2b3c4d").unwrap();
    assert_eq!(rooms.check_invite("K7QFM2XA9P", now()), Err(RoomError::InviteNotFound));
    assert_eq!(rooms.create_room("여섯 번째", "", now()), Err(RoomError::TooManyOwned));
    assert_eq!(rooms.make_invite("p-0123456789ab", now()), Err(RoomError::TooManyInvites));
    let calls: Vec<(HttpMethod, String)> = network.calls().into_iter().skip(1).map(|(method, url)| (method, url.replace(SERVER, ""))).collect();
    assert_eq!(
        calls,
        [
            (HttpMethod::Post, "/v1/rooms".to_owned()),
            (HttpMethod::Get, "/v1/rooms/p-0123456789ab/invites".to_owned()),
            (HttpMethod::Post, "/v1/rooms/p-0123456789ab/invites".to_owned()),
            (HttpMethod::Post, "/v1/rooms/p-0123456789ab/invites/revoke".to_owned()),
            (HttpMethod::Post, "/v1/invites/preview".to_owned()),
            (HttpMethod::Post, "/v1/invites/accept".to_owned()),
            (HttpMethod::Delete, "/v1/rooms/p-0123456789ab/membership".to_owned()),
            (HttpMethod::Delete, "/v1/rooms/p-0123456789ab/members/6f9b1c2d-3e4f-4a5b-8c7d-9e0f1a2b3c4d".to_owned()),
            (HttpMethod::Post, "/v1/invites/preview".to_owned()),
            (HttpMethod::Post, "/v1/rooms".to_owned()),
            (HttpMethod::Post, "/v1/rooms/p-0123456789ab/invites".to_owned()),
        ]
    );
    let body = |index: usize| serde_json::from_slice::<Value>(network.requests()[index].body.as_deref().unwrap()).unwrap();
    assert_eq!(body(1), json!({"name": "우리 팀", "tagline": ""}));
    assert_eq!([body(4), body(5), body(6)], [json!({"code": "K7QFM2XA9P"}), json!({"code": "K7QFM2XA9P"}), json!({"code": "K7QFM2XA9P"})]);
    assert!(network.calls().iter().all(|(_, url)| !url.contains("K7QF")), "no code is ever in an address, which servers log");
}

#[test]
fn nothing_but_a_code_or_the_servers_own_ids_goes_into_an_address() {
    let network = FakeServer::replying(vec![]);
    let rooms = client(&network);
    assert_eq!(rooms.check_invite("../rooms", now()), Err(RoomError::InviteNotFound));
    assert_eq!(rooms.remove_member("p-0123456789ab", "../../devices"), Err(RoomError::NotFound));
    assert_eq!(rooms.quit_room("a/b"), Err(RoomError::NotFound));
    assert!(network.calls().is_empty(), "none of it was sent");
}

#[test]
fn a_full_private_room_reads_as_too_many_members() {
    let network = FakeServer::replying(vec![registered(), reply(409, json!({"error": "too_many_members"}))]);
    assert_eq!(client(&network).accept_invite("K7QFM2XA9P", now()), Err(RoomError::TooManyMembers));
}
