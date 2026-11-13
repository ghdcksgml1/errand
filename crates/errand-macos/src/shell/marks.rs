//! Each service is marked by its installed app's own icon, read from the app at run time and never copied
//! into Errand; without the app, a letter tile in the service's colour stands in.

use super::kit::{color, draw_text, font, ns, rect, text_size, Weight};
use crate::apps;
use block2::RcBlock;
use errand_core::{Provider, theme};
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, DefinedClass};
use objc2_app_kit::{NSAccessibility, NSBezierPath, NSCompositingOperation, NSGraphicsContext, NSImage, NSImageInterpolation, NSView, NSWorkspace};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

thread_local! {
    static ICONS: RefCell<HashMap<Provider, Option<Retained<NSImage>>>> = RefCell::new(HashMap::new());
}

/// The installed app's icon; None when the app is not installed.
pub fn app_icon(provider: Provider) -> Option<Retained<NSImage>> {
    ICONS.with(|icons| {
        icons
            .borrow_mut()
            .entry(provider)
            .or_insert_with(|| {
                let app = match provider {
                    Provider::Codex => apps::codex_app(&std::env::home_dir().unwrap_or_default()),
                    Provider::Claude => apps::installed(Provider::Claude),
                }?;
                Some(NSWorkspace::sharedWorkspace().iconForFile(&ns(&app.to_string_lossy())))
            })
            .clone()
    })
}

/// Draws the app icon over `frame`, or a letter tile in the service's colour when the app is missing.
pub fn draw_app_mark(provider: impl Into<Option<Provider>>, frame: NSRect) {
    let Some(provider) = provider.into() else {
        if let Some(image) = super::kit::icon_image("paperclip", frame.size.width, None, 0.0, 0.0) {
            image.drawInRect(frame);
        }
        return;
    };
    if let Some(icon) = app_icon(provider) {
        // App icons keep a transparent margin of about a tenth; let the artwork itself fill the rect.
        let artwork = rect(
            frame.origin.x - frame.size.width * 0.1,
            frame.origin.y - frame.size.height * 0.1,
            frame.size.width * 1.2,
            frame.size.height * 1.2,
        );
        NSGraphicsContext::saveGraphicsState_class();
        if let Some(context) = NSGraphicsContext::currentContext() {
            context.setImageInterpolation(NSImageInterpolation::High);
        }
        unsafe { icon.drawInRect_fromRect_operation_fraction_respectFlipped_hints(artwork, NSRect::ZERO, NSCompositingOperation::SourceOver, 1.0, true, None) };
        NSGraphicsContext::restoreGraphicsState_class();
        return;
    }
    let accent = theme::accent(provider);
    let radius = frame.size.width * 0.28;
    super::kit::paint(accent.alpha(0.16)).setFill();
    NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(frame, radius, radius).fill();
    let letter = &provider.title()[..1];
    let typeface = font(frame.size.height * 0.55, Weight::Bold);
    let size = text_size(letter, &typeface);
    draw_text(
        letter,
        NSPoint::new(frame.origin.x + (frame.size.width - size.width) / 2.0, frame.origin.y + (frame.size.height - size.height) / 2.0),
        &typeface,
        &color(accent),
    );
}

/// The mark as an image, for menus.
pub fn app_mark_image(provider: Provider, size: f64) -> Retained<NSImage> {
    let handler = RcBlock::new(move |bounds: NSRect| -> Bool {
        draw_app_mark(provider, bounds);
        Bool::YES
    });
    let image = NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(size, size), true, &handler);
    image.setAccessibilityDescription(Some(&ns(provider.title())));
    image
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandAppMarkView"]
    #[ivars = Cell<Option<Provider>>]
    pub struct AppMarkView;

    impl AppMarkView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            draw_app_mark(self.ivars().get(), self.bounds());
        }
    }
);

impl AppMarkView {
    pub fn new(mtm: MainThreadMarker, frame: NSRect, provider: impl Into<Option<Provider>>,
    ) -> Retained<AppMarkView> {
        let provider = provider.into();
        let title = provider.map_or("Attachment", Provider::title);
        let this = AppMarkView::alloc(mtm).set_ivars(Cell::new(provider));
        let view: Retained<AppMarkView> = unsafe { msg_send![super(this), initWithFrame: frame] };
        view.setAccessibilityElement(true);
        view.setAccessibilityLabel(Some(&ns(title)));
        view.setToolTip(Some(&ns(title)));
        view
    }
}
