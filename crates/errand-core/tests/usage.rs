//! Ported from Tests/UsageTests.m, Tests/ClaudeUsageTests.m and the identifier and mood parts of
//! Tests/PetStateTests.m.

use errand_core::format::timestamp_from_seconds;
use errand_core::usage::claude::ClaudeReport;
use errand_core::usage::{self, Mood, Span, UsageWindow, WindowName, claude, codex};
use errand_core::{Language, TimeZone, Timestamp};
use serde_json::{Value, json};

const KO: Language = Language::Korean;
const EN: Language = Language::English;

fn utc() -> TimeZone {
    TimeZone::UTC
}

fn titles(rows: &[UsageWindow], language: Language) -> Vec<String> {
    rows.iter().map(|row| language.window_title(&row.name)).collect()
}

fn weekly() -> Value {
    json!({"primary": {"usedPercent": 4, "windowDurationMins": 10080, "resetsAt": 1791962132}, "secondary": null})
}

#[test]
fn codex_remaining_comes_from_the_authoritative_bucket_map() {
    let response = json!({"rateLimits": {"primary": {"usedPercent": 99}}, "rateLimitsByLimitId": {"codex": weekly()}});
    let rows = codex::windows_from_response(&response);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].remaining_percent, Some(96), "usage is converted to what remains");
    assert_eq!(titles(&rows, KO), ["주간"], "a weekly primary window is labelled weekly, not as a five-hour window");
    assert_eq!(rows[0].resets_at, Some(1791962132.0), "the reset keeps Unix seconds");
    assert_eq!(usage::summary(&rows, KO), "주간 96% 남음");
    assert_eq!(usage::summary(&rows, EN), "Weekly 96% left");
    assert!(usage::reset_details(&rows, &utc(), KO).contains("초기화"), "reset dates sit next to remaining usage");
    assert!(usage::reset_details(&rows, &utc(), EN).starts_with("Weekly resets "));
}

#[test]
fn codex_legacy_payloads_show_short_and_weekly_limits() {
    let legacy = json!({"rateLimits": {"primary": {"usedPercent": 25, "windowDurationMins": 300}, "secondary": {"usedPercent": 100, "windowDurationMins": 10080}}});
    let rows = codex::windows_from_response(&legacy);
    assert_eq!(rows.iter().map(|row| row.remaining_percent).collect::<Vec<_>>(), [Some(75), Some(0)]);
    assert_eq!(usage::summary(&rows, KO), "5시간 75% 남음 · 주간 0% 남음");
    assert_eq!(usage::summary(&rows, EN), "5-hour 75% left · Weekly 0% left");
}

#[test]
fn codex_unknown_or_absent_usage_is_never_a_full_allowance() {
    let unknown = json!({"rateLimits": {"primary": {"usedPercent": null, "windowDurationMins": null, "resetsAt": null}}});
    let summary = usage::summary(&codex::windows_from_response(&unknown), KO);
    assert!(summary.contains("확인 불가") && !summary.contains("100%"), "{summary}");
    assert_eq!(usage::summary(&codex::windows_from_response(&json!({})), KO), "사용량 정보 없음");
    assert_eq!(usage::summary(&codex::windows_from_response(&json!({})), EN), "No usage data");
}

#[test]
fn codex_named_buckets_stay_visible() {
    let multiple = json!({"rateLimitsByLimitId": {"codex": weekly(), "model-x": {"limitName": "모델 X", "primary": {"usedPercent": 60, "windowDurationMins": 60}}}});
    let rows = codex::windows_from_response(&multiple);
    let summary = usage::summary(&rows, KO);
    assert_eq!(rows.len(), 2);
    assert!(summary.contains("모델 X") && summary.contains("40%"), "{summary}");
    assert_eq!(titles(&rows, EN), ["Codex Weekly", "모델 X 1-hour"], "a bucket's own name is shown as the service gave it");
}

#[test]
fn codex_remaining_stays_within_zero_to_one_hundred_and_malformed_maps_yield_nothing() {
    let clamped = codex::windows_from_response(&json!({"rateLimits": {"primary": {"usedPercent": 110}, "secondary": {"usedPercent": -2}}}));
    assert_eq!(clamped.iter().map(|row| row.remaining_percent).collect::<Vec<_>>(), [Some(0), Some(100)]);
    let malformed = json!({"rateLimitsByLimitId": {"bad": null}, "rateLimits": weekly()});
    assert!(codex::windows_from_response(&malformed).is_empty(), "a malformed authoritative map does not reuse unrelated legacy data");
}

#[test]
fn codex_identifiers_follow_bucket_keys_not_labels_or_positions() {
    let legacy = codex::windows_from_response(&json!({"rateLimits": {"primary": {"usedPercent": 30}, "secondary": {"usedPercent": 40}}}));
    assert_eq!(legacy.iter().map(|row| row.identifier.as_str()).collect::<Vec<_>>(), ["codex/primary", "codex/secondary"]);
    let named = codex::windows_from_response(&json!({"rateLimitsByLimitId": {
        "model-x": {"limitName": "Visible label", "primary": {"usedPercent": 30}},
        "codex": {"secondary": {"usedPercent": 40}}
    }}));
    assert_eq!(named.iter().map(|row| row.identifier.as_str()).collect::<Vec<_>>(), ["codex/secondary", "model-x/primary"]);
    assert_eq!(KO.window_title(&named[1].name), "Visible label 한도 1");
    assert_eq!(EN.window_title(&named[1].name), "Visible label Limit 1");
}

#[test]
fn codex_window_names_follow_their_length() {
    let rows: Vec<UsageWindow> = [10080, 2880, 300, 90, 0]
        .into_iter()
        .map(|minutes| codex::windows_from_response(&json!({"rateLimits": {"primary": {"windowDurationMins": minutes}}}))[0].clone())
        .collect();
    assert_eq!(titles(&rows, KO), ["주간", "2일", "5시간", "90분", "한도 1"]);
    assert_eq!(titles(&rows, EN), ["Weekly", "2-day", "5-hour", "90-minute", "Limit 1"]);
}

/// 2026-10-15T09:00:00Z and 2026-10-19T00:00:00Z.
const FIVE_HOUR_RESET: f64 = 1_792_054_800.0;
const WEEKLY_RESET: f64 = 1_792_368_000.0;

fn at(seconds: f64) -> Timestamp {
    timestamp_from_seconds(seconds).unwrap()
}

/// A status line input with these shares used, in Claude Code's words, beside what Errand has no use for.
fn input(five_hour: Option<(f64, f64)>, seven_day: Option<(f64, f64)>) -> Value {
    let limit = |limit: Option<(f64, f64)>| limit.map_or(Value::Null, |(used, resets_at)| json!({"used_percentage": used, "resets_at": resets_at}));
    json!({"session_id": "abc", "cwd": "/Users/me/app", "rate_limits": {"five_hour": limit(five_hour), "seven_day": limit(seven_day)}})
}

fn left(report: &ClaudeReport) -> Vec<Option<i64>> {
    report.windows.iter().map(|window| window.remaining_percent).collect()
}

#[test]
fn claude_limits_round_usage_up_and_keep_their_deadlines() {
    let limits = json!({
        "five_hour": {"used_percentage": 35.2, "resets_at": 1792054800},
        "seven_day": {"used_percentage": 58, "resets_at": 1792368000},
        "spend_limit": {"used_percentage": 99, "resets_at": 1792054800}
    });
    let rows = claude::windows_from_rate_limits(&limits);
    assert_eq!(titles(&rows, KO), ["5시간", "주간"], "only a subscription's own limits");
    assert_eq!(rows[0].identifier, "five_hour", "the identifier reset history already follows");
    assert_eq!(rows[0].remaining_percent, Some(64), "fractional usage is rounded up so remaining is never overstated");
    assert_eq!(rows[0].resets_at, Some(FIVE_HOUR_RESET));
    assert_eq!((rows[1].identifier.as_str(), rows[1].remaining_percent, rows[1].resets_at), ("seven_day", Some(42), Some(WEEKLY_RESET)));
    assert_eq!(usage::summary(&rows, KO), "5시간 64% 남음 · 주간 42% 남음");
    assert_eq!(usage::summary(&rows, EN), "5-hour 64% left · Weekly 42% left");
}

#[test]
fn a_report_starts_with_the_first_limit_claude_code_mentions() {
    let mut report = ClaudeReport::default();
    report.hear(&json!({"session_id": "abc", "cwd": "/Users/me/app"}), at(100.0));
    assert_eq!(report, ClaudeReport::default(), "a session says nothing before its first answer");
    report.hear(&input(Some((20.0, FIVE_HOUR_RESET)), None), at(200.0));
    assert_eq!((report.reported_at, left(&report)), (Some(at(200.0)), vec![Some(80)]));
    report.hear(&json!({"session_id": "new"}), at(300.0));
    assert_eq!((report.reported_at, left(&report)), (Some(at(200.0)), vec![Some(80)]), "a new session starting takes nothing away");
}

#[test]
fn a_limit_left_out_of_a_report_stays_as_it_was() {
    let mut report = ClaudeReport::default();
    report.hear(&input(None, Some((30.0, WEEKLY_RESET))), at(100.0));
    report.hear(&input(Some((20.0, FIVE_HOUR_RESET)), None), at(200.0));
    assert_eq!(report.windows.iter().map(|window| window.identifier.as_str()).collect::<Vec<_>>(), ["five_hour", "seven_day"], "always in the same order");
    assert_eq!(left(&report), [Some(80), Some(70)]);
}

#[test]
fn an_idle_session_cannot_take_usage_back() {
    let mut report = ClaudeReport::default();
    report.hear(&input(Some((40.0, FIVE_HOUR_RESET)), None), at(100.0));
    report.hear(&input(Some((10.0, FIVE_HOUR_RESET)), None), at(200.0));
    assert_eq!(left(&report), [Some(60)], "usage within a period only grows, so the lower share is old news");
    report.hear(&input(Some((70.0, FIVE_HOUR_RESET)), None), at(300.0));
    assert_eq!(left(&report), [Some(30)], "the newest session is heard");
    report.hear(&input(Some((90.0, FIVE_HOUR_RESET - 18_000.0)), None), at(400.0));
    assert_eq!((left(&report), report.windows[0].resets_at), (vec![Some(30)], Some(FIVE_HOUR_RESET)), "the period before is over");
    report.hear(&input(Some((5.0, FIVE_HOUR_RESET + 18_000.0)), None), at(500.0));
    assert_eq!((left(&report), report.windows[0].resets_at), (vec![Some(95)], Some(FIVE_HOUR_RESET + 18_000.0)), "a new period starts over");
}

#[test]
fn a_limit_past_its_deadline_is_no_longer_shown() {
    let mut report = ClaudeReport::default();
    report.hear(&input(Some((20.0, FIVE_HOUR_RESET)), Some((30.0, WEEKLY_RESET))), at(100.0));
    assert_eq!(report.windows_at(at(FIVE_HOUR_RESET - 1.0)).len(), 2);
    let later = report.windows_at(at(FIVE_HOUR_RESET));
    assert_eq!(later.iter().map(|window| window.identifier.as_str()).collect::<Vec<_>>(), ["seven_day"], "it has refilled by an unknown amount");
    assert!(report.windows_at(at(WEEKLY_RESET)).is_empty());
    assert_eq!(report.windows.len(), 2, "the report itself keeps them, until Claude Code says more");
}

/// What `/usage` printed on 2026-11-06, up to the part about what contributed.
const USAGE_TEXT: &str = "You are currently using your subscription to power your Claude Code usage

Current session: 4% used · resets Nov 6 at 3:50am (Asia/Seoul)
Current week (all models): 42% used · resets Nov 6 at 5am (Asia/Seoul)
Current week (Fable): 26% used · resets Nov 6 at 5am (Asia/Seoul)

What's contributing to your usage?
Approximate · sessions on this machine only · shares overlap
";
/// 2026-11-06T00:10:00+09:00, and the two deadlines Claude Code's status line gave for that text.
const USAGE_NOW: f64 = 1_793_891_400.0;
const SESSION_DEADLINE: f64 = 1_793_904_600.0;
const WEEK_DEADLINE: f64 = 1_793_908_800.0;

#[test]
fn usage_text_reads_the_session_the_week_and_each_model() {
    let rows = claude::windows_from_usage_text(USAGE_TEXT, at(USAGE_NOW));
    assert_eq!(rows.iter().map(|row| row.identifier.as_str()).collect::<Vec<_>>(), ["five_hour", "seven_day", "limits/Fable"], "the same limits the status line names");
    assert_eq!(titles(&rows, KO), ["5시간", "주간", "주간 Fable"]);
    assert_eq!(EN.window_title(&rows[2].name), "Weekly Fable");
    assert_eq!(rows.iter().map(|row| row.remaining_percent).collect::<Vec<_>>(), [Some(96), Some(58), Some(74)]);
    assert_eq!(rows.iter().map(|row| row.resets_at).collect::<Vec<_>>(), [Some(SESSION_DEADLINE), Some(WEEK_DEADLINE), Some(WEEK_DEADLINE)], "the same moments the status line gave");
}

/// When one `/usage` line says its limit resets, in Seoul, as of `now`.
fn resets(deadline: &str, now: f64) -> Option<f64> {
    let line = format!("Current session: 4% used · resets {deadline} (Asia/Seoul)");
    claude::windows_from_usage_text(&line, at(now)).first().and_then(|row| row.resets_at)
}

#[test]
fn usage_deadlines_read_however_they_are_written() {
    assert_eq!(resets("Nov 6 at 3:50am", USAGE_NOW), Some(SESSION_DEADLINE));
    assert_eq!(resets("3:50am", USAGE_NOW), Some(SESSION_DEADLINE), "a time alone is the next one");
    assert_eq!(resets("12:10am", USAGE_NOW), Some(USAGE_NOW + 86_400.0), "today's has just gone, so it is tomorrow's");
    assert_eq!(resets("12am", USAGE_NOW), Some(USAGE_NOW - 600.0 + 86_400.0), "midnight");
    assert_eq!(resets("12pm", USAGE_NOW), Some(USAGE_NOW - 600.0 + 43_200.0), "noon");
    assert_eq!(resets("Nov 6, 2026 at 5am", USAGE_NOW), Some(WEEK_DEADLINE), "a year may be named");
    // Late on 2026-12-31 in Seoul, a deadline in January is next year's.
    let new_year = 1_798_729_200.0;
    assert_eq!(resets("Jan 2 at 5am", new_year - 3_600.0), Some(new_year + 86_400.0 + 18_000.0));
    assert_eq!(resets("Nov 6 at 3:49am", SESSION_DEADLINE + 30.0), Some(SESSION_DEADLINE - 60.0), "one just past is this year's, not next year's");
}

#[test]
fn usage_text_that_does_not_read_exactly_so_shows_nothing_false() {
    let now = at(USAGE_NOW);
    let nothing = |text: &str| assert!(claude::windows_from_usage_text(text, now).is_empty(), "{text}");
    nothing("Unknown skill: usage");
    nothing("");
    nothing("Current session: lots used · resets Nov 6 at 3:50am (Asia/Seoul)");
    nothing("Current session: 4% used");
    nothing("Current session: 4% used · resets Nov 6 at 3:50am");
    nothing("Current session: 4% used · resets soon (Asia/Seoul)");
    nothing("Current session: 4% used · resets Nov 6 at 3:50am (Not/AZone)");
    nothing("Current session: 4% used · resets Foo 6 at 3:50am (Asia/Seoul)");
    nothing("Current session: 4% used · resets Nov 31 at 3:50am (Asia/Seoul)");
    nothing("Current session: 4% used · resets Nov 6 at 3:99am (Asia/Seoul)");
    nothing(&format!("Current week ({}): 4% used · resets Nov 6 at 5am (Asia/Seoul)", "x".repeat(65)));
    let sonnet = claude::windows_from_usage_text("Current week (Sonnet only): 10% used · resets Nov 6 at 5am (Asia/Seoul)", now);
    assert_eq!((sonnet[0].identifier.as_str(), KO.window_title(&sonnet[0].name)), ("limits/Sonnet", "주간 Sonnet".to_owned()));
}

#[test]
fn usage_and_the_status_line_are_heard_as_one_report() {
    let mut report = ClaudeReport::default();
    report.hear(&input(Some((3.0, SESSION_DEADLINE)), Some((42.0, WEEK_DEADLINE))), at(USAGE_NOW));
    let rounded = "Current session: 4% used · resets Nov 6 at 3:49am (Asia/Seoul)\nCurrent week (all models): 41% used · resets Nov 6 at 4:59am (Asia/Seoul)\nCurrent week (Fable): 26% used · resets Nov 6 at 5am (Asia/Seoul)";
    assert!(report.hear_usage(rounded, at(USAGE_NOW + 60.0)));
    assert_eq!(report.reported_at, Some(at(USAGE_NOW + 60.0)));
    assert_eq!(left(&report), [Some(96), Some(58), Some(74)], "a deadline a minute off is the same period, and the higher usage stands");
    assert_eq!(report.windows[0].resets_at, Some(SESSION_DEADLINE), "the exact deadline stays, so reset history sees no change");
    report.hear(&input(Some((9.0, SESSION_DEADLINE)), None), at(USAGE_NOW + 120.0));
    assert_eq!(left(&report), [Some(91), Some(58), Some(74)], "the status line leaves the model's limit alone");
    assert_eq!(report.windows.iter().map(|window| window.identifier.as_str()).collect::<Vec<_>>(), ["five_hour", "seven_day", "limits/Fable"]);
    assert!(!report.hear_usage("Unknown skill: usage", at(USAGE_NOW + 180.0)));
    assert_eq!(report.reported_at, Some(at(USAGE_NOW + 120.0)), "saying nothing is not a report");
}

#[test]
fn codex_fractional_usage_rounds_up_like_claude() {
    let rows = codex::windows_from_response(&json!({"rateLimits": {"primary": {"usedPercent": 4.2, "windowDurationMins": 300}}}));
    assert_eq!(rows[0].remaining_percent, Some(95), "a fraction of a percent used is never shown as still left");
}

#[test]
fn claude_unreadable_or_malformed_limits_show_nothing_false() {
    let windows = |limits: Value| claude::windows_from_rate_limits(&limits);
    assert!(windows(json!({"five_hour": {"used_percentage": 20}, "seven_day": {"used_percentage": 20, "resets_at": 0}})).is_empty(), "no deadline, no way to age it");
    assert!(windows(json!({"five_hour": {"used_percentage": null, "resets_at": 1792054800}})).is_empty());
    let clamped = windows(json!({"five_hour": {"used_percentage": 140, "resets_at": 1792054800}, "seven_day": {"used_percentage": -3, "resets_at": 1792368000}}));
    assert_eq!(clamped.iter().map(|row| row.remaining_percent).collect::<Vec<_>>(), [Some(0), Some(100)]);
    assert!(windows(json!({"five_hour": "bad"})).is_empty());
    assert!(windows(json!([])).is_empty());
    assert_eq!(claude::window("seven_day_sonnet", 50, WEEKLY_RESET), None, "a limit Errand has no name for");
    assert_eq!(claude::window("limits/", 50, WEEKLY_RESET), None, "a model with no name");
}

fn window(identifier: &str, remaining: Option<i64>, resets_at: Option<f64>) -> UsageWindow {
    UsageWindow { identifier: identifier.to_owned(), name: WindowName::span(Span::Minutes(300)), remaining_percent: remaining, resets_at }
}

// Strings, NaN and malformed entries, which the Objective-C mood also guarded against, cannot reach a
// `UsageWindow`; its types rule them out.
#[test]
fn mood_reflects_the_lowest_known_remaining() {
    let mood = |windows: &[UsageWindow], stale, recent| usage::mood(windows, stale, recent);
    assert_eq!(mood(&[window("codex/primary", Some(100), Some(200.0))], false, false), Mood::Healthy);
    assert_eq!(mood(&[window("codex/primary", Some(0), Some(200.0))], false, false), Mood::Low);
    assert_eq!(mood(&[window("codex/primary", Some(25), Some(200.0))], false, false), Mood::Low, "25 is the low threshold");
    assert_eq!(mood(&[window("codex/primary", Some(26), Some(200.0))], false, false), Mood::Healthy);
    assert_eq!(mood(&[window("codex/primary", Some(0), Some(200.0)), window("codex/secondary", Some(90), Some(600.0))], false, false), Mood::Low);
    assert_eq!(mood(&[window("codex/primary", Some(100), Some(200.0)), window("model/primary", Some(20), Some(200.0))], false, false), Mood::Low);
    assert_eq!(mood(&[window("codex/primary", Some(90), Some(200.0))], true, true), Mood::Unknown, "stale usage stays unknown even after a reset");
    assert_eq!(mood(&[], false, true), Mood::Unknown, "no windows cannot appear reset or healthy");
    assert_eq!(mood(&[window("codex/primary", None, Some(200.0))], false, false), Mood::Unknown);
    assert_eq!(mood(&[window("codex/primary", Some(100), Some(200.0)), window("codex/secondary", None, Some(600.0))], false, true), Mood::Unknown);
    assert_eq!(mood(&[window("codex/primary", Some(100), None)], false, true), Mood::Reset, "a reset needs valid remaining data, not a deadline");
    assert_eq!(mood(&[window("codex/primary", Some(0), Some(200.0))], false, true), Mood::Reset);
}

#[test]
fn battery_text_names_the_limit_that_runs_out_first() {
    let windows = [window("five_hour", Some(64), Some(1792054800.0)), window("seven_day", Some(12), None), window("other", Some(12), Some(1.0))];
    let limiting = usage::limiting_window(&windows).expect("a window with a percentage");
    assert_eq!(limiting.identifier, "seven_day", "ties keep the earlier window");
    assert_eq!(usage::reset_text(Some(limiting), &utc(), KO), "초기화 확인 불가");
    assert_eq!(usage::reset_text(Some(&windows[0]), &utc(), KO), "10/15 09:00 초기화");
    assert_eq!(usage::reset_text(Some(&windows[0]), &utc(), EN), "resets 10/15 09:00");
    assert_eq!(usage::reset_text(None, &utc(), KO), "초기화 확인 불가");
    assert_eq!(usage::reset_text(None, &utc(), EN), "reset unknown");
    assert!(usage::limiting_window(&[window("x", None, None)]).is_none());
    let weekly = UsageWindow { name: WindowName::span(Span::Minutes(10080)), ..window("seven_day", Some(1), None) };
    let pair = [window("five_hour", Some(1), Some(1792054800.0)), weekly];
    assert_eq!(usage::reset_details(&pair, &utc(), KO), "5시간 초기화 10/15 09:00 · 주간 초기화 시간 확인 불가");
    assert_eq!(usage::reset_details(&pair, &utc(), EN), "5-hour resets 10/15 09:00 · Weekly reset time unknown");
}
