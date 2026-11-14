//! Native controls -> production Runtime -> real settings files -> a fresh Runtime.
//! This is separate from headless CI. It never starts account polling, touches Keychain, or uses a real AI.
#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../src/platform.rs"]
mod platform;
#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../src/runtime.rs"]
mod runtime;

#[cfg(not(target_os = "macos"))]
fn main() {}
#[cfg(target_os = "macos")]
fn main() { e2e::run(); }

#[cfg(target_os = "macos")]
mod e2e {
    use super::{platform::Platform, runtime::Runtime};
    use errand_core::{Language, Provider, Timestamp, model::{Alert, AppState, Confirm, Msg, WidgetPreferences},
        ports::{HttpClient, HttpFailure, HttpRequest, HttpResponse, Poster, Shell}, tasks};
    use errand_io::{api::Api, codex::CodexSource, session_scan::SessionSources};
    use errand_macos::shell::onboarding::Onboarding;
    use objc2::{MainThreadMarker, rc::Retained};
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSButton, NSControlStateValueOff, NSView};
    use std::{cell::RefCell, collections::VecDeque, fs, path::Path, rc::Rc, sync::{Arc, Mutex, atomic::{AtomicUsize, Ordering}}};

    #[derive(Default)]
    struct Inbox(Mutex<VecDeque<Msg>>);
    impl Poster for Inbox { fn post(&self, message: Msg) { self.0.lock().unwrap().push_back(message); } }
    impl Inbox { fn take(&self) -> Option<Msg> { self.0.lock().unwrap().pop_front() } }
    #[derive(Default)]
    struct NoNetwork(AtomicUsize);
    impl HttpClient for NoNetwork {
        fn send(&self, _: &HttpRequest) -> Result<HttpResponse, HttpFailure> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Err(HttpFailure)
        }
    }
    #[derive(Default)]
    struct Observed { state: Option<AppState>, records_opened: usize, setup_opened: usize }
    struct NativeShell {
        mtm: MainThreadMarker,
        onboarding: Rc<RefCell<Onboarding>>,
        observed: Rc<RefCell<Observed>>,
    }
    impl Shell for NativeShell {
        fn show_onboarding(&mut self) {
            self.observed.borrow_mut().setup_opened += 1;
            self.onboarding.borrow().show(self.mtm);
        }
        fn render(&mut self, state: &AppState, _: Timestamp) {
            self.observed.borrow_mut().state = Some(state.clone());
            self.onboarding.borrow_mut().render(state);
        }
        fn show_dashboard(&mut self) { self.observed.borrow_mut().records_opened += 1; }
        fn save_widget_preferences(&mut self, _: WidgetPreferences) {}
        fn open_url(&mut self, _: &str) { panic!("E2E must not open an external URL") }
        fn open_app(&mut self, _: Provider) { panic!("E2E must not open a personal AI app") }
        fn schedule(&mut self, _: jiff::SignedDuration, _: Msg) {}
        fn fade_card(&mut self, _: &str) {}
        fn alert(&mut self, alert: &Alert) { panic!("Unexpected alert: {alert:?}") }
        fn copy_text(&mut self, _: &str) {}
        fn confirm(&mut self, _: &Confirm) { panic!("Unexpected confirmation") }
    }
    struct Harness {
        runtime: Runtime,
        onboarding: Rc<RefCell<Onboarding>>,
        observed: Rc<RefCell<Observed>>,
        inbox: Arc<Inbox>,
        network: Arc<NoNetwork>,
        language: Language,
    }
    impl Harness {
        fn new(root: &Path, language: Language, mtm: MainThreadMarker) -> Self {
            let inbox = Arc::new(Inbox::default());
            let network = Arc::new(NoNetwork::default());
            let onboarding = Rc::new(RefCell::new(Onboarding::new(mtm, inbox.clone(), language)));
            let observed = Rc::new(RefCell::new(Observed::default()));
            let codex = CodexSource { executable: None, home: root.join("codex"), directory: root.into() };
            let platform = Platform {
                home: root.into(), codex: codex.clone(), api: Arc::new(Api::new(network.clone(), root.join("claude-usage.plist"), codex)),
                sessions: SessionSources { codex_home: root.join("codex"), claude_sessions: root.join("claude-sessions"), claude_projects: root.join("claude-projects") },
                reset_history: root.join("history.plist"), session_tray: root.join("tray.plist"), joined_rooms: root.join("rooms.plist"),
                rooms: Arc::new(errand_io::rooms::SampleRooms::new()), language,
            };
            let shell = NativeShell { mtm, onboarding: onboarding.clone(), observed: observed.clone() };
            let runtime = Runtime::new(platform, Box::new(shell), inbox.clone(), WidgetPreferences::default());
            let mut result = Self { runtime, onboarding, observed, inbox, network, language };
            // This is the first message Runtime::start sends. The remaining startup actions involve the
            // person's real apps and Keychain and are deliberately outside this isolated persistence test.
            result.runtime.dispatch(Msg::Tasks(tasks::Msg::BeginOnboarding));
            result.pump();
            result
        }
        fn pump(&mut self) {
            let mut count = 0;
            while let Some(message) = self.inbox.take() {
                count += 1; assert!(count < 1000, "messages must settle");
                self.runtime.dispatch(message);
            }
        }
        fn state(&self) -> AppState { self.observed.borrow().state.clone().unwrap() }
        fn button(&self, korean: &str, english: &str) -> Retained<NSButton> {
            let title = if self.language == Language::Korean { korean } else { english };
            fn find(view: &NSView, title: &str) -> Option<Retained<NSButton>> {
                for child in view.subviews() {
                    if child.isHiddenOrHasHiddenAncestor() { continue; }
                    if let Ok(button) = child.clone().downcast::<NSButton>() {
                        if button.title().to_string() == title { return Some(button); }
                    }
                    if let Some(button) = find(&child, title) { return Some(button); }
                }
                None
            }
            find(&self.onboarding.borrow().view, title).unwrap_or_else(|| panic!("Missing visible button: {title}"))
        }
        fn click(&mut self, korean: &str, english: &str) {
            let button = self.button(korean, english);
            assert!(button.isEnabled());
            unsafe { button.performClick(None) };
            self.pump();
        }
        fn to_ai(&mut self) {
            self.click("둘러보기", "Take a look");
            self.click("다음", "Continue");
        }
        fn finish(&mut self) {
            self.click("설정 확인", "Review choices");
            self.click("작업 기록 열기", "Open work records");
        }
        fn no_network(&self) { assert_eq!(self.network.0.load(Ordering::Relaxed), 0, "onboarding must not make HTTP requests"); }
    }
    impl Drop for Harness { fn drop(&mut self) { self.onboarding.borrow().window.close(); } }

    fn fresh_and_restart(language: Language, mtm: MainThreadMarker) {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let mut app = Harness::new(root.path(), language, mtm);
        assert!(app.state().tasks.onboarding && !app.state().journal.summaries.auto);
        app.to_ai();
        assert_eq!(app.button("하루 작업 기록 자동 요약", "Summarize each day's work automatically").state(), NSControlStateValueOff);
        app.finish();
        assert_eq!(app.observed.borrow().records_opened, 1);
        assert!(!app.onboarding.borrow().window.isVisible());
        let settings_file = root.path().join("tasks/ai-settings.json");
        let settings = errand_io::task_settings::load(&root.path().join("tasks"), Provider::Claude).unwrap();
        assert!(settings.onboarding_complete && !settings.summaries);
        assert_eq!(fs::metadata(&settings_file).unwrap().permissions().mode() & 0o777, 0o600);
        app.no_network(); drop(app);
        let app = Harness::new(root.path(), language, mtm);
        assert_eq!(app.state().tasks.settings, settings);
        assert_eq!(app.observed.borrow().setup_opened, 0);
        assert!(!app.state().journal.summaries.auto);
        app.no_network();
        println!("PASS {language:?}: fresh setup -> native clicks -> private file -> restart without setup");
    }
    fn failed_save_and_retry(language: Language, mtm: MainThreadMarker) {
        let root = tempfile::tempdir().unwrap();
        let mut app = Harness::new(root.path(), language, mtm);
        app.to_ai();
        app.click("하루 작업 기록 자동 요약", "Summarize each day's work automatically");
        assert!(!app.state().journal.summaries.auto, "an unsaved opt-in must not start requests");
        // A directory occupying the atomic save target is a deterministic disk failure, even as root.
        let blocked = root.path().join("tasks/ai-settings.json");
        fs::create_dir_all(&blocked).unwrap();
        app.finish();
        assert!(app.state().tasks.settings_error.is_some());
        assert!(app.state().tasks.onboarding && app.onboarding.borrow().window.isVisible());
        assert!(!app.state().journal.summaries.auto && !app.state().tasks.settings.onboarding_complete);
        assert_eq!(app.observed.borrow().records_opened, 0);
        fs::remove_dir(&blocked).unwrap();
        app.click("작업 기록 열기", "Open work records");
        assert!(app.state().tasks.settings.summaries && app.state().journal.summaries.auto);
        assert_eq!(app.observed.borrow().records_opened, 1);
        app.no_network(); drop(app);
        let app = Harness::new(root.path(), language, mtm);
        assert!(app.state().journal.summaries.auto && app.state().tasks.settings.onboarding_complete);
        assert_eq!(app.observed.borrow().setup_opened, 0);
        app.no_network();
        println!("PASS {language:?}: opt-in -> real disk failure -> retry -> persisted opt-in on restart");
    }
    fn legacy_skip(language: Language, mtm: MainThreadMarker) {
        let root = tempfile::tempdir().unwrap();
        let tasks = root.path().join("tasks"); fs::create_dir_all(&tasks).unwrap();
        fs::write(tasks.join("ai-settings.json"), r#"{"version":1,"provider":"claude","models":{"codex":"","claude":"sonnet"},"summaries":{"auto":true}}"#).unwrap();
        let mut app = Harness::new(root.path(), language, mtm);
        assert!(app.state().tasks.settings.summaries && !app.state().journal.summaries.auto);
        app.click("나중에 설정", "Set up later");
        let saved = errand_io::task_settings::load(&tasks, Provider::Codex).unwrap();
        assert!(saved.onboarding_complete && !saved.summaries);
        assert_eq!(saved.provider, Provider::Claude); assert_eq!(saved.models.claude, "sonnet");
        assert_eq!(app.observed.borrow().records_opened, 0);
        app.no_network(); drop(app);
        let app = Harness::new(root.path(), language, mtm);
        assert_eq!(app.observed.borrow().setup_opened, 0);
        assert!(!app.state().journal.summaries.auto);
        app.no_network();
        println!("PASS {language:?}: legacy settings -> skip -> summaries off and models retained after restart");
    }
    pub fn run() {
        let mtm = MainThreadMarker::new().unwrap();
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        app.finishLaunching();
        for language in Language::ALL {
            fresh_and_restart(language, mtm);
            failed_save_and_retry(language, mtm);
            legacy_skip(language, mtm);
        }
        println!("6 native onboarding persistence E2E flows passed; 0 HTTP calls");
    }
}
