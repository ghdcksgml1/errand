//! What each part of the screen shows, worked out from `AppState`. Every word, colour and state is decided
//! here; the platform shells only draw it, so macOS and Windows show the same widget.

use crate::avatar::{Avatar, CLOTHES, PARTS};
use crate::format::{date_or_waiting, elapsed, month_day_time, relative_time, stopwatch, timestamp_from_seconds};
use crate::i18n::Language;
use crate::layout::{self, DockShape, MEMBER_ROWS, TrayFold};
use crate::model::{AppState, COMPLETION_DELAY, Notice};
use crate::provider::Provider;
use crate::rooms::{INVITE_PAGE, Invite, Joined, Member, NAME_LENGTH, Presence, Role, RoomMember, RoomSummary, RoomsScreen, RoomsState, TAGLINE_LENGTH, Visibility, grouped, invite_code};
use crate::sessions::{SessionState, TaskCounts, VisibleSession, families};
use crate::theme::{self, Rgb};
use crate::update::UpdateState;
use crate::usage::{self, LOW_REMAINING, Mood, UsageWindow};
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};
use unicode_segmentation::UnicodeSegmentation;

pub struct ViewContext {
    pub now: Timestamp,
    pub zone: TimeZone,
    /// What the computer is called in "Mac 시간대 기준": "Mac" or "PC".
    pub computer: &'static str,
    pub language: Language,
}

/// A service's limits, why its last read failed in the person's words, and whether usage is loading.
fn usage_inputs(state: &AppState, provider: Provider, language: Language) -> (&[UsageWindow], Option<String>, bool) {
    let slot = &state.usage[provider];
    (&slot.windows, slot.failure.as_ref().map(|failure| language.usage_error(failure)), state.loading())
}

/// One service's remaining allowance as a battery.
#[derive(Clone, Debug, PartialEq)]
pub struct BatteryVm {
    pub provider: Provider,
    /// 0-1 of the limit that runs out first; None leaves the battery empty.
    pub level: Option<f64>,
    pub color: Rgb,
    pub percent: String,
    pub percent_color: Rgb,
    /// 25% or less: red, with a warning icon.
    pub low: bool,
    pub detail: String,
    pub detail_color: Rgb,
    /// A change to `Mood::Reset` sends a light band through the refilled liquid.
    pub mood: Mood,
    pub tooltip: String,
    pub accessibility_label: String,
    pub accessibility_value: i64,
    pub accessibility_description: String,
}

pub fn battery(state: &AppState, provider: Provider, context: &ViewContext) -> BatteryVm {
    let language = context.language;
    let (windows, failure, loading) = usage_inputs(state, provider, language);
    let limit = usage::limiting_window(windows);
    let title = limit.map(|limit| language.window_title(&limit.name));
    let partial = windows.iter().any(|window| window.remaining_percent.is_none());
    let remaining = limit.and_then(|limit| limit.remaining_percent);
    let low = remaining.is_some_and(|left| failure.is_none() && left <= LOW_REMAINING);
    let percent = match remaining {
        Some(left) => format!("{left}%{}", if partial && failure.is_none() { " ?" } else { "" }),
        None if loading => language.checking().to_owned(),
        None => language.unknown().to_owned(),
    };
    let reset = if limit.is_none() && loading && windows.is_empty() {
        language.checking_reset().to_owned()
    } else {
        usage::reset_text(limit, &context.zone, language)
    };
    let scope = match failure {
        Some(_) if remaining.is_some() => Some(language.last_known()),
        Some(_) => Some(language.check_failed()),
        None => title.as_deref(),
    };
    let detail = match scope.filter(|scope| !scope.is_empty()) {
        Some(scope) => format!("{scope} · {reset}"),
        None => reset.clone(),
    };
    let summary = match remaining {
        Some(_) => language.remaining(title.as_deref().unwrap_or(language.usage()), remaining),
        None => percent.clone(),
    };
    BatteryVm {
        provider,
        level: remaining.map(|left| left.clamp(0, 100) as f64 / 100.0),
        color: theme::usage(remaining, failure.is_some()),
        percent_color: if failure.is_some() || remaining.is_none() {
            theme::MUTED
        } else if low {
            theme::LOW
        } else {
            theme::INK
        },
        low,
        detail_color: if failure.is_some() { theme::WARNING } else { theme::MUTED },
        mood: state.mood(provider, context.now),
        tooltip: language.battery_tooltip(provider, &summary, &reset, context.computer, failure.as_deref()),
        accessibility_label: language.usage_left(provider),
        accessibility_value: remaining.unwrap_or(0),
        accessibility_description: format!("{summary}, {detail}"),
        percent,
        detail,
    }
}

/// A count beside the task title, in its cards' own colour and icon.
#[derive(Clone, Debug, PartialEq)]
pub struct PillVm {
    pub state: SessionState,
    pub count: usize,
    pub color: Rgb,
    pub icon: &'static str,
    pub spinning: bool,
    pub words: String,
}

/// The task header on top of the dock; a click on it folds the cards.
#[derive(Clone, Debug, PartialEq)]
pub struct HeaderVm {
    pub title: String,
    /// New, running and seen: the three add up to the whole stack.
    pub pills: Vec<PillVm>,
    pub collapsed: bool,
    pub chevron: &'static str,
    /// The word beside the chevron; it gives way when the pills need the room.
    pub toggle: &'static str,
    pub accessibility_label: String,
    pub tooltip: &'static str,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProfileVm {
    pub name: String,
    /// The person's character, in a circle beside the name.
    pub avatar: Avatar,
    pub accessibility_label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DockVm {
    pub header: Option<HeaderVm>,
    pub batteries: Vec<BatteryVm>,
    /// While usage reloads, the refresh icon turns in place of the button.
    pub refreshing: bool,
    pub profile: ProfileVm,
    /// The rooms the person is in, as tabs under the profile, with the open one's people.
    pub rooms: Option<DockRoomsVm>,
    /// The list of rooms is open beside the dock.
    pub rooms_listed: bool,
    /// A newer Errand on offer, beside the profile.
    pub update: Option<UpdateVm>,
    pub accessibility_value: String,
}

/// A newer Errand on offer, on the dock and in the menu.
#[derive(Clone, Debug, PartialEq)]
pub struct UpdateVm {
    pub title: String,
    pub menu_title: String,
    pub tooltip: String,
    pub enabled: bool,
}

fn update_offer(state: &AppState, language: Language) -> Option<UpdateVm> {
    match &state.update {
        UpdateState::Current => None,
        UpdateState::Available(release) => Some(UpdateVm {
            title: language.update_offer(&release.version),
            menu_title: language.update_item(&release.version),
            tooltip: language.update_tooltip(&release.version, release.download.is_some()),
            enabled: true,
        }),
        UpdateState::Installing(_) => {
            let title = language.updating_app().to_owned();
            Some(UpdateVm { menu_title: title.clone(), tooltip: title.clone(), title, enabled: false })
        }
        UpdateState::Failed(release) => Some(UpdateVm {
            title: language.update_again().to_owned(),
            menu_title: language.update_again_item().to_owned(),
            tooltip: language.install_failed(&release.version),
            enabled: true,
        }),
    }
}

impl DockVm {
    /// What the dock holds, for its height and where each part goes.
    pub fn shape(&self) -> DockShape {
        DockShape {
            header: self.header.is_some(),
            batteries: self.batteries.len(),
            tabs: self.rooms.is_some(),
            rows: self.rooms.as_ref().map_or(0, DockRoomsVm::rows),
        }
    }
}

/// A room the person is in, as a tab under the profile: a lock for a private room, its name, and how many
/// are in it.
#[derive(Clone, Debug, PartialEq)]
pub struct RoomTabVm {
    pub id: String,
    pub private: bool,
    pub name: String,
    /// Once the room has answered.
    pub people: Option<usize>,
    /// Its people show under the tabs.
    pub open: bool,
    pub tooltip: String,
    pub accessibility_label: String,
}

/// One person in the open room, as thin as the folded task header: a face, a name and its pills.
#[derive(Clone, Debug, PartialEq)]
pub struct MemberRowVm {
    pub name: String,
    pub me: Option<&'static str>,
    pub face: FaceVm,
    /// Their task header's pills: answers to check, running, opened.
    pub pills: Vec<PillVm>,
    /// Away a while: drawn faded.
    pub faded: bool,
    pub tooltip: String,
    /// How long they have been in the room, as a stopwatch shows it and counted here every second while the rows
    /// show: "0:42", "12:05", "1:02:03". None when the room does not say when they came in.
    pub elapsed: Option<String>,
    /// What VoiceOver says for the row: the tooltip, and how long they have been in the room.
    pub accessibility_label: String,
    /// For the room's owner, on everyone else's row: a way to turn them out.
    pub remove: Option<RemoveVm>,
}

/// Turning someone out of the person's room, from their row's menu.
#[derive(Clone, Debug, PartialEq)]
pub struct RemoveVm {
    pub words: &'static str,
    pub room: String,
    pub member: String,
    pub name: String,
}

/// The rooms under the profile: a tab each, and the open room's people.
#[derive(Clone, Debug, PartialEq)]
pub struct DockRoomsVm {
    pub tabs: Vec<RoomTabVm>,
    pub members: Vec<MemberRowVm>,
    /// Everyone past the rows that fit, counted on a row of its own.
    pub more: Option<String>,
    /// Joining, or why the room did not answer, in place of its people.
    pub status: Option<String>,
}

impl DockRoomsVm {
    /// Thin rows under the tabs.
    pub fn rows(&self) -> usize {
        self.members.len() + usize::from(self.more.is_some()) + usize::from(self.status.is_some())
    }
}

/// The first letter of a name, shown on a face without a character.
fn initial(name: &str) -> String {
    name.trim().graphemes(true).next().map(str::to_uppercase).unwrap_or_default()
}

pub fn dock(state: &AppState, context: &ViewContext) -> DockVm {
    let batteries: Vec<BatteryVm> = state.shown_providers().into_iter().map(|provider| battery(state, provider, context)).collect();
    let header = (!state.sessions.shown.is_empty()).then(|| header(state, context.language));
    DockVm {
        accessibility_value: batteries
            .iter()
            .map(|battery| format!("{} {}, {}", battery.provider.title(), battery.percent, battery.detail))
            .collect::<Vec<_>>()
            .join("; "),
        header,
        batteries,
        refreshing: state.loading(),
        profile: ProfileVm {
            avatar: Avatar::named(&state.profile.avatar),
            accessibility_label: context.language.profile_label(&state.profile.name),
            name: state.profile.name.clone(),
        },
        rooms: dock_rooms(state, context),
        rooms_listed: state.rooms.open,
        update: update_offer(state, context.language),
    }
}

fn dock_rooms(state: &AppState, context: &ViewContext) -> Option<DockRoomsVm> {
    let language = context.language;
    let rooms = &state.rooms;
    if rooms.joined.is_empty() {
        return None;
    }
    let mut vm = DockRoomsVm { tabs: rooms.joined.iter().map(|joined| room_tab(joined, rooms, language)).collect(), members: Vec::new(), more: None, status: None };
    let open = rooms.expanded.as_deref().and_then(|id| rooms.joined.iter().find(|joined| joined.id == id));
    match open.map(|joined| (joined.counted(), joined.failure)) {
        None => {}
        Some((None, failure)) => vm.status = Some(failure.map_or(language.room_joining(), |failure| language.room_error(failure)).to_owned()),
        Some((Some(room), _)) => {
            let mut members: Vec<&Member> = room.members.iter().collect();
            // The person first, then whoever is working, the busiest first.
            members.sort_by_key(|member| (!member.me, member.presence(context.now), std::cmp::Reverse(member.tasks.running), member.name.clone()));
            let fit = if members.len() > MEMBER_ROWS { MEMBER_ROWS - 1 } else { members.len() };
            vm.more = (members.len() > fit).then(|| language.more_members(members.len() - fit));
            let owner = room.role.or_else(|| rooms.role(&room.id)) == Some(Role::Owner);
            vm.members = members[..fit]
                .iter()
                .map(|member| {
                    let mut row = member_row(member, context);
                    if owner && !member.me {
                        row.remove = Some(RemoveVm { words: language.remove_member(), room: room.id.clone(), member: member.id.clone(), name: member.name.clone() });
                        row.tooltip = format!("{}\n{}", row.tooltip, language.remove_member_hint());
                    }
                    row
                })
                .collect();
        }
    }
    Some(vm)
}

fn room_tab(joined: &Joined, rooms: &RoomsState, language: Language) -> RoomTabVm {
    let listed = rooms.list.iter().find(|room| room.id == joined.id);
    let (name, tagline) = match (&joined.room, listed) {
        (Some(room), _) => (room.name.clone(), room.tagline.as_str()),
        (None, Some(room)) => (room.name.clone(), room.tagline.as_str()),
        (None, None) => (language.room_joining().to_owned(), ""),
    };
    let people = joined.counted().map(|room| room.members.len());
    let visibility = rooms.visibility(&joined.id);
    let open = rooms.expanded.as_deref() == Some(joined.id.as_str());
    RoomTabVm {
        id: joined.id.clone(),
        private: visibility == Visibility::Private,
        accessibility_label: language.room_tab_label(&name, visibility, people),
        tooltip: language.room_tab_tooltip(&name, tagline, open),
        name,
        people,
        open,
    }
}

fn member_row(member: &Member, context: &ViewContext) -> MemberRowVm {
    let language = context.language;
    let presence = member.presence(context.now);
    // Without a name the person is just "나"; with one, "나" goes beside it.
    let (name, me) = match (member.name.is_empty(), member.me) {
        (true, _) => (language.me().to_owned(), None),
        (false, me) => (member.name.clone(), me.then(|| language.me())),
    };
    let pills = header_pills(member.tasks, language);
    // Away comes first; with nothing to count, how they are is all there is to say.
    let mut said: Vec<&str> = vec![&name];
    if presence == Presence::Away {
        said.push(language.presence(presence));
    }
    said.extend(pills.iter().map(|pill| pill.words.as_str()));
    if said.len() == 1 {
        said.push(language.presence(presence));
    }
    let tooltip = said.join(" · ");
    // Worked out from when they came in; the room says that once, not every second.
    let in_room = member.joined_at.map(|at| context.now.duration_since(at));
    MemberRowVm {
        face: face(&name, member.avatar.as_deref()),
        me,
        accessibility_label: in_room.map_or_else(|| tooltip.clone(), |span| format!("{tooltip} · {}", language.time_in_room(span))),
        elapsed: in_room.map(stopwatch),
        tooltip,
        pills,
        faded: presence == Presence::Away,
        name,
        remove: None,
    }
}

/// The task header's counts as pills in the cards' own colours and icons, leaving out counts of none: answers
/// to check, requests running, and answers opened.
fn header_pills(counts: TaskCounts, language: Language) -> Vec<PillVm> {
    [(SessionState::New, counts.new), (SessionState::Running, counts.running), (SessionState::Seen, counts.seen)]
        .into_iter()
        .filter(|(_, count)| *count > 0)
        .map(|(state, count)| PillVm {
            state,
            count,
            color: theme::session_state(state),
            icon: state.icon(),
            spinning: state == SessionState::Running,
            words: language.pill(state, count),
        })
        .collect()
}

fn header(state: &AppState, language: Language) -> HeaderVm {
    let sessions = &state.sessions;
    let open = sessions.shown.iter().filter(|session| !sessions.pending.contains_key(&session.row.key)).count();
    let collapsed = state.widget.collapsed;
    let pills = header_pills(sessions.counts(), language);
    let spoken: Vec<&str> = pills.iter().map(|pill| pill.words.as_str()).collect();
    HeaderVm {
        title: language.tasks_title(open),
        accessibility_label: language.tasks_label(open, &spoken, collapsed),
        pills,
        collapsed,
        chevron: if collapsed { "chevron-up" } else { "chevron-down" },
        toggle: language.fold_word(collapsed),
        tooltip: language.fold_tooltip(collapsed),
    }
}

/// The line under a full card's request: what happened, or what is happening.
#[derive(Clone, Debug, PartialEq)]
pub struct DetailLineVm {
    pub text: String,
    pub icon: &'static str,
    pub color: Rgb,
    pub tooltip: String,
}

/// A checked card drains a countdown while its 되돌리기 is offered.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CountdownVm {
    pub remaining: SignedDuration,
    /// How much of the wait is left, 0-1.
    pub fraction: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CardVm {
    /// The session key; clicks on the card, its checkbox and its 되돌리기 carry it.
    pub key: String,
    pub provider: Provider,
    pub title: String,
    /// Done while checked, else the session's own state.
    pub state: SessionState,
    pub countdown: Option<CountdownVm>,
    /// Folded to its title and request.
    pub compact: bool,
    pub height: f64,
    pub chip: &'static str,
    pub check_label: String,
    pub check_tooltip: &'static str,
    pub undo_label: String,
    pub prompt: String,
    /// No request was readable; it reads muted.
    pub prompt_missing: bool,
    pub prompt_tooltip: Option<String>,
    /// Full cards only.
    pub detail: Option<DetailLineVm>,
    /// What the app says to do next, on a finished full card.
    pub action: Option<String>,
    pub when: String,
    pub folder: Option<String>,
    pub tooltip: String,
    pub accessibility_label: String,
    /// The sessions started from this one, each a line under it, drawn from a folded card's words.
    pub sub_tasks: Vec<CardVm>,
}

impl CardVm {
    /// The card with its sub-tasks; `height` is its own part.
    pub fn total_height(&self) -> f64 {
        self.height + layout::sub_tasks_height(self.sub_tasks.len())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrayVm {
    pub cards: Vec<CardVm>,
    pub fold: TrayFold,
}

/// `max_height` is the room the shell found for the cards on the dock's side of the screen.
pub fn tray(state: &AppState, context: &ViewContext, max_height: f64) -> TrayVm {
    let families = families(&state.sessions.shown);
    let full: Vec<f64> = families.iter().map(|family| layout::card_height(action_shown(family.head))).collect();
    let sub_tasks: Vec<usize> = families.iter().map(|family| family.sub_tasks.len()).collect();
    let fold = layout::fold_cards(&full, &sub_tasks, max_height);
    let deadline = |session: &VisibleSession| state.sessions.pending.get(&session.row.key).copied();
    let cards = families
        .iter()
        .enumerate()
        .map(|(index, family)| {
            let mut vm = card(family.head, deadline(family.head), fold.compact(index), fold.heights[index], context);
            vm.sub_tasks = family.sub_tasks.iter().map(|session| sub_task(session, deadline(session), context)).collect();
            vm
        })
        .collect();
    TrayVm { cards, fold }
}

fn sub_task(session: &VisibleSession, deadline: Option<Timestamp>, context: &ViewContext) -> CardVm {
    let vm = card(session, deadline, true, layout::SUB_TASK_HEIGHT, context);
    CardVm { accessibility_label: context.language.sub_task_label(&vm.accessibility_label), ..vm }
}

fn action_shown(session: &VisibleSession) -> bool {
    session.row.action.is_some() && !session.row.running
}

fn card(session: &VisibleSession, deadline: Option<Timestamp>, compact: bool, height: f64, context: &ViewContext) -> CardVm {
    let language = context.language;
    let row = &session.row;
    let own_state = SessionState::of(session);
    let state = if deadline.is_some() { SessionState::Done } else { own_state };
    let app = row.provider.title();
    let title = if row.title.is_empty() { language.untitled(row.provider).to_owned() } else { row.title.clone() };
    let asked = language.recorded(&row.prompt);
    let result = row.result.as_deref().map(|result| language.recorded(result));
    let action = row.action.as_deref().map(|action| language.recorded(action));
    let prompt_missing = row.prompt.is_empty();
    let prompt = if prompt_missing { language.prompt_unreadable().to_owned() } else { format!("“{asked}”") };
    let answered = row.finished_at.unwrap_or(row.prompt_at);
    let when = language.card_time(&relative_time(Some(if row.running { row.prompt_at } else { answered }), context.now, &context.zone, language), !row.running);
    let countdown = deadline.map(|deadline| {
        let remaining = deadline.duration_since(context.now).max(SignedDuration::ZERO);
        CountdownVm { remaining, fraction: (remaining.as_secs_f64() / COMPLETION_DELAY.as_secs_f64()).min(1.0) }
    });
    let mut vm = CardVm {
        key: row.key.clone(),
        provider: row.provider,
        title: title.clone(),
        state,
        countdown,
        compact,
        height,
        chip: language.session_state(state),
        check_label: language.complete_label(&title),
        check_tooltip: language.complete_tooltip(deadline.is_some()),
        undo_label: language.undo_label(&title),
        prompt: prompt.clone(),
        prompt_missing,
        prompt_tooltip: None,
        detail: None,
        action: None,
        when: when.clone(),
        folder: (!row.folder.is_empty()).then(|| row.folder.clone()),
        tooltip: String::new(),
        accessibility_label: String::new(),
        sub_tasks: Vec::new(),
    };
    if compact {
        // Folded: the title and the request stay, so "what did I ask" survives a crowded stack.
        let mut lines = vec![format!("{app} · {title}"), prompt];
        lines.extend(result.as_deref().map(|result| language.result_line(result)));
        lines.extend(action.as_deref().map(|action| language.to_do_line(action)));
        lines.push(format!("{when} · {}", language.click_to_open()));
        vm.tooltip = lines.join("\n");
        vm.accessibility_label = language.card_label(app, &title, vm.chip, &asked, None);
        return vm;
    }
    let (text, icon, color) = match own_state {
        SessionState::Stalled => (language.stalled_line().to_owned(), "circle-alert", theme::STALLED),
        SessionState::Running => (language.working(&elapsed(row.prompt_at, context.now, language)), "timer", theme::RUNNING),
        _ if result.is_some() => (result.clone().unwrap_or_default(), "corner-down-right", theme::MUTED),
        SessionState::Aborted => (language.stopped_line().to_owned(), "circle-slash", theme::MUTED),
        _ => (language.answered_line().to_owned(), "circle-check", theme::MUTED),
    };
    let mut notes: Vec<String> = result.iter().cloned().collect();
    notes.extend(action.as_deref().map(|action| language.to_do_line(action)));
    vm.accessibility_label = language.card_label(app, &title, vm.chip, &asked, Some(&text));
    vm.detail = Some(DetailLineVm { tooltip: if notes.is_empty() { text.clone() } else { notes.join("\n") }, text, icon, color });
    vm.action = action.filter(|_| action_shown(session));
    vm.prompt_tooltip = (!prompt_missing).then_some(asked);
    vm.tooltip = format!("{app} · {title}\n{}", language.click_to_open());
    vm
}

/// One quota row on an account card.
#[derive(Clone, Debug, PartialEq)]
pub struct QuotaVm {
    pub title: String,
    pub remaining: Option<i64>,
    pub value: String,
    pub value_color: Rgb,
    pub accent: Rgb,
    pub next_reset: String,
    pub last_reset: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AccountCardVm {
    pub provider: Provider,
    pub status: String,
    pub open_title: &'static str,
    pub open_enabled: bool,
    /// Several limits use a tighter row.
    pub compact: bool,
    pub quotas: Vec<QuotaVm>,
    /// Without limits: a headline and what to do.
    pub empty: Option<(String, String)>,
    pub data_status: String,
    pub data_status_tooltip: String,
    pub data_status_failed: bool,
}

pub fn account_card(state: &AppState, provider: Provider, context: &ViewContext) -> AccountCardVm {
    let language = context.language;
    let (windows, failure, loading) = usage_inputs(state, provider, language);
    let account = &state.accounts[provider];
    let label = account.email.as_deref().unwrap_or(language.sign_in_needed(provider));
    let last_resets = state.last_resets(provider);
    let quotas = windows
        .iter()
        .map(|window| {
            let next = window.valid_reset().and_then(timestamp_from_seconds).map(|at| month_day_time(at, &context.zone));
            QuotaVm {
                title: language.window_title(&window.name),
                remaining: window.remaining_percent,
                value: window.remaining_percent.map_or_else(|| language.unknown().to_owned(), |left| language.percent_left(left)),
                value_color: if failure.is_some() { theme::MUTED } else { theme::INK },
                accent: theme::usage(window.remaining_percent, failure.is_some()),
                next_reset: language.next_reset(next.as_deref()),
                last_reset: language.reset_seen(&date_or_waiting(last_resets.get(&window.identifier).copied(), &context.zone, language)),
            }
        })
        .collect();
    let empty = windows.is_empty().then(|| language.no_limits(provider, failure.clone(), loading));
    let status = match failure {
        Some(_) if !windows.is_empty() => language.update_failed(),
        Some(_) => language.check_failed(),
        None if loading => language.updating(),
        None if windows.is_empty() => language.no_data(),
        None => language.up_to_date(),
    };
    let mut data_status = status.to_owned();
    let slot = &state.usage[provider];
    if let (Some(reported), None) = (slot.reported_at, &failure) {
        data_status = language.reported_at(&month_day_time(reported, &context.zone));
    } else if let Some(fetched) = slot.fetched_at {
        data_status = language.checked_at(&data_status, &month_day_time(fetched, &context.zone));
    }
    if windows.len() > 1 {
        data_status = language.limit_count(&data_status, windows.len());
    }
    AccountCardVm {
        provider,
        status: if account.running { language.running_status(label) } else { label.to_owned() },
        open_title: language.open_button(account.running),
        open_enabled: !state.launching,
        compact: windows.len() > 1,
        quotas,
        empty,
        data_status_tooltip: failure.clone().unwrap_or_else(|| data_status.clone()),
        data_status_failed: failure.is_some(),
        data_status,
    }
}

/// The usage details beside the dock.
#[derive(Clone, Debug, PartialEq)]
pub struct DetailsVm {
    pub cards: Vec<AccountCardVm>,
    pub refresh_enabled: bool,
}

pub fn details(state: &AppState, context: &ViewContext) -> DetailsVm {
    DetailsVm { cards: state.shown_providers().into_iter().map(|provider| account_card(state, provider, context)).collect(), refresh_enabled: !state.loading() }
}

/// One part in the character editor: what it is now, and the character an arrow either side makes.
#[derive(Clone, Debug, PartialEq)]
pub struct AvatarPartVm {
    pub label: &'static str,
    pub choice: &'static str,
    pub previous: Avatar,
    pub next: Avatar,
    /// The arrows, as VoiceOver names them.
    pub previous_label: String,
    pub next_label: String,
}

/// A colour the clothes can take, and the character wearing it.
#[derive(Clone, Debug, PartialEq)]
pub struct ClothesVm {
    pub colour: Rgb,
    pub label: &'static str,
    pub chosen: bool,
    pub avatar: Avatar,
}

/// The character editor beside the dock: the character, a row of arrows per part and the clothes' colours,
/// under a title and over a shuffle for a whole new one.
#[derive(Clone, Debug, PartialEq)]
pub struct AvatarEditorVm {
    pub avatar: Avatar,
    pub parts: Vec<AvatarPartVm>,
    pub clothes: Vec<ClothesVm>,
}

pub fn avatar_editor(state: &AppState, language: Language) -> AvatarEditorVm {
    let avatar = Avatar::named(&state.profile.avatar);
    let parts = PARTS
        .into_iter()
        .map(|part| AvatarPartVm {
            label: language.avatar_part(part),
            choice: language.avatar_choice(&avatar, part),
            previous: avatar.step(part, -1),
            next: avatar.step(part, 1),
            previous_label: language.previous_choice(part),
            next_label: language.next_choice(part),
        })
        .collect();
    let clothes = CLOTHES
        .into_iter()
        .enumerate()
        .map(|(index, colour)| ClothesVm { colour, label: language.clothes_colour(index), chosen: avatar.clothes == colour, avatar: Avatar { clothes: colour, ..avatar } })
        .collect();
    AvatarEditorVm { avatar, parts, clothes }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    #[default]
    Neutral,
    Error,
}

/// The line at the foot of the account window.
#[derive(Clone, Debug, PartialEq)]
pub struct MessageVm {
    pub text: String,
    pub tone: Tone,
}

/// The account window.
#[derive(Clone, Debug, PartialEq)]
pub struct DashboardVm {
    /// Names the services shown.
    pub title: String,
    pub cards: Vec<AccountCardVm>,
    pub batteries: Vec<BatteryVm>,
    /// A friendly line under each battery.
    pub moods: Vec<&'static str>,
    pub refresh_enabled: bool,
    pub usage_status: String,
    pub message: MessageVm,
}

pub fn dashboard(state: &AppState, context: &ViewContext) -> DashboardVm {
    let language = context.language;
    let shown = state.shown_providers();
    let moods = shown
        .iter()
        .map(|provider| {
            let (windows, failure, loading) = usage_inputs(state, *provider, language);
            if failure.is_some() {
                language.waiting_to_connect()
            } else if loading && windows.is_empty() {
                language.waiting_for_usage()
            } else {
                language.mood(state.mood(*provider, context.now))
            }
        })
        .collect();
    DashboardVm {
        title: language.dashboard_title(&shown),
        cards: shown.iter().map(|provider| account_card(state, *provider, context)).collect(),
        batteries: shown.iter().map(|provider| battery(state, *provider, context)).collect(),
        moods,
        refresh_enabled: !state.loading(),
        usage_status: if state.loading() { language.checking_all().to_owned() } else { language.update_rhythm(context.computer) },
        message: MessageVm {
            text: language.notice(&state.notice, &shown),
            tone: if matches!(state.notice, Notice::OpenFailed(_)) { Tone::Error } else { Tone::Neutral },
        },
    }
}

/// A chip as the task cards have: words, with an icon or a spinner, coloured as a card in `style`.
#[derive(Clone, Debug, PartialEq)]
pub struct ChipVm {
    pub text: String,
    pub icon: Option<&'static str>,
    pub spinning: bool,
    pub style: SessionState,
}

/// A count with its icon, as the task header shows them.
#[derive(Clone, Debug, PartialEq)]
pub struct CountVm {
    pub count: usize,
    pub icon: &'static str,
    pub spinning: bool,
    pub color: Rgb,
    /// The whole sentence, for the tooltip and VoiceOver.
    pub words: String,
}

/// A face: the person's own character, in public and private rooms alike, or without one a first letter in a
/// grey circle.
#[derive(Clone, Debug, PartialEq)]
pub struct FaceVm {
    pub avatar: Option<Avatar>,
    pub initial: String,
}

/// A code that names no parts, a seed among them, draws the grey face rather than a seed's character.
fn face(name: &str, avatar: Option<&str>) -> FaceVm {
    FaceVm { avatar: avatar.and_then(Avatar::parse), initial: initial(name) }
}

fn visibility_chip(visibility: Visibility, language: Language) -> ChipVm {
    let style = match visibility {
        Visibility::Public => SessionState::Seen,
        Visibility::Private => SessionState::New,
    };
    ChipVm { text: language.visibility(visibility).to_owned(), icon: None, spinning: false, style }
}

/// A room on offer, and whether the person is in it.
#[derive(Clone, Debug, PartialEq)]
pub struct RoomCardVm {
    pub id: String,
    pub name: String,
    pub tagline: String,
    pub visibility: ChipVm,
    /// A few of the people at work there.
    pub faces: Vec<FaceVm>,
    pub working: CountVm,
    pub people: String,
    /// The person is in it, so its button leaves and its name is marked.
    pub joined: bool,
    pub badge: Option<&'static str>,
    /// For the owner of a private room: a way to its invite link, beside the room's way in or out.
    pub invite: Option<&'static str>,
    /// For a member of a private room they are not in right now: a way out of it for good.
    pub quit: Option<&'static str>,
    pub button: &'static str,
    /// Why its button cannot be pressed, for a room the person cannot join yet.
    pub shut: Option<String>,
}

/// The list of rooms beside the dock, to join and leave; who is in them shows on the dock.
#[derive(Clone, Debug, PartialEq)]
pub struct RoomsVm {
    pub title: &'static str,
    /// What rooms are, or why no more can be joined.
    pub subtitle: String,
    /// The rooms the person is in first, then the rest.
    pub list: Vec<RoomCardVm>,
    /// Over the rooms the person is in and over the rest, once they are in one; a list with none joined has no
    /// headings.
    pub joined_heading: Option<String>,
    pub others_heading: Option<&'static str>,
    /// Loading or a failure, while there is no list to show.
    pub status: Option<String>,
    pub footer: &'static str,
    pub close: &'static str,
    /// The two ways to private rooms under the title: making one, and coming in by a code.
    pub create: &'static str,
    pub enter_code: &'static str,
    /// The list, or a step of making or joining a private room in its place.
    pub screen: RoomsScreenVm,
    pub back: &'static str,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RoomsScreenVm {
    List,
    Create(CreateRoomVm),
    Invite(InviteVm),
    Members(MembersVm),
    Code(Box<CodeVm>),
}

/// Everyone a room the person owns lets in.
#[derive(Clone, Debug, PartialEq)]
pub struct MembersVm {
    pub room: String,
    pub title: &'static str,
    pub subtitle: String,
    pub rows: Vec<MemberListRowVm>,
    /// Loading, or why the list did not come.
    pub status: Option<String>,
    pub note: &'static str,
}

/// One person on the owner's list.
#[derive(Clone, Debug, PartialEq)]
pub struct MemberListRowVm {
    pub face: FaceVm,
    pub name: String,
    /// Without a nickname, a muted stand-in.
    pub named: bool,
    /// Owner or not, working, resting, or when they were last here.
    pub status: String,
    /// Not in the room now: drawn faded.
    pub away: bool,
    pub remove: Option<RemoveVm>,
}

/// Naming a new private room.
#[derive(Clone, Debug, PartialEq)]
pub struct CreateRoomVm {
    pub title: &'static str,
    pub subtitle: &'static str,
    pub name_label: &'static str,
    pub name_placeholder: &'static str,
    pub name_limit: usize,
    pub tagline_label: &'static str,
    pub tagline_placeholder: &'static str,
    pub tagline_limit: usize,
    pub notes: Vec<&'static str>,
    pub button: &'static str,
    pub sending: bool,
    pub failure: Option<String>,
}

/// A room's invite link, for its owner.
#[derive(Clone, Debug, PartialEq)]
pub struct InviteVm {
    pub room: String,
    pub title: &'static str,
    pub subtitle: String,
    /// The link as the box shows it: where it goes, without the code, which shows below.
    pub host: String,
    /// How long it has left and for how many; or loading, or why there is none.
    pub left: String,
    pub link: Option<String>,
    pub copy_link: &'static str,
    pub copied: bool,
    pub code_caption: &'static str,
    /// In fours: K7QF-M2XA-9P.
    pub code: Option<String>,
    pub copy_code: &'static str,
    pub notes: Vec<&'static str>,
    pub renew: &'static str,
    pub delete: &'static str,
    pub remove_room: &'static str,
    /// The way to everyone the room lets in.
    pub members: &'static str,
    pub busy: bool,
    pub failure: Option<String>,
}

/// Typing or pasting an invite code, or one a link brought, and the room it opens.
#[derive(Clone, Debug, PartialEq)]
pub struct CodeVm {
    pub title: &'static str,
    pub subtitle: &'static str,
    pub from_link: bool,
    pub label: &'static str,
    /// What the field holds: the code in fours once read, or what was typed.
    pub code: String,
    pub placeholder: String,
    pub check: &'static str,
    pub checking: bool,
    pub failure: Option<String>,
    pub invited_to: &'static str,
    /// The room the code opens, once the server says.
    pub room: Option<RoomCardVm>,
    pub accept: &'static str,
    pub accepting: bool,
    pub notes: Vec<&'static str>,
}

pub fn rooms(state: &AppState, context: &ViewContext) -> RoomsVm {
    let language = context.language;
    let rooms = &state.rooms;
    // A list once read stays up, even when reading it again fails.
    let status = match rooms.failure {
        _ if !rooms.list.is_empty() => None,
        Some(failure) => Some(language.room_error(failure).to_owned()),
        None => rooms.loading.then(|| language.rooms_loading().to_owned()),
    };
    let mut list: Vec<RoomCardVm> = rooms.list.iter().map(|room| room_card(room, rooms, language)).collect();
    list.sort_by_key(|card| !card.joined);
    let any_joined = list.iter().any(|card| card.joined);
    let joined_heading = any_joined.then(|| language.joined_rooms(rooms.joined.len()));
    let others_heading = (any_joined && list.iter().any(|card| !card.joined)).then(|| language.other_rooms());
    let subtitle = if rooms.at_limit() { language.rooms_full() } else { language.rooms_subtitle().to_owned() };
    let screen = rooms_screen(state, context);
    RoomsVm {
        title: language.rooms_title(),
        subtitle,
        list,
        joined_heading,
        others_heading,
        status,
        footer: language.rooms_footer(rooms.sample),
        close: language.close(),
        create: language.create_room_button(),
        enter_code: language.invite_code_button(),
        screen,
        back: language.back(),
    }
}

/// A room on offer as its card shows it, with the ways in and out the person has.
fn room_card(room: &RoomSummary, rooms: &RoomsState, language: Language) -> RoomCardVm {
    let joined = rooms.is_joined(&room.id);
    RoomCardVm {
        id: room.id.clone(),
        name: room.name.clone(),
        tagline: room.tagline.clone(),
        visibility: visibility_chip(room.visibility, language),
        faces: room.faces.iter().map(|someone| face(&someone.name, someone.avatar.as_deref())).collect(),
        working: CountVm { count: room.working, icon: "loader-circle", spinning: true, color: theme::RUNNING, words: language.working_words(room.working) },
        people: language.room_people(room.members),
        joined,
        badge: joined.then(|| language.joined_badge()),
        invite: (room.role == Some(Role::Owner)).then(|| language.invite_room()),
        quit: (room.role == Some(Role::Member) && !joined).then(|| language.quit_room()),
        button: if joined { language.leave_room() } else { language.join_room() },
        shut: (rooms.at_limit() && !joined).then(|| language.rooms_full()),
    }
}

/// Someone on the owner's list: who, how they are or when they were last here, and a way to turn them out.
fn member_list_row(member: &RoomMember, room: &str, context: &ViewContext) -> MemberListRowVm {
    let language = context.language;
    let name = member.name.clone().filter(|name| !name.trim().is_empty());
    let mut status: Vec<String> = Vec::new();
    if member.role == Role::Owner {
        status.push(language.owner().to_owned());
    }
    if member.me {
        status.push(language.me().to_owned());
    }
    if !member.me {
        status.push(match (member.present, member.working) {
            (true, true) => language.presence(Presence::Working).to_owned(),
            (true, false) => language.presence(Presence::Resting).to_owned(),
            (false, _) => member.last_seen.map_or_else(
                || language.away_now().to_owned(),
                |seen| {
                    let day = |at: Timestamp| at.to_zoned(context.zone.clone()).date();
                    let days = day(seen).until(day(context.now)).map_or(0, |span| span.get_days() as i64).max(0);
                    language.last_here(days)
                },
            ),
        });
    }
    let shown = name.clone().unwrap_or_else(|| language.unnamed_member().to_owned());
    MemberListRowVm {
        face: face(&shown, member.avatar.as_deref()),
        named: name.is_some(),
        status: status.join(" · "),
        away: !member.present && !member.me,
        remove: (member.role != Role::Owner && !member.me).then(|| RemoveVm {
            words: language.remove_member(),
            room: room.to_owned(),
            member: member.id.clone(),
            name: shown.clone(),
        }),
        name: shown,
    }
}

fn rooms_screen(state: &AppState, context: &ViewContext) -> RoomsScreenVm {
    let language = context.language;
    let rooms = &state.rooms;
    match &rooms.screen {
        RoomsScreen::List => RoomsScreenVm::List,
        RoomsScreen::Create { sending, failure } => RoomsScreenVm::Create(CreateRoomVm {
            title: language.create_room_title(),
            subtitle: language.create_room_subtitle(),
            name_label: language.room_name_label(),
            name_placeholder: language.room_name_placeholder(),
            name_limit: NAME_LENGTH,
            tagline_label: language.room_tagline_label(),
            tagline_placeholder: language.room_tagline_placeholder(),
            tagline_limit: TAGLINE_LENGTH,
            notes: language.create_room_notes().to_vec(),
            button: language.create_room_go(*sending),
            sending: *sending,
            failure: failure.map(|failure| language.room_error(failure).to_owned()),
        }),
        RoomsScreen::Invite { room, invite, loading, failure, copied } => {
            let left = match (invite, loading) {
                (Some(invite), _) => {
                    let seconds = invite.expires_at.as_second() - context.now.as_second();
                    language.invite_left((seconds.max(0) + 86_399) / 86_400, invite.uses_left)
                }
                (None, true) => language.invite_loading().to_owned(),
                (None, false) => language.no_invite().to_owned(),
            };
            RoomsScreenVm::Invite(InviteVm {
                room: room.clone(),
                title: language.invite_title(),
                subtitle: language.invite_subtitle(rooms.name(room).unwrap_or_default()),
                host: INVITE_PAGE.trim_start_matches("https://").to_owned(),
                left,
                link: invite.as_ref().map(Invite::link),
                copy_link: language.copy_link(*copied),
                copied: *copied,
                code_caption: language.invite_code_caption(),
                code: invite.as_ref().map(|invite| grouped(&invite.code, '-')),
                copy_code: language.copy_code(),
                notes: language.invite_notes().to_vec(),
                renew: language.renew_invite(),
                delete: language.delete_invite(),
                remove_room: language.remove_room(),
                members: language.members_title(),
                busy: *loading,
                failure: failure.map(|failure| language.room_error(failure).to_owned()),
            })
        }
        RoomsScreen::Members { room, members, loading, failure } => {
            let name = rooms.name(room).unwrap_or_default();
            let rows: Vec<MemberListRowVm> = members.iter().flatten().map(|member| member_list_row(member, room, context)).collect();
            let status = match (members, loading, failure) {
                (_, _, Some(failure)) => Some(language.room_error(*failure).to_owned()),
                (None, true, _) => Some(language.members_loading().to_owned()),
                _ => None,
            };
            RoomsScreenVm::Members(MembersVm {
                room: room.clone(),
                title: language.members_title(),
                subtitle: language.members_subtitle(name, rows.len()),
                rows,
                status,
                note: language.members_note(),
            })
        }
        RoomsScreen::Code { code, checking, room, failure, from_link, accepting } => {
            let card = room.as_ref().map(|summary| {
                let mut card = room_card(summary, rooms, language);
                // The card is about coming in: no other ways in or out, and no mark.
                card.invite = None;
                card.quit = None;
                card.badge = None;
                card
            });
            RoomsScreenVm::Code(Box::new(CodeVm {
                title: language.code_title(*from_link),
                subtitle: language.code_subtitle(*from_link),
                from_link: *from_link,
                label: language.code_label(),
                code: invite_code(code).map_or_else(|| code.clone(), |code| grouped(&code, '-')),
                placeholder: "K7QF-M2XA-9P".to_owned(),
                check: language.check_code(*checking),
                checking: *checking,
                failure: failure.map(|failure| language.room_error(failure).to_owned()),
                invited_to: language.invited_to(),
                room: card,
                accept: language.accept_invite(*accepting),
                accepting: *accepting,
                notes: language.accept_notes().to_vec(),
            }))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    OpenApp(Provider),
    RefreshUsage,
    ShowDashboard,
    ToggleWidget,
    ResetWidgetPosition,
    ToggleService(Provider),
    ShowRooms,
    InstallUpdate,
    Quit,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MenuItemVm {
    pub title: String,
    pub action: Option<MenuAction>,
    pub key: Option<char>,
    pub enabled: bool,
    /// Usage lines sit indented under their service.
    pub indent: u8,
    pub tooltip: Option<String>,
    /// The service's own app icon.
    pub icon: Option<Provider>,
    /// Drawn with a check mark.
    pub checked: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MenuEntry {
    Item(MenuItemVm),
    Separator,
}

fn item(title: impl Into<String>, action: MenuAction, key: Option<char>) -> MenuEntry {
    MenuEntry::Item(MenuItemVm { title: title.into(), action: Some(action), key, enabled: true, indent: 0, tooltip: None, icon: None, checked: false })
}

/// The menu bar menu. `widget_visible` is whether the dock is on screen.
pub fn menu(state: &AppState, widget_visible: bool, context: &ViewContext) -> Vec<MenuEntry> {
    let language = context.language;
    let mut entries = Vec::new();
    if let Some(update) = update_offer(state, language) {
        entries.push(MenuEntry::Item(MenuItemVm {
            title: update.menu_title,
            action: Some(MenuAction::InstallUpdate),
            key: None,
            enabled: update.enabled,
            indent: 0,
            tooltip: Some(update.tooltip),
            icon: None,
            checked: false,
        }));
        entries.push(MenuEntry::Separator);
    }
    // A service keeps its shortcut whether or not the other is shown.
    for (index, provider) in Provider::ALL.into_iter().enumerate().filter(|(_, provider)| state.shows(*provider)) {
        entries.push(MenuEntry::Item(MenuItemVm {
            title: language.open_app_item(provider, state.accounts[provider].email.as_deref()),
            action: Some(MenuAction::OpenApp(provider)),
            key: char::from_digit(index as u32 + 1, 10),
            enabled: !state.launching,
            indent: 0,
            tooltip: None,
            icon: Some(provider),
            checked: false,
        }));
        let slot = &state.usage[provider];
        let mut title = if slot.has_response {
            usage::summary(&slot.windows, language)
        } else if slot.failure.is_some() {
            language.usage_check_failed().to_owned()
        } else {
            language.checking_usage().to_owned()
        };
        if slot.has_response && slot.failure.is_some() {
            title = format!("{title} · {}", language.last_known());
        }
        entries.push(MenuEntry::Item(MenuItemVm {
            title,
            action: None,
            key: None,
            enabled: false,
            indent: 1,
            tooltip: if slot.has_response {
                Some(usage::reset_details(&slot.windows, &context.zone, language))
            } else {
                slot.failure.as_ref().map(|failure| language.usage_error(failure))
            },
            icon: None,
            checked: false,
        }));
    }
    entries.push(MenuEntry::Separator);
    let mut refresh = item(language.refresh_item(), MenuAction::RefreshUsage, Some('r'));
    if let MenuEntry::Item(refresh) = &mut refresh {
        refresh.enabled = !state.loading();
    }
    entries.push(refresh);
    entries.push(item(language.dashboard_item(), MenuAction::ShowDashboard, Some('0')));
    entries.push(item(language.rooms_item(), MenuAction::ShowRooms, None));
    entries.push(item(language.widget_item(widget_visible), MenuAction::ToggleWidget, Some('p')));
    entries.push(item(language.reset_widget_item(), MenuAction::ResetWidgetPosition, None));
    entries.push(MenuEntry::Separator);
    let last = state.shown_providers().len() == 1;
    for provider in Provider::ALL {
        let shown = state.shows(provider);
        entries.push(MenuEntry::Item(MenuItemVm {
            title: language.show_service_item(provider),
            action: Some(MenuAction::ToggleService(provider)),
            key: None,
            // The last service shown cannot be hidden.
            enabled: !(shown && last),
            indent: 0,
            tooltip: Some(language.service_toggle_tooltip(provider, shown)),
            icon: None,
            checked: shown,
        }));
    }
    entries.push(MenuEntry::Separator);
    entries.push(item(language.quit_item(), MenuAction::Quit, Some('q')));
    entries
}

/// The menu bar glyph: one small battery per service, filled to what remains.
#[derive(Clone, Debug, PartialEq)]
pub struct StatusIconVm {
    pub levels: Vec<Option<i64>>,
    pub tooltip: String,
}

pub fn status_icon(state: &AppState, language: Language) -> StatusIconVm {
    let shown = state.shown_providers();
    let levels: Vec<Option<i64>> =
        shown.iter().map(|provider| usage::limiting_window(&state.usage[*provider].windows).and_then(|limit| limit.remaining_percent)).collect();
    let spoken: Vec<String> = shown.iter().zip(&levels).map(|(provider, level)| language.status_level(*provider, *level)).collect();
    StatusIconVm { levels, tooltip: spoken.join(" · ") }
}
