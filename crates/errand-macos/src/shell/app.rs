//! Starting the app: one instance at a time, the Dual Codex records moved over, messages delivered on the
//! main thread, and the timers that keep usage, accounts and tasks current.

use super::dashboard::Dashboard;
use super::kit::ns;
use super::status::StatusMenu;
use super::widget::Widget;
use crate::apps;
use crate::paths::Paths;
use dispatch2::{DispatchQueue, DispatchTime};
use errand_core::model::{Alert, AppState, Msg, OpenError, WidgetPreferences};
use errand_core::{Language, PerProvider, Provider};
use errand_core::ports::{Poster, Shell};
use errand_core::view::{self, MenuAction, ViewContext};
use jiff::{SignedDuration, Timestamp};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAlert, NSApplication, NSApplicationActivationOptions, NSApplicationActivationPolicy, NSApplicationDelegate, NSRunningApplication,
    NSWorkspace, NSWorkspaceOpenConfiguration,
};
use objc2_foundation::{NSArray, NSDate, NSDictionary, NSNotification, NSObject, NSObjectProtocol, NSRunLoop, NSString, NSTimer, NSURL, NSUserDefaults};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs;
use std::os::unix::fs::symlink;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

const BUNDLE_ID: &str = "com.errand.app";
/// The identifier this app had while it was called Dual Codex.
const LEGACY_BUNDLE_ID: &str = "com.dualcodex.app";
const HIDDEN_KEY: &str = "FloatingPetsHidden";
const COLLAPSED_KEY: &str = "SessionTrayCollapsed";
/// The services the person hid, by the keys Errand's records use for them.
const HIDDEN_SERVICES_KEY: &str = "HiddenServices";

type Handler = Box<dyn FnMut(Msg)>;

thread_local! {
    static HANDLER: RefCell<Option<Handler>> = RefCell::new(None);
    static WAITING: RefCell<VecDeque<Msg>> = const { RefCell::new(VecDeque::new()) };
}

/// Messages from any thread are handled one at a time on the main thread, after whatever it is doing.
pub struct MainPoster;

impl Poster for MainPoster {
    fn post(&self, msg: Msg) {
        DispatchQueue::main().exec_async(move || deliver(msg));
    }
}

fn deliver(msg: Msg) {
    let Some(mut handler) = HANDLER.with(|handler| handler.borrow_mut().take()) else {
        // The handler is busy with another message; this one follows it.
        WAITING.with(|waiting| waiting.borrow_mut().push_back(msg));
        return;
    };
    handler(msg);
    while let Some(next) = WAITING.with(|waiting| waiting.borrow_mut().pop_front()) {
        handler(next);
    }
    HANDLER.with(|slot| *slot.borrow_mut() = Some(handler));
}

/// The Mac's windows, menus and timers, as the runtime drives them.
pub struct MacShell {
    mtm: MainThreadMarker,
    poster: Arc<MainPoster>,
    defaults: Retained<NSUserDefaults>,
    status: StatusMenu,
    widget: Rc<RefCell<Widget>>,
    dashboard: Dashboard,
    on_menu: Rc<dyn Fn(MenuAction)>,
    hidden: bool,
    language: Language,
}

impl MacShell {
    fn new(mtm: MainThreadMarker, poster: Arc<MainPoster>, language: Language) -> MacShell {
        let defaults = NSUserDefaults::standardUserDefaults();
        import_legacy_defaults(&defaults);
        let menu_poster = poster.clone();
        let hidden = defaults.boolForKey(&ns(HIDDEN_KEY));
        let widget = Widget::new(mtm, poster.clone(), defaults.clone(), language);
        let menu_widget = Rc::downgrade(&widget);
        let on_menu: Rc<dyn Fn(MenuAction)> = Rc::new(move |action| match action {
            MenuAction::OpenApp(provider) => menu_poster.post(Msg::OpenApp(provider)),
            MenuAction::RefreshUsage => menu_poster.post(Msg::AskUsage),
            MenuAction::ShowDashboard => menu_poster.post(Msg::ShowDashboard),
            MenuAction::ToggleWidget => {
                let hidden = NSUserDefaults::standardUserDefaults().boolForKey(&ns(HIDDEN_KEY));
                menu_poster.post(Msg::SetWidgetHidden(!hidden));
            }
            MenuAction::ToggleService(provider) => menu_poster.post(Msg::ToggleService(provider)),
            MenuAction::ShowRooms => menu_poster.post(Msg::ShowRooms),
            MenuAction::InstallUpdate => menu_poster.post(Msg::InstallUpdate),
            MenuAction::ResetWidgetPosition => {
                if let Some(widget) = menu_widget.upgrade() {
                    widget.borrow_mut().reset_position();
                }
                menu_poster.post(Msg::SetWidgetHidden(false));
            }
            MenuAction::Quit => {
                if let Some(mtm) = MainThreadMarker::new() {
                    NSApplication::sharedApplication(mtm).terminate(None);
                }
            }
        });
        let dashboard = Dashboard::new(mtm, poster.clone(), language);
        MacShell { mtm, status: StatusMenu::new(mtm), widget, dashboard, poster, defaults, on_menu, hidden, language }
    }

    pub fn widget_preferences(&self) -> WidgetPreferences {
        stored_preferences(&self.defaults)
    }

    fn context(&self, now: Timestamp) -> ViewContext {
        ViewContext { now, zone: errand_core::format::local_zone(), computer: "Mac", language: self.language }
    }
}

impl Shell for MacShell {
    fn render(&mut self, state: &AppState, now: Timestamp) {
        let context = self.context(now);
        self.hidden = state.widget.hidden;
        self.status.render(view::status_icon(state, self.language), view::menu(state, !self.hidden, &context), &self.on_menu);
        self.widget.borrow_mut().render(state, &context);
        self.dashboard.render(&view::dashboard(state, &context), self.mtm);
    }

    fn save_widget_preferences(&mut self, preferences: WidgetPreferences) {
        store_preferences(&self.defaults, preferences);
    }

    fn open_url(&mut self, url: &str) {
        if let Some(url) = NSURL::URLWithString(&ns(url)) {
            NSWorkspace::sharedWorkspace().openURL(&url);
        }
    }

    fn open_app(&mut self, provider: Provider) {
        let poster = self.poster.clone();
        let app = match provider {
            Provider::Codex => apps::codex_app(&Paths::current().home),
            Provider::Claude => apps::installed(Provider::Claude),
        };
        let Some(app) = app.and_then(|app| NSURL::from_file_path(&app)) else {
            poster.post(Msg::AppOpened { provider, result: Err(OpenError::NotInstalled(provider)) });
            return;
        };
        // Opening the running app brings it forward, and back a closed main window.
        let configuration = NSWorkspaceOpenConfiguration::configuration();
        configuration.setActivates(true);
        let finished = block2::RcBlock::new(move |_: *mut NSRunningApplication, error: *mut objc2_foundation::NSError| {
            let result = match unsafe { error.as_ref() } {
                Some(error) => Err(OpenError::System(error.localizedDescription().to_string())),
                None => Ok(()),
            };
            poster.post(Msg::AppOpened { provider, result });
        });
        NSWorkspace::sharedWorkspace().openApplicationAtURL_configuration_completionHandler(&app, &configuration, Some(&finished));
    }

    fn schedule(&mut self, after: SignedDuration, msg: Msg) {
        let poster = self.poster.clone();
        let when = DispatchTime::try_from(Duration::try_from(after).unwrap_or_default()).unwrap_or(DispatchTime::NOW);
        let _ = DispatchQueue::main().after(when, move || poster.post(msg));
    }

    fn fade_card(&mut self, key: &str) {
        self.widget.borrow().fade_card(key);
    }

    fn alert(&mut self, alert: &Alert) {
        let ((title, text), ok) = (self.language.alert(alert), self.language.ok());
        // Outside the message being handled, since a modal alert runs its own loop.
        DispatchQueue::main().exec_async(move || {
            let Some(mtm) = MainThreadMarker::new() else { return };
            let alert = NSAlert::new(mtm);
            alert.setMessageText(&ns(&title));
            alert.setInformativeText(&ns(&text));
            alert.addButtonWithTitle(&ns(ok));
            alert.runModal();
        });
    }

    fn show_dashboard(&mut self) {
        self.dashboard.show(self.mtm);
    }
}

/// The widget preferences kept in these defaults, under the keys the Objective-C build used.
pub fn stored_preferences(defaults: &NSUserDefaults) -> WidgetPreferences {
    let hidden: Vec<Provider> = defaults
        .stringArrayForKey(&ns(HIDDEN_SERVICES_KEY))
        .map(|keys| keys.iter().filter_map(|key| Provider::from_storage_key(&key.to_string())).collect())
        .unwrap_or_default();
    let mut hidden_services = PerProvider::from_fn(|provider| hidden.contains(&provider));
    // Every service hidden was never possible; such a value shows them all.
    if Provider::ALL.iter().all(|provider| hidden_services[*provider]) {
        hidden_services = PerProvider::default();
    }
    WidgetPreferences { hidden: defaults.boolForKey(&ns(HIDDEN_KEY)), collapsed: defaults.boolForKey(&ns(COLLAPSED_KEY)), hidden_services }
}

pub fn store_preferences(defaults: &NSUserDefaults, preferences: WidgetPreferences) {
    defaults.setBool_forKey(preferences.hidden, &ns(HIDDEN_KEY));
    defaults.setBool_forKey(preferences.collapsed, &ns(COLLAPSED_KEY));
    let hidden: Vec<Retained<NSString>> =
        Provider::ALL.into_iter().filter(|provider| preferences.hidden_services[*provider]).map(|provider| ns(provider.storage_key())).collect();
    unsafe { defaults.setObject_forKey(Some(&NSArray::from_retained_slice(&hidden)), &ns(HIDDEN_SERVICES_KEY)) };
}

/// This build's bundle identifier; a bare executable run from Cargo has none.
fn own_bundle_id() -> Option<String> {
    objc2_foundation::NSBundle::mainBundle().bundleIdentifier().map(|id| id.to_string())
}

/// The widget keeps its place, visibility and folded tray from the Dual Codex build.
fn import_legacy_defaults(defaults: &NSUserDefaults) {
    if own_bundle_id().as_deref() != Some(BUNDLE_ID) {
        return;
    }
    let current = defaults.persistentDomainForName(&ns(BUNDLE_ID));
    let legacy = defaults.persistentDomainForName(&ns(LEGACY_BUNDLE_ID));
    if let Some(legacy) = legacy
        && legacy.count() > 0
        && current.is_none_or(|current| current.count() == 0)
    {
        let legacy: Retained<NSDictionary<NSString, objc2::runtime::AnyObject>> = legacy;
        unsafe { defaults.setPersistentDomain_forName(&legacy, &ns(BUNDLE_ID)) };
    }
}

/// The app was called Dual Codex and kept everything in its own folder, which moves to Errand's. The old
/// path stays behind as a link. An existing Errand folder is never overwritten.
pub fn migrate_legacy_records(paths: &Paths) -> Result<(), String> {
    let legacy = paths.legacy_app_support();
    let current = paths.app_support();
    let is_folder = fs::symlink_metadata(&legacy).is_ok_and(|metadata| metadata.is_dir());
    if !is_folder || fs::symlink_metadata(&current).is_ok() {
        return Ok(());
    }
    fs::rename(&legacy, &current).map_err(|error| error.to_string())?;
    symlink("Errand", &legacy).map_err(|error| error.to_string())
}

fn terminate(app: &NSRunningApplication) -> bool {
    app.terminate();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    // AppKit updates termination state on turns of the main run loop.
    while !app.isTerminated() && std::time::Instant::now() < deadline {
        NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.1));
    }
    app.isTerminated()
}

/// Leaves only this instance running. Returns false when another instance was brought forward instead,
/// and this one should exit.
pub fn take_over(replace_running: bool) -> bool {
    let Some(bundle_id) = own_bundle_id() else { return true };
    // A Dual Codex build would keep a second widget on screen and write into the folder being moved.
    for legacy in NSRunningApplication::runningApplicationsWithBundleIdentifier(&ns(LEGACY_BUNDLE_ID)).iter() {
        terminate(&legacy);
    }
    let own = std::process::id() as i32;
    for other in NSRunningApplication::runningApplicationsWithBundleIdentifier(&ns(&bundle_id)).iter() {
        if other.processIdentifier() == own {
            continue;
        }
        if replace_running && terminate(&other) {
            continue;
        }
        #[allow(deprecated)]
        other.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps);
        return false;
    }
    true
}

/// Builds the runtime once AppKit has finished launching.
type Setup = Box<dyn FnOnce(MainThreadMarker)>;

struct DelegateIvars {
    setup: RefCell<Option<Setup>>,
    poster: Arc<MainPoster>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandAppDelegate"]
    #[ivars = DelegateIvars]
    struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _notification: &NSNotification) {
            if let Some(setup) = self.ivars().setup.borrow_mut().take() {
                setup(self.mtm());
            }
        }

        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn should_handle_reopen(&self, _app: &NSApplication, _visible: bool) -> bool {
            self.ivars().poster.post(Msg::ShowDashboard);
            true
        }

        #[unsafe(method(applicationSupportsSecureRestorableState:))]
        fn secure_restorable_state(&self, _app: &NSApplication) -> bool {
            true
        }
    }
);

/// Runs the app, its words in `language`. `start` builds the runtime from the shell once AppKit is ready and
/// returns the handler every message goes to; it should also do the first round of work.
pub fn run(mtm: MainThreadMarker, language: Language, start: impl FnOnce(MacShell, Arc<dyn Poster>) -> Handler + 'static) {
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    let poster = Arc::new(MainPoster);
    let shell_poster = poster.clone();
    let setup = move |mtm: MainThreadMarker| {
        let shell = MacShell::new(mtm, shell_poster.clone(), language);
        let handler = start(shell, shell_poster.clone());
        HANDLER.with(|slot| *slot.borrow_mut() = Some(handler));
        // Usage every two minutes.
        // Rooms are looked over every ten seconds; each goes out when it is due.
        let room = errand_core::rooms::SHOWN_SYNC_INTERVAL.as_secs_f64();
        let update = errand_core::update::CHECK_INTERVAL.as_secs_f64();
        for (seconds, tick) in [(3.0, Tick::Accounts), (120.0, Tick::Usage), (4.0, Tick::Sessions), (room, Tick::Room), (update, Tick::Update)] {
            let poster = shell_poster.clone();
            let block = block2::RcBlock::new(move |_| {
                poster.post(match tick {
                    Tick::Accounts => Msg::CheckAccounts,
                    Tick::Usage => Msg::RefreshUsage,
                    Tick::Sessions => Msg::ScanSessions,
                    Tick::Room => Msg::SyncRoom,
                    Tick::Update => Msg::CheckForUpdate,
                })
            });
            let timer = unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(seconds, true, &block) };
            timer.setTolerance(seconds / 4.0);
        }
    };
    let delegate = AppDelegate::alloc(mtm).set_ivars(DelegateIvars { setup: RefCell::new(Some(Box::new(setup))), poster });
    let delegate: Retained<AppDelegate> = unsafe { msg_send![super(delegate), init] };
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
}

#[derive(Clone, Copy)]
enum Tick {
    Accounts,
    Usage,
    Sessions,
    Room,
    Update,
}
