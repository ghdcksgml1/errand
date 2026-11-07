//! Floating panels that never take the person's typing focus unless they click into a text field, and the conversion between AppKit's screen
//! coordinates (origin bottom left, y up) and the core's (origin top left, y down).

use super::kit::{ns, rect};
use errand_core::layout::Rect;
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSBackingStoreType, NSColor, NSFloatingWindowLevel, NSPanel, NSScreen, NSWindow,
    NSWindowCollectionBehavior, NSWindowStyleMask,
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
    // Takes typing only once a click lands in one of its text fields, and even then Errand does not become the
    // active app: the app the person was in stays in front, and gets the keys back when the panel goes.
    #[unsafe(super(NSPanel))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandTypingPanel"]
    pub struct TypingPanel;

    impl TypingPanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key(&self) -> bool {
            true
        }

        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main(&self) -> bool {
            false
        }
    }
);

const STYLE: NSWindowStyleMask = NSWindowStyleMask::Borderless.union(NSWindowStyleMask::NonactivatingPanel);

/// A borderless, transparent, non-activating panel that floats over other apps on every Space.
pub fn make_panel(mtm: MainThreadMarker, size: NSSize, title: &str) -> Retained<NSPanel> {
    let panel: Retained<PassivePanel> = unsafe {
        msg_send![PassivePanel::alloc(mtm), initWithContentRect: rect(0.0, 0.0, size.width, size.height), styleMask: STYLE,
                  backing: NSBackingStoreType::Buffered, defer: false]
    };
    floating(Retained::into_super(panel), title)
}

/// The same panel, for one with a text field to type in.
pub fn make_typing_panel(mtm: MainThreadMarker, size: NSSize, title: &str) -> Retained<NSPanel> {
    let panel: Retained<TypingPanel> = unsafe {
        msg_send![TypingPanel::alloc(mtm), initWithContentRect: rect(0.0, 0.0, size.width, size.height), styleMask: STYLE,
                  backing: NSBackingStoreType::Buffered, defer: false]
    };
    floating(Retained::into_super(panel), title)
}

fn floating(panel: Retained<NSPanel>, title: &str) -> Retained<NSPanel> {
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
    panel
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
