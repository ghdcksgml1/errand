//! The Errand executable: the app, or one of its command-line modes.

mod cli;
mod platform;
mod preview;
mod runtime;

use errand_core::Language;

fn main() {
    // Errand's records are the person's own.
    #[cfg(unix)]
    unsafe {
        libc_umask(0o077);
    }
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let language = language(&arguments);
    if let Some(code) = cli::run(&arguments, language) {
        std::process::exit(code);
    }
    run_app(&arguments, language);
}

/// `--language ko` or `--language en` when given, else the language the computer's person reads.
fn language(arguments: &[String]) -> Language {
    arguments
        .iter()
        .position(|argument| argument == "--language")
        .and_then(|index| arguments.get(index + 1))
        .and_then(|tag| Language::from_tag(tag))
        .unwrap_or_else(platform::system_language)
}

#[cfg(unix)]
unsafe extern "C" {
    #[link_name = "umask"]
    fn libc_umask(mask: u32) -> u32;
}

#[cfg(target_os = "macos")]
fn run_app(arguments: &[String], language: Language) {
    use errand_macos::paths::Paths;
    use errand_macos::shell;
    let mtm = objc2::MainThreadMarker::new().expect("Errand starts on the main thread");
    if arguments.iter().any(|argument| argument == "--preview") {
        // Rendering waits for AppKit to finish launching, as the screens' colour space is set up then.
        objc2_app_kit::NSApplication::sharedApplication(mtm).finishLaunching();
        let now = errand_io::now();
        let state = preview::state(arguments, now, language);
        let output = std::path::PathBuf::from(arguments.last().cloned().unwrap_or_default());
        let has = |flag: &str| arguments.iter().any(|argument| argument == flag);
        let in_room = has("--preview-room") || has("--preview-private-room");
        let character = arguments.iter().position(|argument| argument == "--preview-character").and_then(|index| arguments.get(index + 1));
        let kind = if let Some(character) = character {
            shell::preview::PreviewKind::Character(errand_core::avatar::Avatar::named(character))
        } else if has("--preview-rooms") {
            shell::preview::PreviewKind::Rooms
        } else if has("--preview-tray") {
            shell::preview::PreviewKind::Tray
        } else if has("--preview-details") {
            shell::preview::PreviewKind::Details
        } else if has("--preview-avatar") {
            shell::preview::PreviewKind::AvatarEditor
        } else if has("--preview-floating") || in_room {
            shell::preview::PreviewKind::Dock
        } else {
            shell::preview::PreviewKind::Dashboard
        };
        let saved = shell::preview::render(mtm, &state, now, kind, &output, std::sync::Arc::new(shell::MainPoster), language);
        println!("UI preview: {}", if saved { "saved" } else { "failed" });
        std::process::exit(if saved { 0 } else { 1 });
    }
    if !shell::take_over(arguments.iter().any(|argument| argument == "--replace-running")) {
        return;
    }
    // Before anything reads the folder, so the first Errand launch picks up the Dual Codex records.
    let migration = shell::migrate_legacy_records(&Paths::current());
    shell::run(mtm, language, move |shell, poster| {
        let widget = shell.widget_preferences();
        let mut runtime = runtime::Runtime::new(platform::Platform::current(language), Box::new(shell), poster, widget);
        if let Err(error) = migration {
            runtime.alert(&errand_core::model::Alert::MigrationFailed(error));
        }
        runtime.start();
        Box::new(move |msg| runtime.dispatch(msg))
    });
}

#[cfg(not(target_os = "macos"))]
fn run_app(_arguments: &[String], language: Language) {
    eprintln!("errand {}: {}", env!("CARGO_PKG_VERSION"), language.no_screens());
    std::process::exit(2);
}
