//! The account window: both services side by side with their limits, batteries and a friendly word on
//! each, opened from the menu, the details or the Dock icon.

use super::battery::BatteryView;
use super::details::{AccountCard, button};
use super::kit::{Action, Weight, color, grey, label, ns, rect};
use errand_core::model::Msg;
use errand_core::ports::Poster;
use errand_core::theme::{self, Rgb};
use errand_core::view::{DashboardVm, Tone};
use errand_core::{Language, Provider};
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAccessibility, NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSBackingStoreType, NSBezierPath, NSButton, NSColor,
    NSLineBreakMode, NSTextField, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect};
use std::sync::Arc;

pub const SIZE: (f64, f64) = (1040.0, 588.0);

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandDashboardView"]
    pub struct DashboardView;

    impl DashboardView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, dirty: NSRect) {
            color(Rgb { red: 0.975, green: 0.971, blue: 0.961 }).setFill();
            NSBezierPath::fillRect(dirty);
            grey(0.87, 1.0).setFill();
            NSBezierPath::fillRect(rect(29.0, 507.0, self.bounds().size.width - 58.0, 1.0));
        }
    }
);

pub struct Dashboard {
    pub window: Retained<NSWindow>,
    pub view: Retained<DashboardView>,
    title: Retained<NSTextField>,
    cards: Vec<AccountCard>,
    batteries: Vec<(Provider, Retained<BatteryView>)>,
    moods: Vec<(Provider, Retained<NSTextField>)>,
    /// Each service's column is this wide, however many show.
    column_width: f64,
    /// The services the columns were last laid out for.
    laid_out: Vec<Provider>,
    refresh: Retained<NSButton>,
    usage_status: Retained<NSTextField>,
    message: Retained<NSTextField>,
    _refresh_action: Retained<Action>,
    shown: Option<DashboardVm>,
}

impl Dashboard {
    pub fn new(mtm: MainThreadMarker, poster: Arc<dyn Poster>, language: Language) -> Dashboard {
        let (width, height) = SIZE;
        let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Miniaturizable;
        let window: Retained<NSWindow> = unsafe {
            msg_send![NSWindow::alloc(mtm), initWithContentRect: rect(0.0, 0.0, width, height), styleMask: style,
                      backing: NSBackingStoreType::Buffered, defer: false]
        };
        window.setTitle(&ns(language.window_title_usage()));
        window.setAppearance(NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua }).as_deref());
        unsafe { window.setReleasedWhenClosed(false) };
        let view: Retained<DashboardView> = unsafe { msg_send![DashboardView::alloc(mtm), initWithFrame: rect(0.0, 0.0, width, height)] };
        window.setContentView(Some(&view));
        let title = label(&language.dashboard_title(&Provider::ALL), 24.0, Weight::Semibold, theme::INK, mtm);
        title.setFrame(rect(28.0, 23.0, 570.0, 32.0));
        view.addSubview(&title);
        let subtitle = label(language.dashboard_subtitle(), 12.0, Weight::Regular, theme::MUTED, mtm);
        subtitle.setFrame(rect(29.0, 62.0, 530.0, 20.0));
        view.addSubview(&subtitle);
        let refresh_poster = poster.clone();
        let (refresh, refresh_action) =
            button(language.refresh(), "refresh-cw", false, rect(width - 133.0, 30.0, 107.0, 30.0), move || refresh_poster.post(Msg::AskUsage), mtm);
        refresh.setAccessibilityLabel(Some(&ns(language.refresh_all())));
        view.addSubview(&refresh);
        let count = Provider::ALL.len() as f64;
        let column_width = ((width - 56.0 - 20.0 * (count - 1.0)) / count).floor();
        let card_width = column_width;
        let (mut cards, mut batteries, mut moods) = (Vec::new(), Vec::new(), Vec::new());
        for (index, provider) in Provider::ALL.into_iter().enumerate() {
            let x = 28.0 + index as f64 * (card_width + 20.0);
            let card = AccountCard::new(mtm, rect(x, 103.0, card_width, 298.0), provider, poster.clone(), language);
            view.addSubview(&card.view);
            cards.push(card);
            let battery = BatteryView::new(mtm, rect(x, 417.0, card_width, 44.0), provider, language);
            view.addSubview(&battery);
            batteries.push((provider, battery));
            let mood = label(language.waiting_for_usage(), 11.0, Weight::Regular, theme::MUTED, mtm);
            mood.setFrame(rect(x + 4.0, 469.0, card_width - 8.0, 18.0));
            view.addSubview(&mood);
            moods.push((provider, mood));
        }
        let usage_status = label("", 11.0, Weight::Regular, theme::MUTED, mtm);
        usage_status.setFrame(rect(29.0, 520.0, width - 58.0, 18.0));
        view.addSubview(&usage_status);
        let message = label("", 10.0, Weight::Regular, theme::MUTED, mtm);
        message.setFrame(rect(29.0, 546.0, width - 58.0, 26.0));
        message.setMaximumNumberOfLines(2);
        message.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
        view.addSubview(&message);
        window.center();
        Dashboard {
            window,
            view,
            title,
            cards,
            batteries,
            moods,
            column_width,
            laid_out: Provider::ALL.to_vec(),
            refresh,
            usage_status,
            message,
            _refresh_action: refresh_action,
            shown: None,
        }
    }

    pub fn render(&mut self, vm: &DashboardVm, mtm: MainThreadMarker) {
        if self.shown.as_ref() == Some(vm) {
            return;
        }
        let shown: Vec<Provider> = vm.cards.iter().map(|card| card.provider).collect();
        if shown != self.laid_out {
            self.layout(&shown);
        }
        self.title.setStringValue(&ns(&vm.title));
        for card_vm in &vm.cards {
            if let Some(card) = self.cards.iter_mut().find(|card| card.provider == card_vm.provider) {
                card.update(card_vm, mtm);
            }
        }
        for battery_vm in &vm.batteries {
            if let Some((_, battery)) = self.batteries.iter().find(|(provider, _)| *provider == battery_vm.provider) {
                battery.update(battery_vm);
            }
        }
        for (provider, words) in shown.iter().zip(&vm.moods) {
            if let Some((_, mood)) = self.moods.iter().find(|(own, _)| own == provider) {
                mood.setStringValue(&ns(words));
            }
        }
        self.refresh.setEnabled(vm.refresh_enabled);
        self.usage_status.setStringValue(&ns(&vm.usage_status));
        self.message.setStringValue(&ns(&vm.message.text));
        let tone = match vm.message.tone {
            Tone::Neutral => NSColor::secondaryLabelColor(),
            Tone::Error => NSColor::systemRedColor(),
        };
        self.message.setTextColor(Some(&tone));
        self.shown = Some(vm.clone());
    }

    /// The columns of the services shown, centred; a hidden service's column goes.
    fn layout(&mut self, shown: &[Provider]) {
        let count = shown.len() as f64;
        let start = (SIZE.0 - (count * self.column_width + (count - 1.0) * 20.0)) / 2.0;
        for (index, provider) in Provider::ALL.into_iter().enumerate() {
            let place = shown.iter().position(|own| *own == provider);
            // Each view with how far in from its column's edge it sits, and its top; the mood line is set in a little.
            let column: [(&NSView, f64, f64); 3] = [(&self.cards[index].view, 0.0, 103.0), (&self.batteries[index].1, 0.0, 417.0), (&self.moods[index].1, 4.0, 469.0)];
            for (view, inset, top) in column {
                view.setHidden(place.is_none());
                if let Some(place) = place {
                    view.setFrameOrigin(NSPoint::new(start + place as f64 * (self.column_width + 20.0) + inset, top));
                }
            }
        }
        self.laid_out = shown.to_vec();
    }

    pub fn show(&self, mtm: MainThreadMarker) {
        #[allow(deprecated)]
        objc2_app_kit::NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
        self.window.makeKeyAndOrderFront(None);
    }
}

