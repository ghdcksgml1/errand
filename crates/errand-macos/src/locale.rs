//! The languages the person reads, most preferred first, as macOS gives them to this app: the per-app
//! choice in System Settings when there is one, then the system's own list.

use objc2_foundation::NSLocale;

pub fn preferred_languages() -> Vec<String> {
    NSLocale::preferredLanguages().iter().map(|tag| tag.to_string()).collect()
}
