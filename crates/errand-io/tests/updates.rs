//! Asking GitHub for Errand's latest release, through fake networking.

use errand_core::ports::{HttpClient, HttpFailure, HttpMethod, HttpRequest, HttpResponse};
use errand_core::update::Release;
use errand_io::api::Api;
use errand_io::codex::CodexSource;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

struct FakeGitHub {
    reply: Result<(u16, Value), HttpFailure>,
    requests: Mutex<Vec<HttpRequest>>,
}

impl HttpClient for FakeGitHub {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, HttpFailure> {
        self.requests.lock().unwrap().push(request.clone());
        let (status, body) = self.reply.clone()?;
        Ok(HttpResponse { status, body: body.to_string().into_bytes(), retry_after: None })
    }
}

fn ask(reply: Result<(u16, Value), HttpFailure>) -> (Option<Release>, Vec<HttpRequest>) {
    let github = Arc::new(FakeGitHub { reply, requests: Mutex::default() });
    let home = PathBuf::from("/var/empty/errand-update-tests");
    let codex = CodexSource { executable: None, home: home.join(".codex"), directory: home.clone() };
    let release = Api::new(github.clone(), home.join("claude-usage.plist"), codex).latest_release();
    let requests = github.requests.lock().unwrap().clone();
    (release, requests)
}

const URL: &str = "https://github.com/ghdcksgml1/errand/releases/download/v1.8.0/Errand-1.8.0.zip";

#[test]
fn the_latest_release_names_its_version_and_signed_app() {
    let assets = json!([{"name": "notes.txt", "browser_download_url": "https://github.com/ghdcksgml1/errand/releases/download/v1.8.0/notes.txt"},
                        {"name": "Errand-1.8.0.zip", "browser_download_url": URL}]);
    let (release, requests) = ask(Ok((200, json!({"tag_name": "v1.8.0", "assets": assets}))));
    assert_eq!(release, Some(Release { version: "1.8.0".to_owned(), download: Some(URL.to_owned()) }));
    assert_eq!(requests.len(), 1);
    assert_eq!((requests[0].method, requests[0].url.as_str()), (HttpMethod::Get, "https://api.github.com/repos/ghdcksgml1/errand/releases/latest"));
    let names: Vec<&str> = requests[0].headers.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["User-Agent", "Accept"], "no install ID, no login, nothing about the person");
    assert!(requests[0].body.is_none());
}

#[test]
fn a_release_without_errands_own_download_offers_no_app() {
    let elsewhere = json!({"tag_name": "v1.8.0", "assets": [{"name": "Errand-1.8.0.zip", "browser_download_url": "https://example.com/Errand-1.8.0.zip"}]});
    assert_eq!(ask(Ok((200, elsewhere))).0, Some(Release { version: "1.8.0".to_owned(), download: None }), "an app from anywhere else is not installed");
    let misnamed = json!({"tag_name": "v1.8.0", "assets": [{"name": "Errand-1.7.0.zip", "browser_download_url": URL}]});
    assert_eq!(ask(Ok((200, misnamed))).0.unwrap().download, None, "an archive of another version");
    assert_eq!(ask(Ok((200, json!({"tag_name": "1.8.0"})))).0.unwrap().download, None, "no assets at all");
}

#[test]
fn no_usable_answer_is_no_news() {
    assert_eq!(ask(Ok((404, json!({"message": "Not Found"})))).0, None, "no release yet");
    assert_eq!(ask(Ok((403, json!({"message": "rate limit"})))).0, None);
    assert_eq!(ask(Ok((200, json!({"name": "no tag"})))).0, None);
    assert_eq!(ask(Err(HttpFailure)).0, None, "offline");
}
