//! Errand's own releases on GitHub. Asking sends nothing about the person: only that some Errand of this version
//! wants to know, as GitHub requires a User-Agent. A missing or unreadable answer is no news.

use super::{Endpoint, Refusal, Service};
use errand_core::json;
use errand_core::ports::{HttpMethod, HttpRequest, HttpResponse};
use errand_core::update::{Release, VERSION};
use serde_json::Value;
use std::time::Duration;

/// The only host these calls go to.
pub const HOST: &str = "api.github.com";
const LATEST: &str = "https://api.github.com/repos/ghdcksgml1/errand/releases/latest";
/// A download anywhere else is not Errand's, whatever the answer says.
const DOWNLOADS: &str = "https://github.com/ghdcksgml1/errand/releases/download/";
const TIMEOUT: Duration = Duration::from_secs(15);
/// More than a release's description and assets would ever be.
const REPLY_LIMIT: usize = 1024 * 1024;

pub struct LatestRelease;

impl Endpoint for LatestRelease {
    type Answer = Option<Release>;
    type Error = Refusal;
    const SERVICE: Service = Service::Updates;

    fn request(&self) -> HttpRequest {
        HttpRequest {
            method: HttpMethod::Get,
            url: LATEST.to_owned(),
            headers: vec![
                ("User-Agent".to_owned(), format!("Errand/{VERSION}")),
                ("Accept".to_owned(), "application/vnd.github+json".to_owned()),
            ],
            body: None,
            timeout: TIMEOUT,
        }
    }

    fn answer(&self, response: HttpResponse) -> Result<Option<Release>, Refusal> {
        if response.status != 200 || response.body.len() > REPLY_LIMIT {
            return Ok(None);
        }
        Ok(serde_json::from_slice::<Value>(&response.body).ok().and_then(|reply| release(&reply)))
    }
}

/// The release in GitHub's answer. The app it carries is named for its version, so a release can hold other files.
fn release(reply: &Value) -> Option<Release> {
    let tag = json::text(reply.get("tag_name"))?;
    let version = tag.strip_prefix('v').unwrap_or(tag).to_owned();
    let archive = format!("Errand-{version}.zip");
    let assets = reply.get("assets").and_then(Value::as_array).into_iter().flatten();
    let download = assets
        .filter(|asset| json::text(asset.get("name")) == Some(archive.as_str()))
        .filter_map(|asset| json::text(asset.get("browser_download_url")))
        .find(|url| url.starts_with(DOWNLOADS))
        .map(str::to_owned);
    Some(Release { version, download })
}
