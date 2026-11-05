//! What the shared crates need from the operating system. Each platform crate implements these once;
//! tests use fakes.

use crate::Provider;
use crate::model::{Alert, AppState, Msg, WidgetPreferences};
use crate::rooms::{PresenceReport, Room, RoomError, RoomSummary};
use jiff::{SignedDuration, Timestamp};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub timeout: Duration,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
    /// The Retry-After header as sent, which says how long a rate limit asks to wait.
    pub retry_after: Option<String>,
}

/// The request got no answer: no network, a timeout, or a redirect it would not follow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HttpFailure;

/// HTTPS through the system's own networking, so proxies and trusted certificates work as in every other
/// app. Redirects are never followed: a bearer token must not travel to another host.
pub trait HttpClient: Send + Sync {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, HttpFailure>;
}

/// Hands a message to the UI thread from any thread.
pub trait Poster: Send + Sync {
    fn post(&self, msg: Msg);
}

/// The platform's windows as the runtime drives them. Called on the UI thread only.
pub trait Shell {
    /// Shows the state. Views compare with what they show and change only what differs.
    fn render(&mut self, state: &AppState, now: Timestamp);
    fn save_widget_preferences(&mut self, preferences: WidgetPreferences);
    fn open_url(&mut self, url: &str);
    /// Brings the app forward, opening it if needed, then posts `Msg::AppOpened`.
    fn open_app(&mut self, provider: Provider);
    /// Posts `msg` after `after`.
    fn schedule(&mut self, after: SignedDuration, msg: Msg);
    /// Fades a task card out, then posts `Msg::CardFaded`.
    fn fade_card(&mut self, key: &str);
    fn alert(&mut self, alert: &Alert);
    fn show_dashboard(&mut self);
}

/// Work rooms, on the rooms server or from sample data. The report is all that leaves the computer.
pub trait RoomService: Send + Sync {
    fn rooms(&self, now: Timestamp) -> Result<Vec<RoomSummary>, RoomError>;
    /// Enters the room or tells it the latest counts, and answers with everyone in it.
    fn sync(&self, room: &str, report: &PresenceReport, now: Timestamp) -> Result<Room, RoomError>;
    fn leave(&self, room: &str);
    /// The rooms and their people are made up, standing in for the server; the list says so.
    fn sample(&self) -> bool {
        false
    }
}
