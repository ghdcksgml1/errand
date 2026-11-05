//! Puts a newer release in this app's place, when the person presses update. The app it downloads is used only
//! once macOS agrees it carries Errand's Developer ID signature and Apple's notarization, so a changed release or a
//! stolen GitHub account cannot replace Errand with something else. Anything that fails leaves this app as it is.

use errand_core::update::Release;
use objc2_foundation::NSBundle;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The Developer ID team that signs Errand. A fork signs with its own and sets it here.
const TEAM: &str = "7KRGRBRWNU";
const BUNDLE_ID: &str = "com.errand.app";
const DOWNLOAD_SECONDS: &str = "600";

/// Installs `release` and starts it, which closes this copy. False when that did not happen.
pub fn install(release: &Release) -> bool {
    let (Some(url), Some(app)) = (release.download.as_deref(), running_app()) else { return false };
    let Some(folder) = app.parent() else { return false };
    // Beside the app, so the new one moves into place without crossing disks.
    let staging = folder.join(format!(".errand-update-{}", std::process::id()));
    let _ = fs::remove_dir_all(&staging);
    let installed = fs::create_dir(&staging).is_ok() && replace(release, url, &app, &staging);
    let _ = fs::remove_dir_all(&staging);
    installed && succeeds("/usr/bin/open", &["-n".as_ref(), app.as_os_str(), "--args".as_ref(), "--replace-running".as_ref()])
}

/// This app's bundle; None when Errand runs from a build folder, where there is nothing to replace.
fn running_app() -> Option<PathBuf> {
    let app = PathBuf::from(NSBundle::mainBundle().bundlePath().to_string());
    (app.extension()? == "app").then_some(app)
}

fn replace(release: &Release, url: &str, app: &Path, staging: &Path) -> bool {
    let archive = staging.join("Errand.zip");
    let unpacked = staging.join("unpacked");
    let new = unpacked.join("Errand.app");
    let download = ["--fail", "--silent", "--location", "--proto", "=https", "--proto-redir", "=https", "--max-time", DOWNLOAD_SECONDS, "--output"].map(AsRef::as_ref);
    succeeds("/usr/bin/curl", &[&download[..], &[archive.as_os_str(), url.as_ref()]].concat())
        && succeeds("/usr/bin/ditto", &["-x".as_ref(), "-k".as_ref(), archive.as_os_str(), unpacked.as_os_str()])
        && is_release(&new, &release.version)
        && is_trusted(&new)
        && swap(&new, app, &staging.join("previous.app")).is_ok()
}

/// Whether this is a real bundle, not a link to one, that says it is Errand at this version.
pub fn is_release(app: &Path, version: &str) -> bool {
    if !app.symlink_metadata().is_ok_and(|metadata| metadata.is_dir()) {
        return false;
    }
    let Ok(info) = plist::Value::from_file(app.join("Contents/Info.plist")) else { return false };
    let field = |key: &str| info.as_dictionary().and_then(|info| info.get(key)).and_then(plist::Value::as_string);
    field("CFBundleIdentifier") == Some(BUNDLE_ID) && field("CFBundleShortVersionString") == Some(version)
}

/// Whether macOS finds this app signed by Errand's team and notarized.
pub fn is_trusted(app: &Path) -> bool {
    let requirement = format!("-R=anchor apple generic and identifier \"{BUNDLE_ID}\" and certificate leaf[subject.OU] = \"{TEAM}\"");
    succeeds("/usr/bin/codesign", &["--verify".as_ref(), "--deep".as_ref(), "--strict".as_ref(), requirement.as_ref(), app.as_os_str()])
        && succeeds("/usr/sbin/spctl", &["--assess".as_ref(), "--type".as_ref(), "execute".as_ref(), app.as_os_str()])
}

/// Moves `new` to where `app` is. The old app goes to `previous`, and comes back if the new one will not move.
pub fn swap(new: &Path, app: &Path, previous: &Path) -> io::Result<()> {
    fs::rename(app, previous)?;
    fs::rename(new, app).inspect_err(|_| {
        let _ = fs::rename(previous, app);
    })
}

fn succeeds(program: &str, arguments: &[&std::ffi::OsStr]) -> bool {
    Command::new(program).args(arguments).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|status| status.success())
}

#[cfg(test)]
mod tests {
    use super::{is_release, is_trusted, swap};
    use std::fs;
    use std::path::Path;

    fn bundle(app: &Path, identifier: &str, version: &str) {
        fs::create_dir_all(app.join("Contents")).unwrap();
        let info = plist::Value::Dictionary(
            [("CFBundleIdentifier", identifier), ("CFBundleShortVersionString", version)].map(|(key, value)| (key.to_owned(), plist::Value::String(value.to_owned()))).into_iter().collect(),
        );
        info.to_file_xml(app.join("Contents/Info.plist")).unwrap();
    }

    #[test]
    fn only_errand_at_the_version_on_offer_is_the_release() {
        let folder = tempfile::tempdir().unwrap();
        let app = folder.path().join("Errand.app");
        assert!(!is_release(&app, "1.8.0"), "nothing there");
        bundle(&app, "com.errand.app", "1.8.0");
        assert!(is_release(&app, "1.8.0"));
        assert!(!is_release(&app, "1.9.0"), "not the version offered");
        let link = folder.path().join("Link.app");
        std::os::unix::fs::symlink(&app, &link).unwrap();
        assert!(!is_release(&link, "1.8.0"), "a link to the app is not the app");
        bundle(&app, "com.example.other", "1.8.0");
        assert!(!is_release(&app, "1.8.0"), "some other app");
    }

    #[test]
    fn an_app_without_errands_signature_is_not_trusted() {
        let folder = tempfile::tempdir().unwrap();
        let app = folder.path().join("Errand.app");
        bundle(&app, "com.errand.app", "1.8.0");
        assert!(!is_trusted(&app));
    }

    /// Only a notarized build can pass, so this runs by hand on one:
    /// `ERRAND_RELEASE_APP=$PWD/dist/Errand.app cargo test -p errand-macos --lib -- --ignored`
    #[test]
    #[ignore = "needs a notarized app"]
    fn a_notarized_release_is_trusted() {
        assert!(is_trusted(Path::new(&std::env::var("ERRAND_RELEASE_APP").unwrap())));
    }

    #[test]
    fn the_old_app_comes_back_when_the_new_one_will_not_move() {
        let folder = tempfile::tempdir().unwrap();
        let (app, new, previous) = (folder.path().join("Errand.app"), folder.path().join("new.app"), folder.path().join("previous.app"));
        fs::create_dir(&app).unwrap();
        fs::write(app.join("old"), "").unwrap();
        assert!(swap(&new, &app, &previous).is_err(), "there is no new app");
        assert!(app.join("old").exists() && !previous.exists(), "the app is back where it was");

        fs::create_dir(&new).unwrap();
        fs::write(new.join("fresh"), "").unwrap();
        swap(&new, &app, &previous).unwrap();
        assert!(app.join("fresh").exists() && previous.join("old").exists() && !new.exists());
    }
}
