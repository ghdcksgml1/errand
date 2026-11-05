//! Codex on this computer: where its official app and home are, and who is signed in, from `auth.json`. Its
//! usage comes through [`crate::api::codex`]. The login and its renewal stay the official app's; Errand
//! never copies or changes Codex's tokens.

use errand_core::json;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

/// Where to find Codex on this computer.
#[derive(Clone, Debug)]
pub struct CodexSource {
    /// The `codex` executable inside the official app.
    pub executable: Option<PathBuf>,
    /// `~/.codex`: the login, sessions and settings.
    pub home: PathBuf,
    /// Where the API runs; the user's home.
    pub directory: PathBuf,
}

/// Who is signed in to Codex, for labels only.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodexAccount {
    pub email: Option<String>,
    pub name: Option<String>,
}

/// Reads the claims of the identity token in `auth.json`. Token validation and renewal stay the
/// official app's; a malformed file just leaves the labels empty.
pub fn account(home: &Path) -> CodexAccount {
    let claims = identity_claims(&home.join("auth.json")).unwrap_or_default();
    let claim = |key: &str| json::text(claims.get(key)).filter(|value| json::utf16_len(value) < 256).map(str::to_owned);
    CodexAccount { email: claim("email"), name: claim("name") }
}

fn identity_claims(path: &Path) -> Option<serde_json::Map<String, Value>> {
    if fs::metadata(path).ok()?.len() > 1024 * 1024 {
        return None;
    }
    let auth: Value = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    let token = auth.get("tokens")?.get("id_token")?.as_str()?;
    let parts: Vec<&str> = token.split('.').collect();
    let [_, payload, _] = parts.as_slice() else { return None };
    match serde_json::from_slice(&base64url(payload)?).ok()? {
        Value::Object(claims) => Some(claims),
        _ => None,
    }
}

/// URL-safe base64 with or without padding.
fn base64url(text: &str) -> Option<Vec<u8>> {
    let mut bits = 0u32;
    let mut count = 0;
    let mut bytes = Vec::with_capacity(text.len() * 3 / 4);
    for character in text.trim_end_matches('=').bytes() {
        let value = match character {
            b'A'..=b'Z' => character - b'A',
            b'a'..=b'z' => character - b'a' + 26,
            b'0'..=b'9' => character - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => return None,
        };
        bits = bits << 6 | u32::from(value);
        count += 6;
        if count >= 8 {
            count -= 8;
            bytes.push((bits >> count) as u8);
        }
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64url_decodes_with_or_without_padding() {
        assert_eq!(base64url("eyJhIjoxfQ").unwrap(), br#"{"a":1}"#);
        assert_eq!(base64url("eyJhIjoxfQ==").unwrap(), br#"{"a":1}"#);
        assert_eq!(base64url("-_8").unwrap(), [0xfb, 0xff]);
        assert!(base64url("no spaces").is_none());
    }
}
