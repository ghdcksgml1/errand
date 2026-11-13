//! Room credentials live in Keychain, separately for each canonical server origin.

use objc2_foundation::{NSString, NSUUID, NSUserDefaults};
use security_framework::os::macos::keychain::SecKeychain;
use std::sync::Mutex;

const INSTALL_ID_KEY: &str = "InstallID";
const SERVICE: &str = "com.errand.rooms.credentials";
const ITEM_NOT_FOUND: i32 = -25300;
const DUPLICATE_ITEM: i32 = -25299;
static MIGRATION: Mutex<()> = Mutex::new(());

/// No OS error payload or secret can reach a log or screen through this error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CredentialError;

trait Secrets {
    fn read(&self, origin: &str) -> Result<Option<String>, CredentialError>;
    /// Add only: a competing process's credential must never be overwritten.
    fn insert(&self, origin: &str, secret: &str) -> Result<bool, CredentialError>;
}
trait Legacy {
    fn read(&self) -> Option<String>;
    fn clear(&self) -> Result<(), CredentialError>;
}

struct Keychain { keychain: SecKeychain, service: String }
impl Secrets for Keychain {
    fn read(&self, origin: &str) -> Result<Option<String>, CredentialError> {
        match self.keychain.find_generic_password(&self.service, origin) {
            Ok((password, _)) => String::from_utf8(password.as_ref().to_vec()).map(Some).map_err(|_| CredentialError),
            Err(error) if error.code() == ITEM_NOT_FOUND => Ok(None),
            Err(_) => Err(CredentialError),
        }
    }
    fn insert(&self, origin: &str, secret: &str) -> Result<bool, CredentialError> {
        match self.keychain.add_generic_password(&self.service, origin, secret.as_bytes()) {
            Ok(()) => Ok(true),
            Err(error) if error.code() == DUPLICATE_ITEM => Ok(false),
            Err(_) => Err(CredentialError),
        }
    }
}
impl Legacy for NSUserDefaults {
    fn read(&self) -> Option<String> {
        self.stringForKey(&NSString::from_str(INSTALL_ID_KEY)).map(|id| id.to_string())
    }
    fn clear(&self) -> Result<(), CredentialError> {
        self.removeObjectForKey(&NSString::from_str(INSTALL_ID_KEY));
        // Wait for the plaintext removal to reach disk before considering migration complete.
        if self.synchronize() { Ok(()) } else { Err(CredentialError) }
    }
}

fn valid(id: &str) -> bool {
    id.len() == 36 && id.bytes().enumerate().all(|(i, byte)| {
        if matches!(i, 8 | 13 | 18 | 23) { byte == b'-' } else { byte.is_ascii_hexdigit() }
    })
}

fn credential(secrets: &impl Secrets, origin: &str, previous: Option<String>, random: &impl Fn() -> String) -> Result<String, CredentialError> {
    if let Some(id) = secrets.read(origin)? {
        return if valid(&id) { Ok(id) } else { Err(CredentialError) };
    }
    let candidate = previous.unwrap_or_else(random);
    if !valid(&candidate) { return Err(CredentialError); }
    let inserted = secrets.insert(origin, &candidate)?;
    // Never delete the old value or send a new credential until Keychain confirms it is readable.
    let id = secrets.read(origin)?.filter(|id| valid(id)).ok_or(CredentialError)?;
    if inserted && id != candidate { return Err(CredentialError); }
    Ok(id)
}

fn resolve(secrets: &impl Secrets, legacy: &impl Legacy, origin: &str, official: &str, random: impl Fn() -> String) -> Result<String, CredentialError> {
    // Preserve the existing official account even when starting with a custom server configured.
    let official_id = credential(secrets, official, legacy.read(), &random)?;
    legacy.clear()?;
    if origin == official { Ok(official_id) } else { credential(secrets, origin, None, &random) }
}

/// Call on a worker thread: Keychain may ask the person to unlock it or allow access.
/// `origin` and `official` are canonical scheme/host/port strings from the API's validated server.
/// A failure is retryable; it never resets an account or falls back to plaintext storage.
pub fn install_id_for(origin: &str, official: &str) -> Result<String, CredentialError> {
    let _guard = MIGRATION.lock().map_err(|_| CredentialError)?;
    let secrets = Keychain { keychain: SecKeychain::default().map_err(|_| CredentialError)?, service: SERVICE.to_owned() };
    resolve(&secrets, &*NSUserDefaults::standardUserDefaults(), origin, official, || NSUUID::new().UUIDString().to_string().to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;

    const OFFICIAL: &str = "https://official.example";
    const CUSTOM: &str = "https://custom.example";
    const OLD: &str = "ABCDEF01-2345-4678-9abc-def012345678";
    const NEW: &str = "01234567-89ab-4cde-8f01-23456789abcd";
    #[derive(Default)]
    struct Store {
        values: RefCell<HashMap<String, String>>,
        locked: Cell<bool>,
        write_fails: Cell<bool>,
        readback_fails: Cell<bool>,
        competing: RefCell<Option<String>>,
    }
    impl Secrets for Store {
        fn read(&self, origin: &str) -> Result<Option<String>, CredentialError> {
            if self.locked.get() { return Err(CredentialError); }
            Ok(self.values.borrow().get(origin).cloned())
        }
        fn insert(&self, origin: &str, secret: &str) -> Result<bool, CredentialError> {
            if self.write_fails.get() { return Err(CredentialError); }
            if let Some(other) = self.competing.borrow_mut().take() {
                self.values.borrow_mut().insert(origin.into(), other);
                return Ok(false);
            }
            self.values.borrow_mut().insert(origin.into(), secret.into());
            self.locked.set(self.readback_fails.get());
            Ok(true)
        }
    }
    struct OldDefaults(RefCell<Option<String>>);
    impl Legacy for OldDefaults {
        fn read(&self) -> Option<String> { self.0.borrow().clone() }
        fn clear(&self) -> Result<(), CredentialError> { self.0.borrow_mut().take(); Ok(()) }
    }
    fn legacy() -> OldDefaults { OldDefaults(RefCell::new(Some(OLD.into()))) }

    #[test]
    fn migrates_official_without_changing_case_and_keeps_custom_separate() {
        let store = Store::default();
        let old = legacy();
        assert_eq!(resolve(&store, &old, CUSTOM, OFFICIAL, || NEW.into()), Ok(NEW.into()));
        assert!(old.read().is_none());
        assert_eq!(resolve(&store, &old, OFFICIAL, OFFICIAL, || panic!("must keep the existing account")), Ok(OLD.into()));
        assert_eq!(resolve(&store, &old, CUSTOM, OFFICIAL, || panic!("must keep the custom account")), Ok(NEW.into()));
        for other in ["https://custom.example:444", "http://127.0.0.1:8080", "http://127.0.0.1:8081"] {
            let id = resolve(&store, &old, other, OFFICIAL, || NSUUID::new().UUIDString().to_string()).unwrap();
            assert_ne!(id, OLD);
            assert_ne!(id, NEW);
            assert_eq!(resolve(&store, &old, other, OFFICIAL, || panic!("stable per origin")), Ok(id));
        }
    }

    #[test]
    fn locked_or_failed_keychain_keeps_plaintext_until_verified_and_can_retry() {
        for failure in 0..3 {
            let store = Store::default();
            store.locked.set(failure == 0);
            store.write_fails.set(failure == 1);
            store.readback_fails.set(failure == 2);
            let old = legacy();
            assert_eq!(resolve(&store, &old, OFFICIAL, OFFICIAL, || panic!("must never replace the old account")), Err(CredentialError));
            assert_eq!(old.read().as_deref(), Some(OLD));
            store.locked.set(false);
            store.write_fails.set(false);
            store.readback_fails.set(false);
            assert_eq!(resolve(&store, &old, OFFICIAL, OFFICIAL, || panic!("must preserve account on retry")), Ok(OLD.into()));
            assert!(old.read().is_none());
        }
    }

    #[test]
    fn invalid_credentials_are_never_reset_or_sent() {
        let store = Store::default();
        let old = legacy();
        store.values.borrow_mut().insert(OFFICIAL.into(), "invalid".into());
        assert_eq!(resolve(&store, &old, OFFICIAL, OFFICIAL, || NEW.into()), Err(CredentialError));
        assert!(old.read().is_some());
        store.values.borrow_mut().clear();
        old.0.replace(Some("------------------------------------".into()));
        assert_eq!(resolve(&store, &old, OFFICIAL, OFFICIAL, || NEW.into()), Err(CredentialError));
        assert!(store.values.borrow().is_empty());
    }

    #[test]
    fn competing_creation_does_not_overwrite_the_winning_account() {
        let store = Store::default();
        let old = OldDefaults(RefCell::new(None));
        store.competing.replace(Some(OLD.into()));
        assert_eq!(resolve(&store, &old, OFFICIAL, OFFICIAL, || NEW.into()), Ok(OLD.into()));
        assert_eq!(store.read(OFFICIAL), Ok(Some(OLD.into())));
    }

    #[test]
    #[ignore = "uses an isolated temporary macOS keychain; run explicitly on a Mac"]
    fn real_keychain_migration_and_origin_isolation() {
        use objc2::AnyThread;
        use security_framework::os::macos::keychain::CreateOptions;
        struct TemporaryKeychain(tempfile::TempDir);
        impl Drop for TemporaryKeychain {
            fn drop(&mut self) {
                // Remove only this test's keychain, including its search-list entry.
                let _ = std::process::Command::new("/usr/bin/security").arg("delete-keychain").arg(self.0.path().join("fixture.keychain-db")).output();
            }
        }
        let temporary = TemporaryKeychain(tempfile::tempdir().unwrap());
        let keychain = CreateOptions::new().password("synthetic-keychain-test-password").create(temporary.0.path().join("fixture.keychain-db")).unwrap();
        let store = Keychain { keychain, service: "com.errand.test.credentials".into() };
        let suite = NSString::from_str(&format!("com.errand.test.migration.{}", NSUUID::new().UUIDString()));
        let defaults = NSUserDefaults::initWithSuiteName(NSUserDefaults::alloc(), Some(&suite)).unwrap();
        unsafe { defaults.setObject_forKey(Some(&NSString::from_str(OLD)), &NSString::from_str(INSTALL_ID_KEY)) };
        assert_eq!(resolve(&store, &*defaults, CUSTOM, OFFICIAL, || NEW.into()), Ok(NEW.into()));
        assert!(defaults.read().is_none());
        assert_eq!(resolve(&store, &*defaults, OFFICIAL, OFFICIAL, || panic!("must keep the migrated account")), Ok(OLD.into()));
        assert_eq!(store.insert(OFFICIAL, NEW), Ok(false));
        assert_eq!(store.read(OFFICIAL), Ok(Some(OLD.into())));
        assert_eq!(store.read(CUSTOM), Ok(Some(NEW.into())));
        defaults.removePersistentDomainForName(&suite);
        assert!(defaults.synchronize());
    }
}
