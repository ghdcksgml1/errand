//! Errand's colours, shared so the widget looks the same on every platform.

use crate::Provider;
use crate::sessions::SessionState;
use crate::usage::LOW_REMAINING;

/// sRGB, each component 0-1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
}

const fn rgb(red: f64, green: f64, blue: f64) -> Rgb {
    Rgb { red, green, blue }
}

pub const INK: Rgb = rgb(0.12, 0.16, 0.18);
pub const MUTED: Rgb = rgb(0.43, 0.47, 0.49);
/// Remaining usage: GPT blue.
pub const HEALTHY: Rgb = rgb(0.22, 0.55, 0.95);
/// Red is used only here, at 25% or less.
pub const LOW: Rgb = rgb(0.84, 0.27, 0.24);
/// Stale or unknown usage. macOS draws its own system grey; this is that grey in the light appearance.
pub const UNKNOWN: Rgb = rgb(142.0 / 255.0, 142.0 / 255.0, 147.0 / 255.0);

pub const NEW_RESULT: Rgb = rgb(0.04, 0.40, 0.85);
/// Running work: the deep teal for words, the brighter one for outlines, chips and spinners.
pub const RUNNING: Rgb = rgb(0.00, 0.45, 0.40);
pub const RUNNING_BRIGHT: Rgb = rgb(0.00, 0.64, 0.56);
pub const STALLED: Rgb = rgb(0.80, 0.24, 0.22);
/// Work waiting on the person's answer.
pub const ASKING: Rgb = rgb(0.80, 0.42, 0.02);
/// A checked card waiting to clear.
pub const DONE: Rgb = rgb(0.15, 0.57, 0.35);
/// A failed read beside the previous values: macOS's system orange in the light appearance.
pub const WARNING: Rgb = rgb(1.0, 149.0 / 255.0, 0.0);
pub const WHITE: Rgb = rgb(1.0, 1.0, 1.0);

impl Rgb {
    pub const fn grey(white: f64) -> Rgb {
        rgb(white, white, white)
    }

    /// This colour moved `fraction` of the way to `other`, as AppKit's `blendedColorWithFraction:ofColor:`.
    pub fn blend(self, fraction: f64, other: Rgb) -> Rgb {
        let mix = |from: f64, to: f64| from + (to - from) * fraction;
        rgb(mix(self.red, other.red), mix(self.green, other.green), mix(self.blue, other.blue))
    }
}

/// A colour with opacity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Paint {
    pub color: Rgb,
    pub alpha: f64,
}

impl Rgb {
    pub const fn alpha(self, alpha: f64) -> Paint {
        Paint { color: self, alpha }
    }
}

/// How a task card is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CardStyle {
    pub fill: Rgb,
    pub edge: Paint,
    pub edge_width: f64,
    pub chip_fill: Paint,
    /// The chip's words and icon.
    pub signal: Rgb,
}

/// New results, questions waiting on the person and stalled work are tinted and outlined strongly; running work
/// is outlined but not filled, so it stands out without outshining them; a checked card turns green while it waits.
pub fn card_style(state: SessionState, hovering: bool) -> CardStyle {
    let signal = session_state(state);
    let strong = matches!(state, SessionState::New | SessionState::Asking | SessionState::Stalled);
    let tinted = strong || state == SessionState::Done;
    let running = state == SessionState::Running;
    // New results and questions carry a slightly deeper wash than other tinted cards, so they read first.
    let wash = if matches!(state, SessionState::New | SessionState::Asking) { 0.93 } else { 0.965 };
    let mut fill = if tinted { signal.blend(wash, WHITE) } else { WHITE };
    if hovering && state != SessionState::Done {
        fill = fill.blend(0.035, INK);
    }
    let edge = if running {
        RUNNING_BRIGHT.alpha(0.7)
    } else if tinted {
        signal.alpha(if strong { 0.55 } else { 0.38 })
    } else {
        Rgb::grey(0.86).alpha(1.0)
    };
    CardStyle {
        fill,
        edge,
        edge_width: if strong { 1.5 } else if running { 1.25 } else { 1.0 },
        chip_fill: if running { RUNNING_BRIGHT.alpha(0.16) } else { signal.alpha(0.12) },
        signal,
    }
}

pub fn accent(provider: Provider) -> Rgb {
    match provider {
        Provider::Codex => rgb(0.22, 0.55, 0.95),
        Provider::Claude => rgb(0.80, 0.42, 0.32),
    }
}

/// Blue while there is room, red at 25% or less, grey when stale or unknown.
pub fn usage(remaining: Option<i64>, stale: bool) -> Rgb {
    match remaining {
        Some(left) if !stale => {
            if left <= LOW_REMAINING {
                LOW
            } else {
                HEALTHY
            }
        }
        _ => UNKNOWN,
    }
}

/// Seen work is calm grey; every other state carries its own colour so a glance tells them apart.
pub fn session_state(state: SessionState) -> Rgb {
    match state {
        SessionState::New => NEW_RESULT,
        SessionState::Running => RUNNING,
        SessionState::Asking => ASKING,
        SessionState::Stalled => STALLED,
        SessionState::Done => DONE,
        SessionState::Seen | SessionState::Aborted | SessionState::Unknown => MUTED,
    }
}
