//! What this computer offers Errand, gathered in one place so the rest of the app is the same on every OS.

use errand_core::model::{Attention, Profile};
use errand_core::ports::RoomService;
use errand_core::update::Release;
use errand_core::{Language, Provider};
use errand_io::api::Api;
use errand_io::api::rooms::RoomsSource;
use errand_io::claude;
use errand_io::codex::{self, CodexSource};
use errand_io::session_scan::SessionSources;
use std::path::PathBuf;
use std::sync::Arc;

pub struct Platform {
    pub home: PathBuf,
    pub codex: CodexSource,
    /// Every read of Claude's and Codex's usage and every call to the rooms server goes through here.
    pub api: Arc<Api>,
    pub sessions: SessionSources,
    pub reset_history: PathBuf,
    pub session_tray: PathBuf,
    pub joined_rooms: PathBuf,
    /// The rooms on errand-backend, or where `ERRAND_ROOMS_API` says.
    pub rooms: Arc<dyn RoomService>,
    /// The language Errand speaks, which the desktop window's summaries are written in.
    pub language: Language,
}

/// The language the computer's person reads, as far as Errand speaks it.
#[cfg(target_os = "macos")]
pub fn system_language() -> Language {
    Language::preferred(errand_macos::locale::preferred_languages())
}

#[cfg(not(target_os = "macos"))]
pub fn system_language() -> Language {
    Language::preferred(["LC_ALL", "LC_MESSAGES", "LANG"].iter().filter_map(|name| std::env::var(name).ok()))
}

/// Where the rooms come from: errand-backend, unless `ERRAND_ROOMS_API` names another server, such as one run on
/// this computer at `http://127.0.0.1:<port>`, or says `sample` for the sample rooms, to work offline. A setting
/// that names no server Errand would send this install's ID to falls back to the sample rooms, not to
/// errand-backend, since whoever set it meant some other server.
pub fn rooms_source() -> RoomsSource {
    let setting = std::env::var("ERRAND_ROOMS_API").ok();
    RoomsSource::from_setting(setting.as_deref()).unwrap_or_else(|| {
        eprintln!("errand: ERRAND_ROOMS_API is not https://<host>, http://127.0.0.1:<port> or sample, so the rooms are sample data");
        RoomsSource::Sample
    })
}

/// Where Errand keeps what Claude Code tells the status line.
#[cfg(target_os = "macos")]
pub fn claude_report() -> PathBuf {
    errand_macos::paths::Paths::current().claude_report()
}

impl Platform {
    /// The nickname the person gave themselves in Errand, else the one given to Claude, else the Codex account's
    /// name, else the computer user's, and the character this computer drew for the person.
    pub fn profile(&self) -> Profile {
        let name = [self.nickname(), claude::account(&self.home).nickname, codex::account(&self.codex.home).name, Some(self.full_user_name())]
            .into_iter()
            .flatten()
            .map(|name| name.trim().to_owned())
            .find(|name| !name.is_empty())
            .unwrap_or_else(|| self.login_name());
        Profile { name, avatar: self.avatar() }
    }
}

#[cfg(target_os = "macos")]
impl Platform {
    /// `language` is the one Errand speaks, which the rooms server names people in.
    pub fn current(language: Language) -> Platform {
        use errand_io::api::rooms::HttpRooms;
        use errand_macos::{apps, http::UrlSession, identity, paths::Paths};
        let paths = Paths::current();
        let executable = apps::codex_app(&paths.home).and_then(|app| apps::codex_executable(&app));
        let codex = CodexSource { executable, home: paths.codex_home(), directory: paths.home.clone() };
        let home = paths.home.clone();
        let api = Api::new(Arc::new(UrlSession), paths.claude_report(), codex.clone()).with_claude_code(move || apps::claude_code(&home));
        let (api, rooms): (Arc<Api>, Arc<dyn RoomService>) = match rooms_source() {
            RoomsSource::Server(server) => {
                let api = Arc::new(api.with_rooms_server(server));
                // The install's ID is read when the rooms first call the server, and goes nowhere else.
                (api.clone(), Arc::new(HttpRooms::new(api, identity::install_id, language)))
            }
            RoomsSource::Sample => (Arc::new(api), Arc::new(errand_io::rooms::SampleRooms::new())),
        };
        Platform {
            api,
            codex,
            sessions: SessionSources { codex_home: paths.codex_home(), claude_sessions: paths.claude_sessions(), claude_projects: paths.claude_projects() },
            reset_history: paths.reset_history(),
            session_tray: paths.session_tray(),
            joined_rooms: paths.joined_rooms(),
            rooms,
            home: paths.home,
            language,
        }
    }

    pub fn title_cli(&self, provider: Provider) -> Option<PathBuf> {
        let bundled = match provider {
            Provider::Codex => self.codex.executable.clone(),
            Provider::Claude => errand_macos::apps::claude_code(&self.home),
        };
        bundled.or_else(|| errand_macos::apps::local_cli(&self.home, provider))
    }

    /// Puts this release in place of the app and starts it; false when that did not happen.
    pub fn install_update(&self, release: &Release) -> bool {
        errand_macos::updater::install(release)
    }

    /// The process ID of the service's app while it runs.
    pub fn running(&self, provider: Provider) -> Option<i32> {
        errand_macos::apps::running(provider)
    }

    /// Where the person is right now. Read on the main thread, where AppKit keeps the app in front up to date.
    pub fn attention(&self) -> Attention {
        Attention { front: errand_macos::apps::in_front(), idle: errand_macos::apps::idle() }
    }

    pub fn full_user_name(&self) -> String {
        errand_macos::profile::full_user_name()
    }

    pub fn login_name(&self) -> String {
        errand_macos::profile::login_name()
    }

    /// The person's character as kept: a seed drawn at random the first time, or the parts they picked since.
    pub fn avatar(&self) -> String {
        errand_macos::profile::avatar()
    }

    /// A new random seed for the person's character, kept in place of the last.
    pub fn new_avatar(&self) -> String {
        errand_macos::profile::new_avatar()
    }

    pub fn keep_avatar(&self, avatar: &str) {
        errand_macos::profile::keep_avatar(avatar)
    }

    /// The nickname the person typed in the editor, if they kept one.
    pub fn nickname(&self) -> Option<String> {
        errand_macos::profile::nickname()
    }

    pub fn keep_nickname(&self, name: Option<&str>) {
        errand_macos::profile::keep_nickname(name)
    }

    /// Makes this install's random ID the first time Errand runs. Only the rooms server ever hears it, so it is
    /// not handed out here.
    pub fn make_install_id(&self) {
        errand_macos::identity::install_id();
    }

    pub fn app_installed(&self, provider: Provider) -> bool {
        match provider {
            Provider::Codex => errand_macos::apps::codex_app(&self.home).is_some(),
            Provider::Claude => errand_macos::apps::installed(Provider::Claude).is_some(),
        }
    }
}
