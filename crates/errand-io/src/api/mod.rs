//! Every call Errand makes to another service, in one place. The work rooms go over HTTPS; Codex's usage comes
//! from its own `app-server`. Claude's usage is what Claude Code last said (see [`crate::claude`]): through the
//! status line as it runs, or from its own `/usage` on each refresh, so nothing here holds a Claude
//! login. The app asks [`Api`], or the rooms client built on it, and nothing else opens a connection or starts
//! the Codex API or Claude Code.
//!
//! An HTTP call is an [`Endpoint`]: where it goes, what it sends and how its answer reads, declared beside
//! the other calls of its service. [`Api::send`] sends every one by the same rules: over HTTPS only, to the
//! places its [`Service`] allows, so this install's ID never reaches anyone else; never while the service
//! asked Errand to wait, which a 429 starts; and through the platform's `HttpClient`, which tests replace.
//! The one call not over HTTPS goes to a rooms server run on this computer, at 127.0.0.1, for development
//! (see [`rooms::RoomsServer`]).

pub mod claude;
pub mod codex;
pub mod rooms;
pub mod updates;

use crate::codex::CodexSource;
use crate::stores;
use errand_core::Provider;
use rooms::RoomsServer;
use errand_core::ports::{HTTP_BODY_LIMIT, HttpClient, HttpRequest, HttpResponse};
use errand_core::update::Release;
use errand_core::usage::{self, Reading, UsageError};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

/// A rate limit without a Retry-After holds calls back this long.
const RATE_LIMIT_PAUSE: Duration = Duration::from_secs(60);
/// The longest a rate limit holds calls back, whatever it asks.
const LONGEST_RATE_LIMIT_PAUSE: Duration = Duration::from_secs(15 * 60);
/// A record Claude Code's status line made this recently is fresh without asking: a terminal session reports on
/// every turn.
const RECENT_REPORT: jiff::SignedDuration = jiff::SignedDuration::from_mins(5);

/// A service Errand calls over HTTPS.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Service {
    /// The work rooms' server, wherever the [`Api`] was told it is.
    Rooms,
    /// Errand's own releases, on [`updates::HOST`] only.
    Updates,
}

/// Where an address goes: its scheme, host and port.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Origin {
    secure: bool,
    host: String,
    port: u16,
}

impl Origin {
    /// HTTPS to this host, on HTTPS's own port.
    pub fn https(host: &str) -> Origin {
        Origin { secure: true, host: host.trim_end_matches('.').to_ascii_lowercase(), port: 443 }
    }

    /// Where an `https://` or `http://` address goes, or None for anything else. An address with user info is
    /// refused too, since `https://errand-backend.vercel.app@elsewhere.example` really goes elsewhere.
    pub fn of(url: &str) -> Option<Origin> {
        let (secure, rest) = match url.strip_prefix("https://") {
            Some(rest) => (true, rest),
            None => (false, url.strip_prefix("http://")?),
        };
        let authority = rest.split(['/', '?', '#']).next()?;
        if authority.contains('@') {
            return None;
        }
        let (host, port) = match authority.split_once(':') {
            Some((host, port)) => (host, port.parse().ok()?),
            None => (authority, if secure { 443 } else { 80 }),
        };
        let host = host.trim_end_matches('.');
        if host.is_empty() || !host.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-')) {
            return None;
        }
        Some(Origin { secure, host: host.to_ascii_lowercase(), port })
    }

    /// An address here; `path` starts with a slash.
    pub fn url(&self, path: &str) -> String {
        let scheme = if self.secure { "https" } else { "http" };
        if self.port == if self.secure { 443 } else { 80 } {
            format!("{scheme}://{}{path}", self.host)
        } else {
            format!("{scheme}://{}:{}{path}", self.host, self.port)
        }
    }
}

/// Why a call got no answer to read, whatever the service.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// No answer came: no network, a timeout, or a redirect that was not followed.
    Offline,
    /// The service asked Errand to wait, and the wait is not over.
    Waiting(Service),
    /// Addressed somewhere its service does not allow, or not over HTTPS: it was never sent.
    Elsewhere,
}

/// One HTTP call to a service: where it goes and what it sends, and what its answer means.
pub trait Endpoint {
    type Answer;
    /// The service's own errors, which every shared refusal becomes.
    type Error: From<Refusal>;
    const SERVICE: Service;
    fn request(&self) -> HttpRequest;
    /// Reads the reply. A 429 never gets here: the wait it asks for is kept by [`Api::send`].
    fn answer(&self, response: HttpResponse) -> Result<Self::Answer, Self::Error>;
}

/// The one door to other services.
pub struct Api {
    http: Arc<dyn HttpClient>,
    /// Per service, the moment before which nothing is sent: polling through a rate limit only prolongs it.
    paused_until: Mutex<HashMap<Service, Instant>>,
    /// Errand's record of what Claude Code last said.
    claude_report: PathBuf,
    /// Finds the Claude Code to ask `/usage`, looked for on each refresh as the desktop app updates its own.
    claude_code: Box<dyn Fn() -> Option<PathBuf> + Send + Sync>,
    codex: CodexSource,
    /// The only place the rooms' calls go.
    rooms: RoomsServer,
}

impl Api {
    pub fn new(http: Arc<dyn HttpClient>, claude_report: PathBuf, codex: CodexSource) -> Api {
        Api { http, paused_until: Mutex::new(HashMap::new()), claude_report, claude_code: Box::new(|| None), codex, rooms: RoomsServer::official() }
    }

    /// How to find the Claude Code [`Api::ask_claude_code`] runs.
    pub fn with_claude_code(mut self, find: impl Fn() -> Option<PathBuf> + Send + Sync + 'static) -> Api {
        self.claude_code = Box::new(find);
        self
    }

    /// Sends the rooms' calls to this server instead of errand-backend, and to no other.
    pub fn with_rooms_server(mut self, server: RoomsServer) -> Api {
        self.rooms = server;
        self
    }

    pub fn rooms_server(&self) -> &RoomsServer {
        &self.rooms
    }

    /// A service's usage as the limits Errand shows. Reading Codex's never sends a model request; reading Claude's
    /// sends nothing at all.
    pub fn usage(&self, provider: Provider) -> Result<Reading, UsageError> {
        match provider {
            Provider::Codex => Ok(usage::codex::windows_from_response(&codex::rate_limits(&self.codex)?).into()),
            Provider::Claude => {
                let report = stores::read_claude_report(&self.claude_report).ok_or(UsageError::ClaudeNotHeard)?;
                Ok(Reading { windows: report.windows_at(crate::now()), reported_at: report.reported_at })
            }
        }
    }

    /// Claude's usage after asking Claude Code itself, for automatic and manual refreshes.
    /// When Claude Code gives no answer, the read failed and says so, unless a status line spoke just now: then
    /// the record is as fresh as an answer, and a Claude Code too old for `/usage` does not get in the way. A
    /// failure keeps the limits already on screen, so silence never passes for an update that did not happen.
    pub fn ask_claude_code(&self) -> Result<Reading, UsageError> {
        let answered = (self.claude_code)()
            .and_then(|executable| claude::usage_text(&executable, claude::ANSWER_TIMEOUT))
            .is_some_and(|text| crate::claude::keep_usage_text(&self.claude_report, &text, crate::now()).unwrap_or(false));
        let now = crate::now();
        match self.usage(Provider::Claude) {
            Ok(reading) if answered => Ok(reading),
            Ok(reading) if !reading.windows.is_empty() && reading.reported_at.is_some_and(|at| now.duration_since(at) < RECENT_REPORT) => Ok(reading),
            _ => Err(UsageError::ClaudeAskFailed),
        }
    }

    /// The latest release of Errand, or None when it cannot be told.
    pub fn latest_release(&self) -> Option<Release> {
        self.send(&updates::LatestRelease).ok().flatten()
    }

    /// Sends one call by the rules every service shares.
    pub fn send<E: Endpoint>(&self, endpoint: &E) -> Result<E::Answer, E::Error> {
        self.check(E::SERVICE)?;
        let request = endpoint.request();
        if !Origin::of(&request.url).is_some_and(|origin| self.allows(E::SERVICE, &origin)) {
            return Err(Refusal::Elsewhere.into());
        }
        let response = self.http.send(&request).map_err(|_| Refusal::Offline)?;
        if response.body.len() > HTTP_BODY_LIMIT { return Err(Refusal::Offline.into()); }
        if response.status == 429 {
            let wait = response.retry_after.as_deref().and_then(|value| retry_after(value, SystemTime::now())).unwrap_or(RATE_LIMIT_PAUSE);
            if let Ok(mut paused) = self.paused_until.lock() {
                paused.insert(E::SERVICE, Instant::now() + wait);
            }
            return Err(Refusal::Waiting(E::SERVICE).into());
        }
        endpoint.answer(response)
    }

    /// Where a service's calls may go: the one rooms server this Api was given, and GitHub for releases.
    fn allows(&self, service: Service, origin: &Origin) -> bool {
        match service {
            Service::Rooms => origin == self.rooms.origin(),
            Service::Updates => *origin == Origin::https(updates::HOST),
        }
    }

    fn check(&self, service: Service) -> Result<(), Refusal> {
        let waiting = self.paused_until.lock().is_ok_and(|paused| paused.get(&service).is_some_and(|until| Instant::now() < *until));
        if waiting { Err(Refusal::Waiting(service)) } else { Ok(()) }
    }
}

/// How long a Retry-After header asks to wait: whole seconds or an HTTP date, never more than fifteen minutes.
/// A date already past asks for no wait; anything else unreadable is None.
pub fn retry_after(value: &str, now: SystemTime) -> Option<Duration> {
    let value = value.trim();
    let wait = match value.parse::<u64>() {
        Ok(seconds) => Duration::from_secs(seconds),
        Err(_) => {
            let at = jiff::fmt::rfc2822::parse(value).ok()?.timestamp();
            let now = jiff::Timestamp::try_from(now).ok()?;
            Duration::try_from(at.duration_since(now)).unwrap_or(Duration::ZERO)
        }
    };
    Some(wait.min(LONGEST_RATE_LIMIT_PAUSE))
}
