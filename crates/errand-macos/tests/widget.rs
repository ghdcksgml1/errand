//! The floating widget as AppKit shows it: its panels' behaviour, what the dock, details and task cards say,
//! and what their clicks send. AppKit wants the main thread, so this test has its own `main`
//! (`harness = false`). It drives the widget as the runtime does, on a clock the checks move by hand.

#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
fn main() {
    std::process::exit(widget::run());
}

#[cfg(target_os = "macos")]
mod widget {
    use errand_core::avatar::{Avatar, CLOTHES, Part};
    use errand_core::layout::{self, DOCK_PADDING, STACK_WIDTH};
    use errand_core::model::{AppState, Effect, Msg, Profile, WidgetMode, WidgetPreferences, update};
    use errand_core::ports::Poster;
    use errand_core::reset_history::ResetHistory;
    use errand_core::rooms::{Face, Member, Role, Room, RoomSummary, Visibility};
    use errand_core::sessions::TaskCounts;
    use errand_core::sessions::{Question, SessionRecords, SessionRow};
    use errand_core::theme;
    use errand_core::usage::{Span, UsageError, UsageWindow, WindowName};
    use errand_core::view::ViewContext;
    use errand_core::{Language, Provider, TimeZone, Timestamp};
    use errand_macos::shell::kit::{self, ns, rect};
    use errand_macos::shell::panel::{primary_height, to_core};
    use errand_macos::shell::{Widget, status, store_preferences, stored_preferences};
    use jiff::SignedDuration;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{AnyThread, MainThreadMarker, Message, msg_send};
    use objc2_app_kit::{
        NSAccessibility, NSApplication, NSApplicationActivationPolicy, NSButton, NSColor, NSEvent, NSEventModifierFlags, NSEventType, NSFloatingWindowLevel,
        NSImageView, NSScrollView, NSTextField, NSUserInterfaceItemIdentification, NSView, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
    };
    use objc2_core_graphics::{CGEvent, CGEventFlags, CGScrollEventUnit};
    use objc2_foundation::{NSDate, NSNumber, NSObjectProtocol, NSPoint, NSRect, NSRunLoop, NSUUID, NSUserDefaults};
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};

    /// 2026-10-15 15:00 in Seoul.
    const NOW: i64 = 1_792_044_000;
    /// 10/19 09:00 and 10/15 18:00 in Seoul.
    const WEEKLY_RESET: f64 = 1_792_368_000.0;
    const FIVE_HOUR_RESET: f64 = 1_792_054_800.0;

    #[derive(Default)]
    struct Checks {
        run: usize,
        failed: usize,
    }

    impl Checks {
        fn check(&mut self, pass: bool, message: &str) {
            self.run += 1;
            if !pass {
                self.failed += 1;
                eprintln!("FAIL: {message}");
            }
        }
    }

    /// What the widget sends, kept until the harness handles it.
    #[derive(Default)]
    struct Inbox(Mutex<VecDeque<Msg>>);

    impl Poster for Inbox {
        fn post(&self, msg: Msg) {
            self.0.lock().expect("the inbox").push_back(msg);
        }
    }

    impl Inbox {
        fn take(&self) -> Option<Msg> {
            self.0.lock().expect("the inbox").pop_front()
        }

        fn is_empty(&self) -> bool {
            self.0.lock().expect("the inbox").is_empty()
        }
    }

    /// Plays the runtime: messages go through `update` and the widget draws the result. Reads and launches
    /// are noted instead of carried out, and timers wait for `advance`.
    struct Harness {
        state: AppState,
        now: Timestamp,
        zone: TimeZone,
        language: Language,
        widget: Rc<RefCell<Widget>>,
        inbox: Arc<Inbox>,
        defaults: Retained<NSUserDefaults>,
        timers: Vec<(Timestamp, Msg)>,
        performed: Vec<Effect>,
    }

    impl Harness {
        /// A fresh widget in this language, its clock at `NOW`.
        fn new(mtm: MainThreadMarker, defaults: &Retained<NSUserDefaults>, language: Language) -> Harness {
            let inbox = Arc::new(Inbox::default());
            let now = Timestamp::from_second(NOW).expect("a fixed time");
            Harness {
                state: AppState::new(ResetHistory::new(), SessionRecords::new(now - SignedDuration::from_hours(1)), WidgetPreferences::default(), "맥 사용자".to_owned()),
                now,
                zone: TimeZone::get("Asia/Seoul").expect("the Seoul time zone"),
                language,
                widget: Widget::new(mtm, inbox.clone(), defaults.clone(), language),
                inbox,
                defaults: defaults.clone(),
                timers: Vec::new(),
                performed: Vec::new(),
            }
        }

        fn send(&mut self, msg: Msg) {
            self.inbox.post(msg);
            self.pump();
        }

        /// Handles everything sent so far, then draws.
        fn pump(&mut self) {
            loop {
                while let Some(msg) = self.inbox.take() {
                    for effect in update(&mut self.state, msg, self.now) {
                        self.perform(effect);
                    }
                }
                let context = ViewContext { now: self.now, zone: self.zone.clone(), computer: "Mac", language: self.language };
                self.widget.borrow_mut().render(&self.state, &context);
                if self.inbox.is_empty() {
                    return;
                }
            }
        }

        fn perform(&mut self, effect: Effect) {
            match &effect {
                Effect::ScheduleCompletion { key, after } => self.timers.push((self.now + *after, Msg::CompletionDue(key.clone()))),
                Effect::TickRoomClock => self.timers.push((self.now + errand_core::rooms::CLOCK_TICK, Msg::RoomClock)),
                Effect::FadeCard(key) => self.widget.borrow().fade_card(key),
                Effect::SaveWidgetPreferences => store_preferences(&self.defaults, self.state.widget),
                Effect::OpenApp(provider) => self.inbox.post(Msg::AppOpened { provider: *provider, result: Ok(()) }),
                _ => {}
            }
            self.performed.push(effect);
        }

        /// Moves the clock on; timers that came due fire in order.
        fn advance(&mut self, by: SignedDuration) {
            self.now += by;
            self.timers.sort_by_key(|(at, _)| *at);
            let now = self.now;
            let (due, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.timers).into_iter().partition(|(at, _)| *at <= now);
            self.timers = waiting;
            for (_, msg) in due {
                self.inbox.post(msg);
            }
            self.pump();
        }

        /// Lets animations run for real, then handles what they sent when they ended.
        fn spin(&mut self, seconds: f64) {
            NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(seconds));
            self.pump();
        }

        fn performed(&self, wanted: impl Fn(&Effect) -> bool) -> usize {
            self.performed.iter().filter(|effect| wanted(effect)).count()
        }

        fn ago(&self, seconds: i64) -> Timestamp {
            self.now - SignedDuration::from_secs(seconds)
        }

        fn dock(&self) -> Retained<NSView> {
            Retained::into_super(self.widget.borrow().dock.view.clone())
        }

        fn battery(&self, provider: Provider) -> Retained<NSView> {
            Retained::into_super(self.widget.borrow().dock.battery(provider).expect("a battery per service").clone())
        }

        fn tray(&self) -> Retained<NSView> {
            Retained::into_super(self.widget.borrow().tray.view.clone())
        }

        fn card(&self, key: &str) -> Option<Retained<NSView>> {
            self.widget.borrow().tray.card(key).map(Retained::into_super)
        }

        fn line(&self, key: &str) -> Option<Retained<NSView>> {
            self.widget.borrow().tray.sub_task(key).map(Retained::into_super)
        }

        fn details(&self) -> Retained<NSView> {
            self.widget.borrow().details_panel.contentView().expect("the details' view")
        }

        /// Moves the dock so its bottom-left corner is here, in AppKit's coordinates.
        fn park(&mut self, origin: NSPoint, persist: bool, mtm: MainThreadMarker) {
            let size = self.widget.borrow().panel.frame().size;
            let frame = to_core(NSRect::new(origin, size), primary_height(mtm));
            self.widget.borrow_mut().move_to(frame, persist);
            self.pump();
        }
    }

    fn every(view: &NSView) -> Vec<Retained<NSView>> {
        let mut found = vec![view.retain()];
        for child in view.subviews().iter() {
            found.extend(every(&child));
        }
        found
    }

    fn class_name(view: &NSView) -> String {
        let object: &AnyObject = view.as_ref();
        object.class().name().to_string_lossy().into_owned()
    }

    fn of_class(view: &NSView, class: &str) -> Vec<Retained<NSView>> {
        every(view).into_iter().filter(|found| class_name(found) == class).collect()
    }

    fn first(view: &NSView, class: &str) -> Option<Retained<NSView>> {
        of_class(view, class).into_iter().next()
    }

    fn labels(view: &NSView) -> Vec<Retained<NSTextField>> {
        every(view).into_iter().filter_map(|found| found.downcast::<NSTextField>().ok()).collect()
    }

    fn shows(view: &NSView, text: &str) -> bool {
        labels(view).iter().any(|label| label.stringValue().to_string().contains(text))
    }

    fn label(view: &NSView, text: &str) -> Option<Retained<NSTextField>> {
        labels(view).into_iter().find(|label| label.stringValue().to_string() == text)
    }

    fn buttons(view: &NSView) -> Vec<Retained<NSButton>> {
        every(view).into_iter().filter_map(|found| found.downcast::<NSButton>().ok()).collect()
    }

    fn titled(view: &NSView, title: &str) -> Option<Retained<NSButton>> {
        buttons(view).into_iter().find(|button| button.title().to_string().starts_with(title))
    }

    fn spoken(view: &NSView) -> String {
        view.accessibilityLabel().map(|words| words.to_string()).unwrap_or_default()
    }

    fn number(view: &NSView) -> Option<isize> {
        view.accessibilityValue()?.downcast::<NSNumber>().ok().map(|value| value.integerValue())
    }

    fn tooltip(view: &NSView) -> String {
        view.toolTip().map(|words| words.to_string()).unwrap_or_default()
    }

    /// The Lucide icons drawn in a view, by name, in order.
    fn icons(view: &NSView) -> Vec<String> {
        every(view)
            .into_iter()
            .filter_map(|found| found.downcast::<NSImageView>().ok())
            .filter_map(|icon| icon.image()?.accessibilityDescription())
            .map(|name| name.to_string())
            .collect()
    }

    /// The app each app mark stands for, in order.
    fn marks(view: &NSView) -> Vec<String> {
        of_class(view, "ErrandAppMarkView").iter().map(|mark| spoken(mark)).collect()
    }

    fn same(a: &NSView, b: &NSView) -> bool {
        std::ptr::eq(a, b)
    }

    fn same_color(a: &NSColor, b: &NSColor) -> bool {
        let other: &AnyObject = b.as_ref();
        a.isEqual(Some(other))
    }

    fn max_y(frame: NSRect) -> f64 {
        frame.origin.y + frame.size.height
    }

    fn max_x(frame: NSRect) -> f64 {
        frame.origin.x + frame.size.width
    }

    fn overlap_width(a: NSRect, b: NSRect) -> f64 {
        (max_x(a).min(max_x(b)) - a.origin.x.max(b.origin.x)).max(0.0)
    }

    fn inside(outer: NSRect, inner: NSRect) -> bool {
        inner.origin.x >= outer.origin.x - 0.5
            && inner.origin.y >= outer.origin.y - 0.5
            && max_x(inner) <= max_x(outer) + 0.5
            && max_y(inner) <= max_y(outer) + 0.5
    }

    /// The view the window would hand a click at this point, in the window's coordinates.
    fn hit(window: &NSWindow, point: NSPoint) -> Option<Retained<NSView>> {
        let content = window.contentView()?;
        // The window's frame view; the content view's own hit test takes points in its coordinates.
        let frame = unsafe { content.superview() }?;
        content.hitTest(frame.convertPoint_fromView(point, None))
    }

    /// A swipe, or a wheel turned, at a point of `view`, delivered to whatever the window finds there.
    fn swipe(view: &NSView, point: NSPoint, unit: CGScrollEventUnit, down: i32, across: i32, mtm: MainThreadMarker) {
        let window = view.window().expect("the view is in a window");
        let at = view.convertPoint_toView(point, None);
        let event = CGEvent::new_scroll_wheel_event2(None, unit, 2, down, across, 0).expect("a scroll event");
        // Core Graphics counts down from the top of the primary screen. Made with no window, the event keeps this
        // as its place in the window it is given later, and nothing public gives it one.
        CGEvent::set_location(Some(&event), NSPoint::new(at.x, primary_height(mtm) - at.y));
        CGEvent::set_flags(Some(&event), CGEventFlags(0));
        let event = NSEvent::eventWithCGEvent(&event).expect("a scroll event");
        let _: () = unsafe { msg_send![&*event, setValue: &*window, forKey: &*ns("_window")] };
        window.sendEvent(&event);
    }

    /// A press and release at a point of `view`, delivered to whatever the window finds there.
    fn click(view: &NSView, point: NSPoint) {
        let window = view.window().expect("the view is in a window");
        let at = view.convertPoint_toView(point, None);
        let event = |kind, time, number, pressure| {
            NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                kind,
                at,
                NSEventModifierFlags::empty(),
                time,
                window.windowNumber(),
                None,
                number,
                1,
                pressure,
            )
            .expect("a mouse event")
        };
        let target = hit(&window, at).expect("something under the click");
        target.mouseDown(&event(NSEventType::LeftMouseDown, 0.0, 1, 1.0));
        target.mouseUp(&event(NSEventType::LeftMouseUp, 0.1, 2, 0.0));
    }

    fn pointer(kind: NSEventType, window: &NSWindow) -> Retained<NSEvent> {
        pointer_at(kind, window, NSPoint::ZERO)
    }

    fn pointer_at(kind: NSEventType, window: &NSWindow, location: NSPoint) -> Retained<NSEvent> {
        unsafe {
            NSEvent::enterExitEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_trackingNumber_userData(
                kind,
                location,
                NSEventModifierFlags::empty(),
                0.0,
                window.windowNumber(),
                None,
                0,
                0,
                std::ptr::null_mut(),
            )
        }
        .expect("a pointer event")
    }

    fn scroll(tray: &NSView) -> Option<Retained<NSScrollView>> {
        every(tray).into_iter().find_map(|view| view.downcast::<NSScrollView>().ok())
    }

    /// The cards' keys from the top of the stack down.
    fn card_order(tray: &NSView) -> Vec<String> {
        let Some(document) = scroll(tray).and_then(|scroll| scroll.documentView()) else { return Vec::new() };
        let mut cards: Vec<Retained<NSView>> = document.subviews().to_vec();
        cards.sort_by(|a, b| a.frame().origin.y.total_cmp(&b.frame().origin.y));
        cards.iter().filter_map(|card| card.identifier()).map(|key| key.to_string()).collect()
    }

    /// Only whole cards show: none is cut by the visible part's edge.
    fn none_sliced(tray: &NSView) -> bool {
        let Some(scroll) = scroll(tray) else { return true };
        let visible = scroll.contentView().bounds();
        let Some(document) = scroll.documentView() else { return true };
        document.subviews().iter().all(|card| {
            let frame = card.frame();
            let shown = (max_y(frame).min(max_y(visible)) - frame.origin.y.max(visible.origin.y)).max(0.0);
            shown <= 0.5 || shown >= frame.size.height - 0.5
        })
    }

    const WEEK: i64 = 10080;
    const FIVE_HOURS: i64 = 300;

    fn window(identifier: &str, minutes: i64, left: i64, resets: Option<f64>) -> UsageWindow {
        UsageWindow { identifier: identifier.to_owned(), name: WindowName::span(Span::Minutes(minutes)), remaining_percent: Some(left), resets_at: resets }
    }

    fn row(key: &str, provider: Provider, title: &str, prompt: &str, asked: Timestamp, finished: Option<Timestamp>) -> SessionRow {
        SessionRow {
            key: key.to_owned(),
            provider,
            title: title.to_owned(),
            prompt: prompt.to_owned(),
            prompt_at: asked,
            running: finished.is_none(),
            aborted: false,
            finished_at: finished,
            result: None,
            action: None,
            folder: String::new(),
            url: format!("errand-test://{key}"),
            seen_in_app: false,
            stalled: false,
            parent: None,
            on_screen: false,
            question: None,
        }
    }

    pub fn run() -> i32 {
        let mtm = MainThreadMarker::new().expect("the test runs on the main thread");
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        app.finishLaunching();
        // A suite named by a path keeps its plist there, in Cargo's scratch folder rather than among the
        // Mac's preferences, where the preferences daemon would write it back after the test ends.
        let suite = format!("{}/errand-widget-{}", env!("CARGO_TARGET_TMPDIR"), NSUUID::new().UUIDString());
        let defaults = NSUserDefaults::initWithSuiteName(NSUserDefaults::alloc(), Some(&ns(&suite))).expect("a test defaults suite");
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| checks(mtm, &defaults)));
        defaults.removePersistentDomainForName(&ns(&suite));
        let _ = std::fs::remove_file(format!("{suite}.plist"));
        match outcome {
            Ok(t) => {
                println!("{} widget checks, {} failures", t.run, t.failed);
                i32::from(t.failed > 0)
            }
            Err(_) => 1,
        }
    }

    fn checks(mtm: MainThreadMarker, defaults: &Retained<NSUserDefaults>) -> Checks {
        let mut t = Checks::default();
        korean(&mut t, mtm, defaults);
        english(&mut t, mtm, defaults);
        hiding(&mut t, mtm, defaults);
        modes(&mut t, mtm, defaults);
        rooms(&mut t, mtm, defaults);
        private_rooms(&mut t, mtm, defaults);
        room_tabs(&mut t, mtm, defaults);
        identity(&mut t, defaults);
        t
    }

    /// The install's own ID for the room server: random, made once and kept, and another install's differs.
    fn identity(t: &mut Checks, defaults: &Retained<NSUserDefaults>) {
        let id = errand_macos::identity::install_id_in(defaults);
        let parsed = NSUUID::initWithUUIDString(NSUUID::alloc(), &ns(&id));
        t.check(parsed.is_some() && id == id.to_lowercase(), &format!("An install's ID is a random UUID: {id}"));
        t.check(errand_macos::identity::install_id_in(defaults) == id, "The install keeps its ID");
        let suite = format!("{}/errand-identity-{}", env!("CARGO_TARGET_TMPDIR"), NSUUID::new().UUIDString());
        let other = NSUserDefaults::initWithSuiteName(NSUserDefaults::alloc(), Some(&ns(&suite))).expect("a second defaults suite");
        t.check(errand_macos::identity::install_id_in(&other) != id, "Another install makes an ID of its own");
        other.removePersistentDomainForName(&ns(&suite));
        let _ = std::fs::remove_file(format!("{suite}.plist"));
    }

    fn korean(t: &mut Checks, mtm: MainThreadMarker, defaults: &Retained<NSUserDefaults>) {
        let mut h = Harness::new(mtm, defaults, Language::Korean);
        let panel = h.widget.borrow().panel.clone();
        let details_panel = h.widget.borrow().details_panel.clone();
        let tray_panel = h.widget.borrow().tray_panel.clone();
        let dock = h.dock();

        t.check(panel.level() >= NSFloatingWindowLevel, "The dock floats above normal application windows");
        t.check(!panel.isOpaque() && panel.backgroundColor().alphaComponent() == 0.0, "The dock has a genuinely transparent window around its card");
        t.check(panel.styleMask().contains(NSWindowStyleMask::NonactivatingPanel) && !panel.canBecomeKeyWindow(), "The dock does not steal typing focus");
        t.check(!panel.hidesOnDeactivate(), "The dock stays visible while working in another app");
        let behavior = panel.collectionBehavior();
        t.check(
            behavior.contains(NSWindowCollectionBehavior::CanJoinAllSpaces) && behavior.contains(NSWindowCollectionBehavior::FullScreenAuxiliary),
            "The dock follows Spaces and full-screen desktops",
        );
        h.pump();
        t.check(panel.isVisible() && panel.parentWindow().is_none(), "The dock is visible by default, a window of its own");
        let batteries = of_class(&dock, "ErrandBatteryView");
        t.check(
            batteries.iter().map(|battery| spoken(battery)).collect::<Vec<_>>() == ["Codex 남은 사용량", "Claude 남은 사용량"],
            "The dock holds one battery for Codex and one for Claude",
        );

        let weekly = window("codex/primary", WEEK, 72, Some(WEEKLY_RESET));
        h.send(Msg::UsageRead { provider: Provider::Codex, result: Ok(vec![weekly.clone()].into()) });
        t.check(
            shows(&dock, "72%") && shows(&dock, "주간 · 10/19 09:00 초기화") && number(&h.battery(Provider::Codex)) == Some(72),
            "Codex's battery shows what remains and when its quota resets",
        );
        t.check(marks(&dock) == ["Codex", "Claude"], &format!("Each battery carries its platform's app icon: {:?}", marks(&dock)));

        h.send(Msg::ProfileRead(Profile { name: "한결".to_owned(), avatar: "한결".to_owned() }));
        let nickname = label(&dock, "한결");
        let below_batteries = nickname.as_ref().is_some_and(|nickname| {
            let frame = nickname.convertRect_toView(nickname.bounds(), Some(&*dock));
            let row = unsafe { nickname.superview() }.map(|row| row.convertRect_toView(row.bounds(), Some(&*dock)));
            frame.origin.y > max_y(h.battery(Provider::Claude).frame()) && row.is_some_and(|row| (max_y(row) - dock.bounds().size.height).abs() < 0.5)
        });
        t.check(below_batteries, "The nickname has its own row closing the bottom of the dock, under the batteries");
        let avatar = first(&dock, "ErrandAvatarView");
        t.check(
            avatar.as_ref().is_some_and(|avatar| (avatar.frame().origin.x - h.battery(Provider::Codex).frame().origin.x).abs() < 0.5),
            "The avatar lines up with the battery icons",
        );
        if let Some(avatar) = &avatar {
            click(avatar, NSPoint::new(12.0, 12.0));
        }
        h.pump();
        let editor_panel = h.widget.borrow().avatar_editor_panel.clone();
        t.check(
            editor_panel.isVisible() && !details_panel.isVisible() && h.performed(|effect| *effect == Effect::NewAvatarSeed) == 0,
            "A click on the character opens its editor beside the dock, and the usage details stay closed",
        );
        let editor = editor_panel.contentView().expect("the editor's view");
        let spoken_as = |words: &str| buttons(&editor).into_iter().find(|button| spoken(button) == words);
        let before = Avatar::named(&h.state.profile.avatar);
        if let Some(next) = spoken_as("몸 다음") {
            unsafe { next.performClick(None) };
        }
        h.pump();
        t.check(
            Avatar::named(&h.state.profile.avatar) == before.step(Part::Shape, 1) && h.performed(|effect| matches!(effect, Effect::SaveAvatar(_))) == 1,
            "The arrow beside 몸 turns the character into the next body, and it is kept",
        );
        if let Some(red) = spoken_as("빨강") {
            unsafe { red.performClick(None) };
        }
        h.pump();
        t.check(Avatar::named(&h.state.profile.avatar).clothes == CLOTHES[7], "A colour dot dyes the clothes");
        if let Some(shuffle) = titled(&editor, "무작위") {
            unsafe { shuffle.performClick(None) };
        }
        h.pump();
        t.check(h.performed(|effect| *effect == Effect::NewAvatarSeed) == 1, "무작위 asks for a whole new character");
        let field = labels(&editor).into_iter().find(|field| field.isEditable());
        t.check(
            field.as_ref().is_some_and(|field| field.stringValue().to_string() == "한결" && spoken(field) == "닉네임")
                && editor_panel.canBecomeKeyWindow() && !editor_panel.canBecomeMainWindow(),
            "The editor's nickname field shows the dock's name, and its panel takes typing without becoming Errand's main window",
        );
        if let Some(field) = &field {
            field.setStringValue(&ns(" 야근 요정 "));
            unsafe { field.sendAction_to(field.action(), field.target().as_deref()) };
        }
        h.pump();
        t.check(
            h.performed(|effect| *effect == Effect::SaveNickname(Some("야근 요정".to_owned()))) == 1,
            "Return in the nickname field keeps the name, trimmed",
        );
        if let Some(close) = titled(&editor, "닫기") {
            unsafe { close.performClick(None) };
        }
        h.pump();
        t.check(!editor_panel.isVisible(), "닫기 closes the editor");
        if let Some(avatar) = &avatar {
            avatar.accessibilityPerformPress();
        }
        h.pump();
        let pressed_open = editor_panel.isVisible();
        h.send(Msg::ToggleDetails);
        t.check(pressed_open && details_panel.isVisible() && !editor_panel.isVisible(), "VoiceOver opens the editor too, and opening the details closes it");
        h.send(Msg::ToggleDetails);
        if let Some(nickname) = &nickname {
            click(nickname, NSPoint::new(5.0, 8.0));
        }
        h.pump();
        t.check(details_panel.isVisible(), "The profile row is part of the dock: a click there opens the usage details too");
        h.send(Msg::ToggleDetails);
        let window_point = dock.convertPoint_toView(NSPoint::new(150.0, 20.0), None);
        t.check(
            hit(&panel, window_point).is_some_and(|view| same(&view, &dock)) && dock.acceptsFirstMouse(None),
            "The whole dock is one target that takes the first click from another app",
        );

        let refresh = first(&dock, "ErrandDockButton").and_then(|view| view.downcast::<NSButton>().ok());
        let refreshing = first(&dock, "ErrandSpinningIconView");
        let aimed = refresh.as_ref().and_then(|refresh| hit(&panel, refresh.convertPoint_toView(NSPoint::new(12.0, 12.0), None)));
        if let Some(refresh) = &refresh {
            unsafe { refresh.performClick(None) };
        }
        h.pump();
        t.check(
            refresh.as_ref().is_some_and(|refresh| {
                aimed.as_ref().is_some_and(|aimed| same(aimed, refresh)) && refresh.acceptsFirstMouse(None)
            }) && h.performed(|effect| *effect == Effect::FetchUsage(Provider::Codex)) == 1
                && h.performed(|effect| *effect == Effect::AskClaudeCode) == 1
                && !details_panel.isVisible(),
            "The dock's small refresh button reloads usage on the first click without opening the details",
        );
        t.check(
            refresh.as_ref().is_some_and(|refresh| refresh.isHidden()) && refreshing.as_ref().is_some_and(|spinner| !spinner.isHidden()),
            "While usage reloads the refresh icon turns in place of the button",
        );
        h.send(Msg::UsageRead { provider: Provider::Codex, result: Ok(vec![weekly.clone()].into()) });
        h.send(Msg::UsageRead { provider: Provider::Claude, result: Ok(Vec::new().into()) });
        t.check(
            refresh.as_ref().is_some_and(|refresh| !refresh.isHidden()) && refreshing.as_ref().is_some_and(|spinner| spinner.isHidden()),
            "The button comes back once the reload finishes",
        );

        click(&h.battery(Provider::Codex), NSPoint::new(150.0, 15.0));
        h.pump();
        let details = h.details();
        t.check(details_panel.isVisible() && shows(&details, "Codex") && shows(&details, "Claude"), "Clicking a battery opens the Codex and Claude usage details");
        // The dock's own screen: the main screen follows keyboard focus, which may be on another display.
        let screen = panel.screen().expect("the dock is on a screen").visibleFrame();
        t.check(
            overlap_width(details_panel.frame(), panel.frame()) < 1.0 && inside(screen, details_panel.frame()),
            &format!("The details open beside the dock instead of covering it: dock={:?} details={:?}", panel.frame(), details_panel.frame()),
        );
        t.check(shows(&details, "72% 남음") && shows(&details, "다음 초기화"), "The details include live usage and reset times");
        click(&h.battery(Provider::Codex), NSPoint::new(150.0, 15.0));
        h.pump();
        t.check(!details_panel.isVisible(), "Clicking again closes the details");

        let release = errand_core::update::Release { version: "99.0.0".to_owned(), download: Some("https://example.com/Errand.zip".to_owned()) };
        h.send(Msg::ReleaseRead(Some(release.clone())));
        let update = buttons(&dock).into_iter().find(|button| button.title().to_string() == "새 버전 99.0.0");
        t.check(update.is_some(), "A newer version is offered beside the profile");
        if let Some(update) = &update {
            t.check(
                h.performed(|effect| matches!(effect, Effect::InstallUpdate(_))) == 0 && update.acceptsFirstMouse(None),
                "Finding a release installs nothing, and the offer takes the first click",
            );
            unsafe { update.performClick(None) };
            h.pump();
            t.check(h.performed(|effect| *effect == Effect::InstallUpdate(release.clone())) == 1, "Pressing the offer installs that release");
            h.send(Msg::UpdateFailed);
            h.send(Msg::ReleaseRead(Some(errand_core::update::Release { version: errand_core::update::VERSION.to_owned(), download: None })));
            t.check(update.isHidden(), "The offer goes once nothing newer is out");
        }
        h.send(Msg::UsageRead { provider: Provider::Codex, result: Err(UsageError::CodexClosed) });
        h.send(Msg::ToggleDetails);
        t.check(shows(&details, "이전 값") && shows(&dock, "이전 값 · 10/19 09:00 초기화"), "Stale data is marked in the open details and on the battery");
        let open = |provider: Provider| buttons(&details).into_iter().find(|button| spoken(button) == format!("{} 열기", provider.title()));
        if let Some(button) = open(Provider::Codex) {
            unsafe { button.performClick(None) };
        }
        h.pump();
        t.check(
            h.performed(|effect| *effect == Effect::OpenApp(Provider::Codex)) == 1 && !details_panel.isVisible(),
            "A details button opens its account and closes the details",
        );
        h.send(Msg::ToggleDetails);
        if let Some(button) = open(Provider::Claude) {
            unsafe { button.performClick(None) };
        }
        h.pump();
        t.check(h.performed(|effect| *effect == Effect::OpenApp(Provider::Claude)) == 1, "The Claude card opens its own account");

        let five_hour = window("five_hour", FIVE_HOURS, 64, Some(FIVE_HOUR_RESET));
        h.send(Msg::UsageRead { provider: Provider::Claude, result: Ok(vec![five_hour].into()) });
        t.check(shows(&dock, "64%") && shows(&dock, "5시간 · 10/15 18:00 초기화"), "Claude's battery shows its remaining allowance and reset");
        h.state.usage[Provider::Claude] = Default::default();
        h.send(Msg::UsageRead { provider: Provider::Claude, result: Err(UsageError::ClaudeNotHeard) });
        t.check(shows(&dock, "확인 불가") && shows(&dock, "조회 실패"), "A failed Claude read leaves an empty battery that says so");
        h.send(Msg::ToggleDetails);
        t.check(shows(&details, "새로고침을 누르면 Claude Code에서 받아와요"), "A Claude failure explains what to do inside its details card");
        h.send(Msg::ToggleDetails);
        let earlier = window("codex/primary", FIVE_HOURS, 50, Some(FIVE_HOUR_RESET));
        let limiting = window("codex/secondary", WEEK, 12, Some(WEEKLY_RESET));
        h.send(Msg::UsageRead { provider: Provider::Codex, result: Ok(vec![earlier.clone(), limiting.clone()].into()) });
        let codex = h.battery(Provider::Codex);
        t.check(
            shows(&codex, "12%") && shows(&codex, "10/19 09:00 초기화") && !shows(&codex, "10/15 18:00 초기화"),
            "The battery pairs the lowest allowance with that same quota's reset, not the earliest reset",
        );
        let percent = label(&codex, "12%");
        t.check(
            percent.is_some_and(|percent| percent.textColor().is_some_and(|ink| !same_color(&ink, &kit::color(theme::INK)))) && number(&codex) == Some(12),
            "A battery at 25% or less turns to its low colour",
        );
        let missing_reset = UsageWindow { resets_at: None, ..limiting.clone() };
        h.send(Msg::UsageRead { provider: Provider::Codex, result: Ok(vec![earlier, missing_reset].into()) });
        t.check(shows(&codex, "초기화 확인 불가") && !shows(&codex, "10/15 18:00 초기화"), "A missing reset time never borrows another quota's reset");
        let status = status::battery_image(&[Some(72), None]);
        t.check(status.isTemplate() && status.size().height <= 22.0, "The menu bar shows a small template battery per platform, unknown ones empty");

        for name in errand_core::icons::names() {
            let path = kit::icon_path(name).expect("a vendored icon");
            t.check(
                path.elementCount() > 1 && inside(rect(-0.5, -0.5, 25.0, 25.0), path.bounds()),
                &format!("Lucide icon {name} draws inside its 24-unit grid: {:?}", path.bounds()),
            );
        }
        let refresh_icon = kit::icon_path("refresh-cw").expect("the refresh icon").bounds();
        t.check(
            (refresh_icon.origin.x - 3.0).abs() < 0.5
                && (max_x(refresh_icon) - 21.0).abs() < 0.5
                && (refresh_icon.origin.y - 3.0).abs() < 0.5
                && (max_y(refresh_icon) - 21.0).abs() < 0.5,
            &format!("SVG arcs follow the circle Lucide draws: {refresh_icon:?}"),
        );
        t.check(
            kit::icon_image("check", 12.0, None, 0.0, 0.0).is_some_and(|icon| icon.isTemplate()) && kit::icon_image("no-such-icon", 12.0, None, 0.0, 0.0).is_none(),
            "Icons are template images that controls tint; unknown names draw nothing",
        );
        t.check(
            kit::icon_image("check", 11.0, None, 0.0, 4.0).is_some_and(|icon| icon.size().width == 15.0)
                && kit::icon_image("check", 11.0, None, 0.0, 0.0).is_some_and(|icon| icon.size().width == 11.0),
            "Button icons keep the same gap before their title",
        );

        h.park(NSPoint::new(max_x(screen) - 330.0, screen.origin.y + 20.0), false, mtm);
        let bare_height = panel.frame().size.height;
        h.send(Msg::SessionsScanned(Vec::new()));
        t.check(!tray_panel.isVisible() && !shows(&dock, "내가 시킨 작업"), "Without tasks the dock shows only the batteries");
        let mut readme = row("claude:local_1", Provider::Claude, "README 정리", "README 정리하고 커밋해줘", h.ago(900), Some(h.ago(300)));
        readme.result = Some("README 커밋 완료".to_owned());
        readme.action = Some("푸시 여부 결정".to_owned());
        readme.folder = "docs".to_owned();
        let mut login = row("codex:t-1", Provider::Codex, "로그인 수정", "로그인 버그 고쳐줘", h.ago(240), None);
        login.folder = "app".to_owned();
        let mut deploy = row("codex:t-2", Provider::Codex, "배포 확인", "배포됐는지 봐줘", h.ago(4000), Some(h.ago(3900)));
        deploy.result = Some("배포 완료".to_owned());
        deploy.folder = "app".to_owned();
        deploy.seen_in_app = true;
        let dock_bottom = panel.frame().origin.y;
        h.send(Msg::SessionsScanned(vec![readme.clone(), login.clone(), deploy.clone()]));
        let tray = h.tray();
        t.check(
            tray_panel.isVisible() && tray_panel.level() >= NSFloatingWindowLevel && !tray_panel.canBecomeKeyWindow() && !tray_panel.hidesOnDeactivate(),
            "The task cards float with the dock without taking focus or hiding in other apps",
        );
        t.check(
            panel.frame().size.height > bare_height && panel.frame().origin.y == dock_bottom && shows(&dock, "내가 시킨 작업 3"),
            "The task header joins the top of the dock, which grows upward from where it sits",
        );
        let (dock_frame, tray_frame) = (panel.frame(), tray_panel.frame());
        t.check(
            tray_frame.origin.x == dock_frame.origin.x && tray_frame.size.width == dock_frame.size.width && tray_frame.origin.y >= max_y(dock_frame),
            "The cards are stacked on top of the dock at its width",
        );
        t.check(
            shows(&tray, "README 정리하고 커밋해줘") && shows(&tray, "할 일 · 푸시 여부 결정") && shows(&tray, "작업 중 · 4분째"),
            "Each card shows what was asked, its state and what is left to do",
        );
        t.check(card_order(&tray) == ["codex:t-1", "claude:local_1", "codex:t-2"], &format!("The newest request is on top: {:?}", card_order(&tray)));
        t.check(marks(&tray) == ["Codex", "Claude", "Codex"], &format!("Each card carries its own app's icon: {:?}", marks(&tray)));
        t.check(first(&tray, "ErrandSpinningIconView").is_some(), "A running card has a turning spinner");
        let working = h.card("codex:t-1").and_then(|card| labels(&card).into_iter().find(|label| label.stringValue().to_string().starts_with("작업 중")));
        t.check(
            working.is_some_and(|label| label.textColor().is_some_and(|ink| same_color(&ink, &kit::color(theme::RUNNING)))),
            "A running card shows its elapsed time in the running colour",
        );
        t.check(
            h.card("codex:t-1").is_some_and(|card| titled(&card, "열기").is_none() && titled(&card, "완료").is_none()),
            "Cards have no 열기 or 완료 buttons; the card itself opens and its checkbox completes",
        );

        let card = h.card("claude:local_1").expect("the README card");
        click(&card, NSPoint::new(120.0, 50.0));
        h.pump();
        t.check(
            h.performed(|effect| *effect == Effect::OpenUrl("errand-test://claude:local_1".to_owned())) == 1 && card.acceptsFirstMouse(None),
            "Clicking the text of a card opens its session, even from another app",
        );
        // On the desktop the pointer always rests on the card first, which shows its 열기 hint.
        let card = h.card("claude:local_1").expect("the README card");
        card.mouseEntered(&pointer(NSEventType::MouseEntered, &tray_panel));
        let checkbox = first(&card, "ErrandCheckButton").and_then(|view| view.downcast::<NSButton>().ok()).expect("the card's checkbox");
        let circle = checkbox.convertPoint_toView(NSPoint::new(checkbox.bounds().size.width / 2.0, checkbox.bounds().size.height / 2.0), None);
        t.check(
            hit(&tray_panel, circle).is_some_and(|view| same(&view, &checkbox)),
            "With the pointer on a card, a click on its circle still reaches the checkbox, not the card's open action",
        );
        let opened = h.performed(|effect| matches!(effect, Effect::OpenUrl(_)));
        unsafe { checkbox.performClick(None) };
        h.pump();
        let card = h.card("claude:local_1");
        t.check(
            h.performed(|effect| matches!(effect, Effect::OpenUrl(_))) == opened
                && card.as_ref().is_some_and(|card| titled(card, "되돌리기").is_some())
                && shows(&dock, "내가 시킨 작업 2"),
            "Checking a card marks it done and offers 되돌리기 before anything is removed",
        );
        t.check(first(&tray, "ErrandCountdownBar").is_some(), "A checked card drains a countdown so the wait reads as about to clear");
        h.advance(SignedDuration::from_secs(5));
        h.spin(0.6);
        let completed = |h: &Harness, key: &str| h.state.sessions.records.entries().any(|(own, record)| own == key && record.completed_at.is_some());
        t.check(completed(&h, "claude:local_1"), "After the undo window the checked card is completed");
        t.check(
            tray_panel.isVisible() && h.card("claude:local_1").is_none() && !stored_preferences(defaults).collapsed,
            "The completed card leaves the dock on its own, without folding and unfolding",
        );
        if let Some(check) = h.card("codex:t-2").and_then(|card| first(&card, "ErrandCheckButton")).and_then(|view| view.downcast::<NSButton>().ok()) {
            unsafe { check.performClick(None) };
        }
        h.pump();
        if let Some(undo) = h.card("codex:t-2").and_then(|card| titled(&card, "되돌리기")) {
            unsafe { undo.performClick(None) };
        }
        h.pump();
        h.advance(SignedDuration::from_secs(5));
        h.spin(0.6);
        t.check(
            !completed(&h, "codex:t-2") && h.card("codex:t-2").is_some_and(|card| titled(&card, "되돌리기").is_none()) && shows(&dock, "내가 시킨 작업 2"),
            "되돌리기 takes the check back and nothing is completed",
        );

        let before = card_order(&tray);
        tray.mouseEntered(&pointer(NSEventType::MouseEntered, &tray_panel));
        h.pump();
        let mut finished = login.clone();
        finished.running = false;
        finished.finished_at = Some(h.now);
        finished.result = Some("로그인 고쳤어요".to_owned());
        let mut arrival = row("codex:t-new", Provider::Codex, "새 요청", "이것도 해줘", h.now, None);
        arrival.folder = "app".to_owned();
        let scan = vec![arrival, finished, deploy.clone()];
        h.send(Msg::SessionsScanned(scan.clone()));
        t.check(
            card_order(&tray) == before && shows(&tray, "로그인 고쳤어요") && h.card("codex:t-new").is_none(),
            &format!("With the pointer on the cards, a finished card updates in place and a new card waits: {:?}", card_order(&tray)),
        );
        tray.mouseExited(&pointer(NSEventType::MouseExited, &tray_panel));
        h.pump();
        t.check(
            card_order(&tray) == ["codex:t-new", "codex:t-1", "codex:t-2"],
            &format!("Once the pointer leaves, the new request takes the top: {:?}", card_order(&tray)),
        );
        let header = first(&dock, "ErrandPressableView").expect("the task header");
        let pills: Vec<String> = of_class(&header, "ErrandCountPill").iter().map(|pill| tooltip(pill)).collect();
        t.check(pills == ["확인할 것 1개", "진행 중 1개", "확인함 1개"], &format!("The header counts new, running and seen cards, adding up to the stack: {pills:?}"));
        click(&dock, NSPoint::new(40.0, 18.0));
        h.pump();
        t.check(
            !tray_panel.isVisible() && spoken(&header).contains("펼치기") && stored_preferences(defaults).collapsed && !details_panel.isVisible(),
            "Clicking the header folds the cards but keeps the task count on the dock",
        );
        click(&dock, NSPoint::new(40.0, 18.0));
        h.pump();
        t.check(tray_panel.isVisible() && spoken(&header).contains("접기"), "Clicking it again brings the cards back");

        let many: Vec<SessionRow> = (0..9).map(|index| row(&format!("codex:many-{index}"), Provider::Codex, "작업", "요청", h.now, None)).collect();
        h.send(Msg::SessionsScanned(many));
        t.check(tray_panel.frame().size.height <= 560.0 && shows(&dock, "내가 시킨 작업 9"), "Many sessions scroll inside bounded cards instead of covering the desktop");
        let more = first(&tray, "ErrandMoreBar");
        t.check(
            more.as_ref().is_some_and(|more| shows(more, "개 더 있어요")) && none_sliced(&tray),
            "When cards still overflow, only whole cards show and a bar says how many are below",
        );
        let hidden_before = more.as_ref().map(|more| labels(more).iter().map(|label| label.stringValue().to_string()).collect::<Vec<_>>());
        if let Some(more) = &more {
            more.accessibilityPerformPress();
        }
        h.spin(0.5);
        let hidden_after = more.as_ref().map(|more| labels(more).iter().map(|label| label.stringValue().to_string()).collect::<Vec<_>>());
        t.check(none_sliced(&tray) && hidden_after != hidden_before, "The more bar scrolls the next card fully into view and counts what is left");

        // A session with work it started: one card, a line for each.
        let side = |mut session: SessionRow| {
            session.parent = Some("claude:local_p".to_owned());
            session
        };
        let mut parent = row("claude:local_p", Provider::Claude, "작업방 버튼 배치", "버튼 옮겨줘", h.ago(900), Some(h.ago(800)));
        parent.seen_in_app = true;
        let mut read = side(row("claude:local_read", Provider::Claude, "설명서 고치기", "설명서 고쳐줘", h.ago(700), Some(h.ago(600))));
        read.seen_in_app = true;
        let new = side(row("claude:local_new", Provider::Claude, "탭 스크롤 구현", "탭 스크롤 넣어줘", h.ago(500), Some(h.ago(60))));
        let running = side(row("claude:local_run", Provider::Claude, "탭 넘침 고치기", "넘침 고쳐줘", h.ago(300), None));
        h.send(Msg::SessionsScanned(vec![parent, read, new, running]));
        let family = h.card("claude:local_p").expect("the card over the sub-tasks");
        let lines: Vec<String> = of_class(&family, "ErrandSubTaskRow").iter().filter_map(|line| line.identifier()).map(|key| key.to_string()).collect();
        t.check(
            card_order(&tray) == ["claude:local_p"] && lines == ["claude:local_run", "claude:local_new", "claude:local_read"] && shows(&dock, "내가 시킨 작업 4"),
            &format!("Sub-tasks are lines in the card they came from, newest first, and the header counts them all: {:?} {lines:?}", card_order(&tray)),
        );
        let line = h.line("claude:local_run").expect("a sub-task's line");
        t.check(
            family.frame().size.height == 118.0 + 3.0 * 32.0 + 4.0 && line.frame().origin.y == 118.0 && line.frame().size.height == 32.0,
            "The card grows by its lines, which start under its own part",
        );
        t.check(
            shows(&line, "탭 넘침 고치기") && first(&line, "ErrandSpinningIconView").is_some() && icons(&line).contains(&"corner-down-right".to_owned()),
            "A line shows the sub-task's title and state beside an arrow from the card",
        );
        t.check(
            spoken(&line).starts_with("하위 작업, Claude, 탭 넘침 고치기, 진행 중") && line.isAccessibilityElement() && tooltip(&line).contains("클릭해서 열기"),
            "VoiceOver reads a line as a sub-task, and hovering says it opens",
        );

        let hint_hidden = |card: &NSView| label(card, "열기").is_none_or(|words| words.isHiddenOrHasHiddenAncestor());
        let own_part = family.convertPoint_toView(NSPoint::new(120.0, 50.0), None);
        family.mouseEntered(&pointer_at(NSEventType::MouseEntered, &tray_panel, own_part));
        t.check(!hint_hidden(&family), "The pointer on the card's own part shows its 열기 hint");
        line.mouseEntered(&pointer(NSEventType::MouseEntered, &tray_panel));
        t.check(hint_hidden(&family), "On a line, only the line lights up, not the card around it");
        line.mouseExited(&pointer(NSEventType::MouseExited, &tray_panel));
        family.mouseExited(&pointer(NSEventType::MouseExited, &tray_panel));
        let on_line = family.convertPoint_toView(NSPoint::new(120.0, 130.0), None);
        family.mouseEntered(&pointer_at(NSEventType::MouseEntered, &tray_panel, on_line));
        t.check(hint_hidden(&family), "Coming onto the card at a line lights up the line alone");
        family.mouseExited(&pointer(NSEventType::MouseExited, &tray_panel));

        let opens = |h: &Harness, key: &str| h.performed(|effect| *effect == Effect::OpenUrl(format!("errand-test://{key}")));
        click(&line, NSPoint::new(120.0, 16.0));
        h.pump();
        t.check(opens(&h, "claude:local_run") == 1 && opens(&h, "claude:local_p") == 0, "Clicking a line opens that session, not the card's");
        let family = h.card("claude:local_p").expect("the card over the sub-tasks");
        click(&family, NSPoint::new(120.0, 50.0));
        h.pump();
        t.check(opens(&h, "claude:local_p") == 1 && opens(&h, "claude:local_run") == 1, "Clicking the card's own part still opens the card's session");

        let line = h.line("claude:local_new").expect("a sub-task's line");
        let circle = first(&line, "ErrandCheckButton").and_then(|view| view.downcast::<NSButton>().ok()).expect("a line's own circle");
        let middle = circle.convertPoint_toView(NSPoint::new(circle.bounds().size.width / 2.0, circle.bounds().size.height / 2.0), None);
        t.check(hit(&tray_panel, middle).is_some_and(|view| same(&view, &circle)), "A click on a line's circle reaches it, not the line or the card");
        unsafe { circle.performClick(None) };
        h.pump();
        t.check(
            h.line("claude:local_new").is_some_and(|line| titled(&line, "되돌리기").is_some() && first(&line, "ErrandCountdownBar").is_some())
                && h.line("claude:local_run").is_some_and(|line| titled(&line, "되돌리기").is_none())
                && h.state.sessions.pending.keys().eq(["claude:local_new"].iter().map(|key| key.to_string()).collect::<Vec<_>>().iter()),
            "A line's circle checks that line alone and offers its own 되돌리기 and countdown",
        );
        h.advance(SignedDuration::from_secs(5));
        h.spin(0.6);
        t.check(
            completed(&h, "claude:local_new") && h.line("claude:local_new").is_none() && h.card("claude:local_p").is_some_and(|card| card.frame().size.height == 118.0 + 2.0 * 32.0 + 4.0),
            "After its wait a checked line clears and the card closes up",
        );
        if let Some(circle) = h.card("claude:local_p").and_then(|card| first(&card, "ErrandCheckButton")).and_then(|view| view.downcast::<NSButton>().ok()) {
            unsafe { circle.performClick(None) };
        }
        h.pump();
        h.advance(SignedDuration::from_secs(5));
        h.spin(0.6);
        t.check(
            completed(&h, "claude:local_p") && completed(&h, "claude:local_read") && !completed(&h, "claude:local_run") && card_order(&tray) == ["claude:local_run"],
            &format!("Checking the card clears the lines already read; the rest become cards: {:?}", card_order(&tray)),
        );

        // Work waiting on the person's answer.
        let mut waiting = row("claude:local_q", Provider::Claude, "카드 시안", "시안 보여줘", h.ago(200), None);
        waiting.question = Some(Question { id: "toolu_q".to_owned(), asked_at: h.ago(120), questions: vec!["어느 안으로 할까요?".to_owned()] });
        h.send(Msg::SessionsScanned(vec![waiting]));
        let header = first(&dock, "ErrandPressableView").expect("the task header");
        let pills: Vec<String> = of_class(&header, "ErrandCountPill").iter().map(|pill| tooltip(pill)).collect();
        let card = h.card("claude:local_q");
        t.check(
            card.as_ref().is_some_and(|card| {
                shows(card, "질문 · 어느 안으로 할까요?") && shows(card, "2분 전 질문") && spoken(card).contains("답변 필요") && first(card, "ErrandSpinningIconView").is_none()
            }) && pills == ["답변 필요 1개"],
            &format!("A card waiting on a question says what it asks, and the header counts it apart: {pills:?}"),
        );
        let mut waiting = row("claude:local_q", Provider::Claude, "카드 시안", "시안 보여줘", h.ago(200), None);
        waiting.question = Some(Question { id: "toolu_q".to_owned(), asked_at: h.ago(120), questions: vec!["어느 안으로 할까요?".to_owned()] });
        let mut answered = row("codex:q-new", Provider::Codex, "새 결과", "고쳐줘", h.ago(300), Some(h.ago(60)));
        answered.result = Some("고쳤어요".to_owned());
        let mut read = row("codex:q-seen", Provider::Codex, "확인함", "봐줘", h.ago(400), Some(h.ago(350)));
        read.seen_in_app = true;
        h.send(Msg::SessionsScanned(vec![waiting, answered, row("codex:q-run", Provider::Codex, "진행 중", "해줘", h.ago(100), None), read]));
        let header = first(&dock, "ErrandPressableView").expect("the task header");
        let shown_pills = of_class(&header, "ErrandCountPill");
        let pills: Vec<String> = shown_pills.iter().map(|pill| tooltip(pill)).collect();
        let clear = shown_pills.iter().all(|pill| pill.frame().origin.x + pill.frame().size.width <= header.frame().size.width - 31.0);
        t.check(
            clear && pills.starts_with(&["답변 필요 1개".to_owned(), "확인할 것 1개".to_owned(), "진행 중 1개".to_owned()]) && spoken(&header).contains("확인함 1개"),
            &format!("Counts that would run under the chevron give way, the least pressing first, and VoiceOver still hears them: {pills:?}"),
        );

        let crowd: Vec<SessionRow> = (0..5)
            .map(|index| {
                let asked = h.ago(60 * index);
                let mut session = row(&format!("codex:crowd-{index}"), Provider::Codex, "작업", &format!("요청 {index}"), asked, (index > 0).then_some(h.now));
                session.result = Some("했어요".to_owned());
                session.folder = "app".to_owned();
                session.seen_in_app = index > 0;
                session
            })
            .collect();
        h.send(Msg::SessionsScanned(crowd));
        let (oldest, newest) = (h.card("codex:crowd-4"), h.card("codex:crowd-0"));
        t.check(
            first(&tray, "ErrandMoreBar").is_none()
                && none_sliced(&tray)
                && oldest.as_ref().zip(newest.as_ref()).is_some_and(|(oldest, newest)| oldest.frame().size.height < newest.frame().size.height)
                && oldest.as_ref().is_some_and(|oldest| shows(oldest, "요청 4")),
            "Five tasks fit by folding the oldest cards, which still show what was asked",
        );
        h.park(NSPoint::new(screen.origin.x + 10.0, max_y(screen) - panel.frame().size.height), true, mtm);
        t.check(
            max_y(tray_panel.frame()) <= panel.frame().origin.y && inside(screen, panel.frame()),
            "Parked at the top of the screen, the cards hang below the dock",
        );
        h.send(Msg::SetWidgetHidden(true));
        t.check(!panel.isVisible() && !tray_panel.isVisible() && !details_panel.isVisible(), "Hiding the dock also hides its cards and details");
        t.check(stored_preferences(defaults).hidden, "The hidden preference is saved for the next launch");
        h.send(Msg::SetWidgetHidden(false));
        t.check(panel.isVisible() && tray_panel.isVisible() && !stored_preferences(defaults).hidden, "Showing the dock brings its cards back");

        h.park(NSPoint::new(screen.origin.x + 90.0, screen.origin.y + 80.0), true, mtm);
        let expected = panel.frame().origin;
        let restored = Widget::new(mtm, Arc::new(Inbox::default()), defaults.clone(), Language::Korean);
        let origin = restored.borrow().panel.frame().origin;
        t.check((origin.x - expected.x).abs() < 0.5 && (origin.y - expected.y).abs() < 0.5, "The dragged position survives recreation");

        h.send(Msg::SetWidgetHidden(true));
    }

    /// Outside a room a capsule by the nickname opens the list of rooms beside the dock, as the details open, and
    /// the menu does too; in a room a plus after the tabs takes its place. Each room the person is in gets a tab
    /// under the profile, a lock before a private room's name and how many are in it after; the open tab shows its
    /// people in rows as thin as the folded task header, the dock growing upward to hold them, and a click on it
    /// folds them away.
    fn rooms(t: &mut Checks, mtm: MainThreadMarker, defaults: &Retained<NSUserDefaults>) {
        let mut h = Harness::new(mtm, defaults, Language::Korean);
        let (panel, rooms_panel, details_panel) = {
            let widget = h.widget.borrow();
            (widget.panel.clone(), widget.rooms_panel.clone(), widget.details_panel.clone())
        };
        let dock = h.dock();
        let capsules = || {
            of_class(&dock, "ErrandRoomTab")
                .into_iter()
                .filter(|tab| !tab.isHiddenOrHasHiddenAncestor())
                .filter_map(|tab| tab.downcast::<NSButton>().ok())
                .collect::<Vec<_>>()
        };
        let tabs = || capsules().into_iter().filter(|capsule| spoken(capsule) != "작업방").collect::<Vec<_>>();
        let way_to_rooms = || capsules().into_iter().find(|capsule| spoken(capsule) == "작업방").expect("a way to the rooms");
        let rows = || of_class(&dock, "ErrandMemberRow").into_iter().filter(|row| !row.isHiddenOrHasHiddenAncestor()).collect::<Vec<_>>();
        let center = |view: &NSView| view.convertPoint_toView(NSPoint::new(view.bounds().size.width / 2.0, view.bounds().size.height / 2.0), None);
        h.send(Msg::SetWidgetHidden(false));
        h.send(Msg::ProfileRead(Profile { name: "한결".to_owned(), avatar: "한결".to_owned() }));
        let bare = panel.frame();
        let capsule = way_to_rooms();
        t.check(tabs().is_empty() && rows().is_empty() && shows(&capsule, "작업방") && icons(&capsule).is_empty(), "Outside a room the dock has no tabs, and the rooms capsule by the nickname");
        let capsule_frame = capsule.convertRect_toView(capsule.bounds(), Some(&dock));
        let nickname = label(&dock, "한결").expect("the nickname");
        let nickname_frame = nickname.convertRect_toView(nickname.bounds(), Some(&dock));
        t.check(
            (max_x(capsule_frame) - (STACK_WIDTH - DOCK_PADDING)).abs() < 0.5 && max_x(nickname_frame) <= capsule_frame.origin.x,
            &format!("The capsule ends the profile row, and the nickname stops short of it: {nickname_frame:?} {capsule_frame:?}"),
        );
        t.check(hit(&panel, center(&capsule)).is_some_and(|view| same(&view, &capsule)), "A click on the capsule reaches it, not the dock");
        h.send(Msg::ToggleDetails);
        unsafe { capsule.performClick(None) };
        h.pump();
        t.check(
            rooms_panel.isVisible() && !details_panel.isVisible() && overlap_width(rooms_panel.frame(), panel.frame()) < 1.0,
            &format!("The capsule opens the list beside the dock, in place of the details: {:?}", rooms_panel.frame()),
        );
        t.check(capsule.isAccessibilitySelected(), "The capsule reads pressed while its list is open");
        unsafe { capsule.performClick(None) };
        h.pump();
        t.check(!rooms_panel.isVisible() && !capsule.isAccessibilitySelected(), "A second click closes the list");

        h.send(Msg::ToggleDetails);
        h.send(Msg::ShowRooms);
        let bubble = rooms_panel.frame();
        t.check(
            rooms_panel.isVisible() && !details_panel.isVisible() && overlap_width(bubble, panel.frame()) < 1.0,
            &format!("The menu opens the list beside the dock, in place of the details: {bubble:?}"),
        );
        let summary = |id: &str, name: &str, visibility| RoomSummary {
            id: id.to_owned(),
            name: name.to_owned(),
            tagline: "불 꺼진 사무실".to_owned(),
            visibility,
            members: 3,
            working: 2,
            faces: vec![Face { name: "바쁜 너구리".to_owned(), avatar: Some(Avatar::from_seed("바쁜 너구리").code()) }],
            role: None,
        };
        h.send(Msg::RoomsRead(Ok(vec![summary("late-night", "고독한 야근방", Visibility::Public), summary("team", "우리 팀 야근방", Visibility::Private)])));
        let list = rooms_panel.contentView().expect("the list's view");
        t.check(
            of_class(&list, "ErrandRoomCard").len() == 2 && of_class(&list, "ErrandMemberRow").is_empty() && titled(&list, "들어가기").is_some(),
            "The list shows each room on a card with a way in, and nobody's details",
        );
        unsafe { titled(&list, "들어가기").expect("a way in").performClick(None) };
        h.pump();
        t.check(
            h.performed(|effect| matches!(effect, Effect::SyncRoom { room, visibility: Visibility::Public } if room == "late-night")) == 1,
            "Joining a public room sends it the anonymous report",
        );
        let tasks = |new, running, seen| TaskCounts { new, running, seen };
        let now = h.now;
        // Everyone's own character, but for someone who sent none; and when each came in, but for that one too.
        let character = |name: &str| (name != "조용한 부엉이").then(|| Avatar::from_seed(name).code());
        let came_in = |name: &str| match name {
            "졸린 수달" => Some(now - SignedDuration::from_secs(42)),
            "바쁜 너구리" => Some(now - SignedDuration::from_secs(3723)),
            _ => None,
        };
        let member = |name: &str, tasks: TaskCounts, me: bool| Member {
            id: name.to_owned(),
            name: name.to_owned(),
            avatar: character(name),
            tasks,
            last_active: now,
            joined_at: came_in(name),
            me,
        };
        let room = |id: &str, name: &str, visibility| Room {
            id: id.to_owned(),
            name: name.to_owned(),
            tagline: "불 꺼진 사무실".to_owned(),
            visibility,
            members: vec![member("졸린 수달", tasks(1, 1, 2), true), member("바쁜 너구리", tasks(0, 2, 0), false), member("조용한 부엉이", tasks(0, 0, 1), false)],
            role: None,
        };
        h.send(Msg::RoomRead("late-night".to_owned(), Ok(room("late-night", "고독한 야근방", Visibility::Public))));
        let joined = tabs();
        t.check(
            joined.len() == 1 && spoken(&joined[0]) == "고독한 야근방, 익명 참여, 3명" && shows(&joined[0], "3") && icons(&joined[0]) == ["users"],
            &format!("The room joined gets a tab under the profile, the people icon and how many are in it: {:?}", joined.iter().map(|tab| (spoken(tab), icons(tab))).collect::<Vec<_>>()),
        );
        let plus = way_to_rooms();
        t.check(
            capsule.isHiddenOrHasHiddenAncestor() && icons(&plus) == ["plus"] && (plus.frame().origin.x - (max_x(joined[0].frame()) + 6.0)).abs() < 0.5,
            &format!("In a room the capsule gives way to a plus right after the last tab: {:?} then {:?}", joined[0].frame(), plus.frame()),
        );
        t.check(plus.isAccessibilitySelected(), "The plus reads pressed while the list is open");
        let people = rows();
        t.check(people.len() == 3 && shows(&dock, "졸린 수달") && shows(&dock, "나"), "Its tab opens on joining, a row per person, the person first");
        // Left to right, as they read.
        let pill_icons = |row: &NSView| {
            let mut pills = of_class(row, "ErrandCountPill");
            pills.sort_by(|a, b| a.frame().origin.x.total_cmp(&b.frame().origin.x));
            pills.iter().flat_map(|pill| icons(pill)).collect::<Vec<_>>()
        };
        t.check(
            people.iter().all(|row| row.frame().size.height <= 26.0)
                && pill_icons(&people[0]) == ["circle-check", "check-check"]
                && of_class(&people[0], "ErrandCountPill").len() == 3
                && pill_icons(&people[2]) == ["check-check"],
            &format!("A row says no more than the folded task header: answers to check, running and opened, in its pills: {:?}", pill_icons(&people[0])),
        );
        // How long each has been in the room, worked out here from when they came in: in one column at the rows'
        // right ends, past the pills, so the pills still line up.
        let time = |row: &NSView, text: &str| labels(row).into_iter().find(|label| label.stringValue().to_string() == text);
        let (mine, busy) = (time(&people[0], "0:42"), time(&people[1], "1:02:03"));
        let column = |label: &Option<Retained<NSTextField>>| label.as_ref().map(|label| (label.frame().origin.x, label.frame().size.width));
        t.check(
            mine.is_some() && busy.is_some() && column(&mine) == column(&busy) && labels(&people[2]).iter().all(|label| !label.stringValue().to_string().contains(':')),
            &format!("Each row ends with how long they have been in the room, in one column; someone the room gives no time for has none: {:?}", (column(&mine), column(&busy))),
        );
        // The widest row: the person's name and 나, three pills and the column, left to right without overlapping.
        let parts: Vec<NSRect> = {
            let named = |text: &str| labels(&people[0]).into_iter().find(|label| label.stringValue().to_string() == text).map(|label| label.frame());
            let mut pills: Vec<NSRect> = of_class(&people[0], "ErrandCountPill").iter().map(|pill| pill.frame()).collect();
            pills.sort_by(|a, b| a.origin.x.total_cmp(&b.origin.x));
            [named("졸린 수달"), named("나")].into_iter().flatten().chain(pills).chain(mine.as_ref().map(|label| label.frame())).collect()
        };
        let row_width = people[0].frame().size.width;
        t.check(
            parts.len() == 6
                && parts.windows(2).all(|pair| pair[0].origin.x + pair[0].size.width <= pair[1].origin.x + 0.5)
                && parts.iter().all(|part| part.origin.x >= 0.0 && part.origin.x + part.size.width <= row_width && part.size.height <= 26.0),
            &format!("A row with three pills and an hour-long time still fits its {row_width}pt: {parts:?}"),
        );
        let ticks = |h: &Harness| h.performed(|effect| *effect == Effect::TickRoomClock);
        let started = ticks(&h);
        h.advance(SignedDuration::from_secs(1));
        let counted = rows();
        t.check(
            started == 1
                && ticks(&h) == 2
                && same(&counted[0], &people[0])
                && time(&counted[0], "0:43").is_some_and(|label| same(&label, mine.as_ref().unwrap()))
                && time(&counted[1], "1:02:04").is_some(),
            &format!("A second later the times count up in place, the rows as they were, with no request: {started} then {} ticks", ticks(&h)),
        );
        t.check(titled(&list, "나가기").is_some(), "The list offers a way out of the room the person is in");
        let grown = panel.frame();
        t.check(
            grown.size.height > bare.size.height + 100.0 && (grown.origin.y - bare.origin.y).abs() < 0.5,
            &format!("The dock grows upward to hold the rows, its bottom edge kept: {bare:?} → {grown:?}"),
        );

        unsafe { titled(&list, "들어가기").expect("a way into the team's room").performClick(None) };
        h.pump();
        t.check(
            h.performed(|effect| matches!(effect, Effect::SyncRoom { room, visibility: Visibility::Private } if room == "team")) == 1,
            "Joining a private room as well keeps the first, and the private one hears the profile",
        );
        h.send(Msg::RoomRead("team".to_owned(), Ok(room("team", "우리 팀 야근방", Visibility::Private))));
        let both = tabs();
        t.check(
            both.len() == 2 && icons(&both[1]) == ["lock", "users"] && spoken(&both[1]) == "우리 팀 야근방, 프로필 공개, 3명",
            &format!("A private room's tab has a lock before its name: {:?}", both.iter().map(|tab| icons(tab)).collect::<Vec<_>>()),
        );
        h.send(Msg::CloseRooms);
        t.check(!rooms_panel.isVisible(), "The list closes");

        let tab = tabs().remove(1);
        t.check(hit(&panel, center(&tab)).is_some_and(|view| same(&view, &tab)), "A click on a tab reaches it, not the dock");
        unsafe { tab.performClick(None) };
        h.pump();
        t.check(rows().is_empty() && tabs().len() == 2, "A click on the open tab folds its people away");
        let before = ticks(&h);
        h.advance(SignedDuration::from_secs(1));
        h.advance(SignedDuration::from_secs(1));
        t.check(ticks(&h) == before, &format!("With no one's time showing, the clock stops: {before} then {} ticks", ticks(&h)));
        let folded = panel.frame();
        t.check(
            folded.size.height < grown.size.height && folded.size.height > bare.size.height,
            &format!("Folded, the dock keeps only the tabs: {folded:?}"),
        );
        let row = unsafe { plus.superview() }.expect("the tabs' row").frame();
        click(&dock, NSPoint::new(row.origin.x + row.size.width - 6.0, row.origin.y + row.size.height / 2.0));
        h.pump();
        t.check(!details_panel.isVisible() && !rooms_panel.isVisible(), "A click beside the tabs is the rooms', not the usage details'");
        unsafe { tabs().remove(0).performClick(None) };
        h.pump();
        t.check(rows().len() == 3, "Another tab opens its own room's people");

        h.send(Msg::LeaveRoom("team".to_owned()));
        h.send(Msg::LeaveRoom("late-night".to_owned()));
        t.check(tabs().is_empty() && panel.frame().size.height == bare.size.height, "Leaving every room takes the tabs off the dock");
        t.check(!capsule.isHiddenOrHasHiddenAncestor() && plus.isHiddenOrHasHiddenAncestor(), "and the capsule comes back beside the nickname");
        h.send(Msg::SetWidgetHidden(true));
    }

    /// More rooms than the dock is wide: the tabs keep their whole names and move along beside the plus, which stays
    /// where it is. Past three rooms the list shuts its ways in, and rooms an older Errand joined stay.
    fn private_rooms(t: &mut Checks, mtm: MainThreadMarker, defaults: &Retained<NSUserDefaults>) {
        let mut h = Harness::new(mtm, defaults, Language::Korean);
        let rooms_panel = h.widget.borrow().rooms_panel.clone();
        h.send(Msg::SetWidgetHidden(false));
        h.send(Msg::ProfileRead(Profile { name: "한결".to_owned(), avatar: "한결".to_owned() }));
        h.send(Msg::ShowRooms);
        let owned = RoomSummary {
            id: "p-own".to_owned(),
            name: "내 방".to_owned(),
            tagline: String::new(),
            visibility: Visibility::Private,
            members: 2,
            working: 0,
            faces: Vec::new(),
            role: Some(Role::Owner),
        };
        h.send(Msg::RoomsRead(Ok(vec![owned])));
        let content = || rooms_panel.contentView().expect("the rooms' view");
        let spoken_button = |words: &str| buttons(&content()).into_iter().find(|button| spoken(button) == words);

        // The form's own buttons keep working after the part under its fields is laid out again.
        h.send(Msg::ShowCreateRoom);
        let field = every(&content())
            .into_iter()
            .filter_map(|view| view.downcast::<NSTextField>().ok())
            .find(|field| field.isEditable() && field.placeholderString().is_some_and(|words| words.to_string() == "예: 우리 팀 야근방"))
            .expect("the name's field");
        let submit = || titled(&content(), "만들고 초대 링크 받기").expect("the form's button");
        t.check(!submit().isEnabled(), "Without a name the form's button waits");
        // As typing would: the text changes and the field tells its delegate.
        field.setStringValue(&ns("우리 팀"));
        unsafe {
            objc2_foundation::NSNotificationCenter::defaultCenter().postNotificationName_object(objc2_app_kit::NSControlTextDidChangeNotification, Some(&field))
        };
        t.check(submit().isEnabled() && label(&content(), "4/30").is_some(), "Typing a name counts it and turns the button on");
        unsafe { submit().performClick(None) };
        h.pump();
        t.check(h.performed(|effect| matches!(effect, Effect::CreateRoom { name, .. } if name == "우리 팀")) == 1, "The form sends what was typed");
        h.send(Msg::RoomCreated(Err(errand_core::rooms::RoomError::Unreachable)));
        t.check(label(&content(), "작업방 서버에 연결하지 못했어요.").is_some(), "Why it failed shows under the fields");
        unsafe { spoken_button("뒤로").expect("a way back").performClick(None) };
        h.pump();
        t.check(h.state.rooms.screen == errand_core::rooms::RoomsScreen::List, "Back still goes back once the form has changed");
        h.send(Msg::ShowInviteCode);
        h.send(Msg::CheckInvite("K7QF".to_owned()));
        unsafe { spoken_button("닫기").expect("a way to close").performClick(None) };
        h.pump();
        t.check(!h.state.rooms.open && !rooms_panel.isVisible(), "Close still closes once the code's form has changed");

        // The owner's right-click on someone's row opens a menu that turns them out, though the dock takes clicks.
        h.send(Msg::JoinRoom("p-own".to_owned()));
        let someone = |name: &str, me: bool| Member {
            id: format!("id-{name}"),
            name: name.to_owned(),
            avatar: None,
            tasks: TaskCounts::default(),
            last_active: h.now,
            joined_at: None,
            me,
        };
        let room = Room {
            id: "p-own".to_owned(),
            name: "내 방".to_owned(),
            tagline: String::new(),
            visibility: Visibility::Private,
            members: vec![someone("한결", true), someone("민지", false)],
            role: Some(Role::Owner),
        };
        h.send(Msg::RoomRead("p-own".to_owned(), Ok(room)));
        let dock = h.dock();
        let rows: Vec<Retained<NSView>> = of_class(&dock, "ErrandMemberRow").into_iter().filter(|row| !row.isHiddenOrHasHiddenAncestor()).collect();
        let menu_at = |row: &NSView| -> Option<Retained<objc2_app_kit::NSMenu>> {
            let window = row.window().expect("the row is in a window");
            let at = row.convertPoint_toView(NSPoint::new(row.bounds().size.width / 2.0, row.bounds().size.height / 2.0), None);
            let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                NSEventType::RightMouseDown,
                at,
                NSEventModifierFlags::empty(),
                0.0,
                window.windowNumber(),
                None,
                1,
                1,
                1.0,
            )
            .expect("a right-click");
            let target = hit(&window, at).expect("something under the click");
            unsafe { msg_send![&*target, menuForEvent: &*event] }
        };
        let own = menu_at(&rows[0]).map(|menu| (0..menu.numberOfItems()).filter_map(|index| menu.itemAtIndex(index)).map(|item| item.title().to_string()).collect::<Vec<_>>());
        t.check(
            rows.len() == 2 && own.as_ref().is_some_and(|titles| !titles.iter().any(|title| title == "내보내기")),
            &format!("The owner's own row offers no turning out, only the widget's own menu: {own:?}"),
        );
        let menu = menu_at(&rows[1]);
        t.check(
            menu.as_ref().is_some_and(|menu| menu.itemAtIndex(0).is_some_and(|item| item.title().to_string() == "내보내기")),
            "Someone else's row offers to turn them out",
        );
        if let Some(menu) = menu {
            menu.performActionForItemAtIndex(0);
            h.pump();
        }
        t.check(
            h.performed(|effect| matches!(effect, Effect::Confirm(errand_core::model::Confirm::RemoveMember { name, .. }) if name == "민지")) == 1,
            "Turning someone out asks first",
        );

        // A link brings a hidden widget back on the room it invites to.
        h.send(Msg::SetWidgetHidden(true));
        h.send(Msg::OpenLink("errand://invite/K7QF-M2XA-9P".to_owned()));
        t.check(!h.state.widget.hidden && rooms_panel.isVisible(), "An invite link shows the widget and the room");
    }

    fn room_tabs(t: &mut Checks, mtm: MainThreadMarker, defaults: &Retained<NSUserDefaults>) {
        let mut h = Harness::new(mtm, defaults, Language::Korean);
        let (panel, rooms_panel) = {
            let widget = h.widget.borrow();
            (widget.panel.clone(), widget.rooms_panel.clone())
        };
        let dock = h.dock();
        h.send(Msg::SetWidgetHidden(false));
        let summary = |id: &str, name: &str| RoomSummary {
            id: id.to_owned(),
            name: name.to_owned(),
            tagline: String::new(),
            visibility: Visibility::Public,
            members: 3,
            working: 1,
            faces: Vec::new(),
            role: None,
        };
        let room = |id: &str, name: &str| Room { id: id.to_owned(), name: name.to_owned(), tagline: String::new(), visibility: Visibility::Public, members: Vec::new(), role: None };
        let names = [("late-night", "고독한 야근방"), ("team", "우리 팀 야근방"), ("dawn", "새벽 코딩방"), ("deadline", "마감 직전방"), ("quiet", "조용히 집중방")];
        h.send(Msg::RoomsRead(Ok(names[..4].iter().map(|(id, name)| summary(id, name)).collect())));
        let join = |h: &mut Harness, (id, name): (&str, &str)| {
            h.send(Msg::JoinRoom(id.to_owned()));
            h.send(Msg::RoomRead(id.to_owned(), Ok(room(id, name))));
        };
        join(&mut h, names[0]);
        let capsules = || of_class(&dock, "ErrandRoomTab").into_iter().filter(|tab| !tab.isHiddenOrHasHiddenAncestor()).collect::<Vec<_>>();
        let tabs = || capsules().into_iter().filter(|capsule| spoken(capsule) != "작업방").collect::<Vec<_>>();
        let plus = capsules().into_iter().find(|capsule| spoken(capsule) == "작업방").expect("the plus");
        let alone = tabs()[0].frame().size.width;
        let (strip, fades) = (first(&dock, "ErrandRoomTabStrip").expect("the tabs' strip").downcast::<NSScrollView>().expect("a scroll view"), of_class(&dock, "ErrandTabFade"));
        let clip = strip.contentView();
        let offset = || clip.bounds().origin.x;
        let plus_at = || plus.convertRect_toView(plus.bounds(), Some(&dock));
        let in_view = |tab: &NSView| inside(clip.bounds(), tab.frame());
        let faded = || (!fades[0].isHidden(), !fades[1].isHidden());
        t.check(offset() == 0.0 && faded() == (false, false), "One tab fits, so nothing moves or fades");

        join(&mut h, names[1]);
        join(&mut h, names[2]);
        let three = tabs();
        let whole = |tab: &NSView, name: &str| label(tab, name).is_some_and(|name| name.frame().size.width >= name.fittingSize().width - 0.5);
        t.check(
            three.len() == 3 && three[0].frame().size.width == alone && names[..3].iter().zip(&three).all(|((_, name), tab)| whole(tab, name)),
            &format!("Three tabs keep their whole names and their widths: {alone} then {:?}", three.iter().map(|tab| tab.frame().size.width).collect::<Vec<_>>()),
        );
        let reach = strip.documentView().expect("the tabs").frame().size.width - clip.bounds().size.width;
        let plus_home = plus_at();
        t.check(
            reach > 0.0 && (max_x(plus_home) - (STACK_WIDTH - DOCK_PADDING)).abs() < 0.5 && strip.frame().size.width < plus.frame().origin.x,
            &format!("They do not all fit, so the plus stands at the row's end, clear of them: {plus_home:?}"),
        );
        t.check(offset() > 0.0 && in_view(&three[2]), &format!("The room just joined is brought into view: {} of {reach}", offset()));

        let on_tab = three[1].convertPoint_toView(NSPoint::new(three[1].bounds().size.width / 2.0, 11.0), Some(&dock));
        let point = CGScrollEventUnit::Pixel;
        swipe(&dock, on_tab, point, 0, 1000, mtm);
        t.check(offset() == 0.0 && faded() == (false, true), &format!("A swipe back stops at the first tab, and the far end fades: {}", offset()));
        swipe(&dock, on_tab, point, 0, -40, mtm);
        t.check((offset() - 40.0).abs() < 0.5 && faded() == (true, true), &format!("A swipe over a tab moves the tabs along, and both ends fade: {}", offset()));
        t.check(plus_at() == plus_home, "The plus stays where it is while they move");
        let moved = three[0].convertRect_toView(three[0].bounds(), Some(&dock));
        let gap = NSPoint::new(max_x(moved) + 3.0, on_tab.y);
        swipe(&dock, gap, point, 0, -10, mtm);
        t.check((offset() - 50.0).abs() < 0.5, &format!("So does a swipe over the gap between tabs, which is the dock's: {}", offset()));
        swipe(&dock, NSPoint::new(plus_home.origin.x + 11.0, plus_home.origin.y + 11.0), point, 0, -10, mtm);
        t.check((offset() - 60.0).abs() < 0.5, &format!("And one over the plus: {}", offset()));
        swipe(&dock, on_tab, point, -5, 0, mtm);
        t.check((offset() - 65.0).abs() < 0.5, &format!("A swipe up and down moves them along too: {}", offset()));
        let before = offset();
        swipe(&dock, on_tab, CGScrollEventUnit::Line, -1, 0, mtm);
        t.check(offset() >= before + 8.0, &format!("And a wheel's notch, by more than a point: {before} then {}", offset()));
        let before = offset();
        swipe(&dock, NSPoint::new(40.0, 30.0), point, 0, -10, mtm);
        t.check(offset() == before, "A swipe elsewhere on the dock leaves the tabs alone");

        let held = offset();
        let busy = Room { tagline: "다시 읽음".to_owned(), ..room("team", "우리 팀 야근방") };
        h.send(Msg::RoomRead("team".to_owned(), Ok(busy)));
        t.check(offset() == held, &format!("Drawn again, the tabs stay where they had moved to: {held} then {}", offset()));
        swipe(&dock, on_tab, point, 0, -9999, mtm);
        t.check((offset() - reach).abs() < 0.5 && faded() == (true, false), &format!("They stop at the last tab: {} of {reach}", offset()));
        let drawn = tabs();
        h.send(Msg::ExpandRoom("late-night".to_owned()));
        let opened = tabs();
        t.check(in_view(&opened[0]) && offset() < reach, &format!("A tab opened from out of view comes into it: {:?} {}", drawn[0].frame(), offset()));
        let right = fades[1].frame();
        let under = strip.convertPoint_toView(NSPoint::new(right.origin.x + right.size.width / 2.0, 11.0), None);
        t.check(hit(&panel, under).is_some_and(|view| class_name(&view) == "ErrandRoomTab"), "A click on a fading tab reaches the tab");

        h.send(Msg::ToggleRooms);
        let list = rooms_panel.contentView().expect("the list's view");
        let way_in = titled(&list, "들어가기").expect("a way into the fourth room");
        t.check(
            !way_in.isEnabled() && tooltip(&way_in).contains("3개까지") && shows(&list, "3개까지") && titled(&list, "나가기").is_some_and(|out| out.isEnabled()),
            "At three rooms the list shuts its ways in and says why, and its ways out stay open",
        );
        unsafe { way_in.performClick(None) };
        h.pump();
        t.check(h.performed(|effect| matches!(effect, Effect::SyncRoom { room, .. } if room == "deadline")) == 0, "A shut way in asks no room");
        h.send(Msg::LeaveRoom("dawn".to_owned()));
        let way_in = titled(&list, "들어가기").expect("the ways in");
        t.check(way_in.isEnabled() && !shows(&list, "3개까지"), "Leaving a room opens the ways in again");
        h.send(Msg::CloseRooms);

        h.send(Msg::RoomsRemembered(names[2..].iter().map(|(id, _)| ((*id).to_owned(), Visibility::Public)).collect()));
        let five = tabs();
        swipe(&dock, on_tab, point, 0, -9999, mtm);
        t.check(five.len() == 5 && in_view(&five[4]) && plus_at() == plus_home, "Five rooms from an older Errand all stay within reach, the plus still in place");
        for (id, _) in names {
            h.send(Msg::LeaveRoom(id.to_owned()));
        }
        t.check(tabs().is_empty(), "Leaving every room takes the tabs off the dock");
        h.send(Msg::SetWidgetHidden(true));
    }

    /// A service the person does not use: its battery and card go, the dock and details shrink to fit, and the
    /// choice is kept.
    fn hiding(t: &mut Checks, mtm: MainThreadMarker, defaults: &Retained<NSUserDefaults>) {
        let mut h = Harness::new(mtm, defaults, Language::Korean);
        let (panel, details_panel) = {
            let widget = h.widget.borrow();
            (widget.panel.clone(), widget.details_panel.clone())
        };
        let dock = h.dock();
        let batteries = || of_class(&dock, "ErrandBatteryView").iter().filter(|battery| !battery.isHidden()).map(|battery| spoken(battery)).collect::<Vec<_>>();
        let cards = |h: &Harness| of_class(&h.details(), "ErrandAccountCard").iter().filter(|card| !card.isHidden()).count();
        h.send(Msg::SetWidgetHidden(false));
        h.send(Msg::ToggleDetails);
        let (full, wide) = (panel.frame(), details_panel.frame());
        h.send(Msg::ToggleService(Provider::Codex));
        let (one, narrow) = (panel.frame(), details_panel.frame());
        t.check(batteries() == ["Claude 남은 사용량"], &format!("A hidden service's battery leaves the dock: {:?}", batteries()));
        t.check(
            (full.size.height - one.size.height - 36.0).abs() < 0.5 && (full.origin.y - one.origin.y).abs() < 0.5,
            &format!("The dock loses one battery's height and keeps its bottom edge: {full:?} then {one:?}"),
        );
        t.check(
            details_panel.isVisible() && cards(&h) == 1 && (wide.size.width - narrow.size.width - 256.0).abs() < 0.5,
            &format!("The details hold one card and narrow to it: {wide:?} then {narrow:?}"),
        );
        t.check(stored_preferences(defaults).hidden_services.codex, "Hiding a service is kept for the next launch");
        t.check(
            h.performed(|effect| *effect == Effect::FetchUsage(Provider::Codex)) == 0,
            "A hidden service's usage is not read",
        );
        h.send(Msg::ToggleService(Provider::Codex));
        let back = panel.frame();
        t.check(
            batteries() == ["Codex 남은 사용량", "Claude 남은 사용량"] && (back.size.height - full.size.height).abs() < 0.5 && cards(&h) == 2,
            "Shown again, the service's battery and card come back",
        );
        t.check(
            !stored_preferences(defaults).hidden_services.codex && h.performed(|effect| *effect == Effect::FetchUsage(Provider::Codex)) == 1,
            "Showing it again is kept, and its usage is read at once",
        );
        h.send(Msg::SetWidgetHidden(true));
    }

    /// The smaller widgets: one line under the tasks that call, or the character alone with that line over it while
    /// the pointer rests on it. Each keeps the dock's corner, and the choice is kept for the next launch.
    fn modes(t: &mut Checks, mtm: MainThreadMarker, defaults: &Retained<NSUserDefaults>) {
        let mut h = Harness::new(mtm, defaults, Language::Korean);
        let (panel, tray_panel) = {
            let widget = h.widget.borrow();
            (widget.panel.clone(), widget.tray_panel.clone())
        };
        h.send(Msg::SetWidgetHidden(false));
        let weekly = window("codex-week", WEEK, 72, Some(WEEKLY_RESET));
        h.send(Msg::UsageRead { provider: Provider::Codex, result: Ok(vec![weekly].into()) });
        let answered = row("claude:answered", Provider::Claude, "로그인 화면 리팩터링", "커밋해줘", h.ago(900), Some(h.ago(300)));
        let running = row("codex:running", Provider::Codex, "API 캐시 정리", "캐시 꺼줘", h.ago(200), None);
        h.send(Msg::SessionsScanned(vec![answered, running]));
        let screen = panel.screen().expect("the dock is on a screen").visibleFrame();
        h.park(NSPoint::new(max_x(screen) - STACK_WIDTH - 40.0, screen.origin.y + 40.0), true, mtm);
        let full = panel.frame();
        t.check(tray_panel.isVisible(), "The full widget stacks the cards on the dock");

        h.send(Msg::SetWidgetMode(WidgetMode::Simple));
        let simple = panel.frame();
        let content = panel.contentView().expect("the widget's view");
        let lines = |content: &NSView| of_class(content, "ErrandLineCard").iter().filter_map(|line| line.identifier()).map(|key| key.to_string()).collect::<Vec<_>>();
        t.check(!same(&content, &h.dock()) && !tray_panel.isVisible(), "Simple, the widget is one line and no card stack");
        t.check(lines(&content) == ["claude:answered"], &format!("Only the answer waiting to be read has a line: {:?}", lines(&content)));
        t.check(
            (simple.size.height - layout::simple_height(1)).abs() < 0.5 && (max_x(simple) - max_x(full)).abs() < 0.5 && (simple.origin.y - full.origin.y).abs() < 0.5,
            &format!("The simple widget keeps the dock's bottom-right corner: {full:?} then {simple:?}"),
        );
        t.check(stored_preferences(defaults).mode == WidgetMode::Simple, "The simple widget is kept for the next launch");
        let strip = first(&content, "ErrandStripView").expect("the simple widget's line");
        t.check(spoken(&strip).contains("진행 중 1개") && spoken(&strip).contains("Codex 72%"), &format!("The line says the counts and the usage: {}", spoken(&strip)));
        click(&strip, NSPoint::new(16.0, 20.0));
        h.pump();
        let content = panel.contentView().expect("the widget's view");
        t.check(lines(&content) == ["claude:answered", "codex:running"], &format!("A click on the counts lines up every task: {:?}", lines(&content)));
        t.check((panel.frame().size.height - layout::simple_height(2)).abs() < 0.5, "The widget grows upward by the new line");
        let strip = first(&content, "ErrandStripView").expect("the simple widget's line");
        click(&strip, NSPoint::new(16.0, 20.0));
        h.pump();
        t.check(lines(&panel.contentView().expect("the widget's view")).len() == 1, "Another click folds back to the tasks that call");
        let strip = first(&panel.contentView().expect("the widget's view"), "ErrandStripView").expect("the simple widget's line");
        click(&strip, NSPoint::new(STACK_WIDTH - 30.0, 20.0));
        h.pump();
        t.check(h.state.details_open && h.widget.borrow().details_panel.isVisible(), "A click on the batteries opens the usage details");
        h.send(Msg::CloseDetails);

        h.send(Msg::SetWidgetMode(WidgetMode::Character));
        let character = panel.frame();
        let content = panel.contentView().expect("the widget's view");
        let badge = first(&content, "ErrandBadgeView").expect("the character");
        t.check(
            (character.size.width - layout::BADGE_SIZE).abs() < 0.5 && (max_x(character) - max_x(full)).abs() < 0.5 && (character.origin.y - full.origin.y).abs() < 0.5,
            &format!("The character sits in the dock's bottom-right corner: {character:?}"),
        );
        t.check(first(&badge, "ErrandRunningArcView").is_some_and(|arc| !arc.isHidden()), "Work under way turns an arc round the character");
        t.check(tooltip(&badge).contains("확인할 것 1개"), &format!("The character's tooltip counts what calls: {}", tooltip(&badge)));
        content.mouseEntered(&pointer(NSEventType::MouseEntered, &panel));
        h.pump();
        let open = panel.frame();
        let content = panel.contentView().expect("the widget's view");
        t.check(first(&content, "ErrandStripView").is_some() && lines(&content) == ["claude:answered"], "The pointer on the character brings the simple widget over it");
        t.check(
            (max_x(open) - max_x(character)).abs() < 0.5 && (open.origin.y - character.origin.y).abs() < 0.5 && open.size.height > character.size.height,
            &format!("It opens upward and leftward, the character staying under the pointer: {character:?} then {open:?}"),
        );
        let badge_now = first(&content, "ErrandBadgeView").expect("the character");
        t.check(
            (max_x(badge_now.convertRect_toView(badge_now.bounds(), None)) - max_x(content.bounds())).abs() < 0.5,
            "The character stays at the open widget's right end",
        );
        h.send(Msg::PointerOnBadge(false));
        t.check((panel.frame().size.height - layout::BADGE_SIZE).abs() < 0.5, "Left, it folds back to the character");

        let menu = badge_now.menuForEvent(&pointer(NSEventType::MouseEntered, &panel)).expect("the widget's own menu");
        let titles: Vec<String> = (0..menu.numberOfItems()).filter_map(|index| menu.itemAtIndex(index)).map(|item| item.title().to_string()).collect();
        let checked: Vec<bool> = (0..menu.numberOfItems()).filter_map(|index| menu.itemAtIndex(index)).map(|item| item.state() == objc2_app_kit::NSControlStateValueOn).collect();
        t.check(titles == ["크게 보기", "간단히 보기", "캐릭터만"] && checked == [false, false, true], &format!("A right-click offers the three sizes: {titles:?} {checked:?}"));
        menu.performActionForItemAtIndex(0);
        h.pump();
        t.check(same(&panel.contentView().expect("the widget's view"), &h.dock()) && h.state.widget.mode == WidgetMode::Full, "Picking 크게 보기 brings the dock back");
        t.check((max_x(panel.frame()) - max_x(full)).abs() < 0.5 && (panel.frame().size.height - full.size.height).abs() < 0.5, "The dock comes back where it was");
        let dock_menu = h.dock().menuForEvent(&pointer(NSEventType::MouseEntered, &panel)).map(|menu| menu.numberOfItems());
        t.check(dock_menu == Some(3), "The dock has the same right-click menu");

        // Parked on the left half, the widget keeps its left edge instead.
        h.park(NSPoint::new(screen.origin.x + 40.0, screen.origin.y + 40.0), true, mtm);
        let left = panel.frame();
        h.send(Msg::SetWidgetMode(WidgetMode::Character));
        t.check((panel.frame().origin.x - left.origin.x).abs() < 0.5, &format!("On the left, the character keeps the dock's left edge: {left:?} then {:?}", panel.frame()));
        h.send(Msg::PointerOnBadge(true));
        let content = panel.contentView().expect("the widget's view");
        let badge_now = first(&content, "ErrandBadgeView").expect("the character");
        t.check(
            (panel.frame().origin.x - left.origin.x).abs() < 0.5 && badge_now.frame().origin.x.abs() < 0.5,
            "Opened on the left, it grows rightward with the character at its left end",
        );
        h.send(Msg::PointerOnBadge(false));
        h.send(Msg::SetWidgetMode(WidgetMode::Full));
        t.check((panel.frame().origin.x - left.origin.x).abs() < 0.5, "Back to full, the dock keeps its left edge");
        h.send(Msg::SetWidgetHidden(true));
    }

    /// The same widget in English: its own words, the view's words, and the services' data as they gave it.
    fn english(t: &mut Checks, mtm: MainThreadMarker, defaults: &Retained<NSUserDefaults>) {
        let mut h = Harness::new(mtm, defaults, Language::English);
        let (panel, details_panel, tray_panel) = {
            let widget = h.widget.borrow();
            (widget.panel.clone(), widget.details_panel.clone(), widget.tray_panel.clone())
        };
        let dock = h.dock();
        h.send(Msg::SetWidgetHidden(false));
        h.send(Msg::UsageRead { provider: Provider::Codex, result: Ok(vec![window("codex/primary", WEEK, 72, Some(WEEKLY_RESET))].into()) });
        t.check(
            panel.title().to_string() == "Errand · Usage" && tooltip(&dock) == "Click for usage details · Drag to move",
            "In English the dock's window and tooltip are English",
        );
        t.check(
            shows(&dock, "72%") && shows(&dock, "Weekly · resets 10/19 09:00") && spoken(&h.battery(Provider::Codex)) == "Codex usage left",
            &format!("An English battery reads its limit and reset in English: {:?}", labels(&dock).iter().map(|label| label.stringValue().to_string()).collect::<Vec<_>>()),
        );
        h.send(Msg::ToggleDetails);
        let details = h.details();
        t.check(
            details_panel.isVisible()
                && shows(&details, "Account usage")
                && shows(&details, "Next reset   10/19 09:00")
                && titled(&details, "Show all").is_some()
                && titled(&details, "Close").is_some(),
            "The English details name their limits and buttons in English",
        );
        h.send(Msg::CloseDetails);
        let mut asked = row("claude:en-1", Provider::Claude, "Tidy the README", "Tidy the README and commit", h.ago(900), Some(h.ago(300)));
        asked.action = Some("Decide whether to push".to_owned());
        let running = row("codex:en-2", Provider::Codex, "", "Fix the sign-in bug", h.ago(240), None);
        let mut seen = row("codex:en-3", Provider::Codex, "Check the deploy", "Is it deployed?", h.ago(4000), Some(h.ago(3900)));
        seen.seen_in_app = true;
        h.send(Msg::SessionsScanned(vec![asked, running, seen]));
        let tray = h.tray();
        let header = first(&dock, "ErrandPressableView").expect("the task header");
        let pills: Vec<String> = of_class(&header, "ErrandCountPill").iter().map(|pill| tooltip(pill)).collect();
        t.check(
            shows(&dock, "My tasks 3") && pills == ["1 to check", "1 running", "1 seen"] && spoken(&header).ends_with("Hide"),
            &format!("The English task header counts in English: {pills:?}"),
        );
        t.check(
            shows(&tray, "To do · Decide whether to push") && shows(&tray, "Working · 4 min") && shows(&tray, "Untitled session") && shows(&tray, "“Fix the sign-in bug”"),
            "English cards say what is left to do, how long work has run, and name an untitled session",
        );
        if let Some(check) = h.card("claude:en-1").and_then(|card| first(&card, "ErrandCheckButton")).and_then(|view| view.downcast::<NSButton>().ok()) {
            unsafe { check.performClick(None) };
        }
        h.pump();
        t.check(h.card("claude:en-1").is_some_and(|card| titled(&card, "Undo").is_some()), "A checked English card offers Undo");
        t.check(tray_panel.isVisible() && tray_panel.title().to_string() == "Errand · My tasks", "The English cards' window has an English title");
        h.send(Msg::SetWidgetHidden(true));
    }
}
