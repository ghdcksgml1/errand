//! The dock: one battery per service, the refresh button, the person's profile and, while there are tasks,
//! the task header on top; under the profile, a tab per work room the person is in, and beside the nickname
//! outside them, the way to the rooms. The whole dock is a drag handle: a click on the header folds the cards,
//! one on the person's character opens its editor, a click on the rooms is theirs, and a click anywhere else
//! opens the usage details.

use super::rooms::{DockRooms, MemberRow, RoomTab, TabStrip};
use super::battery::{BatteryView, dock_battery_width};
use super::kit::{self, Action, CHIP_ICON, FlippedView, PressableView, Weight, color, digits_font, fitting_width, icon_image, label, ns, rect};
use super::spinner::SpinningIconView;
use errand_core::avatar::Avatar;
use errand_core::{Language, Provider};
use errand_core::layout::{self, DOCK_PADDING, DockShape, HEADER_HEIGHT, PROFILE_HEIGHT, REFRESH_SIZE, ROOM_TAB_HEIGHT, STACK_WIDTH};
use errand_core::theme::{self, Rgb};
use errand_core::view::{DockVm, HeaderVm, ProfileVm, UpdateVm};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAccessibility, NSAccessibilityButtonRole, NSAccessibilityGroupRole, NSBezierPath, NSButton, NSCellImagePosition, NSEvent, NSImageView, NSMenu,
    NSTextAlignment, NSTextField, NSTrackingArea, NSTrackingAreaOptions, NSView,
};
use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The update button at the end of the profile row.
const UPDATE_WIDTH: f64 = 112.0;

/// What the dock asks of the widget around it.
pub type RemoveMember = Rc<dyn Fn(String, String, String)>;

#[derive(Clone)]
pub struct DockEvents {
    pub toggle_details: Rc<dyn Fn()>,
    pub toggle_tray: Rc<dyn Fn()>,
    pub refresh: Rc<dyn Fn()>,
    pub drag_began: Rc<dyn Fn()>,
    /// The pointer moved this far from where the drag began; `done` once it is let go.
    pub dragged: Rc<dyn Fn(NSPoint, bool)>,
    /// A click on the person's character: its editor opens or closes.
    pub edit_avatar: Rc<dyn Fn()>,
    /// A room's tab: shows or folds its people.
    pub expand_room: Rc<dyn Fn(String)>,
    /// The owner's menu on someone's row: the room, the member's id in it, and their name.
    pub remove_member: RemoveMember,
    /// The way to the rooms: opens their list, or closes it.
    pub show_rooms: Rc<dyn Fn()>,
    /// The update beside the profile.
    pub update: Rc<dyn Fn()>,
    /// The right-click menu anywhere but on a member's row: how much of the widget shows.
    pub mode_menu: Rc<dyn Fn() -> Option<Retained<NSMenu>>>,
}

/// Hover tracking over the view's visible part, whether or not Errand is the active app.
pub fn replace_tracking_area(view: &NSView) {
    for area in view.trackingAreas().iter() {
        view.removeTrackingArea(&area);
    }
    let options = NSTrackingAreaOptions::MouseEnteredAndExited | NSTrackingAreaOptions::ActiveAlways | NSTrackingAreaOptions::InVisibleRect;
    let owner: &AnyObject = view.as_ref();
    let area = unsafe { NSTrackingArea::initWithRect_options_owner_userInfo(NSTrackingArea::alloc(), NSRect::ZERO, options, Some(owner), None) };
    view.addTrackingArea(&area);
}

pub struct AvatarIvars {
    avatar: Cell<Option<Avatar>>,
    /// Without a character, as a room shows someone it has none for: this letter in a grey circle.
    initial: RefCell<String>,
    /// A white ring sets it off from faces it overlaps, and the circle under it is opaque.
    ring: Cell<bool>,
    /// What VoiceOver's press does; the dock handles the mouse.
    press: Option<Rc<dyn Fn()>>,
}

define_class!(
    /// The person's character in a circle, or a room member's face.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandAvatarView"]
    #[ivars = AvatarIvars]
    pub struct AvatarView;

    impl AvatarView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn accessibility_perform_press(&self) -> bool {
            self.ivars().press.as_ref().map(|press| press()).is_some()
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let circle = kit::inset(self.bounds(), 0.5, 0.5);
            let oval = NSBezierPath::bezierPathWithOvalInRect(circle);
            let ring = self.ivars().ring.get();
            if let Some(avatar) = self.ivars().avatar.get() {
                // Opaque where faces overlap, so they do not show through one another.
                if ring { color(theme::HEALTHY.blend(0.86, theme::WHITE)).setFill() } else { kit::paint(theme::HEALTHY.alpha(0.14)).setFill() }
                oval.fill();
                kit::draw_avatar(&avatar, circle);
            } else {
                let tint = theme::UNKNOWN;
                color(tint.blend(0.86, theme::WHITE)).setFill();
                oval.fill();
                let initial = self.ivars().initial.borrow().clone();
                let typeface = kit::font((self.bounds().size.height * 0.46).round(), Weight::Semibold);
                let size = kit::text_size(&initial, &typeface);
                let ink = color(tint.blend(0.35, Rgb { red: 0.0, green: 0.0, blue: 0.0 }));
                let origin = NSPoint::new(circle.origin.x + (circle.size.width - size.width) / 2.0, circle.origin.y + (circle.size.height - size.height) / 2.0);
                kit::draw_text(&initial, origin, &typeface, &ink);
            }
            if ring {
                color(theme::WHITE).setStroke();
                oval.setLineWidth(1.5);
                oval.stroke();
            }
        }
    }
);

impl AvatarView {
    pub fn new(mtm: MainThreadMarker, frame: NSRect, press: Option<Rc<dyn Fn()>>) -> Retained<AvatarView> {
        let ivars = AvatarIvars { avatar: Cell::new(None), initial: RefCell::new(String::new()), ring: Cell::new(false), press };
        let this = AvatarView::alloc(mtm).set_ivars(ivars);
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }

    /// Draws this character, again only when it changed.
    pub fn show(&self, avatar: Avatar) {
        if self.ivars().avatar.get() != Some(avatar) {
            self.ivars().avatar.set(Some(avatar));
            self.setNeedsDisplay(true);
        }
    }

    /// A room member's face: their character, or without one the first letter in a grey circle.
    pub fn show_face(&self, avatar: Option<Avatar>, initial: &str, ring: bool) {
        self.ivars().avatar.set(avatar);
        *self.ivars().initial.borrow_mut() = initial.to_owned();
        self.ivars().ring.set(ring);
        self.setNeedsDisplay(true);
    }
}

define_class!(
    /// A small icon button on the dock; it takes the first click even while another app is active.
    #[unsafe(super(NSButton))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandDockButton"]
    #[ivars = Cell<bool>]
    pub struct DockButton;

    impl DockButton {
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(updateTrackingAreas))]
        fn update_tracking_areas(&self) {
            let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
            replace_tracking_area(self);
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            self.ivars().set(true);
            self.setContentTintColor(Some(&color(theme::INK)));
            NSView::setNeedsDisplay(self, true);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            self.ivars().set(false);
            self.setContentTintColor(Some(&color(theme::MUTED)));
            NSView::setNeedsDisplay(self, true);
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, dirty: NSRect) {
            if self.ivars().get() {
                kit::paint(theme::INK.alpha(0.07)).setFill();
                NSBezierPath::bezierPathWithOvalInRect(self.bounds()).fill();
            }
            let _: () = unsafe { msg_send![super(self), drawRect: dirty] };
        }
    }
);

define_class!(
    /// A count beside the task title: the state's own icon and colour, so the cards' chips explain it.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandCountPill"]
    #[ivars = Cell<Rgb>]
    pub struct PillView;

    impl PillView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let bounds = self.bounds();
            kit::paint(self.ivars().get().alpha(0.12)).setFill();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(bounds, bounds.size.height / 2.0, bounds.size.height / 2.0).fill();
        }
    }
);

impl PillView {
    fn new(mtm: MainThreadMarker, frame: NSRect, tint: Rgb) -> Retained<PillView> {
        let this = PillView::alloc(mtm).set_ivars(Cell::new(tint));
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }
}

/// A count in a pill as the task header shows them: the icon, or a spinner for work under way, then the
/// number, all in one colour, its top left at `origin`.
pub fn count_pill(count: usize, icon: &'static str, spinning: bool, tint: Rgb, words: &str, origin: NSPoint, mtm: MainThreadMarker) -> Retained<PillView> {
    let number = label(&count.to_string(), 11.0, Weight::Semibold, tint, mtm);
    number.setFont(Some(&digits_font(11.0, Weight::Semibold)));
    let number_width = fitting_width(&number);
    let view = PillView::new(mtm, rect(origin.x, origin.y, 7.0 + CHIP_ICON + 3.0 + number_width + 4.0, 18.0), tint);
    let icon_frame = rect(7.0, 3.5, CHIP_ICON, CHIP_ICON);
    if spinning {
        view.addSubview(&SpinningIconView::new(mtm, icon_frame, "loader-circle", tint));
    } else {
        let mark = tinted_icon(icon, CHIP_ICON, tint, mtm);
        mark.setFrameOrigin(icon_frame.origin);
        view.addSubview(&mark);
    }
    number.setFrame(rect(7.0 + CHIP_ICON + 3.0, 1.0, number_width, 16.0));
    view.addSubview(&number);
    view.setToolTip(Some(&ns(words)));
    view
}

pub struct DockIvars {
    events: DockEvents,
    header: RefCell<Option<Retained<PressableView>>>,
    avatar: RefCell<Option<Retained<AvatarView>>>,
    profile_top: Cell<f64>,
    /// Where the room tabs start; the rooms keep the clicks below it.
    rooms_top: Cell<f64>,
    /// Takes the swipes the row of tabs gets.
    strip: RefCell<Option<Retained<TabStrip>>>,
    drag_start: Cell<NSPoint>,
    dragged: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandUsageDockView"]
    #[ivars = DockIvars]
    pub struct DockView;

    impl DockView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> *mut NSView {
            let hit: *mut NSView = unsafe { msg_send![super(self), hitTest: point] };
            // Buttons keep their clicks; everything else is the dock's to drag or click.
            match unsafe { hit.as_ref() } {
                Some(view) if view.isKindOfClass(NSButton::class()) => hit,
                Some(_) => self as *const DockView as *mut NSView,
                None => std::ptr::null_mut(),
            }
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        /// The dock takes clicks for itself, so a right-click on a room member's row opens that row's menu here;
        /// anywhere else, the widget's own.
        #[unsafe(method_id(menuForEvent:))]
        fn menu_for_event(&self, event: &NSEvent) -> Option<Retained<NSMenu>> {
            self.row_menu(event).or_else(|| (self.ivars().events.mode_menu)())
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            self.ivars().drag_start.set(NSEvent::mouseLocation());
            self.ivars().dragged.set(false);
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, _event: &NSEvent) {
            let delta = self.drag_delta();
            if !self.ivars().dragged.get() && delta.x.hypot(delta.y) > 4.0 {
                self.ivars().dragged.set(true);
                (self.ivars().events.drag_began)();
            }
            if self.ivars().dragged.get() {
                (self.ivars().events.dragged)(delta, false);
            }
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let events = self.ivars().events.clone();
            if self.ivars().dragged.get() {
                (events.dragged)(self.drag_delta(), true);
                return;
            }
            let point = self.convertPoint_fromView(event.locationInWindow(), None);
            if point.y >= self.ivars().rooms_top.get() {
                return;
            }
            let on_header = self.ivars().header.borrow().as_ref().is_some_and(|header| !header.isHidden() && kit::contains(header.frame(), point));
            let on_avatar = self.ivars().avatar.borrow().as_ref().is_some_and(|avatar| kit::contains(avatar.convertRect_toView(avatar.bounds(), Some(self)), point));
            if on_header {
                (events.toggle_tray)()
            } else if on_avatar {
                (events.edit_avatar)()
            } else {
                (events.toggle_details)()
            }
        }

        /// A swipe over a tab finds the strip by itself; one beside the tabs or over the plus is the dock's by
        /// `hitTest:` and comes here.
        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            let point = self.convertPoint_fromView(event.locationInWindow(), None);
            let top = self.ivars().rooms_top.get();
            if (top..top + ROOM_TAB_HEIGHT).contains(&point.y)
                && let Some(strip) = self.ivars().strip.borrow().as_ref()
            {
                strip.scrollWheel(event);
            }
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn accessibility_perform_press(&self) -> bool {
            (self.ivars().events.toggle_details)();
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let bounds = self.bounds();
            let card = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(kit::inset(bounds, 1.0, 1.0), 16.0, 16.0);
            color(theme::WHITE).setFill();
            card.fill();
            color(theme::OUTLINE).setStroke();
            card.stroke();
            color(theme::HAIRLINE).setFill();
            if self.ivars().header.borrow().as_ref().is_some_and(|header| !header.isHidden()) {
                NSBezierPath::fillRect(rect(12.0, HEADER_HEIGHT, bounds.size.width - 24.0, 1.0));
            }
            NSBezierPath::fillRect(rect(12.0, self.ivars().profile_top.get(), bounds.size.width - 24.0, 1.0));
        }
    }
);

impl DockView {
    /// The menu of the room member's row under the pointer, if it has one.
    fn row_menu(&self, event: &NSEvent) -> Option<Retained<NSMenu>> {
        let point = self.convertPoint_fromView(event.locationInWindow(), None);
        let mut view = self.subviews().iter().find_map(|view| view.hitTest(point));
        while let Some(current) = view {
            if let Some(row) = current.downcast_ref::<MemberRow>() {
                return row.menu();
            }
            view = unsafe { current.superview() };
        }
        None
    }
}


impl DockView {
    fn drag_delta(&self) -> NSPoint {
        let (pointer, start) = (NSEvent::mouseLocation(), self.ivars().drag_start.get());
        NSPoint::new(pointer.x - start.x, pointer.y - start.y)
    }
}

/// The dock's views, kept to update in place.
pub struct Dock {
    pub view: Retained<DockView>,
    header: Retained<PressableView>,
    batteries: Vec<(Provider, Retained<BatteryView>)>,
    refresh: Retained<DockButton>,
    refreshing: Retained<SpinningIconView>,
    profile: Retained<FlippedView>,
    avatar: Retained<AvatarView>,
    nickname: Retained<NSTextField>,
    /// A newer Errand on offer.
    update: Retained<DockButton>,
    /// The way to the rooms, at the row's end while the person is in none.
    rooms_button: Retained<RoomTab>,
    /// The rooms the person is in, under the profile.
    rooms: DockRooms,
    /// The refresh button holds its target weakly.
    _refresh_action: Retained<Action>,
    _update_action: Retained<Action>,
    _rooms_action: Retained<Action>,
    /// What the dock holds, as last laid out.
    shape: Cell<DockShape>,
    shown: Option<DockVm>,
}

impl Dock {
    pub fn new(mtm: MainThreadMarker, events: DockEvents, language: Language) -> Dock {
        let toggle_tray = events.toggle_tray.clone();
        let refresh_event = events.refresh.clone();
        let update_event = events.update.clone();
        let edit_avatar = events.edit_avatar.clone();
        let expand_event = events.expand_room.clone();
        let remove_event = events.remove_member.clone();
        let show_rooms = events.show_rooms.clone();
        let view: Retained<DockView> = {
            let this = DockView::alloc(mtm).set_ivars(DockIvars {
                events,
                header: RefCell::new(None),
                avatar: RefCell::new(None),
                profile_top: Cell::new(layout::profile_top(DockShape::FULL)),
                rooms_top: Cell::new(f64::INFINITY),
                strip: RefCell::new(None),
                drag_start: Cell::new(NSPoint::ZERO),
                dragged: Cell::new(false),
            });
            unsafe { msg_send![super(this), initWithFrame: rect(0.0, 0.0, STACK_WIDTH, layout::dock_height(DockShape::FULL))] }
        };
        view.setToolTip(Some(&ns(language.dock_tooltip())));
        view.setAccessibilityElement(true);
        view.setAccessibilityRole(Some(unsafe { NSAccessibilityGroupRole }));
        view.setAccessibilityLabel(Some(&ns(language.dock_label())));
        let header = PressableView::new(mtm, rect(0.0, 0.0, STACK_WIDTH, HEADER_HEIGHT), Some(toggle_tray));
        header.setAccessibilityElement(true);
        header.setAccessibilityRole(Some(unsafe { NSAccessibilityButtonRole }));
        header.setHidden(true);
        view.addSubview(&header);
        let batteries: Vec<(Provider, Retained<BatteryView>)> = Provider::ALL
            .iter()
            .map(|provider| {
                let battery = BatteryView::new(mtm, rect(DOCK_PADDING, 0.0, dock_battery_width(), layout::BATTERY_HEIGHT), *provider, language);
                view.addSubview(&battery);
                (*provider, battery)
            })
            .collect();
        let refresh: Retained<DockButton> = {
            let this = DockButton::alloc(mtm).set_ivars(Cell::new(false));
            unsafe { msg_send![super(this), initWithFrame: rect(0.0, 0.0, REFRESH_SIZE, REFRESH_SIZE)] }
        };
        refresh.setBordered(false);
        refresh.setTitle(&ns(""));
        refresh.setImage(icon_image("refresh-cw", 14.0, None, 0.0, 0.0).as_deref());
        refresh.setImagePosition(NSCellImagePosition::ImageOnly);
        refresh.setContentTintColor(Some(&color(theme::MUTED)));
        refresh.setToolTip(Some(&ns(language.refresh_usage())));
        refresh.setAccessibilityLabel(Some(&ns(language.refresh_all())));
        let refresh_action = Action::new(mtm, move || refresh_event());
        refresh_action.attach(&refresh);
        view.addSubview(&refresh);
        let refreshing = SpinningIconView::new(mtm, rect(0.0, 0.0, 14.0, 14.0), "refresh-cw", theme::MUTED);
        refreshing.setHidden(true);
        view.addSubview(&refreshing);
        let profile = FlippedView::new(mtm, rect(0.0, 0.0, STACK_WIDTH, PROFILE_HEIGHT));
        let avatar_size = layout::AVATAR_SIZE;
        let avatar = AvatarView::new(mtm, rect(DOCK_PADDING, (PROFILE_HEIGHT - avatar_size) / 2.0 + 0.5, avatar_size, avatar_size), Some(edit_avatar));
        avatar.setToolTip(Some(&ns(language.avatar_tooltip())));
        avatar.setAccessibilityElement(true);
        avatar.setAccessibilityRole(Some(unsafe { NSAccessibilityButtonRole }));
        avatar.setAccessibilityLabel(Some(&ns(language.edit_avatar())));
        profile.addSubview(&avatar);
        let nickname = label("", 12.5, Weight::Semibold, theme::INK, mtm);
        let name_x = DOCK_PADDING + avatar_size + 9.0;
        nickname.setFrame(rect(name_x, (PROFILE_HEIGHT - 17.0) / 2.0 + 0.5, STACK_WIDTH - name_x - DOCK_PADDING - 4.0, 17.0));
        profile.addSubview(&nickname);
        let update: Retained<DockButton> = {
            let this = DockButton::alloc(mtm).set_ivars(Cell::new(false));
            let frame = rect(STACK_WIDTH - DOCK_PADDING - UPDATE_WIDTH, (PROFILE_HEIGHT - 26.0) / 2.0 + 0.5, UPDATE_WIDTH, 26.0);
            unsafe { msg_send![super(this), initWithFrame: frame] }
        };
        update.setBordered(false);
        update.setImage(icon_image("circle-arrow-right", 13.0, None, 0.0, 4.0).as_deref());
        update.setImagePosition(NSCellImagePosition::ImageLeading);
        update.setImageHugsTitle(true);
        update.setContentTintColor(Some(&color(theme::MUTED)));
        update.setHidden(true);
        let update_action = Action::new(mtm, move || update_event());
        update_action.attach(&update);
        profile.addSubview(&update);
        let rooms_button = RoomTab::rooms_button(mtm, language.rooms_button(), language.rooms_button_tooltip());
        rooms_button.setFrameOrigin(NSPoint::new(STACK_WIDTH - DOCK_PADDING - rooms_button.frame().size.width, (PROFILE_HEIGHT - ROOM_TAB_HEIGHT) / 2.0));
        let rooms_action = {
            let show_rooms = show_rooms.clone();
            Action::new(mtm, move || show_rooms())
        };
        rooms_action.attach(&rooms_button);
        profile.addSubview(&rooms_button);
        profile.setAccessibilityElement(true);
        profile.setAccessibilityRole(Some(unsafe { NSAccessibilityGroupRole }));
        view.addSubview(&profile);
        *view.ivars().header.borrow_mut() = Some(header.clone());
        let rooms = DockRooms::new(mtm, expand_event, remove_event, show_rooms, language);
        view.addSubview(&rooms.tabs);
        view.addSubview(&rooms.members);
        *view.ivars().strip.borrow_mut() = Some(rooms.strip.clone());
        *view.ivars().avatar.borrow_mut() = Some(avatar.clone());
        let dock = Dock {
            view,
            header,
            batteries,
            refresh,
            refreshing,
            profile,
            avatar,
            nickname,
            update,
            rooms_button,
            rooms,
            _refresh_action: refresh_action,
            _update_action: update_action,
            _rooms_action: rooms_action,
            shape: Cell::new(DockShape::FULL),
            shown: None,
        };
        dock.layout(DockShape::FULL, &Provider::ALL);
        dock
    }

    /// Places the parts for what the dock holds and the services shown; the dock's height is the widget's
    /// to set.
    fn layout(&self, shape: DockShape, shown: &[Provider]) {
        self.header.setHidden(!shape.header);
        let mut y = layout::battery_top(shape);
        for (provider, battery) in &self.batteries {
            battery.setHidden(!shown.contains(provider));
            if shown.contains(provider) {
                battery.setFrameOrigin(NSPoint::new(DOCK_PADDING, y));
                y += layout::BATTERY_HEIGHT + layout::BATTERY_GAP;
            }
        }
        let middle = (layout::battery_top(shape) + layout::battery_bottom(shape)) / 2.0;
        let origin = NSPoint::new(STACK_WIDTH - DOCK_PADDING - REFRESH_SIZE, middle - REFRESH_SIZE / 2.0);
        self.refresh.setFrameOrigin(origin);
        self.refreshing.setFrameOrigin(NSPoint::new(origin.x + 5.0, origin.y + 5.0));
        let profile_top = layout::profile_top(shape);
        self.profile.setFrameOrigin(NSPoint::new(0.0, profile_top));
        self.view.ivars().profile_top.set(profile_top);
        self.rooms.place(shape);
        self.view.ivars().rooms_top.set(if shape.tabs { layout::tabs_top(shape) } else { f64::INFINITY });
        self.shape.set(shape);
        self.view.setNeedsDisplay(true);
    }

    pub fn battery(&self, provider: Provider) -> Option<&Retained<BatteryView>> {
        self.batteries.iter().find(|(own, _)| *own == provider).map(|(_, battery)| battery)
    }

    /// What the dock holds, as last laid out.
    pub fn shape(&self) -> DockShape {
        self.shape.get()
    }

    pub fn render(&mut self, vm: &DockVm, mtm: MainThreadMarker) {
        if self.shown.as_ref() == Some(vm) {
            return;
        }
        for battery in &vm.batteries {
            if let Some(view) = self.battery(battery.provider) {
                view.update(battery);
            }
        }
        self.refresh.setHidden(vm.refreshing);
        self.refreshing.setHidden(!vm.refreshing);
        // The rooms' times change every second; the profile's tooltip is left alone unless it changed.
        if self.shown.as_ref().map(|shown| &shown.profile) != Some(&vm.profile) {
            self.render_profile(&vm.profile);
        }
        if self.shown.as_ref().map(|shown| (&shown.update, shown.rooms.is_some())) != Some((&vm.update, vm.rooms.is_some())) {
            self.render_row_end(vm.update.as_ref(), vm.rooms.is_some());
        }
        self.rooms_button.set_open(vm.rooms_listed);
        self.rooms.set_listed(vm.rooms_listed);
        self.rooms.show(vm.rooms.as_ref(), mtm);
        let providers = |vm: &DockVm| vm.batteries.iter().map(|battery| battery.provider).collect::<Vec<_>>();
        let header_changed = self.shown.as_ref().map(|shown| &shown.header) != Some(&vm.header);
        if header_changed && let Some(header) = &vm.header {
            layout_header(&self.header, header, mtm);
        }
        if header_changed || self.shape.get() != vm.shape() || self.shown.as_ref().map(providers) != Some(providers(vm)) {
            self.layout(vm.shape(), &providers(vm));
        }
        unsafe { self.view.setAccessibilityValue(Some(&ns(&vm.accessibility_value))) };
        self.shown = Some(vm.clone());
    }

    /// The way to the rooms ends the row outside them; in them the plus after the tabs takes over. Any update on
    /// offer comes before it, and the nickname makes room for them.
    fn render_row_end(&self, update: Option<&UpdateVm>, in_rooms: bool) {
        self.rooms_button.setHidden(in_rooms);
        let rooms_width = if in_rooms { 0.0 } else { self.rooms_button.frame().size.width + 4.0 };
        let mut offer = self.update.frame();
        offer.origin.x = STACK_WIDTH - DOCK_PADDING - rooms_width - UPDATE_WIDTH;
        self.update.setFrame(offer);
        self.update.setHidden(update.is_none());
        if let Some(update) = update {
            self.update.setTitle(&ns(&update.title));
            self.update.setEnabled(update.enabled);
            self.update.setToolTip(Some(&ns(&update.tooltip)));
            self.update.setAccessibilityLabel(Some(&ns(&update.tooltip)));
        }
        let mut frame = self.nickname.frame();
        frame.size.width = STACK_WIDTH - frame.origin.x - DOCK_PADDING - rooms_width - if update.is_some() { UPDATE_WIDTH + 4.0 } else { 4.0 };
        self.nickname.setFrame(frame);
    }

    fn render_profile(&mut self, profile: &ProfileVm) {
        self.nickname.setStringValue(&ns(&profile.name));
        self.nickname.setToolTip(Some(&ns(&profile.name)));
        self.avatar.show(profile.avatar);
        self.profile.setAccessibilityLabel(Some(&ns(&profile.accessibility_label)));
    }
}

/// The task title, one count per state in the cards' own colours, and the fold control.
fn layout_header(header: &PressableView, vm: &HeaderVm, mtm: MainThreadMarker) {
    kit::clear(header);
    let width = header.bounds().size.width;
    let mark = tinted_icon("list-todo", 14.0, theme::INK, mtm);
    mark.setFrameOrigin(NSPoint::new(13.0, 12.0));
    header.addSubview(&mark);
    let title = label(&vm.title, 12.0, Weight::Semibold, theme::INK, mtm);
    title.setFrame(rect(33.0, 10.0, fitting_width(&title), 18.0));
    header.addSubview(&title);
    let mut x = title.frame().origin.x + title.frame().size.width + 6.0;
    let chevron_x = width - 27.0;
    // Counts that would run under the chevron give way, the least pressing first; the title still counts every
    // card, and VoiceOver hears every count.
    for pill in &vm.pills {
        let view = count_pill(pill.count, pill.icon, pill.spinning, pill.color, &pill.words, NSPoint::new(x, 10.0), mtm);
        if x + view.frame().size.width > chevron_x - 4.0 {
            break;
        }
        header.addSubview(&view);
        x += view.frame().size.width + 5.0;
    }
    let chevron = tinted_icon(vm.chevron, 13.0, theme::MUTED, mtm);
    chevron.setFrameOrigin(NSPoint::new(chevron_x, 12.5));
    header.addSubview(&chevron);
    // The word beside the chevron gives way when the counts need the room.
    let toggle = label(vm.toggle, 11.0, Weight::Medium, theme::MUTED, mtm);
    let toggle_width = fitting_width(&toggle);
    if x + 4.0 <= width - 30.0 - toggle_width {
        toggle.setAlignment(NSTextAlignment::Right);
        toggle.setFrame(rect(width - 30.0 - toggle_width, 11.0, toggle_width, 16.0));
        header.addSubview(&toggle);
    }
    header.setAccessibilityLabel(Some(&ns(&vm.accessibility_label)));
    header.setToolTip(Some(&ns(vm.tooltip)));
}

/// An icon tinted like text; icons repeat what the words beside them say, so VoiceOver skips them.
pub fn tinted_icon(name: &str, size: f64, tint: Rgb, mtm: MainThreadMarker) -> Retained<NSImageView> {
    let view = NSImageView::imageViewWithImage(&icon_image(name, size, None, 0.0, 0.0).expect("a vendored Lucide icon"), mtm);
    view.setFrame(rect(0.0, 0.0, size, size));
    view.setContentTintColor(Some(&color(tint)));
    view.setImageScaling(objc2_app_kit::NSImageScaling::ScaleNone);
    view.setAccessibilityElement(false);
    view
}
