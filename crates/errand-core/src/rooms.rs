//! Work rooms: people keeping each other company while they work. A room shows each person as their folded
//! task header would: how many answers they have to check, requests running, and answers opened. What anyone
//! works on never leaves their computer: the only thing Errand tells a room is a `PresenceReport`, who the
//! person shows up as, their character and those three counts. In a public room nobody has a name: the person
//! joins anonymously and the room gives them an alias. Their face is their own character everywhere.

use crate::avatar::Avatar;
use crate::model::AppState;
use crate::sessions::TaskCounts;
use jiff::{SignedDuration, Timestamp};

/// Quiet this long, a member reads as away.
pub const AWAY_AFTER: SignedDuration = SignedDuration::from_mins(60);
/// How often every room the person is in hears the latest counts and says who is in it, at the least.
pub const SYNC_INTERVAL: SignedDuration = SignedDuration::from_secs(30);
/// How often the room whose people show on the dock hears from the person and says who is there while they are at
/// the Mac, so its people stay current while looked at. A change in the person's counts reaches every room as soon.
pub const SHOWN_SYNC_INTERVAL: SignedDuration = SignedDuration::from_secs(10);
/// With no key pressed and the pointer still for this long, the person has stepped away from the Mac, and the room on
/// show waits its `SYNC_INTERVAL` like the rest.
pub const STEPPED_AWAY_AFTER: SignedDuration = SignedDuration::from_mins(3);
/// Timer ticks come a little early or late, so the intervals are measured with this much give.
const TICK_SLACK: SignedDuration = SignedDuration::from_secs(2);
/// How often the open room's times in the room count up, worked out here rather than sent.
pub const CLOCK_TICK: SignedDuration = SignedDuration::from_secs(1);
/// The longest nickname a private room takes, in characters.
pub const NICKNAME_LENGTH: usize = 20;
/// How many rooms Errand joins at once, fewer than the server allows, so the tabs stay readable. Rooms an older
/// Errand joined past this stay until the person leaves them.
pub const MAX_ROOMS: usize = 3;
/// How many private rooms one install may make, as the server counts them.
pub const OWNED_ROOMS: usize = 5;
/// How many people a private room lets in by invite, its owner among them.
pub const ROOM_MEMBERS: usize = 20;
/// The longest room name and tagline the server takes, in characters.
pub const NAME_LENGTH: usize = 30;
pub const TAGLINE_LENGTH: usize = 60;
/// Where an invite link goes: a page on the rooms server that hands the code, kept in the link's fragment so no
/// server ever hears it, on to Errand as an `errand://invite/<code>` link.
pub const INVITE_PAGE: &str = "https://errand-backend.vercel.app/join";
/// The scheme Errand opens links in.
pub const URL_SCHEME: &str = "errand";

/// Who may see a room, and so who the person shows up as in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    /// Anyone may join, and everyone in it is anonymous.
    Public,
    /// By invitation; members show their own profiles.
    Private,
}

/// The person's standing in a private room they may come into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Made it: invites others and can turn them out.
    Owner,
    /// Came in by an invite.
    Member,
}

/// A code that lets others into a private room, for a week or ten people, whichever runs out first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invite {
    /// Ten characters of Crockford base32, as [`invite_code`] reads them.
    pub code: String,
    pub expires_at: Timestamp,
    pub uses_left: usize,
}

impl Invite {
    /// The link to send: the server's page, the code after a `#`.
    pub fn link(&self) -> String {
        format!("{INVITE_PAGE}#{}", grouped(&self.code, '-'))
    }
}

/// Someone a private room lets in, as its owner's list shows them: by the nickname and character they last
/// had there, whether or not they are in it now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomMember {
    /// The room's own id for them, never their install's.
    pub id: String,
    /// The nickname they last had in the room; None if they never sent one.
    pub name: Option<String>,
    pub avatar: Option<String>,
    pub role: Role,
    pub me: bool,
    pub present: bool,
    pub working: bool,
    pub last_seen: Option<Timestamp>,
}

/// How long an invite code is.
pub const INVITE_CODE_LENGTH: usize = 10;
const CROCKFORD: &str = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// An invite code as typed or pasted: letters and digits only, any case, separators dropped, and the letters
/// Crockford base32 leaves out read as the digits they look like. None unless ten valid characters remain.
pub fn invite_code(text: &str) -> Option<String> {
    let code: String = text
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|character| match character.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            other => other,
        })
        .collect();
    (code.len() == INVITE_CODE_LENGTH && code.chars().all(|character| CROCKFORD.contains(character))).then_some(code)
}

/// A code as people read and type it: in fours, `K7QF-M2XA-9P`, joined by `separator`.
pub fn grouped(code: &str, separator: char) -> String {
    let characters: Vec<char> = code.chars().collect();
    characters.chunks(4).map(|chunk| chunk.iter().collect::<String>()).collect::<Vec<_>>().join(&separator.to_string())
}

/// The code an `errand://invite/<code>` link carries, if it is one.
pub fn invite_from_link(link: &str) -> Option<String> {
    let rest = link.trim().strip_prefix(URL_SCHEME)?.strip_prefix("://")?;
    let (kind, code) = rest.split_once('/')?;
    kind.eq_ignore_ascii_case("invite").then(|| invite_code(code.trim_end_matches('/'))).flatten()
}

/// A room's name or tagline as the server keeps it: no control characters, trimmed.
pub fn clean_text(text: &str) -> String {
    text.chars().filter(|character| !character.is_control()).collect::<String>().trim().to_owned()
}

/// Who the person shows up as in a room.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Identity {
    /// No name at all; the room gives the person an alias.
    Anonymous,
    /// The person's own nickname, which only private rooms ever hear.
    Profile(String),
}

/// Everything Errand tells a room about the person. There is no field for a title, a request, an answer or
/// a folder, so none can be sent.
#[derive(Clone, Debug, PartialEq)]
pub struct PresenceReport {
    pub identity: Identity,
    /// The person's face in every room: the character's parts, which rooms hear as [`Avatar::code`]. Never the
    /// seed it may have been drawn from, which is this install's own and would tell rooms apart.
    pub avatar: Avatar,
    /// What the person's own folded task header counts, for the services shown.
    pub tasks: TaskCounts,
}

/// The person's report for a room. A public room never hears the person's name; a private one hears it as a
/// nickname the server takes, without control characters and at most [`NICKNAME_LENGTH`] long.
pub fn presence_report(state: &AppState, visibility: Visibility) -> PresenceReport {
    let nickname = nickname(&state.profile.name);
    let identity = match visibility {
        Visibility::Private if !nickname.is_empty() => Identity::Profile(nickname),
        _ => Identity::Anonymous,
    };
    PresenceReport { identity, avatar: Avatar::named(&state.profile.avatar), tasks: state.sessions.counts() }
}

/// `name` as a nickname: without control characters or spaces at either end, and at most [`NICKNAME_LENGTH`]
/// long. The person's own nickname is kept this way, so the dock shows what a private room hears.
pub fn nickname(name: &str) -> String {
    let kept: String = name.chars().filter(|character| !character.is_control()).collect();
    kept.trim().chars().take(NICKNAME_LENGTH).collect::<String>().trim_end().to_owned()
}

/// Whether `id` is a room ID as the rooms server makes them: letters, digits, `-` and `_`, 64 at most.
pub fn room_id(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// Someone as a room's list shows them, beside its counts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Face {
    pub name: String,
    /// Their character as [`Avatar::code`] wrote it; None draws a grey face.
    pub avatar: Option<String>,
}

/// A room as the list shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomSummary {
    pub id: String,
    pub name: String,
    pub tagline: String,
    pub visibility: Visibility,
    pub members: usize,
    pub working: usize,
    /// A few of the working members, for faces beside the counts.
    pub faces: Vec<Face>,
    /// The person's standing in it, for a private room they may come into.
    pub role: Option<Role>,
}

/// One person in a room, as the room reports them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub id: String,
    pub name: String,
    /// Their character as [`Avatar::code`] wrote it, the same in every room; None draws a grey face.
    pub avatar: Option<String>,
    /// What their folded task header counts.
    pub tasks: TaskCounts,
    pub last_active: Timestamp,
    /// When they came into the room, which stays while they are in it. None from a room that does not say, and
    /// then no time shows.
    pub joined_at: Option<Timestamp>,
    /// The person themselves.
    pub me: bool,
}

/// How a member reads at a glance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Presence {
    Working,
    Resting,
    Away,
}

impl Member {
    pub fn presence(&self, now: Timestamp) -> Presence {
        let quiet = now.duration_since(self.last_active);
        if self.tasks.running > 0 {
            Presence::Working
        } else if quiet < AWAY_AFTER {
            Presence::Resting
        } else {
            Presence::Away
        }
    }
}

/// A room with everyone in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Room {
    pub id: String,
    pub name: String,
    pub tagline: String,
    pub visibility: Visibility,
    pub members: Vec<Member>,
    /// The person's standing in it, for a private room they may come into.
    pub role: Option<Role>,
}

impl Room {
    pub fn working(&self) -> usize {
        self.members.iter().filter(|member| member.tasks.running > 0).count()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomError {
    /// The rooms server did not answer, or not as a rooms server would.
    Unreachable,
    /// This install's secret could not be read from secure storage. Retrying must preserve membership.
    CredentialsUnavailable,
    /// The server does not know this install, even after it was registered again.
    Unauthorized,
    /// No such room, or a private one the person was not invited to: its tab closes.
    NotFound,
    /// The room has no place left.
    Full,
    /// The person is in as many rooms at once as the server allows.
    TooManyRooms,
    /// The server asked Errand to wait; nothing is sent to it until the wait is over.
    RateLimited,
    /// The server no longer serves this version of Errand.
    ClientTooOld,
    /// The server could not take what Errand sent: a mistake in Errand, not the person's.
    Rejected,
    /// The person has made as many private rooms as the server allows.
    TooManyOwned,
    /// The room has as many live invites as the server allows.
    TooManyInvites,
    /// No such invite, or one that ran out.
    InviteNotFound,
    /// The private room already has as many members as it holds.
    TooManyMembers,
}

impl RoomError {
    /// The room no longer counts the person, so its last answer no longer says who is there with them.
    pub fn left_out(self) -> bool {
        !matches!(self, RoomError::Unreachable | RoomError::CredentialsUnavailable | RoomError::RateLimited | RoomError::TooManyOwned | RoomError::TooManyInvites | RoomError::InviteNotFound | RoomError::TooManyMembers)
    }
}

/// A room the person is in: joined at once, known once it first answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Joined {
    pub id: String,
    /// The room as it last answered.
    pub room: Option<Room>,
    /// Why it last failed to answer.
    pub failure: Option<RoomError>,
    /// How visible it was when Errand last ran, for a room joined then, until it or the list says.
    pub remembered: Option<Visibility>,
    /// When the room was last sent the person's report, and the counts it carried.
    pub reported: Option<(Timestamp, TaskCounts)>,
}

impl Joined {
    /// The room has heard nothing from the person for about `interval`.
    pub fn quiet_for(&self, interval: SignedDuration, now: Timestamp) -> bool {
        self.reported.is_none_or(|(at, _)| now.duration_since(at) >= interval - TICK_SLACK)
    }

    /// The room last heard other counts than these.
    pub fn out_of_date(&self, tasks: TaskCounts) -> bool {
        self.reported.is_some_and(|(_, heard)| heard != tasks)
    }

    /// The room as it last answered, while it still counts the person: one that turned them away since keeps
    /// its name, but no longer says who is there with them.
    pub fn counted(&self) -> Option<&Room> {
        self.room.as_ref().filter(|_| !self.failure.is_some_and(RoomError::left_out))
    }
}

/// The rooms, wherever they are kept.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct RoomsState {
    /// The rooms on offer, once read.
    pub list: Vec<RoomSummary>,
    /// The rooms the person is in, in the order they joined; each has a tab under the profile.
    pub joined: Vec<Joined>,
    /// The room whose people show under the tabs.
    pub expanded: Option<String>,
    /// The list is being read.
    pub loading: bool,
    /// Why the list last failed to read.
    pub failure: Option<RoomError>,
    /// The list of rooms is open beside the dock.
    pub open: bool,
    /// The rooms are sample data, standing in for the server.
    pub sample: bool,
    /// A tick is on its way to count up the open room's times in the room.
    pub clock: bool,
    /// What the bubble beside the dock shows: the list, or a step of making or joining a private room.
    pub screen: RoomsScreen,
}

/// The bubble's screens past the list.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum RoomsScreen {
    #[default]
    List,
    /// Naming a new private room. `sending` while the server makes it.
    Create { sending: bool, failure: Option<RoomError> },
    /// A room's invite link, for its owner.
    Invite { room: String, invite: Option<Invite>, loading: bool, failure: Option<RoomError>, copied: bool },
    /// Everyone a room the person owns lets in, to turn out those who should not be there.
    Members { room: String, members: Option<Vec<RoomMember>>, loading: bool, failure: Option<RoomError> },
    /// Typing or pasting a code, or one a link brought, and the room it opens once the server says.
    Code { code: String, checking: bool, room: Option<RoomSummary>, failure: Option<RoomError>, from_link: bool, accepting: bool },
}

impl RoomsState {
    /// The person's standing in a room, as it last answered or the list says.
    pub fn role(&self, id: &str) -> Option<Role> {
        let answered = self.joined.iter().find(|joined| joined.id == id).and_then(|joined| joined.room.as_ref()).and_then(|room| room.role);
        answered.or_else(|| self.list.iter().find(|room| room.id == id).and_then(|room| room.role))
    }

    /// A room's name as far as Errand knows it.
    pub fn name(&self, id: &str) -> Option<&str> {
        let listed = self.list.iter().find(|room| room.id == id).map(|room| room.name.as_str());
        listed.or_else(|| self.joined.iter().find(|joined| joined.id == id).and_then(|joined| joined.room.as_ref()).map(|room| room.name.as_str()))
    }

    /// How many private rooms the person has made, as far as the list says.
    pub fn owned(&self) -> usize {
        self.list.iter().filter(|room| room.role == Some(Role::Owner)).count()
    }

    pub fn is_joined(&self, id: &str) -> bool {
        self.joined.iter().any(|joined| joined.id == id)
    }

    /// The person is in as many rooms as Errand joins at once.
    pub fn at_limit(&self) -> bool {
        self.joined.len() >= MAX_ROOMS
    }

    /// How visible a room is, as far as Errand knows: as it last answered, as the list says, or as it was when
    /// Errand last ran. A room it knows nothing of counts as public, so the person stays anonymous there.
    pub fn visibility(&self, id: &str) -> Visibility {
        let joined = self.joined.iter().find(|joined| joined.id == id);
        let answered = joined.and_then(|joined| joined.room.as_ref()).map(|room| room.visibility);
        let listed = || self.list.iter().find(|room| room.id == id).map(|room| room.visibility);
        answered.or_else(listed).or_else(|| joined.and_then(|joined| joined.remembered)).unwrap_or(Visibility::Public)
    }

    /// The rooms to join again on the next launch, in the order they were joined, and how visible each is.
    pub fn remembered(&self) -> Vec<(String, Visibility)> {
        self.joined.iter().map(|joined| (joined.id.clone(), self.visibility(&joined.id))).collect()
    }
}
