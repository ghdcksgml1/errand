//! This install's own ID, for the room server: random, made the first time Errand runs and kept after that.

use objc2_foundation::{NSString, NSUUID, NSUserDefaults};

/// Where the ID is kept, beside the widget's preferences.
const INSTALL_ID_KEY: &str = "InstallID";

/// This install's ID in Errand's defaults. The room server only relays, so it tells installs apart by this.
/// It is made from nothing about the person or the Mac, and the services Errand reads never see it.
pub fn install_id() -> String {
    install_id_in(&NSUserDefaults::standardUserDefaults())
}

/// The ID kept in these defaults, made at random the first time.
pub fn install_id_in(defaults: &NSUserDefaults) -> String {
    let key = NSString::from_str(INSTALL_ID_KEY);
    if let Some(id) = defaults.stringForKey(&key).map(|id| id.to_string()).filter(|id| !id.is_empty()) {
        return id;
    }
    let id = NSUUID::new().UUIDString().to_string().to_lowercase();
    unsafe { defaults.setObject_forKey(Some(&NSString::from_str(&id)), &key) };
    id
}
