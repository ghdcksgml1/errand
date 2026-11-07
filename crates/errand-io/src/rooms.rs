//! Work rooms from sample data, until there is a server. Each sample member works in their own rhythm as
//! the clock moves, so a room feels alive; the person appears as themselves, with their own counts and
//! character, in every room they are in, its place in the list included.

use errand_core::Timestamp;
use errand_core::avatar::Avatar;
use errand_core::ports::RoomService;
use errand_core::rooms::{Face, Identity, Invite, Member, OWNED_ROOMS, PresenceReport, Role, Room, RoomError, RoomSummary, Visibility, invite_code};
use errand_core::sessions::TaskCounts;
use jiff::SignedDuration;
use std::hash::{BuildHasher, RandomState};
use std::sync::{Mutex, PoisonError};

struct Sample {
    id: &'static str,
    name: &'static str,
    tagline: &'static str,
    visibility: Visibility,
    /// Aliases the room gave in a public room; profile names in a private one.
    members: &'static [&'static str],
}

/// A room as the samples have it now: one of [`SAMPLES`], or one the person made.
struct Spot {
    id: String,
    name: String,
    tagline: String,
    visibility: Visibility,
    members: Vec<&'static str>,
    role: Option<Role>,
}

/// What the person changed in the samples this run.
#[derive(Default)]
struct Changes {
    /// The person as each room they are in last heard them, by room.
    present: Vec<(String, Member)>,
    /// Private rooms the person made: id, name, tagline.
    made: Vec<(String, String, String)>,
    /// Invites to the person's rooms, by room, newest last.
    invites: Vec<(String, Invite)>,
    /// Rooms the person is out of for good.
    quit: Vec<String>,
    /// Sample members turned out of a room, by member id.
    removed: Vec<String>,
    /// Makes room ids and codes differ.
    counter: u64,
}

/// How long a new invite lasts, and for how many.
const INVITE_DAYS: i64 = 7;
const INVITE_USES: usize = 10;

const SAMPLES: [Sample; 5] = [
    Sample {
        id: "team",
        name: "우리 팀 야근방",
        tagline: "초대받은 사람만 · 서로 프로필이 보여요",
        visibility: Visibility::Private,
        members: &["민지", "지훈", "서연", "도윤"],
    },
    Sample {
        id: "late-night",
        name: "고독한 야근방",
        tagline: "불 꺼진 사무실, 모니터만 켜진 사람들",
        visibility: Visibility::Public,
        members: &["졸린 수달", "바쁜 너구리", "조용한 부엉이", "성실한 펭귄", "배고픈 판다", "차분한 여우", "꼼꼼한 다람쥐"],
    },
    Sample {
        id: "dawn",
        name: "새벽 코딩방",
        tagline: "해 뜨기 전에 커밋하는 사람들",
        visibility: Visibility::Public,
        members: &["느긋한 고래", "용감한 토끼", "수줍은 햄스터", "신난 거북이", "집중한 고양이"],
    },
    Sample {
        id: "deadline",
        name: "마감 직전방",
        tagline: "오늘 안에 끝내야 하는 사람들",
        visibility: Visibility::Public,
        members: &["바쁜 고양이", "급한 토끼", "졸린 판다", "성실한 수달", "배고픈 펭귄", "조용한 여우", "용감한 부엉이", "꼼꼼한 너구리", "차분한 고래"],
    },
    Sample {
        id: "quiet",
        name: "조용히 집중방",
        tagline: "말없이 각자 할 일 하는 방",
        visibility: Visibility::Public,
        members: &["조용한 거북이", "차분한 햄스터", "느긋한 다람쥐", "수줍은 펭귄"],
    },
];

/// Aliases a public room gives, an adjective and an animal.
const ADJECTIVES: [&str; 12] = ["졸린", "바쁜", "조용한", "용감한", "느긋한", "성실한", "수줍은", "배고픈", "신난", "차분한", "집중한", "꼼꼼한"];
const ANIMALS: [&str; 12] = ["수달", "너구리", "고양이", "펭귄", "다람쥐", "판다", "여우", "부엉이", "고래", "토끼", "거북이", "햄스터"];

/// How many faces a room shows beside its counts.
const FACES: usize = 3;

pub struct SampleRooms {
    /// Picks the person's alias, so it differs from launch to launch and says nothing about them.
    alias_seed: u64,
    changes: Mutex<Changes>,
}

impl Default for SampleRooms {
    fn default() -> SampleRooms {
        SampleRooms::new()
    }
}

impl SampleRooms {
    pub fn new() -> SampleRooms {
        SampleRooms { alias_seed: RandomState::new().hash_one("alias"), changes: Mutex::new(Changes::default()) }
    }

    fn changes(&self) -> std::sync::MutexGuard<'_, Changes> {
        self.changes.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The person in `room`, if they are in it.
    fn me(&self, room: &str) -> Option<Member> {
        self.changes().present.iter().find(|(id, _)| id == room).map(|(_, me)| me.clone())
    }

    /// The rooms as they are now: the samples the person is not out of, and the ones they made. The private
    /// sample room is the person's own, so making invites and turning people out can be tried.
    fn spots(&self) -> Vec<Spot> {
        let changes = self.changes();
        let samples = SAMPLES.iter().filter(|sample| !changes.quit.iter().any(|id| id == sample.id)).map(|sample| Spot {
            id: sample.id.to_owned(),
            name: sample.name.to_owned(),
            tagline: sample.tagline.to_owned(),
            visibility: sample.visibility,
            members: sample.members.iter().copied().filter(|name| !changes.removed.contains(&format!("{}/{name}", sample.id))).collect(),
            role: (sample.visibility == Visibility::Private).then_some(Role::Owner),
        });
        let made = changes.made.iter().map(|(id, name, tagline)| Spot {
            id: id.clone(),
            name: name.clone(),
            tagline: tagline.clone(),
            visibility: Visibility::Private,
            members: Vec::new(),
            role: Some(Role::Owner),
        });
        samples.chain(made).collect()
    }

    fn spot(&self, room: &str) -> Option<Spot> {
        self.spots().into_iter().find(|spot| spot.id == room)
    }

    fn members(&self, spot: &Spot, now: Timestamp) -> Vec<Member> {
        let minute = now.as_second().div_euclid(60);
        spot.members
            .iter()
            .map(|name| {
                let seed = seed(&spot.id, name);
                let (tasks, quiet) = activity(seed, minute);
                Member {
                    id: format!("{}/{name}", spot.id),
                    name: (*name).to_owned(),
                    avatar: character(seed, &format!("{}/{name}", spot.id)),
                    tasks,
                    last_active: now - SignedDuration::from_mins(quiet),
                    joined_at: Some(joined(seed, now)),
                    me: false,
                }
            })
            .collect()
    }

    fn summary(&self, spot: &Spot, now: Timestamp) -> RoomSummary {
        let mut members = self.members(spot, now);
        members.extend(self.me(&spot.id));
        let working: Vec<&Member> = members.iter().filter(|member| member.tasks.running > 0).collect();
        RoomSummary {
            id: spot.id.clone(),
            name: spot.name.clone(),
            tagline: spot.tagline.clone(),
            visibility: spot.visibility,
            members: members.len(),
            working: working.len(),
            faces: working.iter().take(FACES).map(|member| Face { name: member.name.clone(), avatar: member.avatar.clone() }).collect(),
            role: spot.role,
        }
    }

    /// The room a live invite opens.
    fn invited(&self, code: &str, now: Timestamp) -> Result<Spot, RoomError> {
        let code = invite_code(code).ok_or(RoomError::InviteNotFound)?;
        let room = {
            let changes = self.changes();
            let live = changes.invites.iter().find(|(_, invite)| invite.code == code && invite.uses_left > 0 && invite.expires_at > now);
            live.map(|(room, _)| room.clone()).ok_or(RoomError::InviteNotFound)?
        };
        self.spot(&room).ok_or(RoomError::InviteNotFound)
    }

    fn owned(&self, room: &str) -> Result<Spot, RoomError> {
        self.spot(room).filter(|spot| spot.role == Some(Role::Owner)).ok_or(RoomError::NotFound)
    }

    /// The alias a public room gives the person: one nobody else in it has.
    fn alias(&self, taken: &[Member]) -> String {
        let count = ADJECTIVES.len() * ANIMALS.len();
        (0..count)
            .map(|step| {
                let pick = (self.alias_seed as usize).wrapping_add(step) % count;
                format!("{} {}", ADJECTIVES[pick % ADJECTIVES.len()], ANIMALS[pick / ADJECTIVES.len()])
            })
            .find(|alias| taken.iter().all(|member| member.name != *alias))
            .unwrap_or_else(|| "익명".to_owned())
    }
}

impl RoomService for SampleRooms {
    fn rooms(&self, now: Timestamp) -> Result<Vec<RoomSummary>, RoomError> {
        Ok(self.spots().iter().map(|spot| self.summary(spot, now)).collect())
    }

    fn sync(&self, room: &str, report: &PresenceReport, now: Timestamp) -> Result<Room, RoomError> {
        // A room the samples do not have, perhaps one joined on a server, is gone, as the server would say.
        let sample = self.spot(room).ok_or(RoomError::NotFound)?;
        let mut members = self.members(&sample, now);
        // A public room never shows a name, even one sent to it; a private one shows the person's own.
        let name = match (&report.identity, sample.visibility) {
            (Identity::Profile(name), Visibility::Private) => name.clone(),
            _ => self.alias(&members),
        };
        let mut changes = self.changes();
        let present = &mut changes.present;
        // In since the first report, until they leave.
        let joined_at = present.iter().find(|(id, _)| id == room).and_then(|(_, me)| me.joined_at).unwrap_or(now);
        let avatar = Some(report.avatar.code());
        let me = Member { id: "me".to_owned(), name, avatar, tasks: report.tasks, last_active: now, joined_at: Some(joined_at), me: true };
        present.retain(|(id, _)| id != room);
        present.push((room.to_owned(), me.clone()));
        members.push(me);
        Ok(Room { id: sample.id, name: sample.name, tagline: sample.tagline, visibility: sample.visibility, members, role: sample.role })
    }

    fn leave(&self, room: &str) {
        self.changes().present.retain(|(id, _)| id != room);
    }

    fn create_room(&self, name: &str, tagline: &str, now: Timestamp) -> Result<RoomSummary, RoomError> {
        let owned = self.spots().iter().filter(|spot| spot.role == Some(Role::Owner)).count();
        if owned >= OWNED_ROOMS {
            return Err(RoomError::TooManyOwned);
        }
        let id = {
            let mut changes = self.changes();
            changes.counter += 1;
            let id = format!("p-sample{}", changes.counter);
            changes.made.push((id.clone(), name.to_owned(), tagline.to_owned()));
            id
        };
        let spot = self.spot(&id).ok_or(RoomError::NotFound)?;
        Ok(self.summary(&spot, now))
    }

    fn invites(&self, room: &str, now: Timestamp) -> Result<Vec<Invite>, RoomError> {
        self.owned(room)?;
        let changes = self.changes();
        let live = changes.invites.iter().rev().filter(|(id, invite)| id == room && invite.uses_left > 0 && invite.expires_at > now);
        Ok(live.map(|(_, invite)| invite.clone()).collect())
    }

    fn make_invite(&self, room: &str, now: Timestamp) -> Result<Invite, RoomError> {
        self.owned(room)?;
        let mut changes = self.changes();
        changes.counter += 1;
        let code = sample_code(self.alias_seed ^ changes.counter.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        let invite = Invite { code, expires_at: now + SignedDuration::from_hours(24 * INVITE_DAYS), uses_left: INVITE_USES };
        changes.invites.push((room.to_owned(), invite.clone()));
        Ok(invite)
    }

    fn delete_invite(&self, room: &str, code: &str) -> Result<(), RoomError> {
        self.owned(room)?;
        self.changes().invites.retain(|(id, invite)| !(id == room && invite.code == code));
        Ok(())
    }

    fn check_invite(&self, code: &str, now: Timestamp) -> Result<RoomSummary, RoomError> {
        let spot = self.invited(code, now)?;
        Ok(self.summary(&spot, now))
    }

    /// The sample rooms the person owns are already theirs, so an invite to one lets them in without using it.
    fn accept_invite(&self, code: &str, now: Timestamp) -> Result<RoomSummary, RoomError> {
        let spot = self.invited(code, now)?;
        Ok(self.summary(&spot, now))
    }

    fn quit_room(&self, room: &str) -> Result<(), RoomError> {
        let spot = self.spot(room).filter(|spot| spot.role.is_some()).ok_or(RoomError::NotFound)?;
        let mut changes = self.changes();
        changes.present.retain(|(id, _)| id != room);
        changes.invites.retain(|(id, _)| id != room);
        changes.made.retain(|(id, _, _)| id != room);
        changes.quit.push(spot.id);
        Ok(())
    }

    fn remove_member(&self, room: &str, member: &str) -> Result<(), RoomError> {
        let spot = self.owned(room)?;
        if !spot.members.iter().any(|name| format!("{room}/{name}") == member) {
            return Err(RoomError::NotFound);
        }
        self.changes().removed.push(member.to_owned());
        Ok(())
    }

    fn sample(&self) -> bool {
        true
    }
}

/// An invite code drawn from a number: ten characters of Crockford base32.
fn sample_code(mut number: u64) -> String {
    const CROCKFORD: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    (0..10)
        .map(|_| {
            let digit = CROCKFORD[(number % 32) as usize] as char;
            number = (number / 32) ^ 0x5bd1_e995;
            digit
        })
        .collect()
}

/// A sample member's character, drawn from who they are so it stays theirs. About one in four, one in each
/// sample room, sent none the server would take and shows the grey face.
fn character(seed: u64, who: &str) -> Option<String> {
    (!(seed >> 16).is_multiple_of(4)).then(|| Avatar::from_seed(who).code())
}

/// When a sample member came into the room, in whole seconds. Each stays an hour to four at a time, in their own
/// rhythm, so their time in the room counts up and now and then starts over, as if they left and came back.
fn joined(seed: u64, now: Timestamp) -> Timestamp {
    let stay = 3600 + (seed % (3 * 3600)) as i64;
    let into = (now.as_second() + (seed / 11 % stay as u64) as i64).rem_euclid(stay);
    Timestamp::from_second(now.as_second() - into).unwrap_or(now)
}

/// A stable number per member, so the same person keeps the same rhythm (FNV-1a).
fn seed(room: &str, name: &str) -> u64 {
    room.bytes().chain(*b"/").chain(name.bytes()).fold(0xcbf2_9ce4_8422_2325, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3))
}

/// What a sample member's folded task header counts at this minute, and how many minutes they have been quiet.
/// Each works in rounds of their own length: requests run, their answers come back to check, then are opened
/// and kept until the member clears them every few rounds.
fn activity(seed: u64, minute: i64) -> (TaskCounts, i64) {
    let period = 20 + (seed % 37) as i64;
    let busy = period * (45 + (seed % 40) as i64) / 100;
    let shifted = minute + (seed / 7 % period as u64) as i64;
    let asked = 1 + (seed % 3) as usize;
    let kept = usize::try_from(shifted / period % (2 + (seed % 3) as i64)).unwrap_or(0) * asked;
    if seed.is_multiple_of(6) {
        // Stepped away a while ago, an answer or two left to check.
        return (TaskCounts { new: (seed % 3) as usize, running: 0, seen: kept }, 70 + (seed % 120) as i64);
    }
    let phase = shifted % period;
    if phase < busy {
        (TaskCounts { new: 0, running: asked, seen: kept }, 0)
    } else if phase < busy + 6 {
        (TaskCounts { new: asked, running: 0, seen: kept }, phase - busy)
    } else {
        (TaskCounts { new: 0, running: 0, seen: kept + asked }, phase - busy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use errand_core::rooms::Presence;

    fn at(minute: i64) -> Timestamp {
        // 2026-10-15 00:00 in Seoul, then this many minutes.
        Timestamp::from_second(1_791_990_000 + minute * 60).unwrap()
    }

    fn report(identity: Identity, new: usize, running: usize, seen: usize) -> PresenceReport {
        PresenceReport { identity, avatar: Avatar::from_seed("한결"), tasks: TaskCounts { new, running, seen } }
    }

    #[test]
    fn the_rooms_count_their_members_and_who_is_working() {
        let list = SampleRooms::new().rooms(at(23 * 60)).unwrap();
        assert_eq!(
            list.iter().map(|room| (room.name.as_str(), room.visibility)).collect::<Vec<_>>(),
            [
                ("우리 팀 야근방", Visibility::Private),
                ("고독한 야근방", Visibility::Public),
                ("새벽 코딩방", Visibility::Public),
                ("마감 직전방", Visibility::Public),
                ("조용히 집중방", Visibility::Public)
            ]
        );
        assert_eq!(list.iter().map(|room| room.members).collect::<Vec<_>>(), [4, 7, 5, 9, 4]);
        for room in &list {
            assert!(room.working <= room.members && room.faces.len() == room.working.min(FACES), "{room:?}");
        }
        assert!(list.iter().any(|room| room.working > 0), "someone is always at it somewhere");
    }

    #[test]
    fn a_public_room_shows_an_alias_and_a_private_one_the_profile() {
        let rooms = SampleRooms::new();
        let profile = report(Identity::Profile("한결".to_owned()), 1, 2, 3);
        let public = rooms.sync("late-night", &profile, at(23 * 60)).unwrap();
        let me = public.members.iter().find(|member| member.me).unwrap();
        assert_ne!(me.name, "한결", "even a name sent by mistake never shows in a public room");
        assert!(ANIMALS.iter().any(|animal| me.name.ends_with(animal)), "{}", me.name);
        assert!(public.members.iter().filter(|member| member.name == me.name).count() == 1, "an alias nobody else has");
        assert_eq!((me.tasks, me.presence(at(23 * 60))), (profile.tasks, Presence::Working));
        assert_eq!(me.avatar, Some(profile.avatar.code()), "the person's own character, in a public room too");
        assert!(public.members.iter().any(|member| member.avatar.is_none()), "someone without a character shows the grey face");
        assert!(public.members.iter().filter_map(|member| member.avatar.as_deref()).all(|code| Avatar::parse(code).is_some()));
        let private = rooms.sync("team", &profile, at(23 * 60)).unwrap();
        assert_eq!(private.members.iter().find(|member| member.me).map(|me| me.name.as_str()), Some("한결"));
        assert_eq!(rooms.sync("nowhere", &profile, at(0)), Err(RoomError::NotFound), "its tab closes");
    }

    #[test]
    fn the_list_counts_the_person_in_the_rooms_they_are_in() {
        let rooms = SampleRooms::new();
        let late_night = |rooms: &SampleRooms| rooms.rooms(at(23 * 60)).unwrap().into_iter().find(|room| room.id == "late-night").unwrap();
        let before = late_night(&rooms);
        rooms.sync("late-night", &report(Identity::Anonymous, 0, 2, 5), at(23 * 60)).unwrap();
        let inside = late_night(&rooms);
        assert_eq!((inside.members, inside.working), (before.members + 1, before.working + 1), "the room counts the person once they are in");
        rooms.leave("late-night");
        assert_eq!(late_night(&rooms), before);
    }

    #[test]
    fn everyone_came_in_a_while_ago_and_stays_in_until_they_leave() {
        let rooms = SampleRooms::new();
        let room = |minute| rooms.sync("deadline", &report(Identity::Anonymous, 0, 1, 0), at(minute)).unwrap();
        let (first, later) = (room(600), room(601));
        let others = |room: &Room| room.members.iter().filter(|member| !member.me).map(|member| member.joined_at.unwrap()).collect::<Vec<_>>();
        let (came, still) = (others(&first), others(&later));
        assert!(came.iter().all(|at_| *at_ <= at(600)) && came.iter().any(|at_| *at_ < at(600) - SignedDuration::from_mins(10)), "{came:?}");
        let held = came.iter().zip(&still).filter(|(before, after)| before == after).count();
        assert!(held >= came.len() - 1, "within each one's stay the time they came in holds, so it counts up: {came:?} {still:?}");
        let me = |room: &Room| room.members.iter().find(|member| member.me).and_then(|member| member.joined_at);
        assert_eq!((me(&first), me(&later)), (Some(at(600)), Some(at(600))), "the person came in with their first report, and stays in");
        rooms.leave("deadline");
        assert_eq!(me(&room(605)), Some(at(605)), "back in after leaving, their time starts over");
    }

    #[test]
    fn members_send_requests_check_answers_and_open_them_as_time_goes_on() {
        let rooms = SampleRooms::new();
        let room = |minute| rooms.sync("deadline", &report(Identity::Anonymous, 0, 0, 0), at(minute)).unwrap();
        let tasks = |room: &Room| room.members.iter().filter(|member| !member.me).map(|member| member.tasks).collect::<Vec<_>>();
        assert_ne!(tasks(&room(600)), tasks(&room(615)), "a quarter of an hour changes what everyone's header counts");
        let day: Vec<TaskCounts> = (0..24 * 4).flat_map(|quarter| tasks(&room(quarter * 15))).collect();
        assert!(day.iter().any(|tasks| tasks.new > 0) && day.iter().any(|tasks| tasks.running > 0) && day.iter().any(|tasks| tasks.seen > 0));
    }
}
