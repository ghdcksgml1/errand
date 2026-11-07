//! Work rooms, drawn as the rest of the widget is. Under the dock's profile, a tab per room the person is in,
//! which keep their width and move along when they do not all fit, a plus after them for the list of rooms and,
//! under the open tab, its people in thin rows saying no more than the folded task header; beside the dock, a
//! bubble like the usage details listing the rooms on cards like the task cards.

use super::details::{PopoverView, button};
use super::dock::{AvatarView, count_pill, replace_tracking_area, tinted_icon};
use super::kit::{self, Action, CHIP_ICON, FlippedView, Weight, color, digits_font, fitting_width, grey, label, ns, rect, text_width};
use super::spinner::SpinningIconView;
use super::tray::{ChipMark, chip_icon_frame, chip_rect, draw_card, draw_chip};
use errand_core::layout::{self, DOCK_PADDING, DockShape, MEMBER_ROW_HEIGHT, MEMBERS_INSET, ROOM_TAB_HEIGHT, STACK_WIDTH};
use errand_core::Language;
use errand_core::model::Msg;
use errand_core::ports::Poster;
use errand_core::sessions::SessionState;
use errand_core::theme::{self, CardStyle, Paint};
use errand_core::view::{ChipVm, DockRoomsVm, FaceVm, MemberRowVm, RoomCardVm, RoomTabVm, RoomsVm};
use objc2::rc::Retained;
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAccessibility, NSBezierPath, NSBox, NSBoxType, NSButton, NSEvent, NSGradient, NSImageView, NSLineBreakMode, NSScrollElasticity, NSScrollView,
    NSScrollerStyle, NSTextAlignment, NSTextField, NSView,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

const ROOM_CARD_HEIGHT: f64 = 100.0;
/// An open tab and its room's panel share this fill, so they read as one.
const OPEN_FILL: Paint = theme::INK.alpha(0.055);
const TAB_GAP: f64 = 6.0;
/// How wide the white closes over tabs that run on past the strip's end.
const FADE: f64 = 18.0;
/// How far one line of a notched wheel moves the tabs, which count in lines where a trackpad counts in points.
const WHEEL_LINE: f64 = 16.0;
const FACE: f64 = 18.0;
/// Where a row's name starts, past its face.
const NAME_X: f64 = 8.0 + FACE + 8.0;
/// Where a row's pills end, and its time's text: a label keeps 2 points of its own inside each edge.
const ROW_END: f64 = 6.0;
const LABEL_INSET: f64 = 2.0;
/// Between a row's time and its pills, the time's own inset aside.
const CLOCK_GAP: f64 = 4.0;
/// Smaller than the pills' numbers: how long someone has been in is the quietest thing on the row.
const CLOCK_SIZE: f64 = 10.0;

/// A chip as a card carries it: its words, what goes before them, and its own colours.
pub struct Chip {
    text: String,
    icon: Option<&'static str>,
    spinning: bool,
    style: CardStyle,
}

impl Chip {
    fn from(vm: &ChipVm) -> Chip {
        Chip { text: vm.text.clone(), icon: vm.icon, spinning: vm.spinning, style: theme::card_style(vm.style, false) }
    }

    fn mark(&self) -> ChipMark<'static> {
        match (self.spinning, self.icon) {
            (true, _) => ChipMark::Spinner,
            (false, Some(icon)) => ChipMark::Icon(icon),
            (false, None) => ChipMark::Nothing,
        }
    }
}

pub struct CardIvars {
    style: CardStyle,
    chip: Option<Chip>,
}

define_class!(
    /// A room on a card like the task cards, its chip in the top right.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandRoomCard"]
    #[ivars = CardIvars]
    pub struct RoomCard;

    impl RoomCard {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let ivars = self.ivars();
            draw_card(self.bounds(), &ivars.style);
            if let Some(chip) = &ivars.chip {
                draw_chip(chip_rect(self.bounds().size.width, &chip.text, chip.mark()), &chip.text, chip.mark(), &chip.style);
            }
        }
    }
);

impl RoomCard {
    fn new(mtm: MainThreadMarker, frame: NSRect, style: SessionState, chip: Option<Chip>) -> Retained<RoomCard> {
        let this = RoomCard::alloc(mtm).set_ivars(CardIvars { style: theme::card_style(style, false), chip });
        let card: Retained<RoomCard> = unsafe { msg_send![super(this), initWithFrame: frame] };
        // A chip whose work is under way turns its spinner where the icon would be.
        if let Some(chip) = &card.ivars().chip
            && chip.spinning
        {
            let spot = chip_icon_frame(chip_rect(frame.size.width, &chip.text, chip.mark()));
            card.addSubview(&SpinningIconView::new(mtm, spot, "loader-circle", theme::RUNNING_BRIGHT));
        }
        card
    }

    /// Where the chip starts, so the words beside it stop short of it.
    fn chip_left(&self) -> f64 {
        let width = self.bounds().size.width;
        self.ivars().chip.as_ref().map_or(width - 14.0, |chip| chip_rect(width, &chip.text, chip.mark()).origin.x)
    }
}

fn avatar(face: &FaceVm, frame: NSRect, ring: bool, mtm: MainThreadMarker) -> Retained<AvatarView> {
    let view = AvatarView::new(mtm, frame, None);
    view.show_face(face.avatar, &face.initial, ring);
    view
}

pub struct TabIvars {
    open: Cell<bool>,
    hovered: Cell<bool>,
}

define_class!(
    /// A room's tab under the profile: a capsule, outlined while closed and filled while open. A button, so
    /// the dock passes its click on, and the click is its own wherever it lands on it.
    #[unsafe(super(NSButton))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandRoomTab"]
    #[ivars = TabIvars]
    pub struct RoomTab;

    impl RoomTab {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> *mut NSView {
            let hit: *mut NSView = unsafe { msg_send![super(self), hitTest: point] };
            if hit.is_null() { hit } else { self as *const RoomTab as *mut NSView }
        }

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
            self.ivars().hovered.set(true);
            NSView::setNeedsDisplay(self, true);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            self.ivars().hovered.set(false);
            NSView::setNeedsDisplay(self, true);
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let ivars = self.ivars();
            let bounds = kit::inset(self.bounds(), 0.5, 0.5);
            let radius = bounds.size.height / 2.0;
            let capsule = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(bounds, radius, radius);
            if ivars.open.get() {
                kit::paint(OPEN_FILL).setFill();
                capsule.fill();
                return;
            }
            if ivars.hovered.get() {
                kit::paint(theme::INK.alpha(0.035)).setFill();
                capsule.fill();
            }
            grey(0.86, 1.0).setStroke();
            capsule.setLineWidth(1.0);
            capsule.stroke();
        }
    }
);

impl RoomTab {
    fn new(mtm: MainThreadMarker, frame: NSRect, open: bool) -> Retained<RoomTab> {
        let this = RoomTab::alloc(mtm).set_ivars(TabIvars { open: Cell::new(open), hovered: Cell::new(false) });
        let tab: Retained<RoomTab> = unsafe { msg_send![super(this), initWithFrame: frame] };
        tab.setBordered(false);
        tab.setTitle(&ns(""));
        tab
    }

    /// The way to the rooms beside the nickname: a capsule like a tab, holding only its word.
    pub fn rooms_button(mtm: MainThreadMarker, words: &str, tooltip: &str) -> Retained<RoomTab> {
        let text = label(words, 11.5, Weight::Semibold, theme::MUTED, mtm);
        let width = fitting_width(&text);
        let button = RoomTab::new(mtm, rect(0.0, 0.0, 8.0 + width + 7.0, ROOM_TAB_HEIGHT), false);
        text.setFrame(rect(8.0, 3.0, width, 16.0));
        button.addSubview(&text);
        button.setToolTip(Some(&ns(tooltip)));
        button.setAccessibilityLabel(Some(&ns(words)));
        button
    }

    /// The way to the rooms after the tabs: a round tab holding a plus.
    fn rooms_plus(mtm: MainThreadMarker, words: &str, tooltip: &str) -> Retained<RoomTab> {
        let button = RoomTab::new(mtm, rect(0.0, 0.0, ROOM_TAB_HEIGHT, ROOM_TAB_HEIGHT), false);
        let plus = tinted_icon("plus", CHIP_ICON, theme::MUTED, mtm);
        plus.setFrameOrigin(NSPoint::new((ROOM_TAB_HEIGHT - CHIP_ICON) / 2.0, (ROOM_TAB_HEIGHT - CHIP_ICON) / 2.0));
        button.addSubview(&plus);
        button.setToolTip(Some(&ns(tooltip)));
        button.setAccessibilityLabel(Some(&ns(words)));
        button
    }

    /// Fills the capsule and darkens what it holds while what it opens is open.
    pub fn set_open(&self, open: bool) {
        if self.ivars().open.replace(open) == open {
            return;
        }
        let ink = color(if open { theme::INK } else { theme::MUTED });
        for view in self.subviews().iter() {
            if let Some(words) = view.downcast_ref::<NSTextField>() {
                words.setTextColor(Some(&ink));
            } else if let Some(icon) = view.downcast_ref::<NSImageView>() {
                icon.setContentTintColor(Some(&ink));
            }
        }
        self.setAccessibilitySelected(open);
        NSView::setNeedsDisplay(self, true);
    }
}

define_class!(
    /// The dock's white closing over tabs that run on past the strip's end, so a tab cut short reads as going on
    /// rather than broken. Clicks go through to the tab under it.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandTabFade"]
    #[ivars = bool]
    struct TabFade;

    impl TabFade {
        #[unsafe(method(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> *mut NSView {
            std::ptr::null_mut()
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let (clear, white) = (kit::paint(theme::WHITE.alpha(0.0)), color(theme::WHITE));
            let (from, to) = if *self.ivars() { (&clear, &white) } else { (&white, &clear) };
            if let Some(gradient) = NSGradient::initWithStartingColor_endingColor(NSGradient::alloc(), from, to) {
                gradient.drawInRect_angle(self.bounds(), 0.0);
            }
        }
    }
);

impl TabFade {
    /// Fades toward the strip's right end, or toward its left.
    fn new(mtm: MainThreadMarker, rightward: bool) -> Retained<TabFade> {
        let this = TabFade::alloc(mtm).set_ivars(rightward);
        let fade: Retained<TabFade> = unsafe { msg_send![super(this), initWithFrame: rect(0.0, 0.0, FADE, ROOM_TAB_HEIGHT)] };
        fade.setHidden(true);
        fade
    }
}

pub struct StripIvars {
    /// Over the left and right ends, while tabs run on that way.
    fades: RefCell<Option<(Retained<TabFade>, Retained<TabFade>)>>,
}

define_class!(
    /// The room tabs: when they do not all fit they keep their width and move along under the pointer. No scroller
    /// shows, and a scroll view takes no swipes without one, so they are moved here. A wheel turns only up and
    /// down, so those move the tabs along as well.
    #[unsafe(super(NSScrollView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandRoomTabStrip"]
    #[ivars = StripIvars]
    pub struct TabStrip;

    impl TabStrip {
        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            let (across, down) = (event.scrollingDeltaX(), event.scrollingDeltaY());
            let delta = if across.abs() > down.abs() { across } else { down };
            let step = if event.hasPreciseScrollingDeltas() { delta } else { delta * WHEEL_LINE };
            self.move_to(self.offset() - step);
        }
    }
);

impl TabStrip {
    fn new(mtm: MainThreadMarker, document: &NSView) -> Retained<TabStrip> {
        let this = TabStrip::alloc(mtm).set_ivars(StripIvars { fades: RefCell::new(None) });
        let strip: Retained<TabStrip> = unsafe { msg_send![super(this), initWithFrame: document.frame()] };
        strip.setDrawsBackground(false);
        strip.setDocumentView(Some(document));
        let fades = (TabFade::new(mtm, false), TabFade::new(mtm, true));
        strip.addSubview(&fades.0);
        strip.addSubview(&fades.1);
        *strip.ivars().fades.borrow_mut() = Some(fades);
        strip
    }

    /// How far along the tabs have moved.
    pub fn offset(&self) -> f64 {
        self.contentView().bounds().origin.x
    }

    /// Moves the tabs along, as far as they go, and fades the ends where tabs run on.
    fn move_to(&self, offset: f64) {
        let clip = self.contentView();
        let shown = clip.bounds().size.width;
        let reach = self.documentView().map_or(0.0, |tabs| (tabs.frame().size.width - shown).max(0.0));
        let offset = offset.clamp(0.0, reach);
        clip.scrollToPoint(NSPoint::new(offset, 0.0));
        self.reflectScrolledClipView(&clip);
        if let Some((left, right)) = self.ivars().fades.borrow().as_ref() {
            left.setHidden(offset < 0.5);
            right.setHidden(offset > reach - 0.5);
            right.setFrameOrigin(NSPoint::new(shown - FADE, 0.0));
        }
    }

    /// Moves along no further than it takes to show this tab clear of the fades, its start first if it is wider
    /// than the strip.
    fn reveal(&self, tab: NSRect) {
        let shown = self.contentView().bounds().size.width;
        let end = tab.origin.x + tab.size.width + FADE;
        let offset = self.offset().max(end - shown);
        self.move_to(offset.min(tab.origin.x - FADE));
    }
}

/// A room's tab before it is placed: its views, and how wide it is around its name.
struct TabParts {
    tab: Retained<RoomTab>,
    /// Before a private room's name.
    lock: Option<Retained<NSImageView>>,
    name: Retained<NSTextField>,
    name_width: f64,
    /// How many are in the room, once it has answered: the people icon and the number.
    people: Option<(Retained<NSImageView>, Retained<NSTextField>)>,
    chrome: f64,
}

/// Between the lock and the name, and the name and the people.
const LOCK_GAP: f64 = 3.0;
const PEOPLE_GAP: f64 = 5.0;

fn tab_parts(vm: &RoomTabVm, mtm: MainThreadMarker) -> TabParts {
    let ink = if vm.open { theme::INK } else { theme::MUTED };
    let name = label(&vm.name, 11.5, Weight::Semibold, ink, mtm);
    let lock = vm.private.then(|| tinted_icon("lock", CHIP_ICON, ink, mtm));
    let people = vm.people.map(|people| {
        let number = label(&people.to_string(), 11.0, Weight::Semibold, ink, mtm);
        number.setFont(Some(&digits_font(11.0, Weight::Semibold)));
        (tinted_icon("users", CHIP_ICON, ink, mtm), number)
    });
    let chrome = 8.0
        + lock.as_ref().map_or(0.0, |_| CHIP_ICON + LOCK_GAP)
        + people.as_ref().map_or(0.0, |(_, number)| PEOPLE_GAP + CHIP_ICON + 1.0 + fitting_width(number))
        + 7.0;
    let tab = RoomTab::new(mtm, rect(0.0, 0.0, 0.0, ROOM_TAB_HEIGHT), vm.open);
    tab.setToolTip(Some(&ns(&vm.tooltip)));
    tab.setAccessibilityLabel(Some(&ns(&vm.accessibility_label)));
    tab.setAccessibilitySelected(vm.open);
    TabParts { name_width: fitting_width(&name), tab, lock, name, people, chrome }
}

define_class!(
    /// The open room's panel under the tabs, filled as the open tab is.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandRoomMembers"]
    pub struct MembersView;

    impl MembersView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            kit::paint(OPEN_FILL).setFill();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(self.bounds(), 11.0, 11.0).fill();
        }
    }
);

define_class!(
    /// One person in the open room.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandMemberRow"]
    pub struct MemberRow;

    impl MemberRow {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

/// The width of the room's column of times, right-aligned at the rows' ends: the widest time among them, so the
/// pills line up from row to row. Digits are all one width, so it changes only when a time grows a digit.
fn clock_column(members: &[MemberRowVm]) -> f64 {
    let font = digits_font(CLOCK_SIZE, Weight::Medium);
    members.iter().filter_map(|member| member.elapsed.as_deref()).map(|time| text_width(time, &font).ceil() + 2.0 * LABEL_INSET + 1.0).fold(0.0, f64::max)
}

/// One person: their face and name, their task header's pills and, in the room's column of times at the right,
/// how long they have been in the room. The time comes back too, to count up in place.
fn member_row(vm: &MemberRowVm, width: f64, clock: f64, mtm: MainThreadMarker) -> (Retained<MemberRow>, Option<Retained<NSTextField>>) {
    let row: Retained<MemberRow> = unsafe { msg_send![MemberRow::alloc(mtm), initWithFrame: rect(0.0, 0.0, width, MEMBER_ROW_HEIGHT)] };
    row.addSubview(&avatar(&vm.face, rect(8.0, (MEMBER_ROW_HEIGHT - FACE) / 2.0, FACE, FACE), false, mtm));
    // The time's text ends where the pills would, its label's inset past it.
    let clock_end = width - ROW_END + LABEL_INSET;
    let time = vm.elapsed.as_deref().map(|elapsed| {
        let time = label(elapsed, CLOCK_SIZE, Weight::Medium, theme::MUTED, mtm);
        time.setFont(Some(&digits_font(CLOCK_SIZE, Weight::Medium)));
        time.setAlignment(NSTextAlignment::Right);
        time.setFrame(rect(clock_end - clock, (MEMBER_ROW_HEIGHT - 16.0) / 2.0 + 1.5, clock, 16.0));
        row.addSubview(&time);
        time
    });
    let mut right = if clock > 0.0 { clock_end - clock - CLOCK_GAP } else { width - ROW_END };
    for pill in vm.pills.iter().rev() {
        let pill = count_pill(pill.count, pill.icon, pill.spinning, pill.color, &pill.words, NSPoint::new(0.0, (MEMBER_ROW_HEIGHT - 18.0) / 2.0), mtm);
        right -= pill.frame().size.width;
        pill.setFrameOrigin(NSPoint::new(right, pill.frame().origin.y));
        row.addSubview(&pill);
        right -= 5.0;
    }
    let badge = vm.me.map(|me| label(me, 11.0, Weight::Semibold, theme::NEW_RESULT, mtm));
    let badge_width = badge.as_ref().map_or(0.0, |badge| fitting_width(badge));
    let name = label(&vm.name, 12.0, Weight::Semibold, theme::INK, mtm);
    let name_width = fitting_width(&name).min(right - 4.0 - NAME_X - badge_width);
    name.setFrame(rect(NAME_X, (MEMBER_ROW_HEIGHT - 17.0) / 2.0, name_width, 17.0));
    row.addSubview(&name);
    if let Some(badge) = badge {
        badge.setFrame(rect(NAME_X + name_width, (MEMBER_ROW_HEIGHT - 16.0) / 2.0 + 0.5, badge_width, 16.0));
        row.addSubview(&badge);
    }
    if vm.faded {
        for view in row.subviews().iter() {
            view.setAlphaValue(0.5);
        }
    }
    row.setToolTip(Some(&ns(&vm.tooltip)));
    row.setAccessibilityElement(true);
    row.setAccessibilityLabel(Some(&ns(&vm.accessibility_label)));
    (row, time)
}

/// The rooms as they would be laid out but for the times, which count up where they are drawn.
fn without_times(vm: &DockRoomsVm) -> DockRoomsVm {
    let members = vm.members.iter().map(|member| MemberRowVm { elapsed: member.elapsed.as_ref().map(|_| String::new()), accessibility_label: String::new(), ..member.clone() });
    DockRoomsVm { members: members.collect(), ..vm.clone() }
}

/// The rooms under the dock's profile: a tab per room the person is in, the plus and, under the open tab, its
/// people.
pub struct DockRooms {
    /// The row: the strip of tabs, then the plus.
    pub tabs: Retained<FlippedView>,
    pub strip: Retained<TabStrip>,
    /// What the strip moves along, holding the tabs.
    document: Retained<FlippedView>,
    plus: Retained<RoomTab>,
    pub members: Retained<MembersView>,
    expand: Rc<dyn Fn(String)>,
    /// Tabs hold their targets weakly.
    actions: Vec<Retained<Action>>,
    _plus_action: Retained<Action>,
    /// The tab last seen open, and whether its room had answered, which widens it: a change brings it into view.
    opened: Option<(String, bool)>,
    /// Each row as drawn, with its time, and the width of the column of times.
    rows: Vec<(Retained<MemberRow>, Option<Retained<NSTextField>>)>,
    clock: f64,
    shown: Option<DockRoomsVm>,
}

impl DockRooms {
    pub fn new(mtm: MainThreadMarker, expand: Rc<dyn Fn(String)>, show_rooms: Rc<dyn Fn()>, language: Language) -> DockRooms {
        let width = STACK_WIDTH - 2.0 * DOCK_PADDING;
        let tabs = FlippedView::new(mtm, rect(DOCK_PADDING, 0.0, width, ROOM_TAB_HEIGHT));
        tabs.setHidden(true);
        let document = FlippedView::new(mtm, rect(0.0, 0.0, 0.0, ROOM_TAB_HEIGHT));
        let strip = TabStrip::new(mtm, &document);
        tabs.addSubview(&strip);
        let plus = RoomTab::rooms_plus(mtm, language.rooms_button(), language.rooms_plus_tooltip());
        let plus_action = Action::new(mtm, move || show_rooms());
        plus_action.attach(&plus);
        tabs.addSubview(&plus);
        let members: Retained<MembersView> = unsafe { msg_send![MembersView::alloc(mtm), initWithFrame: rect(DOCK_PADDING, 0.0, width, 0.0)] };
        members.setHidden(true);
        DockRooms { tabs, strip, document, plus, members, expand, actions: Vec::new(), _plus_action: plus_action, opened: None, rows: Vec::new(), clock: 0.0, shown: None }
    }

    /// The plus reads as pressed while the list of rooms is open.
    pub fn set_listed(&self, listed: bool) {
        self.plus.set_open(listed);
    }

    /// Puts the tabs and the open room's panel where the dock's shape has them.
    pub fn place(&self, shape: DockShape) {
        self.tabs.setFrameOrigin(NSPoint::new(DOCK_PADDING, layout::tabs_top(shape)));
        self.tabs.setHidden(!shape.tabs);
        self.members.setFrame(rect(DOCK_PADDING, layout::members_top(shape), STACK_WIDTH - 2.0 * DOCK_PADDING, layout::members_height(shape.rows)));
        self.members.setHidden(!shape.tabs || shape.rows == 0);
    }

    /// Lays the tabs and rows out again when they change. A second later only the times have changed: they count
    /// up in place, so the rows, their tooltips and the tab under the pointer stay as they are.
    pub fn show(&mut self, vm: Option<&DockRoomsVm>, mtm: MainThreadMarker) {
        if self.shown.as_ref() == vm {
            return;
        }
        if let (Some(shown), Some(vm)) = (&self.shown, vm)
            && without_times(shown) == without_times(vm)
            && clock_column(&vm.members) == self.clock
        {
            for ((row, time), member) in self.rows.iter().zip(&vm.members) {
                if let (Some(time), Some(elapsed)) = (time, &member.elapsed) {
                    time.setStringValue(&ns(elapsed));
                }
                row.setAccessibilityLabel(Some(&ns(&member.accessibility_label)));
            }
            self.shown = Some(vm.clone());
            return;
        }
        self.shown = vm.cloned();
        kit::clear(&self.document);
        kit::clear(&self.members);
        self.actions.clear();
        self.rows.clear();
        let Some(vm) = vm else {
            self.opened = None;
            return;
        };
        self.lay_tabs(&vm.tabs, mtm);
        let width = self.members.frame().size.width;
        self.clock = clock_column(&vm.members);
        let mut y = MEMBERS_INSET;
        for member in &vm.members {
            let (row, time) = member_row(member, width, self.clock, mtm);
            row.setFrameOrigin(NSPoint::new(0.0, y));
            self.members.addSubview(&row);
            self.rows.push((row, time));
            y += MEMBER_ROW_HEIGHT;
        }
        for words in vm.more.iter().chain(&vm.status) {
            let line = label(words, 11.0, Weight::Medium, theme::MUTED, mtm);
            line.setFrame(rect(NAME_X, y + (MEMBER_ROW_HEIGHT - 16.0) / 2.0, width - NAME_X - 8.0, 16.0));
            self.members.addSubview(&line);
            y += MEMBER_ROW_HEIGHT;
        }
    }

    /// The tabs from the left at their own widths, the plus after the last or, when they do not all fit, at the
    /// row's end, where it stays while the tabs move along beside it. Where they have moved to outlasts this, but a
    /// tab just opened is brought into view.
    fn lay_tabs(&mut self, tabs: &[RoomTabVm], mtm: MainThreadMarker) {
        let mut x = 0.0;
        let icon_y = (ROOM_TAB_HEIGHT - CHIP_ICON) / 2.0;
        let mut open = None;
        for vm in tabs {
            let part = tab_parts(vm, mtm);
            let name_width = part.name_width;
            let mut inside = 8.0;
            if let Some(lock) = &part.lock {
                lock.setFrameOrigin(NSPoint::new(inside, icon_y));
                part.tab.addSubview(lock);
                inside += CHIP_ICON + LOCK_GAP;
            }
            part.name.setFrame(rect(inside, 3.0, name_width, 16.0));
            part.tab.addSubview(&part.name);
            inside += name_width;
            if let Some((icon, number)) = &part.people {
                icon.setFrameOrigin(NSPoint::new(inside + PEOPLE_GAP, icon_y));
                number.setFrame(rect(inside + PEOPLE_GAP + CHIP_ICON + 1.0, 3.0, fitting_width(number), 16.0));
                part.tab.addSubview(icon);
                part.tab.addSubview(number);
            }
            let width = part.chrome + name_width;
            part.tab.setFrame(rect(x, 0.0, width, ROOM_TAB_HEIGHT));
            let (expand, room) = (self.expand.clone(), vm.id.clone());
            let action = Action::new(mtm, move || expand(room.clone()));
            action.attach(&part.tab);
            self.document.addSubview(&part.tab);
            self.actions.push(action);
            if vm.open {
                open = Some(((vm.id.clone(), vm.people.is_some()), part.tab.frame()));
            }
            x += width + TAB_GAP;
        }
        let width = (x - TAB_GAP).max(0.0);
        let shown = width.min(self.tabs.frame().size.width - ROOM_TAB_HEIGHT - TAB_GAP);
        self.document.setFrameSize(NSSize::new(width, ROOM_TAB_HEIGHT));
        self.strip.setFrameSize(NSSize::new(shown, ROOM_TAB_HEIGHT));
        self.plus.setFrameOrigin(NSPoint::new(shown + TAB_GAP, 0.0));
        self.strip.move_to(self.strip.offset());
        let opened = open.as_ref().map(|(room, _)| room.clone());
        if let Some((_, tab)) = open.filter(|_| opened != self.opened) {
            self.strip.reveal(tab);
        }
        self.opened = opened;
    }
}

/// The list of rooms beside the dock.
pub struct Rooms {
    pub view: Retained<PopoverView>,
    body: Retained<FlippedView>,
    poster: Arc<dyn Poster>,
    /// Buttons hold their targets weakly.
    actions: Vec<Retained<Action>>,
    scroll: Option<Retained<NSScrollView>>,
    shown: Option<RoomsVm>,
}

impl Rooms {
    pub fn new(mtm: MainThreadMarker, poster: Arc<dyn Poster>) -> Rooms {
        let (width, height) = (layout::ROOMS_WIDTH, layout::ROOMS_HEIGHT);
        let view = PopoverView::new(mtm, NSSize::new(width, height));
        let body = FlippedView::new(mtm, rect(0.0, 0.0, width, height));
        view.addSubview(&body);
        Rooms { view, body, poster, actions: Vec::new(), scroll: None, shown: None }
    }

    /// Turns the tail toward the dock, level with what opens the list.
    pub fn point(&self, tail_on_right: bool, tail_y: f64) {
        self.view.point(&self.body, tail_on_right, tail_y);
    }

    /// Lays the bubble out again from the view model, keeping the list's scroll position.
    pub fn render(&mut self, vm: &RoomsVm, mtm: MainThreadMarker) {
        if self.shown.as_ref() == Some(vm) {
            return;
        }
        let (width, height) = (layout::ROOMS_WIDTH, layout::ROOMS_HEIGHT);
        let previous = self.scroll.as_ref().map_or(NSPoint::ZERO, |scroll| scroll.contentView().bounds().origin);
        let body: &NSView = &self.body;
        kit::clear(body);
        self.actions.clear();
        self.scroll = None;

        let poster = self.poster.clone();
        let (close, action) = button("", "x", false, rect(width - 16.0 - 34.0, 14.0, 34.0, 26.0), move || poster.post(Msg::CloseRooms), mtm);
        close.setToolTip(Some(&ns(vm.close)));
        close.setAccessibilityLabel(Some(&ns(vm.close)));
        body.addSubview(&close);
        self.actions.push(action);
        let title = label(vm.title, 18.0, Weight::Semibold, theme::INK, mtm);
        title.setFrame(rect(18.0, 15.0, width - 16.0 - 34.0 - 8.0 - 18.0, 25.0));
        body.addSubview(&title);
        let subtitle = label(&vm.subtitle, 11.0, Weight::Regular, theme::MUTED, mtm);
        subtitle.setMaximumNumberOfLines(2);
        subtitle.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
        subtitle.setFrame(rect(18.0, 44.0, width - 36.0, 30.0));
        body.addSubview(&subtitle);

        let rule = NSBox::initWithFrame(NSBox::alloc(mtm), rect(18.0, height - 45.0, width - 36.0, 1.0));
        rule.setBoxType(NSBoxType::Separator);
        body.addSubview(&rule);
        let footer = label(vm.footer, 10.0, Weight::Regular, theme::MUTED, mtm);
        footer.setMaximumNumberOfLines(2);
        footer.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
        footer.setFrame(rect(18.0, height - 38.0, width - 36.0, 28.0));
        body.addSubview(&footer);

        let top = 84.0;
        let area = rect(12.0, top, width - 24.0, height - 52.0 - top);
        if let Some(status) = &vm.status {
            let words = label(status, 12.0, Weight::Medium, theme::MUTED, mtm);
            words.setAlignment(NSTextAlignment::Center);
            words.setFrame(rect(area.origin.x, area.origin.y + 40.0, area.size.width, 18.0));
            body.addSubview(&words);
            self.shown = Some(vm.clone());
            return;
        }
        let card_width = area.size.width;
        let mut y = 0.0;
        let document = FlippedView::new(mtm, rect(0.0, 0.0, card_width, 0.0));
        for room in &vm.list {
            let (card, action) = room_card(room, card_width, self.poster.clone(), mtm);
            card.setFrameOrigin(NSPoint::new(0.0, y));
            y += card.frame().size.height + layout::CARD_GAP;
            document.addSubview(&card);
            self.actions.push(action);
        }
        document.setFrameSize(NSSize::new(card_width, y.max(area.size.height)));
        let scroll = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), area);
        scroll.setDrawsBackground(false);
        scroll.setHasVerticalScroller(y > area.size.height);
        scroll.setAutohidesScrollers(true);
        scroll.setScrollerStyle(NSScrollerStyle::Overlay);
        scroll.setHorizontalScrollElasticity(NSScrollElasticity::None);
        scroll.setDocumentView(Some(&document));
        let limit = (document.frame().size.height - area.size.height).max(0.0);
        scroll.contentView().scrollToPoint(NSPoint::new(0.0, previous.y.min(limit)));
        scroll.reflectScrolledClipView(&scroll.contentView());
        body.addSubview(&scroll);
        self.scroll = Some(scroll);
        self.shown = Some(vm.clone());
    }
}

/// A room on offer: its name and what it is like, how people show up there, who is at work, and a way in,
/// or out of a room the person is in, whose card is edged as work under way is.
fn room_card(vm: &RoomCardVm, width: f64, poster: Arc<dyn Poster>, mtm: MainThreadMarker) -> (Retained<RoomCard>, Retained<Action>) {
    let style = if vm.joined { SessionState::Running } else { SessionState::Seen };
    let card = RoomCard::new(mtm, rect(0.0, 0.0, width, ROOM_CARD_HEIGHT), style, Some(Chip::from(&vm.visibility)));
    let name = label(&vm.name, 13.5, Weight::Semibold, theme::INK, mtm);
    name.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    name.setFrame(rect(16.0, 11.0, card.chip_left() - 8.0 - 16.0, 19.0));
    card.addSubview(&name);
    let tagline = label(&vm.tagline, 11.0, Weight::Regular, theme::MUTED, mtm);
    tagline.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    tagline.setFrame(rect(16.0, 33.0, width - 32.0, 16.0));
    card.addSubview(&tagline);
    let (face, step) = (22.0, 15.0);
    for (index, member) in vm.faces.iter().enumerate() {
        card.addSubview(&avatar(member, rect(16.0 + index as f64 * step, 63.0, face, face), true, mtm));
    }
    let faces = if vm.faces.is_empty() { 0.0 } else { face + step * (vm.faces.len() - 1) as f64 + 8.0 };
    let pill = count_pill(vm.working.count, vm.working.icon, vm.working.spinning, vm.working.color, &vm.working.words, NSPoint::new(16.0 + faces, 65.0), mtm);
    let after = pill.frame().origin.x + pill.frame().size.width + 6.0;
    card.addSubview(&pill);
    let people = label(&vm.people, 11.0, Weight::Medium, theme::MUTED, mtm);
    people.setFrame(rect(after, 66.0, 80.0, 16.0));
    card.addSubview(&people);
    let room = vm.id.clone();
    let frame = rect(width - 14.0 - 96.0, 60.0, 96.0, 28.0);
    let (button, action) = if vm.joined {
        button(vm.button, "", false, frame, move || poster.post(Msg::LeaveRoom(room.clone())), mtm)
    } else {
        button(vm.button, "circle-arrow-right", true, frame, move || poster.post(Msg::JoinRoom(room.clone())), mtm)
    };
    if let Some(reason) = &vm.shut {
        button.setEnabled(false);
        button.setToolTip(Some(&ns(reason)));
    }
    card.addSubview(&button);
    card.setAccessibilityElement(true);
    card.setAccessibilityLabel(Some(&ns(&format!("{}, {}, {}, {}", vm.name, vm.visibility.text, vm.people, vm.working.words))));
    (card, action)
}
