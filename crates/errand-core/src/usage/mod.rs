//! Usage limits as both services report them, in one shape: how much is left and when it refills.

pub mod claude;
pub mod codex;

use crate::format::{month_day_time, timestamp_from_seconds};
use crate::i18n::Language;
use crate::Provider;
use jiff::Timestamp;
use jiff::tz::TimeZone;

/// At or below this much remaining, a battery turns red.
pub const LOW_REMAINING: i64 = 25;

/// How long a limit runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Span {
    Minutes(i64),
    /// The limit's place in its bucket, when the service does not say how long it runs.
    Numbered(usize),
}

/// What a limit is called, put into words by `Language::window_title`. Codex names the bucket when it reports
/// several ("Codex 주간"); Claude names the model a weekly limit covers ("주간 Fable").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowName {
    pub span: Span,
    pub bucket: Option<String>,
    pub model: Option<String>,
}

impl WindowName {
    pub fn span(span: Span) -> WindowName {
        WindowName { span, bucket: None, model: None }
    }
}

/// Why a usage read failed; `Language::usage_error` says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UsageError {
    /// Nothing has told Errand Claude's limits yet.
    ClaudeNotHeard,
    /// Claude Code gave no limits and there is no recent report to use for this refresh.
    ClaudeAskFailed,
    /// No official Codex app with its command-line tool.
    CodexMissing,
    /// The tool would not start, with the system's reason.
    CodexLaunch(String),
    /// The tool closed its connection.
    CodexClosed,
    /// This request was refused.
    CodexFailed(String),
    /// This request went unanswered.
    CodexNoAnswer(String),
    /// No Codex account is signed in.
    CodexSignIn,
}

/// What a read of a service's usage found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reading {
    pub windows: Vec<UsageWindow>,
    /// When the service said this, if that was before the read: Claude Code reports only while it runs.
    pub reported_at: Option<Timestamp>,
}

impl From<Vec<UsageWindow>> for Reading {
    fn from(windows: Vec<UsageWindow>) -> Reading {
        Reading { windows, reported_at: None }
    }
}

/// One quota a service reports.
#[derive(Clone, Debug, PartialEq)]
pub struct UsageWindow {
    /// Stable across responses, so reset history keeps following the same quota.
    pub identifier: String,
    pub name: WindowName,
    /// 0-100, or None when the service did not say.
    pub remaining_percent: Option<i64>,
    /// Unix seconds of the next refill, or None when the service did not say.
    pub resets_at: Option<f64>,
}

impl UsageWindow {
    /// The refill time when it is a real moment, never a placeholder zero.
    pub fn valid_reset(&self) -> Option<f64> {
        self.resets_at.filter(|seconds| seconds.is_finite() && *seconds > 0.0)
    }
}

/// How a service's batteries read at a glance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mood {
    Healthy,
    Low,
    /// A limit refilled moments ago.
    Reset,
    /// Stale, missing or partial data: never shown as healthy.
    Unknown,
}

pub fn mood(windows: &[UsageWindow], stale: bool, recent_reset: bool) -> Mood {
    if stale || windows.is_empty() {
        return Mood::Unknown;
    }
    let Some(minimum) = windows.iter().map(|window| window.remaining_percent).collect::<Option<Vec<_>>>().and_then(|left| left.into_iter().min())
    else {
        return Mood::Unknown;
    };
    if recent_reset {
        Mood::Reset
    } else if minimum <= LOW_REMAINING {
        Mood::Low
    } else {
        Mood::Healthy
    }
}

/// The quota that runs out first, or None when no window has a percentage. Ties keep the earlier one.
pub fn limiting_window(windows: &[UsageWindow]) -> Option<&UsageWindow> {
    windows.iter().filter(|window| window.remaining_percent.is_some()).fold(None, |lowest: Option<&UsageWindow>, window| match lowest {
        Some(lowest) if lowest.remaining_percent <= window.remaining_percent => Some(lowest),
        _ => Some(window),
    })
}

/// Claude's account-wide week is the overview; a spent model quota still belongs in the details.
/// Without the week, prefer an account-wide limit, then any known model limit. Codex keeps its lowest limit.
pub fn display_window(provider: Provider, windows: &[UsageWindow]) -> Option<&UsageWindow> {
    if provider == Provider::Claude {
        if let Some(weekly) = windows.iter().find(|window| window.identifier == "seven_day" && window.remaining_percent.is_some()) {
            return Some(weekly);
        }
        if let Some(overall) = windows.iter().filter(|window| window.name.model.is_none() && window.remaining_percent.is_some()).min_by_key(|window| window.remaining_percent) {
            return Some(overall);
        }
    }
    limiting_window(windows)
}

/// "주간 96% 남음 · 5시간 75% 남음" for the menu.
pub fn summary(windows: &[UsageWindow], language: Language) -> String {
    if windows.is_empty() {
        return language.no_usage_data().to_owned();
    }
    let parts: Vec<String> = windows.iter().map(|window| language.remaining(&language.window_title(&window.name), window.remaining_percent)).collect();
    parts.join(" · ")
}

/// "주간 초기화 10/19 09:00 · 5시간 초기화 시간 확인 불가" for the menu tooltip.
pub fn reset_details(windows: &[UsageWindow], zone: &TimeZone, language: Language) -> String {
    let parts: Vec<String> = windows
        .iter()
        .map(|window| {
            let date = window.resets_at.and_then(timestamp_from_seconds).map(|at| month_day_time(at, zone));
            language.window_reset(&language.window_title(&window.name), date.as_deref())
        })
        .collect();
    parts.join(" · ")
}

/// "10/19 09:00 초기화" for a battery, or that the time is unknown.
pub fn reset_text(window: Option<&UsageWindow>, zone: &TimeZone, language: Language) -> String {
    let at = window.and_then(UsageWindow::valid_reset).and_then(timestamp_from_seconds);
    language.resets(at.map(|at| month_day_time(at, zone)).as_deref())
}

/// 100 minus what was used, kept within 0-100.
fn remaining(used: i64) -> i64 {
    (100_i64.saturating_sub(used)).clamp(0, 100)
}
