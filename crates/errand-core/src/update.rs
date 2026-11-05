//! Errand's own new versions: what the latest release offers, and where that leaves this copy. A person always
//! decides: a newer version is offered, and only their press installs it.

use jiff::SignedDuration;

/// The Errand this copy was built as.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Where to read about new versions, for a release without an app to install.
pub const RELEASES_PAGE: &str = "https://github.com/ghdcksgml1/errand/releases/latest";
/// How often Errand asks, besides at launch.
pub const CHECK_INTERVAL: SignedDuration = SignedDuration::from_hours(24);

/// The newest version published.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// As `1.8.0`.
    pub version: String,
    /// Where the signed app is, when the release carries one.
    pub download: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum UpdateState {
    /// Nothing newer is known.
    #[default]
    Current,
    Available(Release),
    Installing(Release),
    /// It would not install; the person can try again.
    Failed(Release),
}

/// Whether `candidate` is a later version than `current`. Each is three numbers, with or without a leading `v`;
/// anything else, a pre-release among them, is not offered.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    fn numbers(version: &str) -> Option<[u64; 3]> {
        let parts: Vec<u64> = version.strip_prefix('v').unwrap_or(version).split('.').map(|part| part.parse().ok()).collect::<Option<_>>()?;
        parts.try_into().ok()
    }
    numbers(candidate).zip(numbers(current)).is_some_and(|(candidate, current)| candidate > current)
}
