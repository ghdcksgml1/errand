//! Runs the app: every message goes through `update`, effects are carried out here, slow reads run on worker
//! threads and come back as messages, and the shell redraws from the new state. The same on every OS.

use crate::platform::Platform;
use errand_core::model::{AccountInfo, Alert, AppState, Effect, Msg, Profile, WidgetPreferences, update};
use errand_core::ports::{Poster, Shell};
use errand_core::rooms::{self, Visibility};
use errand_core::sessions::{SessionRecords, SessionRow};
use errand_core::{PerProvider, Provider};
use errand_io::session_scan::SessionScanner;
use errand_io::{claude, codex, stores};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};
use std::thread;

pub struct Runtime {
    title_cancel: Arc<std::sync::atomic::AtomicBool>,
    state: AppState,
    platform: Arc<Platform>,
    shell: Box<dyn Shell>,
    poster: Arc<dyn Poster>,
    /// The scan thread, which keeps the scanner's caches.
    scans: Sender<SessionRecords>,
    /// The rooms the person was in last time, until the first scan has counted their tasks.
    remembered: Option<Vec<(String, Visibility)>>,
}

impl Runtime {
    /// Loads Errand's records; the shell supplies the widget preferences it keeps.
    pub fn new(platform: Platform, shell: Box<dyn Shell>, poster: Arc<dyn Poster>, widget: WidgetPreferences) -> Runtime {
        let now = errand_io::now();
        let history = stores::load_reset_history(&platform.reset_history);
        let records = stores::read_session_records(&platform.session_tray, now).unwrap_or_else(|| {
            // The first launch starts tracking now and keeps that start, so old work does not flood back later.
            let records = SessionRecords::new(now);
            let _ = stores::save_session_records(&platform.session_tray, &records);
            records
        });
        let mut state = AppState::new(history, records, widget, platform.full_user_name());
        state.rooms.sample = platform.rooms.sample();
        match errand_io::tasks::load(&platform.session_tray.with_file_name("tasks")) {
            Ok((records, provider)) => {
                errand_io::tasks::discard_orphans(&platform.session_tray.with_file_name("tasks"), &records);
                state.tasks.records = records;
                state.tasks.provider = provider;
            }
            Err(error) => {
                state.tasks.error = Some(error);
                state.tasks.readable = false;
            }
        }
        let (scans, requests) = mpsc::channel::<SessionRecords>();
        let mut scanner = SessionScanner::new(platform.sessions.clone());
        let scan_poster = poster.clone();
        thread::Builder::new()
            .name("errand-sessions".to_owned())
            .spawn(move || {
                while let Ok(records) = requests.recv() {
                    let rows: Vec<SessionRow> = scanner.scan(&records, errand_io::now());
                    scan_poster.post(Msg::SessionsScanned(rows));
                }
            })
            .expect("the session scan thread starts");
        let remembered = Some(stores::load_joined_rooms(&platform.joined_rooms));
        Runtime {
            title_cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            state, platform: Arc::new(platform), shell, poster, scans, remembered }
    }

    fn task_effect(&mut self, effect: errand_core::tasks::Effect) -> Option<Msg> {
        use errand_core::tasks::{Effect as TaskEffect, Msg as TaskMsg};
        use std::sync::atomic::{AtomicBool, Ordering};
        let root = self.platform.session_tray.with_file_name("tasks");
        match effect {
            TaskEffect::Import {
                token,
                inputs,
                remaining,
            } => {
                let poster = self.poster.clone();
                thread::spawn(move || {
                    poster.post(Msg::Tasks(TaskMsg::Imported {
                        token,
                        result: errand_io::tasks::import(&root, &inputs, remaining),
                    }))
                });
            }
            TaskEffect::Generate {
                token,
                revision,
                provider,
                attachments,
            } => {
                self.title_cancel.store(true, Ordering::Relaxed);
                self.title_cancel = Arc::new(AtomicBool::new(false));
                let (cancel, poster) = (self.title_cancel.clone(), self.poster.clone());
                let executable = self.platform.title_cli(provider);
                let language = crate::platform::system_language();
                thread::spawn(move || {
                    let result = errand_io::task_title::generate(
                        &root,
                        executable.as_deref(),
                        provider,
                        &attachments,
                        language,
                        &cancel,
                    );
                    poster.post(Msg::Tasks(TaskMsg::Generated {
                        token,
                        revision,
                        result,
                    }));
                });
            }
            TaskEffect::CancelTitle => self.title_cancel.store(true, Ordering::Relaxed),
            TaskEffect::Cleanup(files) => {
                thread::spawn(move || errand_io::tasks::cleanup(&root, &files));
            }
            TaskEffect::Save { records, draft } => {
                let result = if self.state.tasks.readable {
                    errand_io::tasks::save(&root, &records, self.state.tasks.provider)
                } else {
                    Err(errand_io::tasks::STORAGE_ERROR.into())
                };
                return Some(Msg::Tasks(TaskMsg::Saved {
                    records,
                    draft,
                    result,
                }));
            }
            TaskEffect::Open(file) => {
                if let Some(path) = errand_io::tasks::attachment_path(&root, &file) {
                    self.shell.open_task_attachment(&path);
                }
            }
            TaskEffect::PickFiles => self.shell.choose_task_files(),
        }
        None
    }

    pub fn alert(&mut self, alert: &Alert) {
        self.shell.alert(alert);
    }

    /// What the app does on launch: check accounts, read usage and sessions, and draw. The first launch also
    /// makes this install's ID, ready for the room server.
    pub fn start(&mut self) {
        self.platform.make_install_id();
        for msg in [Msg::CheckAccounts, Msg::RefreshUsage, Msg::ScanSessions, Msg::CheckForUpdate] {
            self.dispatch(msg);
        }
    }

    pub fn dispatch(&mut self, msg: Msg) {
        // The rooms the person was in come back once the first scan has counted their tasks, so the first
        // report each room hears is right.
        let first_scan = matches!(msg, Msg::SessionsScanned(_)).then(|| self.remembered.take()).flatten();
        let mut queue = VecDeque::from([msg]);
        queue.extend(first_scan.map(Msg::RoomsRemembered));
        let mut now = errand_io::now();
        while let Some(msg) = queue.pop_front() {
            now = errand_io::now();
            for effect in update(&mut self.state, msg, now) {
                queue.extend(self.perform(effect));
            }
        }
        self.shell.render(&self.state, now);
    }

    /// Carries out an effect; quick reads answer at once, slow ones post their answer later.
    fn perform(&mut self, effect: Effect) -> Option<Msg> {
        match effect {
            Effect::Tasks(effect) => self.task_effect(effect),
            Effect::FetchUsage(provider) => {
                let (platform, poster) = (self.platform.clone(), self.poster.clone());
                thread::spawn(move || poster.post(Msg::UsageRead { provider, result: platform.api.usage(provider) }));
                None
            }
            Effect::AskClaudeCode => {
                let (platform, poster) = (self.platform.clone(), self.poster.clone());
                thread::spawn(move || poster.post(Msg::UsageRead { provider: Provider::Claude, result: platform.api.ask_claude_code() }));
                None
            }
            Effect::FetchRelease => {
                let (platform, poster) = (self.platform.clone(), self.poster.clone());
                thread::spawn(move || poster.post(Msg::ReleaseRead(platform.api.latest_release())));
                None
            }
            Effect::InstallUpdate(release) => {
                let (platform, poster) = (self.platform.clone(), self.poster.clone());
                // On success the new copy closes this one, so only a failure has anything to say.
                thread::spawn(move || {
                    if !platform.install_update(&release) {
                        poster.post(Msg::UpdateFailed);
                    }
                });
                None
            }
            Effect::ReadProfile => Some(Msg::ProfileRead(self.platform.profile())),
            Effect::NewAvatarSeed => Some(Msg::ProfileRead(Profile { avatar: self.platform.new_avatar(), ..self.state.profile.clone() })),
            Effect::SaveAvatar(avatar) => {
                self.platform.keep_avatar(&avatar);
                None
            }
            Effect::SaveNickname(name) => {
                self.platform.keep_nickname(name.as_deref());
                None
            }
            Effect::ReadAccounts => Some(Msg::AccountsRead(self.read_accounts())),
            Effect::ScanSessions(records) => {
                let _ = self.scans.send(records);
                None
            }
            Effect::ReadAttention => Some(Msg::AttentionRead(self.platform.attention())),
            Effect::SaveResetHistory => {
                let _ = stores::save_reset_history(&self.platform.reset_history, &self.state.history);
                None
            }
            Effect::SaveSessionRecords => {
                let _ = stores::save_session_records(&self.platform.session_tray, &self.state.sessions.records);
                None
            }
            Effect::SaveWidgetPreferences => {
                self.shell.save_widget_preferences(self.state.widget);
                None
            }
            Effect::OpenUrl(url) => {
                self.shell.open_url(&url);
                None
            }
            Effect::OpenApp(provider) => {
                self.shell.open_app(provider);
                None
            }
            Effect::ScheduleCompletion { key, after } => {
                self.shell.schedule(after, Msg::CompletionDue(key));
                None
            }
            Effect::FadeCard(key) => {
                self.shell.fade_card(&key);
                None
            }
            Effect::ShowAlert(alert) => {
                self.shell.alert(&alert);
                None
            }
            Effect::ShowDashboard => {
                self.shell.show_dashboard();
                None
            }
            Effect::FetchRooms => {
                let (rooms, poster) = (self.platform.rooms.clone(), self.poster.clone());
                thread::spawn(move || poster.post(Msg::RoomsRead(rooms.rooms(errand_io::now()))));
                None
            }
            Effect::SyncRoom { room, visibility } => {
                let now = errand_io::now();
                // The folded task header's three counts, and a nickname only for a private room: all that
                // leaves the computer.
                let report = rooms::presence_report(&self.state, visibility);
                let (rooms, poster) = (self.platform.rooms.clone(), self.poster.clone());
                thread::spawn(move || {
                    let read = rooms.sync(&room, &report, now);
                    poster.post(Msg::RoomRead(room, read));
                });
                None
            }
            Effect::LeaveRoom(room) => {
                let rooms = self.platform.rooms.clone();
                thread::spawn(move || rooms.leave(&room));
                None
            }
            Effect::TickRoomClock => {
                self.shell.schedule(rooms::CLOCK_TICK, Msg::RoomClock);
                None
            }
            Effect::SaveRooms => {
                let _ = stores::save_joined_rooms(&self.platform.joined_rooms, &self.state.rooms.remembered());
                None
            }
            Effect::CreateRoom { name, tagline } => {
                self.in_background(move |rooms| Msg::RoomCreated(rooms.create_room(&name, &tagline, errand_io::now())));
                None
            }
            Effect::ReadInvites(room) => {
                self.in_background(move |rooms| Msg::InvitesRead(room.clone(), rooms.invites(&room, errand_io::now())));
                None
            }
            Effect::MakeInvite { room, replacing } => {
                let (rooms, poster) = (self.platform.rooms.clone(), self.poster.clone());
                thread::spawn(move || {
                    // The link it replaces stops working first; if that fails, it still works, and no second
                    // link is made. Gone already is as good as deleted.
                    if let Some(code) = replacing {
                        match gone(rooms.delete_invite(&room, &code)) {
                            Ok(()) => poster.post(Msg::InviteReplaced(room.clone())),
                            Err(failure) => return poster.post(Msg::InviteMade(room, Err(failure))),
                        }
                    }
                    poster.post(Msg::InviteMade(room.clone(), rooms.make_invite(&room, errand_io::now())));
                });
                None
            }
            Effect::DeleteInvite { room, code } => {
                self.in_background(move |rooms| Msg::InviteDeleted(room.clone(), gone(rooms.delete_invite(&room, &code))));
                None
            }
            Effect::CheckInvite(code) => {
                self.in_background(move |rooms| Msg::InviteChecked(code.clone(), rooms.check_invite(&code, errand_io::now())));
                None
            }
            Effect::AcceptInvite(code) => {
                self.in_background(move |rooms| Msg::InviteAccepted(code.clone(), rooms.accept_invite(&code, errand_io::now())));
                None
            }
            Effect::QuitRoom(room) => {
                self.in_background(move |rooms| Msg::RoomQuit(room.clone(), rooms.quit_room(&room)));
                None
            }
            Effect::ReadMembers(room) => {
                self.in_background(move |rooms| Msg::MembersRead(room.clone(), rooms.members(&room, errand_io::now())));
                None
            }
            Effect::RemoveMember { room, member } => {
                self.in_background(move |rooms| Msg::MemberRemoved(room.clone(), rooms.remove_member(&room, &member)));
                None
            }
            Effect::Copy(text) => {
                self.shell.copy_text(&text);
                None
            }
            Effect::Confirm(confirm) => {
                self.shell.confirm(&confirm);
                None
            }
        }
    }

    /// Calls the rooms service off the main thread and posts what it says.
    fn in_background(&self, call: impl FnOnce(&dyn errand_core::ports::RoomService) -> Msg + Send + 'static) {
        let (rooms, poster) = (self.platform.rooms.clone(), self.poster.clone());
        thread::spawn(move || poster.post(call(rooms.as_ref())));
    }

    fn read_accounts(&self) -> PerProvider<AccountInfo> {
        PerProvider::from_fn(|provider| AccountInfo {
            email: match provider {
                Provider::Codex => codex::account(&self.platform.codex.home).email,
                Provider::Claude => claude::account(&self.platform.home).email,
            },
            running: self.platform.running(provider).is_some(),
        })
    }
}

/// An invite deleted, or one already gone, which is the same to the person.
fn gone(result: Result<(), errand_core::rooms::RoomError>) -> Result<(), errand_core::rooms::RoomError> {
    match result {
        Err(errand_core::rooms::RoomError::InviteNotFound) => Ok(()),
        other => other,
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.title_cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}
