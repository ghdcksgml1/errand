//! The person at the bottom of the dock: the Mac account's name, the nickname they gave themselves, and their
//! character.

use objc2_foundation::{NSFullUserName, NSString, NSUUID, NSUserDefaults, NSUserName};

/// Where the character is kept, beside the widget's preferences: its seed, or the parts picked in the editor.
/// The name is from when it only held seeds.
const AVATAR_KEY: &str = "AvatarSeed";
/// The nickname the person typed in the editor, beside the character.
const NICKNAME_KEY: &str = "Nickname";

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

/// The nickname the person gave themselves in Errand, if they did.
pub fn nickname() -> Option<String> {
    let stored = NSUserDefaults::standardUserDefaults().stringForKey(&NSString::from_str(NICKNAME_KEY));
    stored.map(|name| name.to_string()).filter(|name| !name.trim().is_empty())
}

/// Keeps the nickname the person gave themselves, or forgets it so the usual name comes back.
pub fn keep_nickname(name: Option<&str>) {
    let defaults = NSUserDefaults::standardUserDefaults();
    let key = NSString::from_str(NICKNAME_KEY);
    match name {
        Some(name) => unsafe { defaults.setObject_forKey(Some(&NSString::from_str(name)), &key) },
        None => defaults.removeObjectForKey(&key),
    }
}
