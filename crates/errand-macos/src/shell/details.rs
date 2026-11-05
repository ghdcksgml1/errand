//! Account cards with each limit's remaining share, next reset and observed reset, shown in the usage
//! details beside the dock and in the account window.

use super::kit::{self, Action, FlippedView, Weight, color, grey, icon_image, label, ns, rect};
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
    NSAccessibility, NSBezelStyle, NSBezierPath, NSBox, NSBoxType, NSButton, NSCellImagePosition, NSControlSize, NSLineBreakMode, NSScrollElasticity,
    NSScrollView, NSScrollerStyle, NSTextAlignment, NSTextField, NSView,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use std::cell::Cell;
use std::sync::Arc;

/// A bordered button with an icon beside its title, a consistent gap on the side facing the words.
pub fn button(title: &str, icon: &str, trailing: bool, frame: NSRect, run: impl Fn() + 'static, mtm: MainThreadMarker) -> (Retained<NSButton>, Retained<Action>) {
    let button = NSButton::new(mtm);
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
    button.setBezelStyle(NSBezelStyle::Push);
    button.setFrame(frame);
    let action = Action::new(mtm, run);
    action.attach(&button);
    (button, action)
}

pub struct QuotaIvars {
    remaining: Cell<Option<i64>>,
    accent: Cell<Rgb>,
    compact: Cell<bool>,
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
            let mut track = rect(0.0, if compact { 30.0 } else { 60.0 }, self.bounds().size.width, if compact { 6.0 } else { 8.0 });
            kit::color(Rgb { red: 0.91, green: 0.92, blue: 0.92 }).setFill();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(track, 4.0, 4.0).fill();
            if let Some(left) = self.ivars().remaining.get() {
                color(self.ivars().accent.get()).setFill();
                track.size.width *= left.clamp(0, 100) as f64 / 100.0;
                if track.size.width > 0.0 {
                    NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(track, 4.0, 4.0).fill();
                }
            }
        }
    }
);

impl QuotaView {
    fn new(mtm: MainThreadMarker, frame: NSRect, vm: &QuotaVm, compact: bool) -> Retained<QuotaView> {
        let this = QuotaView::alloc(mtm).set_ivars(QuotaIvars { remaining: Cell::new(vm.remaining), accent: Cell::new(vm.accent), compact: Cell::new(compact) });
        unsafe { msg_send![super(this), initWithFrame: frame] }
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
            grey(0.87, 1.0).setStroke();
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
        let title = label(provider.title(), 16.0, Weight::Semibold, theme::INK, mtm);
        title.setFrame(rect(64.0, 18.0, (110.0f64).min(width - 184.0), 23.0));
        view.addSubview(&title);
        let status = label("", 10.0, Weight::Regular, theme::MUTED, mtm);
        status.setFrame(rect(64.0, 43.0, width - 84.0, 15.0));
        view.addSubview(&status);
        let (open, open_action) = button(language.open_button(false), "external-link", true, rect(width - 115.0, 16.0, 99.0, 28.0), move || poster.post(Msg::OpenApp(provider)), mtm);
        open.setControlSize(NSControlSize::Regular);
        open.setFont(Some(&kit::font(12.0, Weight::Medium)));
        open.setContentTintColor(Some(&color(theme::accent(provider))));
        open.setAccessibilityLabel(Some(&ns(&language.open_label(provider))));
        view.addSubview(&open);
        let scroll = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), rect(22.0, 77.0, width - 44.0, frame.size.height - 129.0));
        scroll.setDrawsBackground(false);
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);
        scroll.setScrollerStyle(NSScrollerStyle::Overlay);
        scroll.setHorizontalScrollElasticity(NSScrollElasticity::None);
        view.addSubview(&scroll);
        let data_status = label(language.checking_usage(), 10.0, Weight::Regular, theme::MUTED, mtm);
        data_status.setFrame(rect(22.0, frame.size.height - 43.0, width - 44.0, 16.0));
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
        let width = self.scroll.contentSize().width - 6.0;
        let row_height = if vm.compact { 90.0 } else { 125.0 };
        let previous = self.scroll.contentView().bounds().origin;
        let document = FlippedView::new(mtm, rect(0.0, 0.0, width, self.scroll.contentSize().height.max(vm.quotas.len() as f64 * row_height)));
        for (index, quota) in vm.quotas.iter().enumerate() {
            let row = QuotaView::new(mtm, rect(0.0, index as f64 * row_height, width, row_height - 1.0), quota, vm.compact);
            let name = label(&quota.title, 11.0, Weight::Medium, theme::MUTED, mtm);
            name.setFrame(rect(0.0, if vm.compact { 5.0 } else { 0.0 }, if vm.compact { width - 142.0 } else { width }, 17.0));
            name.setToolTip(Some(&ns(&quota.title)));
            row.addSubview(&name);
            let value = label(&quota.value, if vm.compact { 21.0 } else { 29.0 }, Weight::Semibold, quota.value_color, mtm);
            value.setFrame(if vm.compact { rect(width - 140.0, 0.0, 140.0, 28.0) } else { rect(0.0, 20.0, width, 36.0) });
            value.setAlignment(if vm.compact { NSTextAlignment::Right } else { NSTextAlignment::Left });
            value.setAccessibilityLabel(Some(&ns(&format!("{} {}", quota.title, quota.value))));
            row.addSubview(&value);
            let next = label(&quota.next_reset, 11.0, Weight::Medium, theme::INK, mtm);
            next.setFrame(rect(0.0, if vm.compact { 42.0 } else { 79.0 }, width, 18.0));
            row.addSubview(&next);
            let last = label(&quota.last_reset, 11.0, Weight::Regular, theme::MUTED, mtm);
            last.setFrame(rect(0.0, if vm.compact { 62.0 } else { 101.0 }, width, 18.0));
            last.setToolTip(Some(&ns(self.language.reset_seen_tooltip())));
            row.addSubview(&last);
            if index + 1 < vm.quotas.len() {
                let line = NSBox::initWithFrame(NSBox::alloc(mtm), rect(0.0, if vm.compact { 85.0 } else { 136.0 }, width, 1.0));
                line.setBoxType(NSBoxType::Separator);
                row.addSubview(&line);
            }
            document.addSubview(&row);
        }
        if let Some((headline, help)) = &vm.empty {
            let empty = label(headline, 17.0, Weight::Semibold, theme::INK, mtm);
            empty.setFrame(rect(0.0, 23.0, width, 28.0));
            document.addSubview(&empty);
            let advice = label(help, 12.0, Weight::Regular, theme::MUTED, mtm);
            advice.setMaximumNumberOfLines(3);
            advice.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
            advice.setFrame(rect(0.0, 64.0, width, 62.0));
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
            kit::color(Rgb { red: 0.985, green: 0.982, blue: 0.975 }).setFill();
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
        let (all, all_action) = button(language.show_all(), "layout-dashboard", false, rect(14.0, body_size.height - 40.0, 104.0, 28.0), post(|| Msg::ShowDashboard), mtm);
        body.addSubview(&all);
        let (refresh, refresh_action) = button("", "refresh-cw", false, rect(body_size.width - 64.0, 16.0, 48.0, 28.0), post(|| Msg::AskUsage), mtm);
        refresh.setToolTip(Some(&ns(language.refresh())));
        refresh.setAccessibilityLabel(Some(&ns(language.refresh_all())));
        body.addSubview(&refresh);
        let (close, close_action) = button(language.close(), "x", false, rect(body_size.width - 95.0, body_size.height - 40.0, 79.0, 28.0), post(|| Msg::CloseDetails), mtm);
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
        self.refresh.setFrameOrigin(NSPoint::new(size.width - 64.0, 16.0));
        self.close.setFrameOrigin(NSPoint::new(size.width - 95.0, size.height - 40.0));
        self.laid_out = shown.to_vec();
        self.view.setNeedsDisplay(true);
    }

    /// Turns the tail toward the dock, level with the middle of its batteries.
    pub fn point(&self, tail_on_right: bool, tail_y: f64) {
        self.view.point(&self.body, tail_on_right, tail_y);
    }
}

