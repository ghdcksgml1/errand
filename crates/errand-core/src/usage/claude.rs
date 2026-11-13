//! Claude's limits, from the two places Claude Code shares them: a status line command as it runs, its documented
//! way, and its own `/usage` on automatic or manual refresh. Errand reads no login and calls no server for them,
//! so a limit is only as fresh as the last time Claude Code spoke.

use super::{Span, UsageWindow, WindowName, remaining};
use crate::{format, json};
use jiff::Timestamp;
use jiff::civil::{Date, Time};
use jiff::tz::TimeZone;
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

const WEEK: i64 = 7 * 24 * 60;
/// The limits Claude Code reports, under the names it gives them, and how many minutes each runs.
const LIMITS: [(&str, i64); 2] = [("five_hour", 5 * 60), ("seven_day", WEEK)];
/// What starts the identifier of a weekly limit on one model, before the model's name.
const MODEL_LIMIT: &str = "limits/";
/// `/usage` says deadlines to the minute, and they wobble by one from call to call, so two this close are one
/// period. Periods are hours apart.
const SAME_DEADLINE: f64 = 120.0;
const DAY: f64 = 86_400.0;
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// A limit by its identifier, or None for one Errand has no name for.
pub fn window(identifier: &str, remaining_percent: i64, resets_at: f64) -> Option<UsageWindow> {
    let name = match identifier.strip_prefix(MODEL_LIMIT) {
        Some(model) if !model.is_empty() && json::utf16_len(model) <= 64 => WindowName { model: Some(model.to_owned()), ..WindowName::span(Span::Minutes(WEEK)) },
        Some(_) => return None,
        None => WindowName::span(Span::Minutes(LIMITS.into_iter().find(|(name, _)| *name == identifier)?.1)),
    };
    Some(UsageWindow { identifier: identifier.to_owned(), name, remaining_percent: Some(remaining_percent), resets_at: Some(resets_at) })
}

/// The limits in a status line input's `rate_limits`. Usage is rounded up so a nearly exhausted limit never shows
/// more room than it has. One without a deadline is left out: nothing could say when it stops being true.
pub fn windows_from_rate_limits(rate_limits: &Value) -> Vec<UsageWindow> {
    LIMITS
        .into_iter()
        .filter_map(|(identifier, _)| {
            let limit = json::object(rate_limits.get(identifier))?;
            let used = json::number(limit.get("used_percentage")).filter(|used| used.is_finite())?;
            let resets_at = json::number(limit.get("resets_at")).filter(|at| at.is_finite() && *at > 0.0)?;
            window(identifier, remaining(used.ceil() as i64), resets_at.floor())
        })
        .collect()
}

/// The limits in the text `/usage` prints, a line each: `Current session: 4% used · resets Nov 6 at 3:50am
/// (Asia/Seoul)`, `Current week (all models): …` and `Current week (Fable): …`. It is written for people, so a line
/// that does not read exactly so is left out rather than guessed at.
pub fn windows_from_usage_text(text: &str, now: Timestamp) -> Vec<UsageWindow> {
    static LINE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?m)^\s*Current (?:session|week \(([^()]+)\)): ([0-9]+(?:\.[0-9]+)?)% used · resets (.+) \(([^()]+)\)\s*$").expect("valid pattern")
    });
    LINE.captures_iter(text)
        .filter_map(|line| {
            let identifier = match line.get(1).map(|limit| limit.as_str().trim()) {
                None => "five_hour".to_owned(),
                Some("all models") => "seven_day".to_owned(),
                Some(model) => format!("{MODEL_LIMIT}{}", model.strip_suffix(" only").unwrap_or(model)),
            };
            let used: f64 = line[2].parse().ok()?;
            window(&identifier, remaining(used.ceil() as i64), deadline(&line[3], &line[4], now)?)
        })
        .collect()
}

/// When a deadline `/usage` names falls: `Nov 6 at 3:50am`, `Nov 6 at 5am`, `Jan 2, 2027 at 5am` or `3:50am`, in the
/// zone it gives. Without a year or a date it is the next such moment, since a deadline lies ahead.
fn deadline(text: &str, zone: &str, now: Timestamp) -> Option<f64> {
    static AT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(?:([A-Z][a-z]{2}) ([0-9]{1,2})(?:, ([0-9]{4}))? at )?([0-9]{1,2})(?::([0-9]{2}))?(am|pm)$").expect("valid pattern")
    });
    let at = AT.captures(text)?;
    let number = |index: usize| at.get(index)?.as_str().parse::<i16>().ok();
    let hour = number(4)? % 12 + if &at[6] == "pm" { 12 } else { 0 };
    let time = Time::new(hour as i8, number(5).unwrap_or(0) as i8, 0, 0).ok()?;
    let zone = TimeZone::get(zone).ok()?;
    let moment = |date: Date| Some(format::seconds(date.to_datetime(time).to_zoned(zone.clone()).ok()?.timestamp()));
    let today = now.to_zoned(zone.clone()).date();
    let seconds = format::seconds(now);
    let Some(month) = at.get(1) else {
        let today_at = moment(today)?;
        return if today_at > seconds { Some(today_at) } else { moment(today.tomorrow().ok()?) };
    };
    let month = MONTHS.iter().position(|name| *name == month.as_str())? as i8 + 1;
    let day = number(2)? as i8;
    if let Some(year) = number(3) {
        return moment(Date::new(year, month, day).ok()?);
    }
    let this_year = moment(Date::new(today.year(), month, day).ok()?)?;
    if this_year > seconds - DAY { Some(this_year) } else { moment(Date::new(today.year() + 1, month, day).ok()?) }
}

/// What Claude Code last said of its limits, kept because it speaks only while it runs or when asked.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClaudeReport {
    /// The last time it reported a limit. A session says nothing before its first answer, and an account
    /// without these limits never does.
    pub reported_at: Option<Timestamp>,
    pub windows: Vec<UsageWindow>,
}

impl ClaudeReport {
    /// Takes in one status line input. A limit it does not mention stays as it was, since each can be absent on
    /// its own.
    pub fn hear(&mut self, input: &Value, now: Timestamp) {
        self.keep(input.get("rate_limits").map(windows_from_rate_limits).unwrap_or_default(), now);
    }

    /// Takes in what `/usage` printed; false when it named no limit.
    pub fn hear_usage(&mut self, text: &str, now: Timestamp) -> bool {
        let heard = windows_from_usage_text(text, now);
        let any = !heard.is_empty();
        self.keep(heard, now);
        any
    }

    fn keep(&mut self, heard: Vec<UsageWindow>, now: Timestamp) {
        if heard.is_empty() {
            return;
        }
        self.reported_at = Some(now);
        for window in heard {
            match self.windows.iter_mut().find(|kept| kept.identifier == window.identifier) {
                Some(kept) => *kept = fresher(kept.clone(), window),
                None => self.windows.push(window),
            }
        }
        // The session, the week, then each model in the order first heard.
        self.windows.sort_by_key(|window| LIMITS.iter().position(|(name, _)| *name == window.identifier).unwrap_or(LIMITS.len()));
    }

    /// The limits still running at `now`. One past its deadline has refilled by an amount only Claude Code can
    /// tell, so it goes, as Claude Code drops it too.
    pub fn windows_at(&self, now: Timestamp) -> Vec<UsageWindow> {
        let now = format::seconds(now);
        self.windows.iter().filter(|window| window.valid_reset().is_some_and(|at| at > now)).cloned().collect()
    }
}

/// Which of two reports of a limit to believe. A session left idle repeats what it heard long ago, so a later
/// period wins, and within one period usage only grows. The deadline already kept stays, so a rounded one from
/// `/usage` does not read as a new period in reset history.
fn fresher(kept: UsageWindow, heard: UsageWindow) -> UsageWindow {
    let (Some(kept_at), Some(heard_at)) = (kept.resets_at, heard.resets_at) else { return heard };
    if (heard_at - kept_at).abs() <= SAME_DEADLINE {
        UsageWindow { remaining_percent: kept.remaining_percent.min(heard.remaining_percent), ..kept }
    } else if heard_at > kept_at {
        heard
    } else {
        kept
    }
}
