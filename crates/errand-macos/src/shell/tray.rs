//! The task cards stacked on the dock. A card opens its session when clicked; its circle checks it off,
//! after which it drains a countdown and offers 되돌리기 before it clears.

use super::dock::{replace_tracking_area, tinted_icon};
use super::kit::{self, Action, CHIP_ICON, FlippedView, Weight, color, fitting_width, grey, icon_image, label, ns, rect};
use super::marks::AppMarkView;
use super::spinner::SpinningIconView;
use errand_core::Language;
use errand_core::layout::{CARD_GAP, MORE_BAR_HEIGHT, STACK_WIDTH};
use errand_core::model::Msg;
use errand_core::ports::Poster;
use errand_core::sessions::SessionState;
use errand_core::theme::{self, CardStyle};
use errand_core::view::{CardVm, TrayVm};
use objc2::rc::Retained;
use objc2::{ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{
    NSAccessibility, NSAccessibilityButtonRole, NSAccessibilityCheckBoxRole, NSAnimatablePropertyContainer, NSAnimationContext, NSBezierPath, NSButton, NSCellImagePosition,
    NSCursor, NSEvent, NSGraphicsContext, NSScrollElasticity, NSScrollView, NSScrollerStyle, NSTextAlignment, NSTextField, NSUserInterfaceItemIdentification, NSView,
    NSViewBoundsDidChangeNotification, NSWorkspace,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{NSAttributedString, NSMutableAttributedString, NSNotificationCenter, NSNumber, NSObjectProtocol, NSPoint, NSRange, NSRect, NSSize};
use objc2_quartz_core::{CABasicAnimation, CALayer, CAMediaTiming};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

const CONTENT_X: f64 = 40.0;

fn chip_font() -> Retained<objc2_app_kit::NSFont> {
    kit::font(10.5, Weight::Semibold)
}

/// What sits before a chip's words.
#[derive(Clone, Copy)]
pub enum ChipMark<'a> {
    Icon(&'a str),
    /// The spot is left for a spinner laid over it.
    Spinner,
    Nothing,
}

/// A chip's width: its words, with room for an icon or a spinner before them.
pub fn chip_width(text: &str, mark: ChipMark) -> f64 {
    let words = kit::text_width(text, &chip_font()).ceil();
    match mark {
        ChipMark::Nothing => words + 18.0,
        _ => words + CHIP_ICON + 20.0,
    }
}

/// Where a card's state chip sits: the top right, as wide as its words.
pub fn chip_rect(card_width: f64, text: &str, mark: ChipMark) -> NSRect {
    let width = chip_width(text, mark);
    rect(card_width - 14.0 - width, 12.0, width, 18.0)
}

/// The spot a chip's icon takes; a spinner goes there for running work.
pub fn chip_icon_frame(chip: NSRect) -> NSRect {
    rect(chip.origin.x + 8.0, chip.origin.y + chip.size.height / 2.0 - CHIP_ICON / 2.0, CHIP_ICON, CHIP_ICON)
}

/// A task card's body, in the colours of its state.
pub fn draw_card(bounds: NSRect, style: &CardStyle) {
    let card = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(kit::inset(bounds, 1.0, 1.0), 16.0, 16.0);
    color(style.fill).setFill();
    card.fill();
    kit::paint(style.edge).setStroke();
    card.setLineWidth(style.edge_width);
    card.stroke();
}

/// A chip in `frame`: its wash, its icon, and its words in the state's colour.
pub fn draw_chip(frame: NSRect, text: &str, mark: ChipMark, style: &CardStyle) {
    kit::paint(style.chip_fill).setFill();
    NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(frame, 9.0, 9.0).fill();
    if let ChipMark::Icon(name) = mark
        && let Some(icon) = icon_image(name, CHIP_ICON, Some(style.signal), 0.0, 0.0)
    {
        unsafe { icon.drawInRect_fromRect_operation_fraction_respectFlipped_hints(chip_icon_frame(frame), NSRect::ZERO, objc2_app_kit::NSCompositingOperation::SourceOver, 1.0, true, None) };
    }
    let typeface = chip_font();
    let size = kit::text_size(text, &typeface);
    let x = match mark {
        ChipMark::Nothing => frame.origin.x + 9.0,
        _ => frame.origin.x + 12.0 + CHIP_ICON,
    };
    kit::draw_text(text, NSPoint::new(x, frame.origin.y + frame.size.height / 2.0 - size.height / 2.0), &typeface, &color(style.signal));
}

pub struct CheckIvars {
    checked: Cell<bool>,
    hovering: Cell<bool>,
}

define_class!(
    /// The to-do circle that clears a card: hover previews the check, a click fills it.
    #[unsafe(super(NSButton))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandCheckButton"]
    #[ivars = CheckIvars]
    pub struct CheckButton;

    impl CheckButton {
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
            self.ivars().hovering.set(true);
            NSView::setNeedsDisplay(self, true);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            self.ivars().hovering.set(false);
            NSView::setNeedsDisplay(self, true);
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let bounds = self.bounds();
            let circle = kit::inset(rect(bounds.size.width / 2.0 - 9.0, bounds.size.height / 2.0 - 9.0, 18.0, 18.0), 0.75, 0.75);
            let ring = NSBezierPath::bezierPathWithOvalInRect(circle);
            let mark = kit::inset(circle, 2.75, 2.75);
            if self.ivars().checked.get() {
                color(theme::DONE).setFill();
                ring.fill();
                if let Some(check) = icon_image("check", 12.0, Some(theme::WHITE), 0.0, 0.0) {
                    check.drawInRect(mark);
                }
                return;
            }
            let hovering = self.ivars().hovering.get();
            if hovering { color(theme::DONE).setStroke() } else { grey(0.70, 1.0).setStroke() }
            ring.setLineWidth(1.5);
            ring.stroke();
            if hovering && let Some(check) = icon_image("check", 12.0, Some(theme::DONE), 0.0, 0.0) {
                check.drawInRect_fromRect_operation_fraction(mark, NSRect::ZERO, objc2_app_kit::NSCompositingOperation::SourceOver, 0.55);
            }
        }
    }
);

pub struct CountdownIvars {
    fraction: Cell<f64>,
    remaining: Cell<f64>,
    fill: RefCell<Option<Retained<CALayer>>>,
}

define_class!(
    /// Drains along a checked card's foot until the card clears, so the wait reads as "about to go".
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandCountdownBar"]
    #[ivars = CountdownIvars]
    pub struct CountdownBar;

    impl CountdownBar {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(viewDidMoveToWindow))]
        fn view_did_move_to_window(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidMoveToWindow] };
            if let Some(fill) = self.ivars().fill.borrow_mut().take() {
                fill.removeFromSuperlayer();
            }
            self.setNeedsDisplay(true);
            if self.window().is_none() || self.ivars().remaining.get() <= 0.0 || NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion() {
                return;
            }
            self.setWantsLayer(true);
            let Some(layer) = self.layer() else { return };
            let height = self.bounds().size.height;
            let fill = CALayer::layer();
            fill.setBackgroundColor(Some(&color(theme::DONE).CGColor()));
            fill.setCornerRadius(height / 2.0);
            fill.setAnchorPoint(CGPoint::new(0.0, 0.0));
            fill.setPosition(CGPoint::new(0.0, 0.0));
            fill.setBounds(CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(0.0, height)));
            let drain = CABasicAnimation::animationWithKeyPath(Some(&ns("bounds.size.width")));
            unsafe {
                drain.setFromValue(Some(&NSNumber::new_f64(self.bounds().size.width * self.ivars().fraction.get())));
                drain.setToValue(Some(&NSNumber::new_f64(0.0)));
            }
            drain.setDuration(self.ivars().remaining.get());
            fill.addAnimation_forKey(&drain, Some(&ns("drain")));
            layer.addSublayer(&fill);
            *self.ivars().fill.borrow_mut() = Some(fill);
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let bounds = self.bounds();
            let radius = bounds.size.height / 2.0;
            kit::paint(theme::DONE.alpha(0.15)).setFill();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(bounds, radius, radius).fill();
            // On screen the draining layer shows what is left; snapshots and Reduce Motion draw it still.
            if self.ivars().fill.borrow().is_some() && NSGraphicsContext::currentContextDrawingToScreen() {
                return;
            }
            color(theme::DONE).setFill();
            let left = rect(0.0, 0.0, bounds.size.width * self.ivars().fraction.get(), bounds.size.height);
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(left, radius, radius).fill();
        }
    }
);

impl CountdownBar {
    fn new(mtm: MainThreadMarker, frame: NSRect, remaining: f64, fraction: f64, language: Language) -> Retained<CountdownBar> {
        let this = CountdownBar::alloc(mtm).set_ivars(CountdownIvars { fraction: Cell::new(fraction), remaining: Cell::new(remaining), fill: RefCell::new(None) });
        let bar: Retained<CountdownBar> = unsafe { msg_send![super(this), initWithFrame: frame] };
        bar.setToolTip(Some(&ns(language.clears_soon())));
        bar
    }
}

pub struct CardIvars {
    key: String,
    state: SessionState,
    /// The chip's words: the card's state, or done while it is checked.
    chip: &'static str,
    pending: bool,
    compact: bool,
    hovering: Cell<bool>,
    hint: RefCell<Option<Retained<FlippedView>>>,
    open: Rc<dyn Fn()>,
}

define_class!(
    /// One task card; clicking anywhere on it opens its session, its buttons keep their own clicks.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandSessionCard"]
    #[ivars = CardIvars]
    pub struct SessionCard;

    impl SessionCard {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> *mut NSView {
            let hit: *mut NSView = unsafe { msg_send![super(self), hitTest: point] };
            match unsafe { hit.as_ref() } {
                Some(view) if view.isKindOfClass(NSButton::class()) => hit,
                Some(_) => self as *const SessionCard as *mut NSView,
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

        #[unsafe(method(viewDidMoveToWindow))]
        fn view_did_move_to_window(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidMoveToWindow] };
            // Cards are rebuilt while the pointer may already rest on one.
            let Some(window) = self.window() else { return };
            let pointer = self.convertPoint_fromView(window.mouseLocationOutsideOfEventStream(), None);
            if kit::contains(self.bounds(), pointer) && kit::contains(window.frame(), NSEvent::mouseLocation()) {
                self.set_hover(true);
            }
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            self.set_hover(true);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            self.set_hover(false);
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let ivars = self.ivars();
            let state = if ivars.pending { SessionState::Done } else { ivars.state };
            let style = theme::card_style(state, ivars.hovering.get() && !ivars.pending);
            draw_card(self.bounds(), &style);
            // A folded card that is checked shows its 되돌리기 where the chip would be.
            if ivars.pending && ivars.compact {
                return;
            }
            // A running chip's icon is the spinner laid over its spot.
            let mark = if state == SessionState::Running { ChipMark::Spinner } else { ChipMark::Icon(state.icon()) };
            draw_chip(chip_rect(self.bounds().size.width, ivars.chip, mark), ivars.chip, mark, &style);
        }
    }
);

impl SessionCard {
    fn set_hover(&self, hover: bool) {
        self.ivars().hovering.set(hover);
        if let Some(hint) = self.ivars().hint.borrow().as_ref() {
            hint.setHidden(!hover || self.ivars().pending);
        }
        if hover { NSCursor::pointingHandCursor().set() } else { NSCursor::arrowCursor().set() }
        self.setNeedsDisplay(true);
    }

    pub fn key(&self) -> &str {
        &self.ivars().key
    }
}

/// What a card's controls send, and the language its own words are in.
#[derive(Clone)]
pub struct CardEvents {
    pub poster: Arc<dyn Poster>,
    pub language: Language,
}

fn undo_button(vm: &CardVm, frame: NSRect, events: &CardEvents, mtm: MainThreadMarker) -> (Retained<NSButton>, Retained<Action>) {
    let undo = NSButton::new(mtm);
    undo.setTitle(&ns(events.language.undo()));
    undo.setBordered(false);
    undo.setFont(Some(&kit::font(11.0, Weight::Semibold)));
    undo.setContentTintColor(Some(&color(theme::DONE)));
    undo.setImagePosition(NSCellImagePosition::ImageLeading);
    undo.setImageHugsTitle(true);
    undo.setImage(icon_image("undo-2", 12.0, None, 0.0, 4.0).as_deref());
    undo.setFrame(frame);
    undo.setAlignment(NSTextAlignment::Right);
    undo.setAccessibilityLabel(Some(&ns(&vm.undo_label)));
    let (poster, key) = (events.poster.clone(), vm.key.clone());
    let action = Action::new(mtm, move || poster.post(Msg::ToggleComplete(key.clone())));
    action.attach(&undo);
    (undo, action)
}

/// Builds one card from its view model; the actions it returns must outlive the card's buttons.
fn card(vm: &CardVm, width: f64, events: &CardEvents, mtm: MainThreadMarker) -> (Retained<SessionCard>, Vec<Retained<Action>>) {
    let pending = vm.countdown.is_some();
    let open: Rc<dyn Fn()> = {
        let (poster, key) = (events.poster.clone(), vm.key.clone());
        Rc::new(move || poster.post(Msg::OpenSession(key.clone())))
    };
    let own_state = if pending { SessionState::Done } else { vm.state };
    let this = SessionCard::alloc(mtm).set_ivars(CardIvars {
        key: vm.key.clone(),
        state: own_state,
        chip: vm.chip,
        pending,
        compact: vm.compact,
        hovering: Cell::new(false),
        hint: RefCell::new(None),
        open,
    });
    let view: Retained<SessionCard> = unsafe { msg_send![super(this), initWithFrame: rect(0.0, 0.0, width, vm.height)] };
    let mut actions = Vec::new();
    let mut content: Vec<Retained<NSView>> = Vec::new();

    let check: Retained<CheckButton> = {
        let this = CheckButton::alloc(mtm).set_ivars(CheckIvars { checked: Cell::new(pending), hovering: Cell::new(false) });
        unsafe { msg_send![super(this), initWithFrame: rect(10.0, 10.5, 24.0, 20.0)] }
    };
    check.setBordered(false);
    check.setTitle(&ns(""));
    check.setAccessibilityRole(Some(unsafe { NSAccessibilityCheckBoxRole }));
    unsafe { check.setAccessibilityValue(Some(&NSNumber::new_bool(pending))) };
    check.setToolTip(Some(&ns(vm.check_tooltip)));
    check.setAccessibilityLabel(Some(&ns(&vm.check_label)));
    let check_action = {
        let (poster, key) = (events.poster.clone(), vm.key.clone());
        Action::new(mtm, move || poster.post(Msg::ToggleComplete(key.clone())))
    };
    check_action.attach(&check);
    actions.push(check_action);
    view.addSubview(&check);
    let mark = AppMarkView::new(mtm, rect(CONTENT_X, 11.5, 18.0, 18.0), vm.provider);
    view.addSubview(&mark);
    content.push(Retained::into_super(mark));

    let heading_end = if pending && vm.compact { width - 14.0 - 78.0 } else { chip_rect(width, vm.chip, ChipMark::Spinner).origin.x };
    let heading = label(&vm.title, 13.0, Weight::Semibold, theme::INK, mtm);
    heading.setFrame(rect(CONTENT_X + 24.0, 11.0, heading_end - 8.0 - CONTENT_X - 24.0, 19.0));
    heading.setToolTip(Some(&ns(&vm.title)));
    if pending {
        heading.setAttributedStringValue(&struck(&vm.title, &heading));
    }
    view.addSubview(&heading);
    if vm.state == SessionState::Running && !pending {
        let frame = chip_icon_frame(chip_rect(width, vm.chip, ChipMark::Spinner));
        view.addSubview(&SpinningIconView::new(mtm, frame, "loader-circle", theme::RUNNING_BRIGHT));
    }
    let prompt_color = if vm.prompt_missing { theme::MUTED } else { theme::INK };

    if vm.compact {
        // Folded: the title and the request stay, so "what did I ask" survives a crowded stack.
        let ask = label(&vm.prompt, 12.0, Weight::Regular, prompt_color, mtm);
        ask.setFrame(rect(CONTENT_X, 36.0, width - CONTENT_X - 14.0, 17.0));
        view.addSubview(&ask);
        if let Some(countdown) = vm.countdown {
            ask.setAlphaValue(0.45);
            content[0].setAlphaValue(0.45);
            let (undo, undo_action) = undo_button(vm, rect(width - 14.0 - 78.0, 10.5, 78.0, 20.0), events, mtm);
            view.addSubview(&undo);
            actions.push(undo_action);
            let bar_frame = rect(CONTENT_X, vm.height - 6.0, width - CONTENT_X - 14.0, 2.5);
            view.addSubview(&CountdownBar::new(mtm, bar_frame, countdown.remaining.as_secs_f64(), countdown.fraction, events.language));
        }
        finish(&view, vm, events.language);
        return (view, actions);
    }

    let ask = NSTextField::wrappingLabelWithString(&ns(&vm.prompt), mtm);
    ask.setFont(Some(&kit::font(12.0, Weight::Regular)));
    ask.setTextColor(Some(&color(prompt_color)));
    ask.setMaximumNumberOfLines(2);
    if let Some(cell) = ask.cell() {
        cell.setTruncatesLastVisibleLine(true);
    }
    ask.setSelectable(false);
    ask.setFrame(rect(CONTENT_X, 37.0, width - CONTENT_X - 14.0, 32.0));
    if let Some(tooltip) = &vm.prompt_tooltip {
        ask.setToolTip(Some(&ns(tooltip)));
    }
    view.addSubview(&ask);
    content.push(Retained::into_super(Retained::into_super(ask)));
    if let Some(detail) = &vm.detail {
        let icon = tinted_icon(detail.icon, 12.0, detail.color, mtm);
        icon.setFrameOrigin(NSPoint::new(CONTENT_X, 74.0));
        view.addSubview(&icon);
        content.push(Retained::into_super(Retained::into_super(icon)));
        let answer = label(&detail.text, 11.0, Weight::Regular, detail.color, mtm);
        answer.setFrame(rect(CONTENT_X + 17.0, 72.0, width - CONTENT_X - 31.0, 16.0));
        answer.setToolTip(Some(&ns(&detail.tooltip)));
        view.addSubview(&answer);
        content.push(Retained::into_super(Retained::into_super(answer)));
    }
    let extra = if vm.action.is_some() { errand_core::layout::ACTION_HEIGHT } else { 0.0 };
    if let Some(action) = &vm.action {
        let icon = tinted_icon("circle-arrow-right", 12.0, theme::NEW_RESULT, mtm);
        icon.setFrameOrigin(NSPoint::new(CONTENT_X, 92.0));
        view.addSubview(&icon);
        content.push(Retained::into_super(Retained::into_super(icon)));
        let next = label(&events.language.to_do(action), 11.0, Weight::Semibold, theme::NEW_RESULT, mtm);
        next.setFrame(rect(CONTENT_X + 17.0, 90.0, width - CONTENT_X - 31.0, 16.0));
        next.setToolTip(Some(&ns(action)));
        view.addSubview(&next);
        content.push(Retained::into_super(Retained::into_super(next)));
    }

    let footer = 95.0 + extra;
    let mut footer_end = width - 14.0 - 50.0;
    if pending {
        let (undo, undo_action) = undo_button(vm, rect(width - 14.0 - 78.0, footer - 3.0, 78.0, 20.0), events, mtm);
        footer_end = undo.frame().origin.x - 8.0;
        view.addSubview(&undo);
        actions.push(undo_action);
    } else {
        // Clicking anywhere opens the session; the hint shows only where the pointer already is. It covers
        // just its own corner, so a card-sized layer never sits over the checkbox.
        let hint = FlippedView::new(mtm, rect(width - 14.0 - 46.0, footer, 46.0, 15.0));
        let words = label(events.language.open(), 11.0, Weight::Medium, theme::INK, mtm);
        words.setFrame(rect(0.0, 0.0, 30.0, 15.0));
        words.setAlignment(NSTextAlignment::Right);
        hint.addSubview(&words);
        let link = tinted_icon("external-link", 12.0, theme::INK, mtm);
        link.setFrameOrigin(NSPoint::new(34.0, 1.5));
        hint.addSubview(&link);
        hint.setHidden(true);
        view.addSubview(&hint);
        *view.ivars().hint.borrow_mut() = Some(hint);
    }
    let clock = tinted_icon("clock", 11.0, theme::MUTED, mtm);
    clock.setFrameOrigin(NSPoint::new(CONTENT_X, footer + 2.0));
    view.addSubview(&clock);
    content.push(Retained::into_super(Retained::into_super(clock)));
    let time = label(&vm.when, 10.0, Weight::Regular, theme::MUTED, mtm);
    time.setFrame(rect(CONTENT_X + 14.0, footer, fitting_width(&time).min(footer_end - CONTENT_X - 14.0), 15.0));
    view.addSubview(&time);
    let time_end = time.frame().origin.x + time.frame().size.width;
    content.push(Retained::into_super(Retained::into_super(time)));
    if let Some(folder) = &vm.folder
        && footer_end - time_end > 40.0
    {
        let icon = tinted_icon("folder", 11.0, theme::MUTED, mtm);
        icon.setFrameOrigin(NSPoint::new(time_end + 8.0, footer + 2.0));
        view.addSubview(&icon);
        let icon_end = icon.frame().origin.x + icon.frame().size.width;
        content.push(Retained::into_super(Retained::into_super(icon)));
        let place = label(folder, 10.0, Weight::Regular, theme::MUTED, mtm);
        place.setFrame(rect(icon_end + 4.0, footer, footer_end - icon_end - 4.0, 15.0));
        place.setToolTip(Some(&ns(folder)));
        view.addSubview(&place);
        content.push(Retained::into_super(Retained::into_super(place)));
    }
    // A checked card fades back and drains a countdown while its undo window is open.
    if let Some(countdown) = vm.countdown {
        for view in &content {
            view.setAlphaValue(0.45);
        }
        let bar_frame = rect(CONTENT_X, vm.height - 7.0, width - CONTENT_X - 14.0, 2.5);
        view.addSubview(&CountdownBar::new(mtm, bar_frame, countdown.remaining.as_secs_f64(), countdown.fraction, events.language));
    }
    finish(&view, vm, events.language);
    (view, actions)
}

fn finish(view: &SessionCard, vm: &CardVm, language: Language) {
    view.setIdentifier(Some(&ns(&vm.key)));
    view.setAccessibilityElement(true);
    view.setAccessibilityRole(Some(unsafe { NSAccessibilityButtonRole }));
    view.setAccessibilityLabel(Some(&ns(&vm.accessibility_label)));
    view.setAccessibilityHelp(Some(&ns(language.opens_session())));
    view.setToolTip(Some(&ns(&vm.tooltip)));
}

/// A checked card's title, struck through and muted.
fn struck(title: &str, label: &NSTextField) -> Retained<NSAttributedString> {
    let text = NSMutableAttributedString::from_nsstring(&ns(title));
    let range = NSRange::new(0, ns(title).length());
    unsafe {
        if let Some(font) = label.font() {
            text.addAttribute_value_range(objc2_app_kit::NSFontAttributeName, &font, range);
        }
        text.addAttribute_value_range(objc2_app_kit::NSForegroundColorAttributeName, &color(theme::MUTED), range);
        text.addAttribute_value_range(objc2_app_kit::NSStrikethroughStyleAttributeName, &NSNumber::new_isize(1), range);
    }
    Retained::into_super(text)
}

pub struct MoreIvars {
    language: Language,
    scroll: Retained<NSScrollView>,
    label: Retained<NSTextField>,
    chevron: RefCell<Retained<objc2_app_kit::NSImageView>>,
}

define_class!(
    /// Says how many cards sit below the visible ones; a click scrolls one card further, or back to the top
    /// at the end.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandMoreBar"]
    #[ivars = MoreIvars]
    pub struct MoreBar;

    impl MoreBar {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> *mut NSView {
            let hit: *mut NSView = unsafe { msg_send![super(self), hitTest: point] };
            if hit.is_null() { hit } else { self as *const MoreBar as *mut NSView }
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: &NSEvent) {
            self.advance();
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn accessibility_perform_press(&self) -> bool {
            self.advance();
            true
        }

        #[unsafe(method(refresh))]
        fn refresh_selector(&self) {
            self.refresh();
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let pill = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(kit::inset(self.bounds(), 1.0, 1.0), 14.0, 14.0);
            grey(1.0, 0.96).setFill();
            pill.fill();
            grey(0.84, 0.9).setStroke();
            pill.stroke();
        }
    }
);

impl MoreBar {
    fn new(mtm: MainThreadMarker, frame: NSRect, scroll: Retained<NSScrollView>, language: Language) -> Retained<MoreBar> {
        let text = label("", 11.0, Weight::Medium, theme::MUTED, mtm);
        text.setAlignment(NSTextAlignment::Center);
        text.setFrame(rect(0.0, 7.0, frame.size.width - 20.0, 16.0));
        let chevron = tinted_icon("chevron-down", 13.0, theme::MUTED, mtm);
        let this = MoreBar::alloc(mtm).set_ivars(MoreIvars { language, scroll: scroll.clone(), label: text.clone(), chevron: RefCell::new(chevron.clone()) });
        let bar: Retained<MoreBar> = unsafe { msg_send![super(this), initWithFrame: frame] };
        bar.addSubview(&text);
        bar.addSubview(&chevron);
        bar.setAccessibilityElement(true);
        bar.setAccessibilityRole(Some(unsafe { NSAccessibilityButtonRole }));
        let clip = scroll.contentView();
        clip.setPostsBoundsChangedNotifications(true);
        unsafe {
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(&bar, objc2::sel!(refresh), Some(NSViewBoundsDidChangeNotification), Some(&clip));
        }
        bar.refresh();
        bar
    }

    /// Cards whose bottom edge is below the visible part, top first.
    fn hidden_below(&self) -> Vec<NSRect> {
        let scroll = &self.ivars().scroll;
        let visible = scroll.contentView().bounds();
        let Some(document) = scroll.documentView() else { return Vec::new() };
        let mut below: Vec<NSRect> = document
            .subviews()
            .iter()
            .map(|card| card.frame())
            .filter(|frame| frame.origin.y + frame.size.height > visible.origin.y + visible.size.height + 0.5)
            .collect();
        below.sort_by(|a, b| a.origin.y.total_cmp(&b.origin.y));
        below
    }

    fn refresh(&self) {
        let below = self.hidden_below().len();
        let ivars = self.ivars();
        let language = ivars.language;
        let text = if below > 0 { language.more_below(below) } else { language.back_to_top().to_owned() };
        ivars.label.setStringValue(&ns(&text));
        let text_width = kit::text_width(&text, &ivars.label.font().unwrap_or_else(|| kit::font(11.0, Weight::Medium))).ceil();
        let width = self.bounds().size.width;
        ivars.label.setFrame(rect(0.0, 7.0, width - 4.0, 16.0));
        let chevron = ivars.chevron.borrow();
        chevron.setImage(icon_image(if below > 0 { "chevron-down" } else { "chevron-up" }, 13.0, None, 0.0, 0.0).as_deref());
        chevron.setFrameOrigin(NSPoint::new(((width + text_width) / 2.0).round() + 2.0, 8.5));
        let spoken = if below > 0 { language.more_below_label(below) } else { language.back_to_top_label().to_owned() };
        self.setAccessibilityLabel(Some(&ns(&spoken)));
        self.setToolTip(Some(&ns(&spoken)));
    }

    /// Brings the next card fully into view, keeping the cards above it whole.
    fn advance(&self) {
        let scroll = self.ivars().scroll.clone();
        let clip = scroll.contentView();
        let visible = clip.bounds();
        let document_height = scroll.documentView().map_or(0.0, |document| document.frame().size.height);
        let target = self
            .hidden_below()
            .first()
            .map_or(0.0, |next| (next.origin.y + next.size.height - visible.size.height).min(document_height - visible.size.height));
        let duration = if NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion() { 0.0 } else { 0.25 };
        let moving = clip.clone();
        let changes = block2::RcBlock::new(move |context: std::ptr::NonNull<NSAnimationContext>| {
            unsafe { context.as_ref() }.setDuration(duration);
            moving.animator().setBoundsOrigin(NSPoint::new(0.0, target.max(0.0)));
        });
        let finished_scroll = scroll.clone();
        let finished_bar = self.retain();
        let finished = block2::RcBlock::new(move || {
            finished_scroll.reflectScrolledClipView(&finished_scroll.contentView());
            finished_bar.refresh();
        });
        NSAnimationContext::runAnimationGroup_completionHandler(&changes, Some(&finished));
    }
}

define_class!(
    /// The scrolling stack of cards. The pointer resting on it holds new cards back.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandSessionTrayView"]
    #[ivars = Arc<dyn Poster>]
    pub struct TrayView;

    impl TrayView {
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
            self.ivars().post(Msg::PointerOnCards(true));
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            self.ivars().post(Msg::PointerOnCards(false));
        }
    }
);

pub struct Tray {
    pub view: Retained<TrayView>,
    scroll: Option<Retained<NSScrollView>>,
    events: CardEvents,
    actions: Vec<Retained<Action>>,
    shown: Option<TrayVm>,
    height: f64,
}

impl Tray {
    pub fn new(mtm: MainThreadMarker, poster: Arc<dyn Poster>, language: Language) -> Tray {
        let this = TrayView::alloc(mtm).set_ivars(poster.clone());
        let view: Retained<TrayView> = unsafe { msg_send![super(this), initWithFrame: rect(0.0, 0.0, STACK_WIDTH, 120.0)] };
        Tray { view, scroll: None, events: CardEvents { poster, language }, actions: Vec::new(), shown: None, height: 0.0 }
    }

    /// Lays the cards out and returns their height, capped by the fold; 0 without cards.
    pub fn update(&mut self, vm: &TrayVm, mtm: MainThreadMarker) -> f64 {
        if self.shown.as_ref() == Some(vm) {
            return self.height;
        }
        let width = self.view.bounds().size.width;
        let previous = self.scroll.as_ref().map_or(NSPoint::ZERO, |scroll| scroll.contentView().bounds().origin);
        kit::clear(&self.view);
        self.actions.clear();
        self.scroll = None;
        if vm.cards.is_empty() {
            self.view.setFrameSize(NSSize::new(width, 0.0));
            self.shown = Some(vm.clone());
            self.height = 0.0;
            return 0.0;
        }
        let viewport = vm.fold.viewport_height;
        let scroll = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), rect(0.0, 0.0, width, viewport));
        scroll.setDrawsBackground(false);
        scroll.setHasVerticalScroller(vm.fold.content_height > viewport);
        scroll.setAutohidesScrollers(true);
        scroll.setScrollerStyle(NSScrollerStyle::Overlay);
        scroll.setHorizontalScrollElasticity(NSScrollElasticity::None);
        let document = FlippedView::new(mtm, rect(0.0, 0.0, width, vm.fold.content_height));
        let mut y = 0.0;
        for card_vm in &vm.cards {
            let (view, actions) = card(card_vm, width, &self.events, mtm);
            view.setFrameOrigin(NSPoint::new(0.0, y));
            y += card_vm.height + CARD_GAP;
            document.addSubview(&view);
            self.actions.extend(actions);
        }
        scroll.setDocumentView(Some(&document));
        let limit = (vm.fold.content_height - viewport).max(0.0);
        scroll.contentView().scrollToPoint(NSPoint::new(previous.x, previous.y.min(limit)));
        scroll.reflectScrolledClipView(&scroll.contentView());
        self.view.addSubview(&scroll);
        if vm.fold.more_bar {
            self.view.addSubview(&MoreBar::new(mtm, rect(0.0, viewport + CARD_GAP, width, MORE_BAR_HEIGHT), scroll.clone(), self.events.language));
        }
        let height = vm.fold.total_height();
        self.view.setFrameSize(NSSize::new(width, height));
        self.scroll = Some(scroll);
        self.shown = Some(vm.clone());
        self.height = height;
        height
    }

    pub fn card(&self, key: &str) -> Option<Retained<SessionCard>> {
        let document = self.scroll.as_ref()?.documentView()?;
        document.subviews().iter().filter_map(|view| view.downcast::<SessionCard>().ok()).find(|card| card.key() == key)
    }

    /// Fades a checked card out, so leaving the list reads as the end of the check, not a jump.
    pub fn fade(&self, key: &str, visible: bool, poster: Arc<dyn Poster>) {
        let key = key.to_owned();
        let Some(card) = self.card(&key).filter(|_| visible && !NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()) else {
            poster.post(Msg::CardFaded(key));
            return;
        };
        let changes = block2::RcBlock::new(move |context: std::ptr::NonNull<NSAnimationContext>| {
            unsafe { context.as_ref() }.setDuration(0.2);
            card.animator().setAlphaValue(0.0);
        });
        let finished = block2::RcBlock::new(move || poster.post(Msg::CardFaded(key.clone())));
        NSAnimationContext::runAnimationGroup_completionHandler(&changes, Some(&finished));
    }
}

