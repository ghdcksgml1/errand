//! The smaller widgets for small screens. The simple one is a line of counts and small batteries under a one-line
//! card for each task that calls the person; the smallest is the person's character alone, with a badge counting
//! those tasks. Both stand in the dock's corner, so changing between them only changes the size.

use super::dock::{AvatarView, count_pill, replace_tracking_area};
use super::kit::{self, Action, FlippedView, Weight, color, digits_font, grey, icon_image, label, ns, rect};
use super::marks::{AppMarkView, draw_app_mark};
use super::spinner::SpinningIconView;
use super::tray::{self, CardEvents, ChipMark};
use errand_core::Language;
use errand_core::layout::{self, BADGE_CIRCLE, BADGE_SIZE, LINE_GAP, LINE_HEIGHT, STACK_WIDTH, STRIP_HEIGHT};
use errand_core::ports::Poster;
use errand_core::sessions::SessionState;
use errand_core::theme::{self, Rgb};
use errand_core::view::{BadgeVm, BatteryVm, CardVm, MenuAction, MenuItemVm, SimpleVm};
use objc2::rc::Retained;
use objc2::{ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSAccessibilityButtonRole, NSAccessibilityGroupRole, NSAccessibilityLevelIndicatorRole, NSBezierPath, NSButton, NSCompositingOperation,
    NSControlStateValueOff, NSControlStateValueOn, NSCursor, NSEvent, NSGraphicsContext, NSLineCapStyle, NSMenu, NSMenuItem, NSView, NSWorkspace,
};
use objc2_core_foundation::CGPoint;
use objc2_core_graphics::CGMutablePath;
use objc2_foundation::{NSNumber, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use objc2_quartz_core::{CABasicAnimation, CAMediaTiming, CAShapeLayer};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

/// A small battery: the app's mark, the body and its cap.
const MARK: f64 = 16.0;
const BODY_WIDTH: f64 = 40.0;
const MINI_HEIGHT: f64 = 18.0;
const MINI_WIDTH: f64 = MARK + 5.5 + BODY_WIDTH + 4.0;
/// A one-line card's corners: rounder than a card's would look heavy on a card this thin.
const LINE_RADIUS: f64 = 12.0;

define_class!(
    /// One service's battery, small: its app's mark, how full it is and the percent left. It is drawn still; the
    /// dock's battery is the one whose liquid ripples.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandMiniBatteryView"]
    #[ivars = RefCell<Option<BatteryVm>>]
    pub struct MiniBatteryView;

    impl MiniBatteryView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            if let Some(vm) = self.ivars().borrow().as_ref() {
                draw_mini_battery(vm);
            }
        }
    }
);

impl MiniBatteryView {
    fn new(mtm: MainThreadMarker, origin: NSPoint, vm: &BatteryVm) -> Retained<MiniBatteryView> {
        let this = MiniBatteryView::alloc(mtm).set_ivars(RefCell::new(Some(vm.clone())));
        let view: Retained<MiniBatteryView> = unsafe { msg_send![super(this), initWithFrame: rect(origin.x, origin.y, MINI_WIDTH, MINI_HEIGHT)] };
        view.setToolTip(Some(&ns(&vm.tooltip)));
        view.setAccessibilityElement(true);
        view.setAccessibilityRole(Some(unsafe { NSAccessibilityLevelIndicatorRole }));
        view.setAccessibilityLabel(Some(&ns(&vm.accessibility_label)));
        unsafe { view.setAccessibilityValue(Some(&NSNumber::new_i64(vm.accessibility_value))) };
        view.setAccessibilityValueDescription(Some(&ns(&vm.accessibility_description)));
        view
    }
}

/// The dock's battery in miniature: the same shell, cap and colours, the liquid level and still.
fn draw_mini_battery(vm: &BatteryVm) {
    draw_app_mark(vm.provider, rect(0.0, 1.0, MARK, MARK));
    let body = rect(MARK + 5.5, 0.5, BODY_WIDTH, MINI_HEIGHT - 1.0);
    let shell = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(body, 5.5, 5.5);
    color(Rgb { red: 0.975, green: 0.973, blue: 0.966 }).setFill();
    shell.fill();
    if let Some(level) = vm.level.filter(|level| *level > 0.002) {
        let inner = kit::inset(body, 2.0, 2.0);
        // No edge line, as the dock's liquid has: at this size it would cut through the percent.
        NSGraphicsContext::saveGraphicsState_class();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(inner, 3.5, 3.5).addClip();
        kit::paint(vm.color.alpha(0.42)).setFill();
        NSBezierPath::fillRect(rect(inner.origin.x, inner.origin.y, inner.size.width * level, inner.size.height));
        NSGraphicsContext::restoreGraphicsState_class();
    }
    grey(0.80, 1.0).setStroke();
    shell.setLineWidth(1.0);
    shell.stroke();
    grey(0.78, 1.0).setFill();
    NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect(body.origin.x + body.size.width + 1.5, MINI_HEIGHT / 2.0 - 3.0, 2.5, 6.0), 1.2, 1.2).fill();
    let typeface = digits_font(11.0, Weight::Semibold);
    let size = kit::text_size(&vm.percent, &typeface);
    let origin = NSPoint::new(body.origin.x + ((body.size.width - size.width) / 2.0).round(), body.origin.y + ((body.size.height - size.height) / 2.0).round());
    kit::draw_text(&vm.percent, origin, &typeface, &color(vm.percent_color));
}

/// What the smaller widgets send.
#[derive(Clone)]
pub struct CompactEvents {
    /// The simple widget's counts: every task's line opens, or folds back to those that call.
    pub toggle_list: Rc<dyn Fn()>,
    /// Anywhere else on the simple widget's line: the usage details.
    pub toggle_details: Rc<dyn Fn()>,
    /// A click on the character: its editor, as on the dock's profile.
    pub edit_avatar: Rc<dyn Fn()>,
    pub drag_began: Rc<dyn Fn()>,
    /// The pointer moved this far from where the drag began; `done` once it is let go.
    pub dragged: Rc<dyn Fn(NSPoint, bool)>,
    /// The right-click menu: how much of the widget shows.
    pub menu: Rc<dyn Fn() -> Option<Retained<NSMenu>>>,
    /// The pointer came onto the character-only widget, or left it.
    pub hover: Rc<dyn Fn(bool)>,
}

impl CompactEvents {
    /// Events that do nothing, for pictures.
    pub fn none() -> CompactEvents {
        let nothing: Rc<dyn Fn()> = Rc::new(|| {});
        CompactEvents {
            toggle_list: nothing.clone(),
            toggle_details: nothing.clone(),
            edit_avatar: nothing,
            drag_began: Rc::new(|| {}),
            dragged: Rc::new(|_, _| {}),
            menu: Rc::new(|| None),
            hover: Rc::new(|_| {}),
        }
    }
}

/// A drag of the widget from a view that also takes clicks: the dock's own rule, a few points of travel first.
#[derive(Default)]
pub struct Drag {
    start: Cell<NSPoint>,
    moved: Cell<bool>,
}

impl Drag {
    fn down(&self) {
        self.start.set(NSEvent::mouseLocation());
        self.moved.set(false);
    }

    fn delta(&self) -> NSPoint {
        let (pointer, start) = (NSEvent::mouseLocation(), self.start.get());
        NSPoint::new(pointer.x - start.x, pointer.y - start.y)
    }

    fn dragged(&self, events: &CompactEvents) {
        let delta = self.delta();
        if !self.moved.get() && delta.x.hypot(delta.y) > 4.0 {
            self.moved.set(true);
            (events.drag_began)();
        }
        if self.moved.get() {
            (events.dragged)(delta, false);
        }
    }

    /// Ends a drag, or says the press was a click.
    fn up(&self, events: &CompactEvents) -> bool {
        if self.moved.get() {
            (events.dragged)(self.delta(), true);
        }
        !self.moved.get()
    }
}

pub struct StripIvars {
    open: Cell<bool>,
    /// Where the counts end; a click before it opens or folds the list.
    counts_end: Cell<f64>,
    events: CompactEvents,
    drag: Drag,
}

define_class!(
    /// The simple widget's own line: the counts on the left, a small battery per service on the right.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandStripView"]
    #[ivars = StripIvars]
    pub struct StripView;

    impl StripView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> *mut NSView {
            let hit: *mut NSView = unsafe { msg_send![super(self), hitTest: point] };
            if hit.is_null() { hit } else { self as *const StripView as *mut NSView }
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            self.ivars().drag.down();
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, _event: &NSEvent) {
            self.ivars().drag.dragged(&self.ivars().events);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let events = &self.ivars().events;
            if !self.ivars().drag.up(events) {
                return;
            }
            let point = self.convertPoint_fromView(event.locationInWindow(), None);
            if point.x < self.ivars().counts_end.get() { (events.toggle_list)() } else { (events.toggle_details)() }
        }

        #[unsafe(method_id(menuForEvent:))]
        fn menu_for_event(&self, _event: &NSEvent) -> Option<Retained<NSMenu>> {
            (self.ivars().events.menu)()
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let bounds = self.bounds();
            let shape = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(kit::inset(bounds, 1.0, 1.0), 14.0, 14.0);
            color(theme::WHITE).setFill();
            shape.fill();
            grey(0.84, 1.0).setStroke();
            shape.stroke();
            // Open, the counts sit on a light wash, as a pressed button would.
            let end = self.ivars().counts_end.get();
            if self.ivars().open.get() && end > 0.0 {
                kit::paint(theme::INK.alpha(0.06)).setFill();
                NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect(5.0, 7.0, end - 5.0, STRIP_HEIGHT - 14.0), 13.0, 13.0).fill();
            }
        }
    }
);

fn strip(vm: &SimpleVm, events: &CompactEvents, mtm: MainThreadMarker) -> Retained<StripView> {
    let this = StripView::alloc(mtm).set_ivars(StripIvars { open: Cell::new(vm.open), counts_end: Cell::new(0.0), events: events.clone(), drag: Drag::default() });
    let view: Retained<StripView> = unsafe { msg_send![super(this), initWithFrame: rect(0.0, 0.0, STACK_WIDTH, STRIP_HEIGHT)] };
    // The batteries keep the right end, the last service at the edge as in the dock's order.
    let mut right = STACK_WIDTH - 12.0;
    for battery in vm.batteries.iter().rev() {
        right -= MINI_WIDTH;
        view.addSubview(&MiniBatteryView::new(mtm, NSPoint::new(right, (STRIP_HEIGHT - MINI_HEIGHT) / 2.0), battery));
        right -= 10.0;
    }
    if let Some(idle) = &vm.idle {
        let avatar = AvatarView::new(mtm, rect(8.0, 8.0, 24.0, 24.0), None);
        avatar.show(idle.avatar);
        view.addSubview(&avatar);
        let words = label(idle.words, 12.0, Weight::Medium, theme::MUTED, mtm);
        words.setFrame(rect(38.0, 11.5, right - 38.0, 17.0));
        view.addSubview(&words);
    }
    // Counts that would run into the batteries give way, the least pressing first, as the task header's do.
    let mut x = 10.0;
    for pill in &vm.pills {
        let count = count_pill(pill.count, pill.icon, pill.spinning, pill.color, &pill.words, NSPoint::new(x, (STRIP_HEIGHT - 18.0) / 2.0), mtm);
        if x + count.frame().size.width > right - 4.0 {
            break;
        }
        count.setToolTip(Some(&ns(&format!("{}\n{}", pill.words, vm.counts_tooltip))));
        x += count.frame().size.width + 5.0;
        view.addSubview(&count);
    }
    if !vm.pills.is_empty() {
        view.ivars().counts_end.set(x);
    }
    view.setAccessibilityElement(true);
    view.setAccessibilityRole(Some(unsafe { NSAccessibilityGroupRole }));
    view.setAccessibilityLabel(Some(&ns(&vm.summary)));
    view
}

pub struct LineIvars {
    state: SessionState,
    chip: &'static str,
    pending: bool,
    hovering: Cell<bool>,
    open: Rc<dyn Fn()>,
}

define_class!(
    /// A task's one-line card over the simple widget's line: a click opens its session, its circle checks it off.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandLineCard"]
    #[ivars = LineIvars]
    pub struct LineCard;

    impl LineCard {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> *mut NSView {
            let hit: *mut NSView = unsafe { msg_send![super(self), hitTest: point] };
            match unsafe { hit.as_ref() } {
                Some(view) if view.isKindOfClass(NSButton::class()) => hit,
                Some(_) => self as *const LineCard as *mut NSView,
                None => std::ptr::null_mut(),
            }
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            if kit::contains(self.bounds(), self.convertPoint_fromView(event.locationInWindow(), None)) {
                (self.ivars().open)();
            }
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn accessibility_perform_press(&self) -> bool {
            (self.ivars().open)();
            true
        }

        #[unsafe(method(updateTrackingAreas))]
        fn update_tracking_areas(&self) {
            let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
            replace_tracking_area(self);
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            NSCursor::pointingHandCursor().set();
            self.ivars().hovering.set(true);
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            NSCursor::arrowCursor().set();
            self.ivars().hovering.set(false);
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let ivars = self.ivars();
            let state = if ivars.pending { SessionState::Done } else { ivars.state };
            let style = theme::card_style(state, ivars.hovering.get() && !ivars.pending);
            let bounds = self.bounds();
            let card = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(kit::inset(bounds, 1.0, 1.0), LINE_RADIUS, LINE_RADIUS);
            color(style.fill).setFill();
            card.fill();
            kit::paint(style.edge).setStroke();
            card.setLineWidth(style.edge_width);
            card.stroke();
            // A checked line shows its 되돌리기 where the chip would be.
            if !ivars.pending {
                let mark = tray::chip_mark(state);
                tray::draw_chip(line_chip(bounds.size.width, ivars.chip, mark), ivars.chip, mark, &style);
            }
        }
    }
);

/// A one-line card's chip: at its right end, in the middle of the line.
fn line_chip(width: f64, text: &str, mark: ChipMark) -> NSRect {
    let chip = tray::chip_width(text, mark);
    rect(width - 10.0 - chip, (LINE_HEIGHT - 18.0) / 2.0, chip, 18.0)
}

/// One task's line, `y` down the widget; the actions it returns must outlive its buttons.
fn line(vm: &CardVm, y: f64, events: &CardEvents, mtm: MainThreadMarker) -> (Retained<LineCard>, Vec<Retained<Action>>) {
    let pending = vm.countdown.is_some();
    let this = LineCard::alloc(mtm).set_ivars(LineIvars { state: vm.state, chip: vm.chip, pending, hovering: Cell::new(false), open: tray::open_session(vm, events) });
    let view: Retained<LineCard> = unsafe { msg_send![super(this), initWithFrame: rect(0.0, y, STACK_WIDTH, LINE_HEIGHT)] };
    let middle = LINE_HEIGHT / 2.0;
    let (check, check_action) = tray::check_button(vm, rect(6.0, middle - 10.0, 24.0, 20.0), events, mtm);
    view.addSubview(&check);
    let mut actions = vec![check_action];
    let mark = AppMarkView::new(mtm, rect(34.0, middle - MARK / 2.0, MARK, MARK), vm.provider);
    view.addSubview(&mark);
    let chip = line_chip(STACK_WIDTH, vm.chip, tray::chip_mark(vm.state));
    let title_x = 34.0 + MARK + 7.0;
    let title_end = if pending { STACK_WIDTH - 10.0 - 78.0 } else { chip.origin.x };
    let heading = label(&vm.title, 12.5, Weight::Semibold, theme::INK, mtm);
    heading.setFrame(rect(title_x, middle - 8.5, title_end - 8.0 - title_x, 17.0));
    view.addSubview(&heading);
    if vm.state == SessionState::Running && !pending {
        view.addSubview(&SpinningIconView::new(mtm, tray::chip_icon_frame(chip), "loader-circle", theme::RUNNING_BRIGHT));
    }
    if pending {
        heading.setAttributedStringValue(&tray::struck(&vm.title, &heading));
        mark.setAlphaValue(0.45);
        let (undo, undo_action) = tray::undo_button(vm, rect(STACK_WIDTH - 10.0 - 78.0, middle - 10.0, 78.0, 20.0), events, mtm);
        view.addSubview(&undo);
        actions.push(undo_action);
    }
    tray::finish(&view, vm, events.language);
    (view, actions)
}

/// The simple widget: one-line cards stacked over its line, the line at the bottom so it stays put in the corner.
pub struct Simple {
    pub view: Retained<FlippedView>,
    events: CardEvents,
    strip_events: CompactEvents,
    actions: Vec<Retained<Action>>,
    shown: Option<SimpleVm>,
}

impl Simple {
    pub fn new(mtm: MainThreadMarker, poster: Arc<dyn Poster>, language: Language, strip_events: CompactEvents) -> Simple {
        let view = FlippedView::new(mtm, rect(0.0, 0.0, STACK_WIDTH, STRIP_HEIGHT));
        Simple { view, events: CardEvents { poster, language }, strip_events, actions: Vec::new(), shown: None }
    }

    /// Lays the lines and the line out and returns the height they take.
    pub fn render(&mut self, vm: &SimpleVm, mtm: MainThreadMarker) -> f64 {
        let height = layout::simple_height(vm.lines.len());
        if self.shown.as_ref() == Some(vm) {
            return height;
        }
        kit::clear(&self.view);
        self.actions.clear();
        self.view.setFrameSize(NSSize::new(STACK_WIDTH, height));
        for (index, line_vm) in vm.lines.iter().enumerate() {
            let (view, actions) = line(line_vm, index as f64 * (LINE_HEIGHT + LINE_GAP), &self.events, mtm);
            self.view.addSubview(&view);
            self.actions.extend(actions);
        }
        let strip = strip(vm, &self.strip_events, mtm);
        strip.setFrameOrigin(NSPoint::new(0.0, height - STRIP_HEIGHT));
        self.view.addSubview(&strip);
        self.shown = Some(vm.clone());
        height
    }
}

pub struct BadgeIvars {
    vm: RefCell<Option<BadgeVm>>,
    events: CompactEvents,
    drag: Drag,
}

define_class!(
    /// The character-only widget: the person's character in a circle. An arc turns round it while work is under way,
    /// and its marks sit on top.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandBadgeView"]
    #[ivars = BadgeIvars]
    pub struct BadgeView;

    impl BadgeView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> *mut NSView {
            let hit: *mut NSView = unsafe { msg_send![super(self), hitTest: point] };
            if hit.is_null() { hit } else { self as *const BadgeView as *mut NSView }
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            self.ivars().drag.down();
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, _event: &NSEvent) {
            self.ivars().drag.dragged(&self.ivars().events);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: &NSEvent) {
            if self.ivars().drag.up(&self.ivars().events) {
                (self.ivars().events.edit_avatar)();
            }
        }

        #[unsafe(method_id(menuForEvent:))]
        fn menu_for_event(&self, _event: &NSEvent) -> Option<Retained<NSMenu>> {
            (self.ivars().events.menu)()
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn accessibility_perform_press(&self) -> bool {
            (self.ivars().events.edit_avatar)();
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            if let Some(vm) = self.ivars().vm.borrow().as_ref() {
                draw_face(vm);
            }
        }
    }
);

/// The circle and the character in it, as the dock's profile shows it, larger.
fn draw_face(vm: &BadgeVm) {
    let disc = NSBezierPath::bezierPathWithOvalInRect(kit::inset(circle(), 0.5, 0.5));
    color(theme::WHITE).setFill();
    disc.fill();
    grey(0.84, 1.0).setStroke();
    disc.setLineWidth(1.0);
    disc.stroke();
    let face = kit::inset(circle(), 5.0, 5.0);
    kit::paint(theme::HEALTHY.alpha(0.14)).setFill();
    NSBezierPath::bezierPathWithOvalInRect(face).fill();
    kit::draw_avatar(&vm.avatar, face);
}

/// The character's circle in the badge view, with room around it for the marks on its edge.
fn circle() -> NSRect {
    let inset = (BADGE_SIZE - BADGE_CIRCLE) / 2.0;
    rect(inset, inset, BADGE_CIRCLE, BADGE_CIRCLE)
}

/// The running arc's radius, just inside the circle's edge.
const ARC_RADIUS: f64 = BADGE_CIRCLE / 2.0 - 2.0;
/// How much of the circle the arc covers, in degrees.
const ARC_SWEEP: f64 = 100.0;

define_class!(
    /// The arc that turns round the character while work is under way. Core Animation turns it, so it costs the app
    /// nothing per frame; snapshots and Reduce Motion show it still.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandRunningArcView"]
    #[ivars = RefCell<Option<Retained<CAShapeLayer>>>]
    pub struct ArcView;

    impl ArcView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(viewDidMoveToWindow))]
        fn view_did_move_to_window(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidMoveToWindow] };
            if let Some(arc) = self.ivars().borrow_mut().take() {
                arc.removeFromSuperlayer();
            }
            self.setNeedsDisplay(true);
            if self.window().is_none() || NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion() {
                return;
            }
            self.setWantsLayer(true);
            let Some(layer) = self.layer() else { return };
            let arc = CAShapeLayer::layer();
            arc.setFrame(layer.bounds());
            let path = CGMutablePath::new();
            let middle = BADGE_SIZE / 2.0;
            unsafe { CGMutablePath::add_arc(Some(&path), std::ptr::null(), middle, middle, ARC_RADIUS, (-90.0f64).to_radians(), (ARC_SWEEP - 90.0).to_radians(), false) };
            arc.setPath(Some(&path));
            arc.setFillColor(None);
            arc.setStrokeColor(Some(&color(theme::RUNNING_BRIGHT).CGColor()));
            arc.setLineWidth(3.0);
            unsafe { arc.setLineCap(objc2_quartz_core::kCALineCapRound) };
            arc.setPosition(CGPoint::new(middle, middle));
            let spin = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("transform.rotation.z")));
            unsafe {
                spin.setFromValue(Some(&NSNumber::new_f64(0.0)));
                spin.setToValue(Some(&NSNumber::new_f64(-2.0 * std::f64::consts::PI)));
            }
            // Slower than a chip's spinner: it is bigger, and should not pull the eye from the screen's work.
            spin.setDuration(2.4);
            spin.setRepeatCount(f32::INFINITY);
            arc.addAnimation_forKey(&spin, Some(&NSString::from_str("spin")));
            layer.addSublayer(&arc);
            *self.ivars().borrow_mut() = Some(arc);
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            // On screen the turning layer shows the arc.
            if self.ivars().borrow().is_some() && NSGraphicsContext::currentContextDrawingToScreen() {
                return;
            }
            let arc = NSBezierPath::bezierPath();
            let middle = NSPoint::new(BADGE_SIZE / 2.0, BADGE_SIZE / 2.0);
            arc.appendBezierPathWithArcWithCenter_radius_startAngle_endAngle(middle, ARC_RADIUS, -90.0, ARC_SWEEP - 90.0);
            arc.setLineWidth(3.0);
            arc.setLineCapStyle(NSLineCapStyle::Round);
            color(theme::RUNNING_BRIGHT).setStroke();
            arc.stroke();
        }
    }
);

define_class!(
    /// The marks on the character's edge, over the arc: how many tasks call, and a red battery when a service runs low.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandBadgeMarksView"]
    #[ivars = RefCell<Option<BadgeVm>>]
    pub struct MarksView;

    impl MarksView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            if let Some(vm) = self.ivars().borrow().as_ref() {
                draw_marks(vm);
            }
        }
    }
);

fn draw_marks(vm: &BadgeVm) {
    if let Some((count, tint)) = vm.count {
        let text = count.to_string();
        let typeface = digits_font(11.5, Weight::Bold);
        let size = kit::text_size(&text, &typeface);
        let width = (size.width + 12.0).max(21.0).ceil();
        let frame = rect(BADGE_SIZE - width, 0.0, width, 21.0);
        color(theme::WHITE).setFill();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(frame, 10.5, 10.5).fill();
        color(tint).setFill();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(kit::inset(frame, 2.0, 2.0), 8.5, 8.5).fill();
        let origin = NSPoint::new(frame.origin.x + ((width - size.width) / 2.0).round(), ((frame.size.height - size.height) / 2.0).round());
        kit::draw_text(&text, origin, &typeface, &color(theme::WHITE));
    }
    if vm.low {
        // Filled, as the count is: an outlined battery this small reads as a bracketed mark.
        let frame = rect(0.0, BADGE_SIZE - 22.0, 22.0, 22.0);
        color(theme::WHITE).setFill();
        NSBezierPath::bezierPathWithOvalInRect(frame).fill();
        color(theme::LOW).setFill();
        NSBezierPath::bezierPathWithOvalInRect(kit::inset(frame, 2.0, 2.0)).fill();
        if let Some(icon) = icon_image("battery-warning", 12.0, Some(theme::WHITE), 0.0, 0.0) {
            unsafe { icon.drawInRect_fromRect_operation_fraction_respectFlipped_hints(kit::inset(frame, 5.0, 5.0), NSRect::ZERO, NSCompositingOperation::SourceOver, 1.0, true, None) };
        }
    }
}

pub struct Badge {
    pub view: Retained<BadgeView>,
    arc: Retained<ArcView>,
    marks: Retained<MarksView>,
    shown: Option<BadgeVm>,
}

impl Badge {
    pub fn new(mtm: MainThreadMarker, events: CompactEvents, language: Language) -> Badge {
        let this = BadgeView::alloc(mtm).set_ivars(BadgeIvars { vm: RefCell::new(None), events, drag: Drag::default() });
        let view: Retained<BadgeView> = unsafe { msg_send![super(this), initWithFrame: rect(0.0, 0.0, BADGE_SIZE, BADGE_SIZE)] };
        let arc: Retained<ArcView> = unsafe { msg_send![super(ArcView::alloc(mtm).set_ivars(RefCell::new(None))), initWithFrame: view.bounds()] };
        arc.setHidden(true);
        view.addSubview(&arc);
        let marks: Retained<MarksView> = unsafe { msg_send![super(MarksView::alloc(mtm).set_ivars(RefCell::new(None))), initWithFrame: view.bounds()] };
        view.addSubview(&marks);
        view.setAccessibilityElement(true);
        view.setAccessibilityRole(Some(unsafe { NSAccessibilityButtonRole }));
        view.setAccessibilityHelp(Some(&ns(language.edit_avatar())));
        Badge { view, arc, marks, shown: None }
    }

    pub fn render(&mut self, vm: &BadgeVm) {
        if self.shown.as_ref() == Some(vm) {
            return;
        }
        *self.view.ivars().vm.borrow_mut() = Some(vm.clone());
        *self.marks.ivars().borrow_mut() = Some(vm.clone());
        self.arc.setHidden(!vm.running);
        self.view.setToolTip(Some(&ns(&vm.tooltip)));
        self.view.setAccessibilityLabel(Some(&ns(&vm.tooltip)));
        self.view.setNeedsDisplay(true);
        self.marks.setNeedsDisplay(true);
        self.shown = Some(vm.clone());
    }
}

pub struct CharacterIvars {
    open: Cell<bool>,
    events: CompactEvents,
}

define_class!(
    /// The character-only widget with, while the pointer rests on it, the simple widget over it. Leaving the whole of
    /// it folds the simple widget away.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandCharacterView"]
    #[ivars = CharacterIvars]
    pub struct CharacterView;

    impl CharacterView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(updateTrackingAreas))]
        fn update_tracking_areas(&self) {
            let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
            replace_tracking_area(self);
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            (self.ivars().events.hover)(true);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            (self.ivars().events.hover)(false);
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            // Open, the gaps hold a wash too faint to see, so the pointer crossing them is still over the widget.
            if self.ivars().open.get() {
                grey(1.0, 0.012).setFill();
                NSBezierPath::fillRect(self.bounds());
            }
        }
    }
);

/// The character-only widget and the simple widget it opens into.
pub struct Character {
    pub view: Retained<CharacterView>,
    badge: Badge,
    simple: Simple,
}

impl Character {
    pub fn new(mtm: MainThreadMarker, poster: Arc<dyn Poster>, language: Language, events: CompactEvents) -> Character {
        let this = CharacterView::alloc(mtm).set_ivars(CharacterIvars { open: Cell::new(false), events: events.clone() });
        let view: Retained<CharacterView> = unsafe { msg_send![super(this), initWithFrame: rect(0.0, 0.0, BADGE_SIZE, BADGE_SIZE)] };
        let badge = Badge::new(mtm, events.clone(), language);
        view.addSubview(&badge.view);
        let simple = Simple::new(mtm, poster, language, events);
        Character { view, badge, simple }
    }

    /// Lays the character out, with the simple widget over it when given, and returns the size it all takes. The
    /// circle keeps the side `on_right` says, so opening never moves it from under the pointer.
    pub fn render(&mut self, badge: &BadgeVm, simple: Option<&SimpleVm>, on_right: bool, mtm: MainThreadMarker) -> NSSize {
        self.badge.render(badge);
        let inset = (BADGE_SIZE - BADGE_CIRCLE) / 2.0;
        let size = match simple {
            None => {
                self.simple.view.removeFromSuperview();
                self.badge.view.setFrameOrigin(NSPoint::ZERO);
                NSSize::new(BADGE_SIZE, BADGE_SIZE)
            }
            Some(vm) => {
                let height = self.simple.render(vm, mtm);
                if unsafe { self.simple.view.superview() }.is_none() {
                    self.view.addSubview(&self.simple.view);
                }
                let width = STACK_WIDTH + inset;
                self.simple.view.setFrameOrigin(NSPoint::new(if on_right { 0.0 } else { inset }, 0.0));
                // The circle's edge in line with the simple widget's, 8 points under it.
                let top = height + layout::STACK_GAP - inset;
                self.badge.view.setFrameOrigin(NSPoint::new(if on_right { width - BADGE_SIZE } else { 0.0 }, top));
                NSSize::new(width, top + BADGE_SIZE)
            }
        };
        self.view.ivars().open.set(simple.is_some());
        self.view.setFrameSize(size);
        self.view.setNeedsDisplay(true);
        size
    }
}

/// The widget's right-click menu: how much of it shows, the one showing checked. `actions` keeps the items' targets
/// alive while the menu is up.
pub fn mode_menu(items: &[MenuItemVm], on_action: &Rc<dyn Fn(MenuAction)>, actions: &RefCell<Vec<Retained<Action>>>, mtm: MainThreadMarker) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm);
    menu.setAutoenablesItems(false);
    let mut targets = Vec::new();
    for vm in items {
        let item = unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &ns(&vm.title), Some(sel!(perform:)), &ns("")) };
        if let Some(action) = vm.action {
            let on_action = on_action.clone();
            let target = Action::new(mtm, move || on_action(action));
            unsafe { item.setTarget(Some(&target)) };
            targets.push(target);
        }
        item.setState(if vm.checked { NSControlStateValueOn } else { NSControlStateValueOff });
        menu.addItem(&item);
    }
    *actions.borrow_mut() = targets;
    menu
}
