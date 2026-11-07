//! The app's state and how every input changes it. Timers, clicks and finished reads all arrive as a `Msg`;
//! `update` changes the state and returns the `Effect`s for the runtime to carry out. Nothing here waits,
//! touches a file or draws.

use crate::avatar::Avatar;
use crate::provider::{PerProvider, Provider};
use crate::reset_history::ResetHistory;
use crate::rooms::{
    Invite, Joined, NAME_LENGTH, RoomMember, Role, Room, RoomError, RoomSummary, RoomsScreen, RoomsState, TAGLINE_LENGTH, Visibility, clean_text, invite_code,
    invite_from_link, room_id,
};
use crate::sessions::{SessionRecords, SessionRow, SessionState, TaskCounts, VisibleSession, families};
use crate::update::{self, Release, UpdateState};
use crate::usage::{self, Mood, Reading, UsageError, UsageWindow};
use jiff::{SignedDuration, Timestamp};
use std::collections::{BTreeMap, BTreeSet};

/// A checked card waits this long, draining a countdown and offering 되돌리기, before it is cleared.
pub const COMPLETION_DELAY: SignedDuration = SignedDuration::from_secs(5);
/// Usage is read every two minutes.
pub const USAGE_INTERVAL: SignedDuration = SignedDuration::from_mins(2);
pub const SESSION_INTERVAL: SignedDuration = SignedDuration::from_secs(4);
/// Sign-ins and open apps are checked this often.
pub const STATUS_INTERVAL: SignedDuration = SignedDuration::from_secs(3);
/// A reset seen this recently shows as a fresh refill.
pub const RECENT_RESET: SignedDuration = SignedDuration::from_mins(1);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsageSlot {
    /// From the last successful read.
    pub windows: Vec<UsageWindow>,
    /// A read has succeeded, even if it reported no limits.
    pub has_response: bool,
    pub fetched_at: Option<Timestamp>,
    /// When the service said what the last read found, if that was before the read.
    pub reported_at: Option<Timestamp>,
    /// Why the last read failed; the previous values stay beside it.
    pub failure: Option<UsageError>,
}

/// Who is signed in and whether the app is open.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AccountInfo {
    pub email: Option<String>,
    pub running: bool,
}

/// The person at the bottom of the dock.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Profile {
    pub name: String,
    /// The person's character as the platform keeps it: a random seed, or the parts they picked (see
    /// [`Avatar::named`]).
    pub avatar: String,
}

/// Why a service's app could not be brought forward.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenError {
    /// The app is not in Applications.
    NotInstalled(Provider),
    /// The system's own reason, already in the person's language.
    System(String),
}

/// The line at the foot of the account window: what Errand is doing, or what went wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    /// At launch, before anything else happened.
    Checking,
    Opening(Provider),
    Opened(Provider),
    OpenFailed(OpenError),
}

/// Something worth stopping the person for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Alert {
    OpenFailed(OpenError),
    /// The Dual Codex folder could not move to Errand's, with the system's reason.
    MigrationFailed(String),
    /// Leaving a private room for good, or deleting one, did not go through.
    QuitFailed(RoomError),
}

/// A question the person answers before Errand goes on, asked in a dialog with a button that does it and one
/// that does not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Confirm {
    /// Out of a private room for good. For its owner the room goes with them, and everyone else loses it too.
    QuitRoom { room: String, name: String, owner: bool },
    /// Turning someone out of the person's room; they need a new invite to come back.
    RemoveMember { room: String, member: String, name: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionsState {
    pub records: SessionRecords,
    /// The last scan, before Errand's own marks.
    pub scan: Vec<SessionRow>,
    pub scanning: bool,
    /// The cards on screen, in order.
    pub shown: Vec<VisibleSession>,
    /// Newer cards waiting while the pointer rests on the stack, so a click lands on the card aimed at.
    pub held: bool,
    /// Checked cards and when each will be cleared.
    pub pending: BTreeMap<String, Timestamp>,
    pub pointer_on_cards: bool,
}

impl SessionsState {
    /// The cards on screen not yet checked off, counted as the task header counts them.
    pub fn counts(&self) -> TaskCounts {
        let open = self.shown.iter().filter(|session| !self.pending.contains_key(&session.row.key));
        open.fold(TaskCounts::default(), |counts, session| match SessionState::of(session) {
            SessionState::New | SessionState::Stalled => TaskCounts { new: counts.new + 1, ..counts },
            SessionState::Running => TaskCounts { running: counts.running + 1, ..counts },
            SessionState::Seen => TaskCounts { seen: counts.seen + 1, ..counts },
            _ => counts,
        })
    }
}

/// Where the person is, as far as the computer can tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attention {
    /// The service whose app is in front, if either is.
    pub front: Option<Provider>,
    /// How long since the person last pressed a key or moved the pointer.
    pub idle: SignedDuration,
}

/// What the person chose about what Errand shows; it survives relaunch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WidgetPreferences {
    pub hidden: bool,
    /// The task cards are folded away.
    pub collapsed: bool,
    /// Services the person does not use: shown nowhere, their usage not read and their tasks left out.
    pub hidden_services: PerProvider<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AppState {
    pub usage: PerProvider<UsageSlot>,
    /// Reads in flight; usage is loading while any is.
    pub fetching: BTreeSet<Provider>,
    pub history: ResetHistory,
    pub accounts: PerProvider<AccountInfo>,
    /// An app is being opened; open buttons wait for it.
    pub launching: bool,
    pub notice: Notice,
    pub profile: Profile,
    pub sessions: SessionsState,
    pub widget: WidgetPreferences,
    pub details_open: bool,
    /// The character editor beside the dock; it, the details and the rooms take turns.
    pub avatar_editor_open: bool,
    pub rooms: RoomsState,
    pub update: UpdateState,
}

impl AppState {
    pub fn new(history: ResetHistory, records: SessionRecords, widget: WidgetPreferences, profile_name: String) -> AppState {
        AppState {
            usage: PerProvider::default(),
            fetching: BTreeSet::new(),
            history,
            accounts: PerProvider::default(),
            launching: false,
            notice: Notice::Checking,
            profile: Profile { name: profile_name, avatar: String::new() },
            sessions: SessionsState {
                records,
                scan: Vec::new(),
                scanning: false,
                shown: Vec::new(),
                held: false,
                pending: BTreeMap::new(),
                pointer_on_cards: false,
            },
            widget,
            details_open: false,
            avatar_editor_open: false,
            rooms: RoomsState::default(),
            update: UpdateState::default(),
        }
    }

    pub fn loading(&self) -> bool {
        !self.fetching.is_empty()
    }

    /// Whether this service shows anywhere; the person may hide one they do not use.
    pub fn shows(&self, provider: Provider) -> bool {
        !self.widget.hidden_services[provider]
    }

    /// The services shown, in display order. There is always at least one.
    pub fn shown_providers(&self) -> Vec<Provider> {
        Provider::ALL.into_iter().filter(|provider| self.shows(*provider)).collect()
    }

    /// The card stack is on screen.
    pub fn tray_visible(&self) -> bool {
        !self.widget.hidden && !self.widget.collapsed && !self.sessions.shown.is_empty()
    }

    /// When each of a service's limits was seen to reset, by window identifier.
    pub fn last_resets(&self, provider: Provider) -> BTreeMap<String, Timestamp> {
        self.usage[provider]
            .windows
            .iter()
            .filter_map(|window| Some((window.identifier.clone(), self.history.last_reset(provider, &window.identifier)?)))
            .collect()
    }

    pub fn mood(&self, provider: Provider, now: Timestamp) -> Mood {
        let slot = &self.usage[provider];
        let recent = self.last_resets(provider).values().any(|reset| {
            let age = now.duration_since(*reset);
            age >= SignedDuration::ZERO && age < RECENT_RESET
        });
        usage::mood(&slot.windows, slot.failure.is_some(), recent)
    }
}

pub enum Msg {
    /// Every few seconds: time to check who is signed in and which apps are open.
    CheckAccounts,
    AccountsRead(PerProvider<AccountInfo>),
    ProfileRead(Profile),
    /// A click on the person's character: the editor opens beside the dock, or closes.
    ToggleAvatarEditor,
    CloseAvatarEditor,
    /// A part picked in the editor: this is the person's character now.
    PickAvatar(Avatar),
    /// The editor's shuffle: a new random character.
    ShuffleAvatar,
    /// The timer, a refresh button or the menu.
    RefreshUsage,
    /// The person pressed refresh, so Claude Code is asked too. Nothing automatic sends this.
    AskUsage,
    /// At launch and once a day.
    CheckForUpdate,
    /// What the latest release is, or None when it could not be told.
    ReleaseRead(Option<Release>),
    /// The person pressed the update button. Nothing automatic sends this.
    InstallUpdate,
    UpdateFailed,
    UsageRead { provider: Provider, result: Result<Reading, UsageError> },
    ScanSessions,
    SessionsScanned(Vec<SessionRow>),
    /// Asked for while the session on screen has an unread answer.
    AttentionRead(Attention),
    OpenSession(String),
    /// A card's checkbox or its 되돌리기.
    ToggleComplete(String),
    /// A checked card's wait ran out. The shell fades the card, then sends `CardFaded`.
    CompletionDue(String),
    CardFaded(String),
    PointerOnCards(bool),
    ToggleTray,
    ToggleDetails,
    CloseDetails,
    SetWidgetHidden(bool),
    /// Shows a hidden service, or hides a shown one unless it is the last.
    ToggleService(Provider),
    OpenApp(Provider),
    AppOpened { provider: Provider, result: Result<(), OpenError> },
    ShowDashboard,
    /// The widget moved or the screens changed: nothing to update, only to draw again.
    Redraw,
    /// The menu: opens the list of rooms beside the dock.
    ShowRooms,
    /// The dock's way to the rooms: opens the list, or closes it.
    ToggleRooms,
    CloseRooms,
    RoomsRead(Result<Vec<RoomSummary>, RoomError>),
    JoinRoom(String),
    LeaveRoom(String),
    /// A room's tab: shows its people under the tabs, or folds them away.
    ExpandRoom(String),
    /// Every so often: tell each room the latest counts and see who is in it.
    SyncRoom,
    /// A room's answer, by the id it was asked by.
    RoomRead(String, Result<Room, RoomError>),
    /// The rooms the person was in when Errand last ran, and how visible each was: in again.
    RoomsRemembered(Vec<(String, Visibility)>),
    /// A second on, while the open room's people show: only their times in the room to draw again.
    RoomClock,
    /// The bubble back to the list of rooms.
    ShowRoomsList,
    /// The bubble to naming a new private room.
    ShowCreateRoom,
    CreateRoom { name: String, tagline: String },
    RoomCreated(Result<RoomSummary, RoomError>),
    /// The bubble to a room's invite link, for its owner.
    ShowInvite(String),
    /// A room's live invites, newest first.
    InvitesRead(String, Result<Vec<Invite>, RoomError>),
    InviteMade(String, Result<Invite, RoomError>),
    /// The link shown was deleted on the way to a new one.
    InviteReplaced(String),
    /// A new link in place of the one shown, which stops working.
    RenewInvite(String),
    /// The link shown stops working, and none takes its place until asked for.
    DeleteInvite(String),
    InviteDeleted(String, Result<(), RoomError>),
    CopyInviteLink,
    CopyInviteCode,
    /// The bubble to typing or pasting an invite code.
    ShowInviteCode,
    /// A code typed or pasted, to see which room it opens.
    CheckInvite(String),
    InviteChecked(String, Result<RoomSummary, RoomError>),
    /// In by the invite checked, then into the room.
    AcceptInvite,
    InviteAccepted(String, Result<RoomSummary, RoomError>),
    /// A link Errand was opened with, such as `errand://invite/<code>`.
    OpenLink(String),
    /// Asks first, then `Confirmed`.
    AskQuitRoom(String),
    /// The person went ahead with what they were asked.
    Confirmed(Confirm),
    RoomQuit(String, Result<(), RoomError>),
    /// The bubble to everyone a room the person owns lets in.
    ShowMembers(String),
    MembersRead(String, Result<Vec<RoomMember>, RoomError>),
    /// Asks first, then `Confirmed`.
    AskRemoveMember { room: String, member: String, name: String },
    MemberRemoved(String, Result<(), RoomError>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    FetchUsage(Provider),
    /// Reads Claude's usage after asking Claude Code, which only a press of refresh does.
    AskClaudeCode,
    FetchRelease,
    /// Downloads this release and puts it in place of the app, which then starts again.
    InstallUpdate(Release),
    ReadProfile,
    /// Draw and keep a new seed for the person's character, then send `ProfileRead` with it.
    NewAvatarSeed,
    /// Keep the character someone picked, written as `Avatar::code`, in place of the last.
    SaveAvatar(String),
    ReadAccounts,
    /// Scan with this snapshot of the tray's records, then send `SessionsScanned`.
    ScanSessions(SessionRecords),
    /// See where the person is, then send `AttentionRead`.
    ReadAttention,
    SaveResetHistory,
    SaveSessionRecords,
    SaveWidgetPreferences,
    OpenUrl(String),
    /// Bring the app forward, opening it if it is not running; then send `AppOpened`.
    OpenApp(Provider),
    /// Send `CompletionDue(key)` after this long.
    ScheduleCompletion { key: String, after: SignedDuration },
    FadeCard(String),
    ShowAlert(Alert),
    ShowDashboard,
    FetchRooms,
    /// Send this room the person's presence report, as this visible a room may hear it, then `RoomRead`.
    SyncRoom { room: String, visibility: Visibility },
    LeaveRoom(String),
    /// Keep the rooms the person is in, `RoomsState::remembered`, for the next launch.
    SaveRooms,
    /// Send `RoomClock` a `rooms::CLOCK_TICK` from now.
    TickRoomClock,
    /// Make a private room, then send `RoomCreated`.
    CreateRoom { name: String, tagline: String },
    /// Read a room's live invites, then send `InvitesRead`.
    ReadInvites(String),
    /// Make an invite, first deleting the one it replaces, then send `InviteMade`.
    MakeInvite { room: String, replacing: Option<String> },
    /// Delete an invite, then send `InviteDeleted`.
    DeleteInvite { room: String, code: String },
    /// Ask which room a code opens, without using it up, then send `InviteChecked`.
    CheckInvite(String),
    /// Use the code, then send `InviteAccepted`.
    AcceptInvite(String),
    /// Out of a private room for good, then send `RoomQuit`.
    QuitRoom(String),
    /// Read everyone a room lets in, then send `MembersRead`.
    ReadMembers(String),
    /// Turn someone out of a room, then send `MemberRemoved`.
    RemoveMember { room: String, member: String },
    /// Put this on the clipboard.
    Copy(String),
    /// Ask this, and send `Confirmed` with it if the person goes ahead.
    Confirm(Confirm),
}

pub fn update(state: &mut AppState, msg: Msg, now: Timestamp) -> Vec<Effect> {
    let remembered = state.rooms.remembered();
    let mut effects = apply(state, msg, now);
    // Joining, leaving, a tab closing or learning how visible a room is: the next launch should know.
    if state.rooms.remembered() != remembered {
        effects.push(Effect::SaveRooms);
    }
    // While the open room's people show, their times in the room count up a tick at a time; once they no longer
    // show, the last tick asks for no other.
    if !state.rooms.clock && room_clock_wanted(state) {
        state.rooms.clock = true;
        effects.push(Effect::TickRoomClock);
    }
    effects
}

/// The widget shows the open room's people, and the room says when some of them came in.
fn room_clock_wanted(state: &AppState) -> bool {
    let rooms = &state.rooms;
    let open = rooms.expanded.as_deref().and_then(|id| rooms.joined.iter().find(|joined| joined.id == id));
    !state.widget.hidden && open.and_then(Joined::counted).is_some_and(|room| room.members.iter().any(|member| member.joined_at.is_some()))
}

fn apply(state: &mut AppState, msg: Msg, now: Timestamp) -> Vec<Effect> {
    match msg {
        Msg::CheckAccounts => vec![Effect::ReadAccounts],
        Msg::AccountsRead(accounts) => {
            state.accounts = accounts;
            vec![]
        }
        Msg::ProfileRead(profile) => {
            state.profile = profile;
            vec![]
        }
        Msg::ToggleAvatarEditor => {
            state.avatar_editor_open = !state.avatar_editor_open;
            if state.avatar_editor_open {
                state.details_open = false;
                state.rooms.open = false;
            }
            vec![]
        }
        Msg::CloseAvatarEditor => {
            state.avatar_editor_open = false;
            vec![]
        }
        Msg::PickAvatar(avatar) => {
            state.profile.avatar = avatar.code();
            vec![Effect::SaveAvatar(state.profile.avatar.clone())]
        }
        Msg::ShuffleAvatar => vec![Effect::NewAvatarSeed],
        Msg::RefreshUsage => refresh_usage(state),
        Msg::AskUsage => refresh_usage(state)
            .into_iter()
            .map(|effect| if effect == Effect::FetchUsage(Provider::Claude) { Effect::AskClaudeCode } else { effect })
            .collect(),
        Msg::CheckForUpdate => {
            if matches!(state.update, UpdateState::Installing(_)) { vec![] } else { vec![Effect::FetchRelease] }
        }
        Msg::ReleaseRead(release) => {
            // No answer says nothing new, so what was on offer stays.
            if let Some(release) = release
                && !matches!(state.update, UpdateState::Installing(_))
            {
                state.update = if update::is_newer(&release.version, update::VERSION) { UpdateState::Available(release) } else { UpdateState::Current };
            }
            vec![]
        }
        Msg::InstallUpdate => match state.update.clone() {
            UpdateState::Available(release) | UpdateState::Failed(release) if release.download.is_some() => {
                state.update = UpdateState::Installing(release.clone());
                vec![Effect::InstallUpdate(release)]
            }
            UpdateState::Available(_) | UpdateState::Failed(_) => vec![Effect::OpenUrl(update::RELEASES_PAGE.to_owned())],
            UpdateState::Current | UpdateState::Installing(_) => vec![],
        },
        Msg::UpdateFailed => {
            if let UpdateState::Installing(release) = state.update.clone() {
                state.update = UpdateState::Failed(release);
            }
            vec![]
        }
        Msg::UsageRead { provider, result } => {
            state.fetching.remove(&provider);
            let slot = &mut state.usage[provider];
            match result {
                Ok(reading) => {
                    slot.has_response = true;
                    slot.fetched_at = Some(now);
                    slot.reported_at = reading.reported_at;
                    slot.failure = None;
                    let changed = state.history.record(provider, &reading.windows, now);
                    slot.windows = reading.windows;
                    if changed { vec![Effect::SaveResetHistory] } else { vec![] }
                }
                Err(failure) => {
                    slot.failure = Some(failure);
                    vec![]
                }
            }
        }
        Msg::ScanSessions => {
            if state.sessions.scanning {
                return vec![];
            }
            state.sessions.scanning = true;
            vec![Effect::ScanSessions(state.sessions.records.clone())]
        }
        Msg::SessionsScanned(rows) => {
            state.sessions.scanning = false;
            state.sessions.scan = rows;
            apply_sessions(state);
            if unread_on_screen(&state.sessions).is_some() { vec![Effect::ReadAttention] } else { vec![] }
        }
        Msg::AttentionRead(attention) => {
            let Some(row) = unread_on_screen(&state.sessions) else { return vec![] };
            let Some(finished) = row.finished_at else { return vec![] };
            // Being in front is not enough: the person may have walked away with the app open.
            if attention.front != Some(row.provider) || attention.idle > now.duration_since(finished) {
                return vec![];
            }
            let key = row.key.clone();
            state.sessions.records.mark_seen(&key, now);
            apply_sessions(state);
            vec![Effect::SaveSessionRecords]
        }
        Msg::OpenSession(key) => {
            let Some(url) = state.sessions.scan.iter().find(|row| row.key == key).map(|row| row.url.clone()) else { return vec![] };
            state.sessions.records.mark_seen(&key, now);
            apply_sessions(state);
            vec![Effect::SaveSessionRecords, Effect::OpenUrl(url)]
        }
        Msg::ToggleComplete(key) => {
            if state.sessions.pending.remove(&key).is_some() {
                return vec![];
            }
            state.sessions.pending.insert(key.clone(), now + COMPLETION_DELAY);
            vec![Effect::ScheduleCompletion { key, after: COMPLETION_DELAY }]
        }
        Msg::CompletionDue(key) => {
            // A timer from an earlier check that was taken back must not clear a later one early.
            let tolerance = SignedDuration::from_millis(50);
            match state.sessions.pending.get(&key) {
                Some(deadline) if now + tolerance >= *deadline => vec![Effect::FadeCard(key)],
                _ => vec![],
            }
        }
        Msg::CardFaded(key) => {
            // 되돌리기 during the fade wins.
            if state.sessions.pending.remove(&key).is_none() {
                return vec![];
            }
            for sub_task in read_sub_tasks(&state.sessions, &key) {
                state.sessions.pending.remove(&sub_task);
                state.sessions.records.complete(&sub_task, now);
            }
            state.sessions.records.complete(&key, now);
            apply_sessions(state);
            vec![Effect::SaveSessionRecords]
        }
        Msg::PointerOnCards(on) => {
            state.sessions.pointer_on_cards = on;
            if !on && state.sessions.held {
                apply_sessions(state);
            }
            vec![]
        }
        Msg::ToggleTray => {
            state.widget.collapsed = !state.widget.collapsed;
            vec![Effect::SaveWidgetPreferences]
        }
        Msg::ToggleDetails => {
            state.details_open = !state.details_open;
            // One panel beside the dock at a time.
            if state.details_open {
                state.avatar_editor_open = false;
                state.rooms.open = false;
            }
            vec![]
        }
        Msg::CloseDetails => {
            state.details_open = false;
            vec![]
        }
        Msg::SetWidgetHidden(hidden) => {
            state.widget.hidden = hidden;
            if hidden {
                state.details_open = false;
                state.avatar_editor_open = false;
                state.rooms.open = false;
            }
            vec![Effect::SaveWidgetPreferences]
        }
        Msg::ToggleService(provider) => {
            // One service always stays, or the widget would have nothing to show.
            if state.shows(provider) && state.shown_providers().len() == 1 {
                return vec![];
            }
            state.widget.hidden_services[provider] = state.shows(provider);
            apply_sessions(state);
            let mut effects = vec![Effect::SaveWidgetPreferences];
            if !state.shows(provider) {
                // Its read in flight no longer keeps the others loading.
                state.fetching.remove(&provider);
            } else if !state.fetching.contains(&provider) {
                // Shown again: read it now rather than at the next tick.
                state.fetching.insert(provider);
                effects.push(Effect::FetchUsage(provider));
            }
            effects
        }
        Msg::OpenApp(provider) => {
            if state.launching {
                return vec![];
            }
            state.details_open = false;
            if !state.accounts[provider].running {
                state.launching = true;
                state.notice = Notice::Opening(provider);
            }
            vec![Effect::OpenApp(provider)]
        }
        Msg::AppOpened { provider, result } => {
            let launched = std::mem::take(&mut state.launching);
            match result {
                Ok(()) => {
                    if launched {
                        state.notice = Notice::Opened(provider);
                    }
                    vec![Effect::ReadAccounts]
                }
                Err(failure) => {
                    state.notice = Notice::OpenFailed(failure.clone());
                    vec![Effect::ShowAlert(Alert::OpenFailed(failure)), Effect::ReadAccounts]
                }
            }
        }
        Msg::Redraw => vec![],
        Msg::ShowRooms => open_rooms(state),
        Msg::ToggleRooms => {
            if state.rooms.open {
                state.rooms.open = false;
                state.rooms.screen = RoomsScreen::List;
                return vec![];
            }
            open_rooms(state)
        }
        Msg::CloseRooms => {
            state.rooms.open = false;
            state.rooms.screen = RoomsScreen::List;
            vec![]
        }
        Msg::RoomsRead(result) => {
            state.rooms.loading = false;
            match result {
                Ok(list) => {
                    state.rooms.list = list;
                    state.rooms.failure = None;
                }
                Err(failure) => state.rooms.failure = Some(failure),
            }
            vec![]
        }
        Msg::JoinRoom(room) => {
            if state.rooms.is_joined(&room) || state.rooms.at_limit() {
                return vec![];
            }
            let visibility = state.rooms.visibility(&room);
            state.rooms.joined.push(Joined { id: room.clone(), room: None, failure: None, remembered: None });
            // A room just joined opens its tab, to see who is there.
            state.rooms.expanded = Some(room.clone());
            vec![Effect::SyncRoom { room, visibility }]
        }
        Msg::LeaveRoom(room) => {
            if !state.rooms.is_joined(&room) {
                return vec![];
            }
            close_room(state, &room);
            vec![Effect::LeaveRoom(room), Effect::FetchRooms]
        }
        Msg::ExpandRoom(room) => {
            if state.rooms.is_joined(&room) {
                let open = state.rooms.expanded.as_deref() == Some(room.as_str());
                state.rooms.expanded = (!open).then_some(room);
            }
            vec![]
        }
        Msg::SyncRoom => sync_rooms(state),
        Msg::RoomClock => {
            state.rooms.clock = false;
            vec![]
        }
        Msg::RoomsRemembered(rooms) => {
            let mut effects = Vec::new();
            for (room, visibility) in rooms {
                if !room_id(&room) || state.rooms.is_joined(&room) {
                    continue;
                }
                state.rooms.joined.push(Joined { id: room.clone(), room: None, failure: None, remembered: Some(visibility) });
                effects.push(Effect::SyncRoom { visibility: state.rooms.visibility(&room), room });
            }
            effects
        }
        Msg::RoomRead(room, result) => {
            // An answer from a room since left finds no tab.
            let Some(joined) = state.rooms.joined.iter_mut().find(|joined| joined.id == room) else { return vec![] };
            match result {
                Ok(room) => {
                    joined.room = Some(room);
                    joined.failure = None;
                }
                // Gone, or never there for the person: its tab closes, and the list is read again if it is open.
                Err(RoomError::NotFound) => {
                    close_room(state, &room);
                    return if state.rooms.open { vec![Effect::FetchRooms] } else { vec![] };
                }
                // Out of reach for now, the room as it last answered stays; turned away, the person is no longer in
                // it, and the reason shows in place of its people (`Joined::counted`).
                Err(failure) => joined.failure = Some(failure),
            }
            vec![]
        }
        Msg::ShowRoomsList => {
            state.rooms.screen = RoomsScreen::List;
            vec![Effect::FetchRooms]
        }
        Msg::ShowCreateRoom => {
            state.rooms.screen = RoomsScreen::Create { sending: false, failure: None };
            vec![]
        }
        Msg::CreateRoom { name, tagline } => {
            let (name, tagline) = (clean_text(&name), clean_text(&tagline));
            let fits = (1..=NAME_LENGTH).contains(&name.chars().count()) && tagline.chars().count() <= TAGLINE_LENGTH;
            if !fits || !matches!(state.rooms.screen, RoomsScreen::Create { sending: false, .. }) {
                return vec![];
            }
            state.rooms.screen = RoomsScreen::Create { sending: true, failure: None };
            vec![Effect::CreateRoom { name, tagline }]
        }
        Msg::RoomCreated(result) => {
            // Made after the person moved on: the room is theirs all the same, in the list.
            if !matches!(state.rooms.screen, RoomsScreen::Create { .. }) {
                let Ok(room) = result else { return vec![] };
                upsert_listed(state, room);
                return vec![Effect::FetchRooms];
            }
            match result {
                Ok(room) => {
                    let id = room.id.clone();
                    upsert_listed(state, room);
                    state.rooms.screen = RoomsScreen::Invite { room: id.clone(), invite: None, loading: true, failure: None, copied: false };
                    // The person goes into the room they made, if they have a place left.
                    let mut effects = apply(state, Msg::JoinRoom(id.clone()), now);
                    effects.extend([Effect::ReadInvites(id), Effect::FetchRooms]);
                    effects
                }
                Err(failure) => {
                    state.rooms.screen = RoomsScreen::Create { sending: false, failure: Some(failure) };
                    vec![]
                }
            }
        }
        Msg::ShowInvite(room) => {
            if state.rooms.role(&room) != Some(Role::Owner) {
                return vec![];
            }
            state.rooms.screen = RoomsScreen::Invite { room: room.clone(), invite: None, loading: true, failure: None, copied: false };
            vec![Effect::ReadInvites(room)]
        }
        Msg::InvitesRead(room, result) => {
            let RoomsScreen::Invite { room: shown, invite, loading, failure, .. } = &mut state.rooms.screen else { return vec![] };
            if *shown != room {
                return vec![];
            }
            match result {
                // The newest live link, or a first one made at once.
                Ok(invites) => match invites.into_iter().next() {
                    Some(newest) => {
                        *invite = Some(newest);
                        *loading = false;
                    }
                    None => return vec![Effect::MakeInvite { room, replacing: None }],
                },
                Err(error) => {
                    *loading = false;
                    *failure = Some(error);
                }
            }
            vec![]
        }
        Msg::InviteReplaced(room) => {
            // The link shown no longer works, whatever comes next.
            if let RoomsScreen::Invite { room: shown, invite, copied, .. } = &mut state.rooms.screen
                && *shown == room
            {
                *invite = None;
                *copied = false;
            }
            vec![]
        }
        Msg::InviteMade(room, result) => {
            let RoomsScreen::Invite { room: shown, invite, loading, failure, copied } = &mut state.rooms.screen else { return vec![] };
            if *shown != room {
                return vec![];
            }
            *loading = false;
            *copied = false;
            match result {
                Ok(made) => {
                    *invite = Some(made);
                    *failure = None;
                }
                Err(error) => *failure = Some(error),
            }
            vec![]
        }
        Msg::RenewInvite(room) => {
            let RoomsScreen::Invite { room: shown, invite, loading, failure, .. } = &mut state.rooms.screen else { return vec![] };
            if *shown != room || *loading {
                return vec![];
            }
            *loading = true;
            *failure = None;
            vec![Effect::MakeInvite { room, replacing: invite.as_ref().map(|invite| invite.code.clone()) }]
        }
        Msg::DeleteInvite(room) => {
            let RoomsScreen::Invite { room: shown, invite, loading, .. } = &mut state.rooms.screen else { return vec![] };
            let Some(code) = invite.as_ref().map(|invite| invite.code.clone()).filter(|_| *shown == room && !*loading) else { return vec![] };
            *loading = true;
            vec![Effect::DeleteInvite { room, code }]
        }
        Msg::InviteDeleted(room, result) => {
            let RoomsScreen::Invite { room: shown, invite, loading, failure, copied } = &mut state.rooms.screen else { return vec![] };
            if *shown != room {
                return vec![];
            }
            *loading = false;
            *copied = false;
            match result {
                Ok(()) => *invite = None,
                Err(error) => *failure = Some(error),
            }
            vec![]
        }
        Msg::CopyInviteLink | Msg::CopyInviteCode => {
            let RoomsScreen::Invite { invite: Some(invite), copied, .. } = &mut state.rooms.screen else { return vec![] };
            let text = if msg_is_link(&msg) { invite.link() } else { crate::rooms::grouped(&invite.code, '-') };
            *copied = msg_is_link(&msg);
            vec![Effect::Copy(text)]
        }
        Msg::ShowInviteCode => {
            state.rooms.screen = RoomsScreen::Code { code: String::new(), checking: false, room: None, failure: None, from_link: false, accepting: false };
            vec![]
        }
        Msg::CheckInvite(text) => {
            let RoomsScreen::Code { code, checking, room, failure, accepting, .. } = &mut state.rooms.screen else { return vec![] };
            if *accepting || *checking {
                return vec![];
            }
            // The code already checked, its room on screen: Return goes in.
            if room.is_some() && invite_code(&text).as_deref() == Some(code.as_str()) {
                return apply(state, Msg::AcceptInvite, now);
            }
            *room = None;
            match invite_code(&text) {
                Some(valid) => {
                    *code = valid.clone();
                    *checking = true;
                    *failure = None;
                    vec![Effect::CheckInvite(valid)]
                }
                None => {
                    *code = text;
                    *checking = false;
                    *failure = Some(RoomError::InviteNotFound);
                    vec![]
                }
            }
        }
        Msg::InviteChecked(checked, result) => {
            let RoomsScreen::Code { code, checking, room, failure, .. } = &mut state.rooms.screen else { return vec![] };
            if *code != checked {
                return vec![];
            }
            *checking = false;
            match result {
                Ok(found) => *room = Some(found),
                Err(error) => *failure = Some(error),
            }
            vec![]
        }
        Msg::AcceptInvite => {
            let RoomsScreen::Code { code, room: Some(_), accepting, .. } = &mut state.rooms.screen else { return vec![] };
            if *accepting {
                return vec![];
            }
            *accepting = true;
            vec![Effect::AcceptInvite(code.clone())]
        }
        Msg::InviteAccepted(accepted, result) => {
            let current = matches!(&state.rooms.screen, RoomsScreen::Code { code, .. } if *code == accepted);
            // Accepted after the person moved on: they are in the room's list all the same.
            if !current {
                let Ok(room) = result else { return vec![] };
                upsert_listed(state, room);
                return vec![Effect::FetchRooms];
            }
            let RoomsScreen::Code { accepting, failure, .. } = &mut state.rooms.screen else { return vec![] };
            *accepting = false;
            match result {
                Ok(room) => {
                    let id = room.id.clone();
                    upsert_listed(state, room);
                    state.rooms.screen = RoomsScreen::List;
                    let mut effects = apply(state, Msg::JoinRoom(id), now);
                    effects.push(Effect::FetchRooms);
                    effects
                }
                Err(error) => {
                    *failure = Some(error);
                    vec![]
                }
            }
        }
        Msg::OpenLink(link) => {
            let Some(code) = invite_from_link(&link) else { return vec![] };
            // The person clicked a link to see a room: the widget shows again if it was hidden.
            let mut effects = if state.widget.hidden {
                state.widget.hidden = false;
                vec![Effect::SaveWidgetPreferences]
            } else {
                vec![]
            };
            effects.extend(open_rooms(state));
            state.rooms.screen = RoomsScreen::Code { code: code.clone(), checking: true, room: None, failure: None, from_link: true, accepting: false };
            effects.push(Effect::CheckInvite(code));
            effects
        }
        Msg::AskQuitRoom(room) => {
            let Some(role) = state.rooms.role(&room) else { return vec![] };
            let name = state.rooms.name(&room).unwrap_or_default().to_owned();
            vec![Effect::Confirm(Confirm::QuitRoom { room, name, owner: role == Role::Owner })]
        }
        Msg::Confirmed(Confirm::QuitRoom { room, .. }) => {
            if state.rooms.role(&room).is_none() {
                return vec![];
            }
            close_room(state, &room);
            state.rooms.list.retain(|listed| listed.id != room);
            state.rooms.screen = RoomsScreen::List;
            vec![Effect::QuitRoom(room)]
        }
        Msg::RoomQuit(_, result) => match result {
            // Already gone is what was asked for.
            Ok(()) | Err(RoomError::NotFound) => vec![Effect::FetchRooms],
            Err(failure) => vec![Effect::ShowAlert(Alert::QuitFailed(failure)), Effect::FetchRooms],
        },
        Msg::ShowMembers(room) => {
            if state.rooms.role(&room) != Some(Role::Owner) {
                return vec![];
            }
            state.rooms.screen = RoomsScreen::Members { room: room.clone(), members: None, loading: true, failure: None };
            vec![Effect::ReadMembers(room)]
        }
        Msg::MembersRead(room, result) => {
            let RoomsScreen::Members { room: shown, members, loading, failure } = &mut state.rooms.screen else { return vec![] };
            if *shown != room {
                return vec![];
            }
            *loading = false;
            match result {
                Ok(read) => {
                    *members = Some(read);
                    *failure = None;
                }
                Err(error) => *failure = Some(error),
            }
            vec![]
        }
        Msg::AskRemoveMember { room, member, name } => {
            if state.rooms.role(&room) != Some(Role::Owner) {
                return vec![];
            }
            vec![Effect::Confirm(Confirm::RemoveMember { room, member, name })]
        }
        Msg::Confirmed(Confirm::RemoveMember { room, member, .. }) => {
            if state.rooms.role(&room) != Some(Role::Owner) {
                return vec![];
            }
            // Gone from the room's rows and its list at once; the room's next answer says the same.
            if let Some(shown) = state.rooms.joined.iter_mut().find(|joined| joined.id == room).and_then(|joined| joined.room.as_mut()) {
                shown.members.retain(|someone| someone.id != member);
            }
            if let RoomsScreen::Members { room: shown, members: Some(members), .. } = &mut state.rooms.screen
                && *shown == room
            {
                members.retain(|someone| someone.id != member);
            }
            vec![Effect::RemoveMember { room, member }]
        }
        Msg::MemberRemoved(room, _) => {
            let mut effects = Vec::new();
            if matches!(&state.rooms.screen, RoomsScreen::Members { room: shown, .. } if *shown == room) {
                effects.push(Effect::ReadMembers(room.clone()));
            }
            if state.rooms.is_joined(&room) {
                effects.push(Effect::SyncRoom { visibility: state.rooms.visibility(&room), room });
            }
            effects
        }
        Msg::ShowDashboard => {
            state.details_open = false;
            state.avatar_editor_open = false;
            let mut effects = vec![Effect::ShowDashboard, Effect::ReadAccounts];
            effects.extend(refresh_usage(state));
            effects
        }
    }
}

/// Opens the list of rooms in place of the details or the character editor, reads it afresh, and tells the
/// rooms the person is in the latest counts.
fn open_rooms(state: &mut AppState) -> Vec<Effect> {
    state.rooms.open = true;
    state.rooms.screen = RoomsScreen::List;
    state.details_open = false;
    state.avatar_editor_open = false;
    state.rooms.loading = state.rooms.list.is_empty();
    let mut effects = vec![Effect::FetchRooms];
    effects.extend(sync_rooms(state));
    effects
}

/// A room the server just told of, in the list at once, ahead of the next read.
fn upsert_listed(state: &mut AppState, room: RoomSummary) {
    match state.rooms.list.iter_mut().find(|listed| listed.id == room.id) {
        Some(listed) => *listed = room,
        None => state.rooms.list.insert(0, room),
    }
}

fn msg_is_link(msg: &Msg) -> bool {
    matches!(msg, Msg::CopyInviteLink)
}

/// Takes a room's tab off the dock, folding its people away if they show.
fn close_room(state: &mut AppState, room: &str) {
    state.rooms.joined.retain(|joined| joined.id != room);
    if state.rooms.expanded.as_deref() == Some(room) {
        state.rooms.expanded = None;
    }
}

/// Tells each room the person is in the latest counts.
fn sync_rooms(state: &AppState) -> Vec<Effect> {
    state.rooms.joined.iter().map(|joined| Effect::SyncRoom { room: joined.id.clone(), visibility: state.rooms.visibility(&joined.id) }).collect()
}

/// A changed nickname shows up with the next usage read.
fn refresh_usage(state: &mut AppState) -> Vec<Effect> {
    if state.loading() {
        return vec![];
    }
    let shown = state.shown_providers();
    state.fetching = shown.iter().copied().collect();
    let mut effects = vec![Effect::ReadProfile];
    effects.extend(shown.into_iter().map(Effect::FetchUsage));
    effects
}

/// The session the app has on screen, while its latest answer is finished and not yet counted as read. The app
/// marks a session read when it comes on screen, not when an answer lands in one already there.
fn unread_on_screen(sessions: &SessionsState) -> Option<&SessionRow> {
    sessions.scan.iter().find(|row| row.on_screen && row.finished_at.is_some() && !row.running && !sessions.records.seen(row))
}

/// The sub-tasks in this card with nothing left to look at, which clear with it. The rest stay, as cards of their own.
fn read_sub_tasks(sessions: &SessionsState, card: &str) -> Vec<String> {
    let cards = families(&sessions.shown);
    let family = cards.iter().find(|family| family.head.row.key == card);
    let read = family.iter().flat_map(|family| &family.sub_tasks).filter(|session| SessionState::of(session) == SessionState::Seen);
    read.map(|session| session.row.key.clone()).collect()
}

/// Puts the latest scan on screen. While the pointer rests on the stack, cards already shown update in
/// place and new or reordered ones wait until it leaves.
fn apply_sessions(state: &mut AppState) {
    let scan = state.sessions.scan.iter().filter(|row| state.shows(row.provider)).cloned().collect();
    let visible = state.sessions.records.visible(scan);
    let sessions = &mut state.sessions;
    let tray_visible = !state.widget.hidden && !state.widget.collapsed && !sessions.shown.is_empty();
    sessions.held = false;
    if sessions.pointer_on_cards && tray_visible {
        let in_place: Vec<VisibleSession> = sessions
            .shown
            .iter()
            .filter_map(|shown| visible.iter().find(|session| session.row.key == shown.row.key).cloned())
            .collect();
        sessions.held = in_place != visible;
        sessions.shown = in_place;
    } else {
        sessions.shown = visible;
    }
}
