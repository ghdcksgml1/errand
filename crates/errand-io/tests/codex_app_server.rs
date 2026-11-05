//! Codex's usage through a stand-in for `codex app-server`. A test binary of its own, since it sets an
//! environment variable the app server must not inherit.

use errand_core::usage::UsageError;
use errand_io::api;
use errand_io::codex::CodexSource;
use std::fs;

#[cfg(unix)]
#[test]
fn codex_usage_comes_from_the_app_server_as_the_signed_in_account() {
    use std::os::unix::fs::PermissionsExt;
    let folder = tempfile::tempdir().unwrap();
    let executable = folder.path().join("codex");
    // Answers the way `codex app-server --stdio` does: a notification and a stray reply before the real one.
    fs::write(
        &executable,
        r#"#!/bin/sh
[ "$1 $2" = "app-server --stdio" ] || exit 64
read -r initialize
printf '%s\n' '{"method":"note","params":{}}' '{"id":1,"result":{}}'
read -r initialized
read -r request
case "$request" in
  *'"account/rateLimits/read"'*'"excludeResetCreditDetails":true'*)
    printf '%s\n' '{"id":99,"result":{}}' "{\"id\":2,\"result\":{\"rateLimits\":{\"primary\":{\"usedPercent\":28}},\"home\":\"$CODEX_HOME\",\"key\":\"${OPENAI_API_KEY:-none}\"}}" ;;
  *) printf '%s\n' '{"id":2,"error":{"message":"unknown"}}' ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    let home = folder.path().join(".codex");
    fs::create_dir(&home).unwrap();
    // SAFETY: set before any thread of this test reads the environment.
    unsafe { std::env::set_var("OPENAI_API_KEY", "would-sign-in-as-someone-else") };
    let source = CodexSource { executable: Some(executable), home: home.clone(), directory: folder.path().to_owned() };
    let usage = api::codex::rate_limits(&source).unwrap();
    assert_eq!(usage["rateLimits"]["primary"]["usedPercent"], 28);
    assert_eq!(usage["home"], home.to_str().unwrap(), "the API reads the signed-in account's own home");
    assert_eq!(usage["key"], "none", "inherited API keys cannot pick another account");

    let missing = CodexSource { home: folder.path().join("nowhere"), ..source.clone() };
    assert_eq!(api::codex::rate_limits(&missing).unwrap_err(), UsageError::CodexSignIn);
    let no_app = CodexSource { executable: None, ..source };
    assert_eq!(api::codex::rate_limits(&no_app).unwrap_err(), UsageError::CodexMissing);
}
