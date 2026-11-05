//! The official Codex and Claude apps: where they are installed and whether they are running.

use errand_core::Provider;
use objc2_app_kit::{NSRunningApplication, NSWorkspace};
use objc2_foundation::NSString;
use std::fs;
use std::path::{Path, PathBuf};

pub const CODEX_BUNDLE: &str = "com.openai.codex";
pub const CLAUDE_BUNDLE: &str = "com.anthropic.claudefordesktop";

pub fn bundle_id(provider: Provider) -> &'static str {
    match provider {
        Provider::Codex => CODEX_BUNDLE,
        Provider::Claude => CLAUDE_BUNDLE,
    }
}

fn bundle_id_at(app: &Path) -> Option<String> {
    let info = plist::Value::from_file(app.join("Contents/Info.plist")).ok()?;
    info.as_dictionary()?.get("CFBundleIdentifier")?.as_string().map(str::to_owned)
}

/// The installed app, as Launch Services knows it.
pub fn installed(provider: Provider) -> Option<PathBuf> {
    let url = NSWorkspace::sharedWorkspace().URLForApplicationWithBundleIdentifier(&NSString::from_str(bundle_id(provider)))?;
    url.to_file_path()
}

/// The official Codex app: ChatGPT.app or Codex.app, in /Applications or ~/Applications, identified by its
/// bundle ID; otherwise wherever Launch Services has it.
pub fn codex_app(home: &Path) -> Option<PathBuf> {
    let candidates = ["/Applications/ChatGPT.app", "/Applications/Codex.app"]
        .map(PathBuf::from)
        .into_iter()
        .chain(["Applications/ChatGPT.app", "Applications/Codex.app"].map(|relative| home.join(relative)));
    candidates.into_iter().find(|app| bundle_id_at(app).as_deref() == Some(CODEX_BUNDLE)).or_else(|| installed(Provider::Codex))
}

/// The `codex` command-line tool the app ships.
pub fn codex_executable(app: &Path) -> Option<PathBuf> {
    ["Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex", "Contents/Resources/codex"]
        .map(|relative| app.join(relative))
        .into_iter()
        .find(|path| path.metadata().is_ok_and(|metadata| metadata.is_file() && std::os::unix::fs::PermissionsExt::mode(&metadata.permissions()) & 0o111 != 0))
}

/// The Claude Code the desktop app keeps for its Code sessions, in a folder per version: the newest one.
pub fn claude_code(home: &Path) -> Option<PathBuf> {
    let versions = fs::read_dir(home.join("Library/Application Support/Claude/claude-code")).ok()?;
    versions
        .flatten()
        .filter_map(|version| {
            let number: Vec<u64> = version.file_name().to_str()?.split('.').map(|part| part.parse().ok()).collect::<Option<_>>()?;
            let builds = fs::read_dir(version.path()).ok()?;
            let executable = builds.flatten().map(|build| build.path().join("claude.app/Contents/MacOS/claude")).find(|path| path.is_file())?;
            Some((number, executable))
        })
        .max()
        .map(|(_, executable)| executable)
}

/// The process ID of the service's running app, if one is open.
pub fn running(provider: Provider) -> Option<i32> {
    let apps = NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(bundle_id(provider)));
    apps.iter().find(|app| !app.isTerminated()).map(|app| app.processIdentifier())
}

#[cfg(test)]
mod tests {
    use super::claude_code;
    use std::fs;
    use std::path::Path;

    fn version(home: &Path, version: &str, build: &str) {
        let executable = home.join("Library/Application Support/Claude/claude-code").join(version).join(build).join("claude.app/Contents/MacOS");
        fs::create_dir_all(&executable).unwrap();
        fs::write(executable.join("claude"), "stand-in").unwrap();
    }

    #[test]
    fn the_newest_claude_code_goes_by_its_version_numbers() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(claude_code(home.path()), None, "no desktop app");
        version(home.path(), "2.1.9", "aaa");
        version(home.path(), "2.1.10", "bbb");
        fs::create_dir_all(home.path().join("Library/Application Support/Claude/claude-code/2.1.11/half-installed")).unwrap();
        version(home.path(), "latest", "ccc");
        let found = claude_code(home.path()).unwrap();
        assert!(found.ends_with("2.1.10/bbb/claude.app/Contents/MacOS/claude"), "{}", found.display());
    }
}
