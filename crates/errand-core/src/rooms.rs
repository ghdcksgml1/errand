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
/// How often a joined room is told the latest counts and asked who is in it.
pub const SYNC_INTERVAL: SignedDuration = SignedDuration::from_secs(30);
/// How often the open room's times in the room count up, worked out here rather than sent.
pub const CLOCK_TICK: SignedDuration = SignedDuration::from_secs(1);
/// The longest nickname a private room takes, in characters.
pub const NICKNAME_LENGTH: usize = 20;

/// Who may see a room, and so who the person shows up as in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    /// Anyone may join, and everyone in it is anonymous.
    Public,
    /// By invitation; members show their own profiles.
    Private,
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

fn nickname(name: &str) -> String {
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
}

impl RoomError {
    /// The room no longer counts the person, so its last answer no longer says who is there with them.
    pub fn left_out(self) -> bool {
        !matches!(self, RoomError::Unreachable | RoomError::RateLimited)
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
}

impl Joined {
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
}

impl RoomsState {
    pub fn is_joined(&self, id: &str) -> bool {
        self.joined.iter().any(|joined| joined.id == id)
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
