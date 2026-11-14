//! Account cards with each limit's remaining share, next reset and observed reset, shown in the usage
//! details beside the dock and in the account window.

use super::kit::{self, Action, FlippedView, Weight, color, icon_image, label, ns, rect};
use super::marks::AppMarkView;
use errand_core::Language;
use errand_core::model::Msg;
use errand_core::ports::Poster;
use errand_core::theme::{self, Rgb};
use errand_core::view::{AccountCardVm, DetailsVm, QuotaVm};
use errand_core::{Provider, layout};
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAccessibility, NSBezierPath, NSButton, NSCellImagePosition, NSControlSize, NSLineBreakMode, NSScrollElasticity,
    NSScrollView, NSScrollerStyle, NSTextAlignment, NSTextField, NSView,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use std::cell::Cell;
use std::sync::Arc;

define_class!(
    /// A button drawn as the desktop window's pills are: a round fill under its words, soft for most and dark
    /// for the one thing a screen is there to do.
    #[unsafe(super(NSButton))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandPillButton"]
    #[ivars = Cell<bool>]
    pub struct PillButton;

    impl PillButton {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, dirty: NSRect) {
            let bounds = self.bounds();
            let radius = bounds.size.height / 2.0;
            // Paler when it cannot be pressed, and a step stronger while it is held down.
            let strength = match (self.ivars().get(), self.isEnabled(), self.isHighlighted()) {
                (true, false, _) => 0.22,
                (true, true, true) => 0.82,
                (true, true, false) => 1.0,
                (false, false, _) => 0.04,
                (false, true, true) => 0.13,
                (false, true, false) => 0.07,
            };
            kit::paint(theme::INK.alpha(strength)).setFill();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(bounds, radius, radius).fill();
            let _: () = unsafe { msg_send![super(self), drawRect: dirty] };
        }
    }
);

/// A button with an icon beside its title, a consistent gap on the side facing the words.
pub fn button(title: &str, icon: &str, trailing: bool, frame: NSRect, run: impl Fn() + 'static, mtm: MainThreadMarker) -> (Retained<NSButton>, Retained<Action>) {
    pill(title, icon, trailing, frame, run, false, mtm)
}

/// The same in the dark fill, for the one thing a screen is there to do.
pub fn primary_button(title: &str, icon: &str, trailing: bool, frame: NSRect, run: impl Fn() + 'static, mtm: MainThreadMarker) -> (Retained<NSButton>, Retained<Action>) {
    pill(title, icon, trailing, frame, run, true, mtm)
}

/// A button that draws nothing of its own, over a row that is pressed whole.
pub fn cover_button(frame: NSRect, run: impl Fn() + 'static, mtm: MainThreadMarker) -> (Retained<NSButton>, Retained<Action>) {
    let button = NSButton::new(mtm);
    button.setTitle(&ns(""));
    button.setBordered(false);
    button.setTransparent(true);
    button.setFrame(frame);
    let action = Action::new(mtm, run);
    action.attach(&button);
    (button, action)
}

#[allow(clippy::too_many_arguments)]
fn pill(title: &str, icon: &str, trailing: bool, frame: NSRect, run: impl Fn() + 'static, dark: bool, mtm: MainThreadMarker) -> (Retained<NSButton>, Retained<Action>) {
    let pill = PillButton::alloc(mtm).set_ivars(Cell::new(dark));
    let pill: Retained<PillButton> = unsafe { msg_send![super(pill), initWithFrame: frame] };
    let button: Retained<NSButton> = Retained::into_super(pill);
    button.setTitle(&ns(title));
    let (leading, trailing_gap) = if title.is_empty() { (0.0, 0.0) } else if trailing { (4.0, 0.0) } else { (0.0, 4.0) };
    button.setImage(icon_image(icon, if title.is_empty() { 14.0 } else { 13.0 }, None, leading, trailing_gap).as_deref());
    button.setImagePosition(if title.is_empty() {
        NSCellImagePosition::ImageOnly
    } else if trailing {
        NSCellImagePosition::ImageTrailing
    } else {
        NSCellImagePosition::ImageLeading
    });
    button.setImageHugsTitle(true);
    button.setBordered(false);
    button.setFont(Some(&kit::font(12.0, Weight::Semibold)));
    // A borderless button's words and icon take this colour.
    button.setContentTintColor(Some(&color(if dark { theme::WHITE } else { theme::INK })));
    let action = Action::new(mtm, run);
    action.attach(&button);
    (button, action)
}

/// A pill wide enough for these words, with an icon beside them or not.
fn pill_width(title: &str, icon: bool) -> f64 {
    kit::text_width(title, &kit::font(12.0, Weight::Semibold)) + if icon { 45.0 } else { 28.0 }
}

/// A card's limits: one alone is drawn large; several each take a short row, so that all of them show whole.
const LIMITS_HEIGHT: f64 = 132.0;
const LIMIT_ROW: f64 = 66.0;

pub struct QuotaIvars {
    remaining: Cell<Option<i64>>,
    accent: Cell<Rgb>,
    compact: Cell<bool>,
    /// Another limit follows, so a rule parts the two.
    ruled: Cell<bool>,
}

define_class!(
    /// One limit: its bar, filled to what remains.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandQuotaView"]
    #[ivars = QuotaIvars]
    pub struct QuotaView;

    impl QuotaView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let compact = self.ivars().compact.get();
            let bounds = self.bounds();
            let (top, height) = if compact { (29.0, 6.0) } else { (66.0, 8.0) };
            let mut track = rect(0.0, top, bounds.size.width, height);
            kit::color(theme::TRACK).setFill();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(track, height / 2.0, height / 2.0).fill();
            if let Some(left) = self.ivars().remaining.get() {
                color(self.ivars().accent.get()).setFill();
                // Never shorter than it is tall, so that a little left still reads as a round end.
                track.size.width = (track.size.width * left.clamp(0, 100) as f64 / 100.0).max(if left > 0 { height } else { 0.0 });
                if track.size.width > 0.0 {
                    NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(track, height / 2.0, height / 2.0).fill();
                }
            }
            if self.ivars().ruled.get() {
                kit::color(theme::HAIRLINE).setFill();
                NSBezierPath::fillRect(rect(0.0, bounds.size.height - 1.0, bounds.size.width, 1.0));
            }
        }
    }
);

impl QuotaView {
    /// One limit with its words: its name, what is left, the bar, and when it refills.
    fn new(mtm: MainThreadMarker, frame: NSRect, vm: &QuotaVm, compact: bool, ruled: bool, tooltip: &str) -> Retained<QuotaView> {
        let this = QuotaView::alloc(mtm).set_ivars(QuotaIvars { remaining: Cell::new(vm.remaining), accent: Cell::new(vm.accent), compact: Cell::new(compact), ruled: Cell::new(ruled) });
        let row: Retained<QuotaView> = unsafe { msg_send![super(this), initWithFrame: frame] };
        let width = frame.size.width;
        let name = label(&vm.title, 11.0, Weight::Medium, theme::MUTED, mtm);
        name.setToolTip(Some(&ns(&vm.title)));
        row.addSubview(&name);
        // The share left in large figures, and what it is a share of beside them in small words.
        let (figure_size, note_size) = if compact { (19.0, 10.5) } else { (32.0, 13.0) };
        let figure_font = kit::font(figure_size, Weight::Bold);
        // A label keeps a little room round its words, so each is given more than its words measure.
        let (figure_width, note_width) = (kit::text_width(&vm.value, &figure_font) + 8.0, kit::text_width(vm.value_note, &kit::font(note_size, Weight::Medium)) + 8.0);
        let value = label(&vm.value, figure_size, Weight::Bold, vm.value_color, mtm);
        value.setAccessibilityLabel(Some(&ns(&format!("{} {} {}", vm.title, vm.value, vm.value_note))));
        row.addSubview(&value);
        let note = label(vm.value_note, note_size, Weight::Medium, theme::MUTED, mtm);
        row.addSubview(&note);
        let next = label(&vm.next_reset, if compact { 11.5 } else { 12.5 }, if compact { Weight::Medium } else { Weight::Semibold }, theme::INK, mtm);
        row.addSubview(&next);
        let date = label(&vm.reset_at, 11.0, Weight::Regular, theme::MUTED, mtm);
        date.setAlignment(NSTextAlignment::Right);
        let date_width = if vm.reset_at.is_empty() { 0.0 } else { kit::text_width(&vm.reset_at, &kit::font(11.0, Weight::Regular)) + 8.0 };
        row.addSubview(&date);
        if compact {
            // The name on the left and the share on the right of one line, then the bar, then the refill.
            let gap = if vm.value_note.is_empty() { 0.0 } else { note_width - 2.0 };
            name.setFrame(rect(0.0, 6.0, width - figure_width - gap - 8.0, 16.0));
            value.setFrame(rect(width - gap - figure_width, 0.0, figure_width, 25.0));
            note.setFrame(rect(width - note_width, 8.0, note_width, 15.0));
            next.setFrame(rect(0.0, 42.0, width - date_width - 6.0, 16.0));
            date.setFrame(rect(width - date_width, 42.5, date_width, 15.0));
            // No line to spare for when it last refilled; the row says it where the pointer rests.
            if let Some(last) = &vm.last_reset {
                row.setToolTip(Some(&ns(&format!("{last}\n{tooltip}"))));
            }
        } else {
            name.setFrame(rect(0.0, 4.0, width, 16.0));
            value.setFrame(rect(0.0, 21.0, figure_width, 40.0));
            note.setFrame(rect(figure_width - 1.0, 39.0, note_width, 18.0));
            next.setFrame(rect(0.0, 85.0, width - date_width - 6.0, 18.0));
            date.setFrame(rect(width - date_width, 86.5, date_width, 15.0));
            if let Some(last) = &vm.last_reset {
                let last = label(last, 11.0, Weight::Regular, theme::MUTED, mtm);
                last.setFrame(rect(0.0, 107.0, width, 16.0));
                last.setToolTip(Some(&ns(tooltip)));
                row.addSubview(&last);
            }
        }
        row
    }
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandAccountCard"]
    pub struct AccountCardView;

    impl AccountCardView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let bounds = self.bounds();
            let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect(1.0, 1.0, bounds.size.width - 2.0, bounds.size.height - 2.0), 20.0, 20.0);
            color(theme::WHITE).setFill();
            path.fill();
            color(theme::EDGE).setStroke();
            path.setLineWidth(1.0);
            path.stroke();
        }
    }
);

/// One service's card: its app, who is signed in, its open button and its limits.
pub struct AccountCard {
    pub view: Retained<AccountCardView>,
    pub provider: Provider,
    language: Language,
    status: Retained<NSTextField>,
    open: Retained<NSButton>,
    scroll: Retained<NSScrollView>,
    data_status: Retained<NSTextField>,
    _open_action: Retained<Action>,
    shown: Option<AccountCardVm>,
}

impl AccountCard {
    pub fn new(mtm: MainThreadMarker, frame: NSRect, provider: Provider, poster: Arc<dyn Poster>, language: Language) -> AccountCard {
        let view: Retained<AccountCardView> = unsafe { msg_send![AccountCardView::alloc(mtm), initWithFrame: frame] };
        let width = frame.size.width;
        view.addSubview(&AppMarkView::new(mtm, rect(20.0, 18.0, 34.0, 34.0), provider));
        let open_width = pill_width(language.open_button(), true);
        let title = label(provider.title(), 16.0, Weight::Semibold, theme::INK, mtm);
        title.setFrame(rect(64.0, 18.0, width - 64.0 - open_width - 24.0, 23.0));
        view.addSubview(&title);
        let status = label("", 10.0, Weight::Regular, theme::MUTED, mtm);
        status.setFrame(rect(64.0, 43.0, width - 84.0, 15.0));
        view.addSubview(&status);
        let (open, open_action) = button(language.open_button(), "external-link", true, rect(width - 16.0 - open_width, 16.0, open_width, 28.0), move || poster.post(Msg::OpenApp(provider)), mtm);
        open.setControlSize(NSControlSize::Regular);
        open.setAccessibilityLabel(Some(&ns(&language.open_label(provider))));
        view.addSubview(&open);
        let scroll = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), rect(22.0, 72.0, width - 44.0, LIMITS_HEIGHT));
        scroll.setDrawsBackground(false);
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);
        scroll.setScrollerStyle(NSScrollerStyle::Overlay);
        scroll.setHorizontalScrollElasticity(NSScrollElasticity::None);
        view.addSubview(&scroll);
        let data_status = label(language.checking_usage(), 10.0, Weight::Regular, theme::MUTED, mtm);
        data_status.setFrame(rect(22.0, frame.size.height - 38.0, width - 44.0, 16.0));
        view.addSubview(&data_status);
        AccountCard { view, provider, language, status, open, scroll, data_status, _open_action: open_action, shown: None }
    }

    pub fn update(&mut self, vm: &AccountCardVm, mtm: MainThreadMarker) {
        if self.shown.as_ref() == Some(vm) {
            return;
        }
        self.status.setStringValue(&ns(&vm.status));
        self.status.setToolTip(Some(&ns(&vm.status)));
        self.open.setTitle(&ns(vm.open_title));
        self.open.setEnabled(vm.open_enabled);
        let width = self.scroll.contentSize().width;
        let row_height = if vm.compact { LIMIT_ROW } else { LIMITS_HEIGHT };
        let previous = self.scroll.contentView().bounds().origin;
        let document = FlippedView::new(mtm, rect(0.0, 0.0, width, self.scroll.contentSize().height.max(vm.quotas.len() as f64 * row_height)));
        for (index, quota) in vm.quotas.iter().enumerate() {
            let frame = rect(0.0, index as f64 * row_height, width, row_height);
            document.addSubview(&QuotaView::new(mtm, frame, quota, vm.compact, index + 1 < vm.quotas.len(), self.language.reset_seen_tooltip()));
        }
        if let Some((headline, help)) = &vm.empty {
            let empty = label(headline, 17.0, Weight::Semibold, theme::INK, mtm);
            empty.setFrame(rect(0.0, 14.0, width, 26.0));
            document.addSubview(&empty);
            let advice = label(help, 12.0, Weight::Regular, theme::MUTED, mtm);
            advice.setMaximumNumberOfLines(5);
            advice.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
            advice.setFrame(rect(0.0, 46.0, width, 84.0));
            document.addSubview(&advice);
        }
        self.scroll.setDocumentView(Some(&document));
        let limit = (document.bounds().size.height - self.scroll.contentSize().height).max(0.0);
        self.scroll.contentView().scrollToPoint(NSPoint::new(previous.x, previous.y.min(limit)));
        self.scroll.reflectScrolledClipView(&self.scroll.contentView());
        self.data_status.setStringValue(&ns(&vm.data_status));
        self.data_status.setToolTip(Some(&ns(&vm.data_status_tooltip)));
        self.data_status.setTextColor(Some(&color(if vm.data_status_failed { theme::WARNING } else { theme::MUTED })));
        self.shown = Some(vm.clone());
    }
}

pub struct PopoverIvars {
    tail_on_right: Cell<bool>,
    tail_y: Cell<f64>,
}

define_class!(
    /// A bubble beside the dock: a rounded body with a tail pointing sideways at what it explains.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandUsagePopoverView"]
    #[ivars = PopoverIvars]
    pub struct PopoverView;

    impl PopoverView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let bounds = self.bounds();
            let tail = layout::POPOVER_TAIL;
            let right = self.ivars().tail_on_right.get();
            let body = rect(if right { 0.0 } else { tail }, 0.0, bounds.size.width - tail, bounds.size.height);
            kit::color(theme::GROUND).setFill();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(body, 22.0, 22.0).fill();
            let wanted = if self.ivars().tail_y.get() > 0.0 { self.ivars().tail_y.get() } else { bounds.size.height / 2.0 };
            let y = wanted.clamp(34.0, bounds.size.height - 34.0);
            let base = if right { body.origin.x + body.size.width - 1.0 } else { body.origin.x + 1.0 };
            let tip = if right { bounds.size.width } else { 0.0 };
            let shape = NSBezierPath::bezierPath();
            shape.moveToPoint(NSPoint::new(base, y - 12.0));
            shape.lineToPoint(NSPoint::new(tip, y));
            shape.lineToPoint(NSPoint::new(base, y + 12.0));
            shape.closePath();
            shape.fill();
        }
    }
);

impl PopoverView {
    /// A bubble whose body is this big, with room beside it for the tail.
    pub fn new(mtm: MainThreadMarker, body: NSSize) -> Retained<PopoverView> {
        let this = PopoverView::alloc(mtm).set_ivars(PopoverIvars { tail_on_right: Cell::new(false), tail_y: Cell::new(0.0) });
        unsafe { msg_send![super(this), initWithFrame: rect(0.0, 0.0, body.width + layout::POPOVER_TAIL, body.height)] }
    }

    /// Turns the tail toward the dock, `tail_y` down the bubble, and moves `body` clear of it.
    pub fn point(&self, body: &NSView, tail_on_right: bool, tail_y: f64) {
        self.ivars().tail_on_right.set(tail_on_right);
        self.ivars().tail_y.set(tail_y);
        body.setFrameOrigin(NSPoint::new(if tail_on_right { 0.0 } else { layout::POPOVER_TAIL }, 0.0));
        self.setNeedsDisplay(true);
    }
}

/// The round button that reads the usage again.
const REFRESH_WIDTH: f64 = 40.0;

/// The usage details beside the dock.
pub struct Details {
    pub view: Retained<PopoverView>,
    body: Retained<FlippedView>,
    cards: Vec<AccountCard>,
    refresh: Retained<NSButton>,
    close: Retained<NSButton>,
    /// The services the cards were last laid out for.
    laid_out: Vec<Provider>,
    _actions: Vec<Retained<Action>>,
}

impl Details {
    pub fn new(mtm: MainThreadMarker, poster: Arc<dyn Poster>, language: Language) -> Details {
        let body_size = NSSize::new(layout::detail_body_width(Provider::ALL.len()), layout::DETAIL_HEIGHT);
        let view = PopoverView::new(mtm, body_size);
        let body = FlippedView::new(mtm, rect(0.0, 0.0, body_size.width, body_size.height));
        view.addSubview(&body);
        let heading = label(language.details_title(), 20.0, Weight::Semibold, theme::INK, mtm);
        heading.setFrame(rect(20.0, 15.0, 270.0, 30.0));
        body.addSubview(&heading);
        let cards: Vec<AccountCard> = Provider::ALL
            .iter()
            .enumerate()
            .map(|(index, provider)| {
                let frame = rect(12.0 + index as f64 * (layout::DETAIL_CARD_WIDTH + layout::DETAIL_CARD_GAP), 53.0, layout::DETAIL_CARD_WIDTH, 252.0);
                let card = AccountCard::new(mtm, frame, *provider, poster.clone(), language);
                body.addSubview(&card.view);
                card
            })
            .collect();
        let post = |msg: fn() -> Msg| {
            let poster = poster.clone();
            move || poster.post(msg())
        };
        let (all, all_action) = button(language.show_all(), "layout-dashboard", false, rect(14.0, body_size.height - 40.0, pill_width(language.show_all(), true), 28.0), post(|| Msg::ShowDashboard), mtm);
        body.addSubview(&all);
        let (refresh, refresh_action) = button("", "refresh-cw", false, rect(body_size.width - 14.0 - REFRESH_WIDTH, 16.0, REFRESH_WIDTH, 28.0), post(|| Msg::AskUsage), mtm);
        refresh.setToolTip(Some(&ns(language.refresh())));
        refresh.setAccessibilityLabel(Some(&ns(language.refresh_all())));
        body.addSubview(&refresh);
        let close_width = pill_width(language.close(), true);
        let (close, close_action) = button(language.close(), "x", false, rect(body_size.width - 14.0 - close_width, body_size.height - 40.0, close_width, 28.0), post(|| Msg::CloseDetails), mtm);
        body.addSubview(&close);
        Details { view, body, cards, refresh, close, laid_out: Provider::ALL.to_vec(), _actions: vec![all_action, refresh_action, close_action] }
    }

    pub fn render(&mut self, vm: &DetailsVm, mtm: MainThreadMarker) {
        let shown: Vec<Provider> = vm.cards.iter().map(|card| card.provider).collect();
        if shown != self.laid_out {
            self.layout(&shown);
        }
        for card_vm in &vm.cards {
            if let Some(card) = self.cards.iter_mut().find(|card| card.provider == card_vm.provider) {
                card.update(card_vm, mtm);
            }
        }
        self.refresh.setEnabled(vm.refresh_enabled);
    }

    /// A card per service shown, and the popover only as wide as they need; the widget sizes its window to it.
    fn layout(&mut self, shown: &[Provider]) {
        let size = NSSize::new(layout::detail_body_width(shown.len()), layout::DETAIL_HEIGHT);
        self.body.setFrameSize(size);
        self.view.setFrameSize(NSSize::new(size.width + layout::POPOVER_TAIL, size.height));
        for card in &self.cards {
            let place = shown.iter().position(|provider| *provider == card.provider);
            card.view.setHidden(place.is_none());
            if let Some(place) = place {
                card.view.setFrameOrigin(NSPoint::new(12.0 + place as f64 * (layout::DETAIL_CARD_WIDTH + layout::DETAIL_CARD_GAP), 53.0));
            }
        }
        self.refresh.setFrameOrigin(NSPoint::new(size.width - 14.0 - REFRESH_WIDTH, 16.0));
        self.close.setFrameOrigin(NSPoint::new(size.width - 14.0 - self.close.frame().size.width, size.height - 40.0));
        self.laid_out = shown.to_vec();
        self.view.setNeedsDisplay(true);
    }

    /// Turns the tail toward the dock, level with the middle of its batteries.
    pub fn point(&self, tail_on_right: bool, tail_y: f64) {
        self.view.point(&self.body, tail_on_right, tail_y);
    }
}

