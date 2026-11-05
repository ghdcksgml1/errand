//! The person at the bottom of the dock: the Mac account's name, and their character.

use objc2_foundation::{NSFullUserName, NSString, NSUUID, NSUserDefaults, NSUserName};

/// Where the character is kept, beside the widget's preferences: its seed, or the parts picked in the editor.
/// The name is from when it only held seeds.
const AVATAR_KEY: &str = "AvatarSeed";

pub fn full_user_name() -> String {
    NSFullUserName().to_string()
}

pub fn login_name() -> String {
    NSUserName().to_string()
}

/// The person's character as kept in Errand's defaults: a random seed the first time, then whatever they pick.
pub fn avatar() -> String {
    let stored = NSUserDefaults::standardUserDefaults().stringForKey(&NSString::from_str(AVATAR_KEY));
    stored.map(|avatar| avatar.to_string()).filter(|avatar| !avatar.is_empty()).unwrap_or_else(new_avatar)
}

/// A new random seed, kept in place of the last character: the person's next one.
pub fn new_avatar() -> String {
    let seed = NSUUID::new().UUIDString().to_string();
    keep_avatar(&seed);
    seed
}

/// Keeps the person's character, a seed or the parts they picked, in place of the last.
pub fn keep_avatar(avatar: &str) {
    unsafe { NSUserDefaults::standardUserDefaults().setObject_forKey(Some(&NSString::from_str(avatar)), &NSString::from_str(AVATAR_KEY)) };
}
