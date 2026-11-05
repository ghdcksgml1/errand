//! The work rooms' server, errand-backend: the rooms on offer, the person's presence in each, and leaving.
//! The server knows this install only by its ID, a random secret that goes as a bearer token to the rooms
//! server and nowhere else, and is never shown, logged or printed. Besides it, the server hears only the
//! presence report. Errand registers the install the first time it calls in a run, and once more, sending
//! the call again, when the server has forgotten it.

use super::{Api, Endpoint, Origin, Refusal, Service};
use errand_core::ports::{HttpMethod, HttpRequest, HttpResponse, RoomService};
use errand_core::rooms::{Face, Identity, Member, PresenceReport, Room, RoomError, RoomSummary, Visibility, room_id};
use errand_core::sessions::TaskCounts;
use errand_core::{Language, Timestamp};
use serde_json::{Map, Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

/// errand-backend on Vercel, in Seoul.
pub const OFFICIAL_HOST: &str = "errand-backend.vercel.app";
/// Long enough for the server's function to start cold.
const TIMEOUT: Duration = Duration::from_secs(15);
/// A reply this long is not the rooms server's.
const REPLY_LIMIT: usize = 1024 * 1024;
/// Every call says which Errand sends it, so the server can turn away versions it no longer serves.
const USER_AGENT: &str = concat!("Errand/", env!("CARGO_PKG_VERSION"));
/// The most a count can be, as the server takes and gives them.
const MOST_TASKS: usize = 999;
/// How many faces the list shows beside a room's counts.
const FACES: usize = 3;
/// Longer than any character's code: anything longer is not one.
const LONGEST_CODE: usize = 100;
/// Names and taglines are cut here; the server's own are 30 and 60 characters at most.
const LONGEST_TEXT: usize = 200;

/// Where the rooms server is: errand-backend, or during development the address `ERRAND_ROOMS_API` gives.
/// Its calls carry this install's ID, so it is only ever reached over HTTPS, or over plain HTTP to a server
/// run on this computer at 127.0.0.1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomsServer(Origin);

impl RoomsServer {
    pub fn official() -> RoomsServer {
        RoomsServer(Origin::https(OFFICIAL_HOST))
    }

    /// The server at `https://host[:port]`, or `http://127.0.0.1[:port]`, with at most a slash after it. Anything
    /// else is None, rather than a server the ID might reach by mistake: plain HTTP elsewhere, user info, a path.
    pub fn at(address: &str) -> Option<RoomsServer> {
        let address = address.trim();
        let origin = Origin::of(address)?;
        let rest = address.split_once("://")?.1;
        let after = &rest[rest.find(['/', '?', '#']).unwrap_or(rest.len())..];
        let on_this_computer = !origin.secure && origin.host == "127.0.0.1";
        ((origin.secure || on_this_computer) && (after.is_empty() || after == "/")).then_some(RoomsServer(origin))
    }

    pub fn origin(&self) -> &Origin {
        &self.0
    }
}

/// Where the rooms come from, as `ERRAND_ROOMS_API` says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoomsSource {
    /// A rooms server: errand-backend, unless the setting names another.
    Server(RoomsServer),
    /// The sample rooms, for working without the network.
    Sample,
}

impl RoomsSource {
    /// Unset or empty, errand-backend; `sample`, the sample rooms; anything else, the server at that address
    /// ([`RoomsServer::at`]). None when the setting names no server Errand would send this install's ID to.
    pub fn from_setting(setting: Option<&str>) -> Option<RoomsSource> {
        match setting.map(str::trim).filter(|setting| !setting.is_empty()) {
            None => Some(RoomsSource::Server(RoomsServer::official())),
            Some(setting) if setting.eq_ignore_ascii_case("sample") => Some(RoomsSource::Sample),
            Some(address) => RoomsServer::at(address).map(RoomsSource::Server),
        }
    }
}

impl From<Refusal> for RoomError {
    fn from(refusal: Refusal) -> RoomError {
        match refusal {
            Refusal::Waiting(_) => RoomError::RateLimited,
            Refusal::Offline | Refusal::Elsewhere => RoomError::Unreachable,
        }
    }
}

/// This install's ID: the only proof of who is asking, so it is kept like a password. It has no Debug or
/// Display, so no error, log or screen can show it.
struct InstallId(String);

impl InstallId {
    /// The UUID the platform made; anything else is not Errand's own and is never sent.
    fn read(id: String) -> Option<InstallId> {
        let uuid = id.len() == 36 && id.bytes().all(|byte| byte.is_ascii_hexdigit() || byte == b'-');
        uuid.then_some(InstallId(id))
    }
}

/// Who calls, and in which language the server should name people.
#[derive(Clone, Copy)]
struct Caller<'a> {
    server: &'a Origin,
    id: &'a InstallId,
    language: Language,
}

impl Caller<'_> {
    fn request(&self, method: HttpMethod, path: &str, body: Option<Value>) -> HttpRequest {
        let mut headers = vec![
            ("Authorization".to_owned(), format!("Bearer {}", self.id.0)),
            ("User-Agent".to_owned(), USER_AGENT.to_owned()),
            // A public room's aliases come back in the person's language.
            ("Accept-Language".to_owned(), self.language.tag().to_owned()),
            ("Accept".to_owned(), "application/json".to_owned()),
        ];
        if body.is_some() {
            headers.push(("Content-Type".to_owned(), "application/json".to_owned()));
        }
        HttpRequest { method, url: self.server.url(path), headers, body: body.map(|body| body.to_string().into_bytes()), timeout: TIMEOUT }
    }
}

/// Registers this install, or finds it already registered. The server keeps only a hash of the ID.
struct Register<'a>(Caller<'a>);

impl Endpoint for Register<'_> {
    type Answer = ();
    type Error = RoomError;
    const SERVICE: Service = Service::Rooms;

    fn request(&self) -> HttpRequest {
        self.0.request(HttpMethod::Post, "/v1/devices", None)
    }

    fn answer(&self, response: HttpResponse) -> Result<(), RoomError> {
        succeeded(&response)
    }
}

/// Every public room, and the private ones the person was invited to.
struct ListRooms<'a>(Caller<'a>);

impl Endpoint for ListRooms<'_> {
    type Answer = Vec<RoomSummary>;
    type Error = RoomError;
    const SERVICE: Service = Service::Rooms;

    fn request(&self) -> HttpRequest {
        self.0.request(HttpMethod::Get, "/v1/rooms", None)
    }

    fn answer(&self, response: HttpResponse) -> Result<Vec<RoomSummary>, RoomError> {
        let reply = reply(&response)?;
        let rooms = reply.get("rooms").and_then(Value::as_array).ok_or(RoomError::Unreachable)?;
        Ok(rooms.iter().filter_map(Value::as_object).filter_map(summary).collect())
    }
}

/// Enters a room or tells it the latest report, and hears everyone in it.
struct Report<'a> {
    caller: Caller<'a>,
    room: &'a str,
    report: &'a PresenceReport,
    now: Timestamp,
}

impl Endpoint for Report<'_> {
    type Answer = Room;
    type Error = RoomError;
    const SERVICE: Service = Service::Rooms;

    fn request(&self) -> HttpRequest {
        self.caller.request(HttpMethod::Put, &presence(self.room), Some(report_json(self.report)))
    }

    fn answer(&self, response: HttpResponse) -> Result<Room, RoomError> {
        room(&reply(&response)?, self.room, self.now).ok_or(RoomError::Unreachable)
    }
}

struct Leave<'a> {
    caller: Caller<'a>,
    room: &'a str,
}

impl Endpoint for Leave<'_> {
    type Answer = ();
    type Error = RoomError;
    const SERVICE: Service = Service::Rooms;

    fn request(&self) -> HttpRequest {
        self.caller.request(HttpMethod::Delete, &presence(self.room), None)
    }

    fn answer(&self, response: HttpResponse) -> Result<(), RoomError> {
        succeeded(&response)
    }
}

fn presence(room: &str) -> String {
    format!("/v1/rooms/{room}/presence")
}

/// The report as the server takes it: the character's parts and the three counts, and a nickname only when the
/// person shows up by one, which only a private room's report has.
fn report_json(report: &PresenceReport) -> Value {
    let tasks = report.tasks;
    let mut body = json!({
        "avatar": report.avatar.code(),
        "tasks": {"new": tasks.new.min(MOST_TASKS), "running": tasks.running.min(MOST_TASKS), "seen": tasks.seen.min(MOST_TASKS)},
    });
    if let Identity::Profile(nickname) = &report.identity {
        body["profile"] = json!({"nickname": nickname});
    }
    body
}

fn succeeded(response: &HttpResponse) -> Result<(), RoomError> {
    if (200..300).contains(&response.status) { Ok(()) } else { Err(failure(response)) }
}

/// A successful reply's JSON object.
fn reply(response: &HttpResponse) -> Result<Map<String, Value>, RoomError> {
    succeeded(response)?;
    match serde_json::from_slice::<Value>(&response.body) {
        Ok(Value::Object(reply)) if response.body.len() <= REPLY_LIMIT => Ok(reply),
        _ => Err(RoomError::Unreachable),
    }
}

/// What a reply other than success means, from its status and the server's `{"error": "<code>"}`. A 429 never
/// gets here: [`Api::send`] keeps the wait it asks for.
fn failure(response: &HttpResponse) -> RoomError {
    let reply = serde_json::from_slice::<Value>(&response.body).ok().filter(|_| response.body.len() <= REPLY_LIMIT);
    let code = reply.as_ref().and_then(|reply| reply.get("error")).and_then(Value::as_str);
    let failure = match (response.status, code) {
        (401, _) => RoomError::Unauthorized,
        (404, Some("room_not_found")) => RoomError::NotFound,
        // An address with no rooms server behind it says 404 too, and that closes no tab.
        (404, _) => RoomError::Unreachable,
        (409, Some("room_full")) => RoomError::Full,
        (409, Some("too_many_rooms")) => RoomError::TooManyRooms,
        (426, _) => RoomError::ClientTooOld,
        (400..=499, _) => RoomError::Rejected,
        _ => RoomError::Unreachable,
    };
    if failure == RoomError::Rejected {
        // A mistake in what Errand sent: worth a line for whoever runs it from a terminal. The code is the
        // server's, kept only when it is a plain word.
        let code = code.filter(|code| code.len() <= 40 && code.bytes().all(|byte| byte.is_ascii_lowercase() || byte == b'_'));
        eprintln!("errand: the rooms server turned a call down ({} {})", response.status, code.unwrap_or("without a reason"));
    }
    failure
}

fn text(value: Option<&Value>) -> Option<String> {
    value?.as_str().map(|text| text.chars().take(LONGEST_TEXT).collect())
}

/// A count as the server sends it, kept to what a task header holds.
fn count(value: Option<&Value>) -> usize {
    value.and_then(Value::as_u64).map_or(0, |count| usize::try_from(count).unwrap_or(usize::MAX))
}

/// A room the server does not call private is public, so the person stays anonymous if it is wrong.
fn visibility(value: Option<&Value>) -> Visibility {
    if value.and_then(Value::as_str) == Some("private") { Visibility::Private } else { Visibility::Public }
}

/// A character's code as sent; the view draws only one that names parts.
fn avatar(value: Option<&Value>) -> Option<String> {
    value?.as_str().filter(|code| code.len() <= LONGEST_CODE).map(str::to_owned)
}

fn summary(room: &Map<String, Value>) -> Option<RoomSummary> {
    let id = room.get("id")?.as_str().filter(|id| room_id(id))?;
    let faces = room.get("faces").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_object).take(FACES);
    Some(RoomSummary {
        id: id.to_owned(),
        name: text(room.get("name"))?,
        tagline: text(room.get("tagline")).unwrap_or_default(),
        visibility: visibility(room.get("visibility")),
        members: count(room.get("members")),
        working: count(room.get("working")),
        faces: faces.map(|face| Face { name: text(face.get("name")).unwrap_or_default(), avatar: avatar(face.get("avatar")) }).collect(),
    })
}

/// The room asked about, with everyone in it; a reply about another room is no answer.
fn room(reply: &Map<String, Value>, asked: &str, now: Timestamp) -> Option<Room> {
    if reply.get("id")?.as_str()? != asked {
        return None;
    }
    let members = reply.get("members")?.as_array()?.iter().filter_map(Value::as_object).filter_map(|member| self::member(member, now)).collect();
    Some(Room {
        id: asked.to_owned(),
        name: text(reply.get("name"))?,
        tagline: text(reply.get("tagline")).unwrap_or_default(),
        visibility: visibility(reply.get("visibility")),
        members,
    })
}

fn member(member: &Map<String, Value>, now: Timestamp) -> Option<Member> {
    let counts = member.get("tasks").and_then(Value::as_object);
    let tasks = |name| count(counts.and_then(|counts| counts.get(name))).min(MOST_TASKS);
    Some(Member {
        id: text(member.get("id"))?,
        name: text(member.get("name")).unwrap_or_default(),
        avatar: avatar(member.get("avatar")),
        tasks: TaskCounts { new: tasks("new"), running: tasks("running"), seen: tasks("seen") },
        // When they last did something, not just checked in; a time that does not read counts as now.
        last_active: member.get("lastActive").and_then(Value::as_str).and_then(|at| at.parse().ok()).unwrap_or(now),
        // When they came in, which stays while they are in the room; without it no time shows.
        joined_at: member.get("joinedAt").and_then(Value::as_str).and_then(|at| at.parse().ok()),
        me: member.get("me").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// Work rooms on the rooms server, every call through [`Api`].
pub struct HttpRooms {
    api: Arc<Api>,
    /// Reads this install's ID where the platform keeps it, the first time a call needs it.
    read_id: Box<dyn Fn() -> String + Send + Sync>,
    id: OnceLock<Option<InstallId>>,
    language: Language,
    /// Registered with the server this run. Held while registering, so calls made at once register once.
    registered: Mutex<bool>,
    /// The server turned this version away; nothing more is sent this run.
    too_old: AtomicBool,
}

impl HttpRooms {
    /// `read_id` gives this install's ID; `language` is the one Errand speaks, for the server's aliases.
    pub fn new(api: Arc<Api>, read_id: impl Fn() -> String + Send + Sync + 'static, language: Language) -> HttpRooms {
        HttpRooms { api, read_id: Box::new(read_id), id: OnceLock::new(), language, registered: Mutex::new(false), too_old: AtomicBool::new(false) }
    }

    fn caller(&self) -> Result<Caller<'_>, RoomError> {
        let id = self.id.get_or_init(|| InstallId::read((self.read_id)())).as_ref().ok_or(RoomError::Rejected)?;
        Ok(Caller { server: self.api.rooms_server().origin(), id, language: self.language })
    }

    /// Sends a call, registering this install first when it has not been this run, and once more, sending the
    /// call again, when the server has forgotten it.
    fn call<'a, E: Endpoint<Error = RoomError>>(&'a self, call: impl Fn(Caller<'a>) -> E) -> Result<E::Answer, RoomError> {
        if self.too_old.load(Ordering::Relaxed) {
            return Err(RoomError::ClientTooOld);
        }
        let caller = self.caller()?;
        let answer = self.register(caller, false).and_then(|()| match self.api.send(&call(caller)) {
            Err(RoomError::Unauthorized) => self.register(caller, true).and_then(|()| self.api.send(&call(caller))),
            answer => answer,
        });
        if matches!(answer, Err(RoomError::ClientTooOld)) {
            self.too_old.store(true, Ordering::Relaxed);
        }
        answer
    }

    fn register(&self, caller: Caller<'_>, again: bool) -> Result<(), RoomError> {
        let mut registered = self.registered.lock().unwrap_or_else(PoisonError::into_inner);
        if *registered && !again {
            return Ok(());
        }
        *registered = false;
        self.api.send(&Register(caller))?;
        *registered = true;
        Ok(())
    }
}

impl RoomService for HttpRooms {
    fn rooms(&self, _now: Timestamp) -> Result<Vec<RoomSummary>, RoomError> {
        self.call(ListRooms)
    }

    /// No ID but one the server makes goes into an address, so nothing a room says can steer a call elsewhere on
    /// the server.
    fn sync(&self, room: &str, report: &PresenceReport, now: Timestamp) -> Result<Room, RoomError> {
        if !room_id(room) {
            return Err(RoomError::NotFound);
        }
        self.call(|caller| Report { caller, room, report, now })
    }

    fn leave(&self, room: &str) {
        if room_id(room) {
            let _ = self.call(|caller| Leave { caller, room });
        }
    }
}
