//! Floating panels that never take the person's typing focus, and the conversion between AppKit's screen
//! coordinates (origin bottom left, y up) and the core's (origin top left, y down).

use super::kit::{ns, rect};
use errand_core::layout::Rect;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use std::cell::Cell;
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSBackingStoreType, NSColor, NSFloatingWindowLevel, NSPanel, NSScreen, NSWindow,
    NSView, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSRect, NSSize};

define_class!(
    // A click on it never makes Errand the key or main window: the app the person types in keeps focus.
    #[unsafe(super(NSPanel))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandPassivePanel"]
    pub struct PassivePanel;

    impl PassivePanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key(&self) -> bool {
            false
        }

        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main(&self) -> bool {
            false
        }
    }
);

define_class!(
    /// A panel like the others that takes typing only while it holds a form, such as naming a new room. It
    /// stays non-activating: the app the person was in stays the active one, and gets its typing back with a
    /// click.
    #[unsafe(super(NSPanel))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandTypingPanel"]
    #[ivars = Cell<bool>]
    pub struct TypingPanel;

    impl TypingPanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key(&self) -> bool {
            self.ivars().get()
        }

        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main(&self) -> bool {
            false
        }
    }
);

impl TypingPanel {
    /// Takes typing, typing into `field`, while a form shows; gives it up with the form.
    pub fn set_typing(&self, field: Option<&NSView>) {
        self.ivars().set(field.is_some());
        if let Some(field) = field {
            self.makeKeyWindow();
            self.makeFirstResponder(Some(field));
        } else if self.isKeyWindow() {
            self.resignKeyWindow();
        }
    }
}

const STYLE: NSWindowStyleMask = NSWindowStyleMask::Borderless.union(NSWindowStyleMask::NonactivatingPanel);

/// A borderless, transparent, non-activating panel that floats over other apps on every Space.
pub fn make_panel(mtm: MainThreadMarker, size: NSSize, title: &str) -> Retained<NSPanel> {
    let panel: Retained<PassivePanel> = unsafe {
        msg_send![PassivePanel::alloc(mtm), initWithContentRect: rect(0.0, 0.0, size.width, size.height), styleMask: STYLE,
                  backing: NSBackingStoreType::Buffered, defer: false]
    };
    let panel = Retained::into_super(panel);
    configure(&panel, title);
    panel
}

/// The same panel, able to take typing while it holds a form.
pub fn make_typing_panel(mtm: MainThreadMarker, size: NSSize, title: &str) -> Retained<TypingPanel> {
    let this = TypingPanel::alloc(mtm).set_ivars(Cell::new(false));
    let panel: Retained<TypingPanel> = unsafe {
        msg_send![super(this), initWithContentRect: rect(0.0, 0.0, size.width, size.height), styleMask: STYLE,
                  backing: NSBackingStoreType::Buffered, defer: false]
    };
    configure(&panel, title);
    panel
}

fn configure(panel: &NSPanel, title: &str) {
    panel.setLevel(NSFloatingWindowLevel);
    panel.setFloatingPanel(true);
    panel.setHidesOnDeactivate(false);
    panel.setBecomesKeyOnlyIfNeeded(true);
    panel.setWorksWhenModal(true);
    panel.setOpaque(false);
    panel.setBackgroundColor(Some(&NSColor::clearColor()));
    panel.setHasShadow(true);
    unsafe { panel.setReleasedWhenClosed(false) };
    panel.setCollectionBehavior(NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::FullScreenAuxiliary);
    panel.setAppearance(NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua }).as_deref());
    panel.setTitle(&ns(title));
}

/// The height AppKit's flipped y is measured against: the primary screen's.
pub fn primary_height(mtm: MainThreadMarker) -> f64 {
    NSScreen::screens(mtm).firstObject().map_or(0.0, |screen| screen.frame().size.height)
}

pub fn to_core(frame: NSRect, primary: f64) -> Rect {
    Rect::new(frame.origin.x, primary - frame.origin.y - frame.size.height, frame.size.width, frame.size.height)
}

pub fn to_cocoa(frame: Rect, primary: f64) -> NSRect {
    rect(frame.x, primary - frame.y - frame.height, frame.width, frame.height)
}

/// Every screen's usable area, without the menu bar and Dock, in core coordinates.
pub fn visible_frames(mtm: MainThreadMarker) -> Vec<Rect> {
    let primary = primary_height(mtm);
    NSScreen::screens(mtm).iter().map(|screen| to_core(screen.visibleFrame(), primary)).collect()
}

/// The usable area of the screen a window is on, or the main screen's.
pub fn screen_frame(window: &NSWindow, mtm: MainThreadMarker) -> Rect {
    let screen = window.screen().or_else(|| NSScreen::mainScreen(mtm));
    to_core(screen.map_or(NSRect::ZERO, |screen| screen.visibleFrame()), primary_height(mtm))
}
