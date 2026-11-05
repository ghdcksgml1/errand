//! Codex's `account/rateLimits/read` response.

use super::{Span, UsageWindow, WindowName, remaining};
use crate::json;
use serde_json::{Map, Value};

/// Each row has a stable `bucket/window` identifier. With several buckets the name says which bucket.
pub fn windows_from_response(response: &Value) -> Vec<UsageWindow> {
    let wrapped: Map<String, Value>;
    // The per-limit map is authoritative; the single legacy bucket is used only when it is absent.
    let map = match json::object(response.get("rateLimitsByLimitId")) {
        Some(map) => map,
        None => {
            wrapped = match response.get("rateLimits").filter(|bucket| bucket.is_object()) {
                Some(bucket) => Map::from_iter([("codex".to_owned(), bucket.clone())]),
                None => Map::new(),
            };
            &wrapped
        }
    };
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();
    if let Some(index) = keys.iter().position(|key| *key == "codex") {
        let codex = keys.remove(index);
        keys.insert(0, codex);
    }
    let mut rows = Vec::new();
    for key in keys {
        let Some(bucket) = map[key].as_object() else { continue };
        let bucket_name = json::text(bucket.get("limitName")).unwrap_or(if key == "codex" { "Codex" } else { key });
        for (index, window_key) in ["primary", "secondary"].into_iter().enumerate() {
            let Some(window) = json::object(bucket.get(window_key)) else { continue };
            let name = WindowName { bucket: (map.len() > 1).then(|| bucket_name.to_owned()), ..WindowName::span(span(window.get("windowDurationMins"), index)) };
            rows.push(UsageWindow {
                identifier: format!("{key}/{window_key}"),
                name,
                // Rounded up, as Claude's is, so a nearly spent limit never shows more room than it has.
                remaining_percent: json::number(window.get("usedPercent")).filter(|used| used.is_finite()).map(|used| remaining(used.ceil() as i64)),
                resets_at: json::number(window.get("resetsAt")),
            });
        }
    }
    rows
}

/// The limit's length in minutes, or its place in the bucket when the length is unknown.
fn span(minutes: Option<&Value>, index: usize) -> Span {
    match json::number(minutes).map(|minutes| minutes.trunc() as i64).filter(|minutes| *minutes > 0) {
        Some(minutes) => Span::Minutes(minutes),
        None => Span::Numbered(index + 1),
    }
}
