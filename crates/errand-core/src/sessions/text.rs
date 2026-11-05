//! Turning what was typed or answered into one plain line for a card.

use crate::json;
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;
use unicode_segmentation::UnicodeSegmentation;

/// One line without markup, cut to `limit` UTF-16 units at a whole character, with "…" when cut.
pub fn preview(text: &str, limit: usize) -> String {
    static MARKUP: LazyLock<[(Regex, &str); 4]> = LazyLock::new(|| {
        [
            (Regex::new(r"<[^>\n]{1,200}>").expect("valid pattern"), " "),
            (Regex::new(r"\[([^\]\n]+)\]\([^)\s]+\)").expect("valid pattern"), "$1"),
            (Regex::new(r"(^|\s)#{1,6}\s").expect("valid pattern"), "$1"),
            (Regex::new(r"\*\*|__|`").expect("valid pattern"), ""),
        ]
    });
    let mut value = if text.contains("<command-name>") {
        // A slash command reads as what was typed: "/review 12".
        format!("{} {}", between(text, "<command-name>", "</command-name>"), between(text, "<command-args>", "</command-args>"))
    } else {
        text.to_owned()
    };
    for (pattern, replacement) in MARKUP.iter() {
        value = pattern.replace_all(&value, *replacement).into_owned();
    }
    let joined = value.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate(&joined, limit)
}

/// The text between the first `open` and the first `close` after it began, or "".
fn between<'a>(text: &'a str, open: &str, close: &str) -> &'a str {
    match (text.find(open), text.find(close)) {
        (Some(start), Some(end)) if end > start + open.len() => &text[start + open.len()..end],
        _ => "",
    }
}

fn truncate(text: &str, limit: usize) -> String {
    if json::utf16_len(text) <= limit {
        return text.to_owned();
    }
    let mut units = 0;
    for (start, grapheme) in text.grapheme_indices(true) {
        let width = grapheme.encode_utf16().count();
        if units + width > limit {
            return format!("{}…", &text[..start]);
        }
        units += width;
    }
    text.to_owned()
}

/// Marks a picture in recorded text: the object replacement character, which the view names in the
/// person's language.
pub const PICTURE: &str = "\u{FFFC}";

/// The words of a message: its text parts, with `PICTURE` standing in for pictures.
pub fn message_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => {
            let words: Vec<&str> = parts
                .iter()
                .filter_map(Value::as_object)
                .filter_map(|part| match part.get("type").and_then(Value::as_str) {
                    Some("text") => json::text(part.get("text")),
                    Some("image") => Some(PICTURE),
                    _ => None,
                })
                .collect();
            words.join("\n")
        }
        _ => String::new(),
    }
}

/// Turns an app starts on its own, not words the person typed.
pub fn is_automated_prompt(text: &str) -> bool {
    const PREFIXES: [&str; 7] =
        ["<side-sessions-event", "<ci-monitor-event", "<task-notification", "<local-command", "<system-reminder", "<bash-stdout", "<bash-stderr"];
    let trimmed = text.trim();
    trimmed.is_empty() || PREFIXES.iter().any(|prefix| trimmed.starts_with(prefix))
}

/// The folder Claude Code keeps a working directory's transcripts in: every UTF-16 unit that is not an
/// ASCII letter or digit becomes "-".
pub fn claude_project_directory(path: &str) -> String {
    path.encode_utf16()
        .map(|unit| match u8::try_from(unit) {
            Ok(byte) if byte.is_ascii_alphanumeric() => char::from(byte),
            _ => '-',
        })
        .collect()
}

/// The last component of a path, ignoring a trailing separator. Codex and Claude record Windows paths with
/// backslashes, so both separators count.
pub fn last_path_component(path: &str) -> &str {
    let trimmed = path.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return if path.is_empty() { "" } else { &path[..1] };
    }
    trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_drop_markup_and_keep_link_text() {
        assert_eq!(preview("## 결과\n로그인 **수정** 완료", 200), "결과 로그인 수정 완료");
        assert_eq!(preview("[이슈](https://x.test/1) 보기 `fix.rs` __굵게__", 200), "이슈 보기 fix.rs 굵게");
        assert_eq!(preview("<b>굵게</b>\t그리고\n\n줄", 200), "굵게 그리고 줄");
        assert_eq!(preview("C# 말고 # 제목", 200), "C# 말고 제목");
    }

    #[test]
    fn slash_commands_read_as_typed() {
        let typed = "<command-message>review</command-message>\n<command-name>/review</command-name>\n<command-args>12</command-args>";
        assert_eq!(preview(typed, 280), "/review 12");
        assert_eq!(preview("<command-name>/clear</command-name>", 280), "/clear");
        assert_eq!(preview("<command-name>/never closed", 280), "");
    }

    #[test]
    fn message_text_keeps_words_and_marks_images() {
        let parts = serde_json::json!([{"type": "text", "text": "보기"}, {"type": "image"}, {"type": "tool_use"}, "loose"]);
        assert_eq!(message_text(Some(&parts)), format!("보기\n{PICTURE}"));
        assert_eq!(message_text(Some(&Value::from("그대로"))), "그대로");
        assert_eq!(message_text(None), "");
    }

    #[test]
    fn automated_prompts_are_recognised() {
        assert!(is_automated_prompt("  <side-sessions-event>done</side-sessions-event>"));
        assert!(is_automated_prompt("\n"));
        assert!(!is_automated_prompt("README 고쳐줘"));
    }

    #[test]
    fn transcript_folders_follow_claude_codes_encoding() {
        assert_eq!(
            claude_project_directory("/Users/me/Desktop/work/api.example.com/.claude/worktrees/review-mr-12-a1b2c3"),
            "-Users-me-Desktop-work-api-example-com--claude-worktrees-review-mr-12-a1b2c3"
        );
        assert_eq!(claude_project_directory("/Users/me/문서"), "-Users-me---");
        assert_eq!(claude_project_directory(r"C:\Users\me\app"), "C--Users-me-app");
    }

    #[test]
    fn last_path_components() {
        assert_eq!(last_path_component("/Users/me/app"), "app");
        assert_eq!(last_path_component("/Users/me/api-server/"), "api-server");
        assert_eq!(last_path_component(r"C:\Users\me\app"), "app");
        assert_eq!(last_path_component("/"), "/");
        assert_eq!(last_path_component(""), "");
    }
}
