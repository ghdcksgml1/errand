//! A Lucide icon that turns, for work in progress. Core Animation turns a cached image, so it costs the app
//! nothing per frame; snapshots and Reduce Motion show it still.

use super::kit::icon_image;
use errand_core::theme::Rgb;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSGraphicsContext, NSView, NSWorkspace};
use objc2_foundation::{NSNumber, NSRect, NSString};
use objc2_quartz_core::{CABasicAnimation, CALayer, CAMediaTiming};
use std::cell::RefCell;

pub struct SpinnerIvars {
    icon: &'static str,
    color: Rgb,
    glyph: RefCell<Option<Retained<CALayer>>>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandSpinningIconView"]
    #[ivars = SpinnerIvars]
    pub struct SpinningIconView;

    impl SpinningIconView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(viewDidMoveToWindow))]
        fn view_did_move_to_window(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidMoveToWindow] };
            if let Some(glyph) = self.ivars().glyph.borrow_mut().take() {
                glyph.removeFromSuperlayer();
            }
            self.setNeedsDisplay(true);
            let Some(window) = self.window() else { return };
            if NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion() {
                return;
            }
            self.setWantsLayer(true);
            let Some(layer) = self.layer() else { return };
            let scale = if window.backingScaleFactor() > 0.0 { window.backingScaleFactor() } else { 2.0 };
            let glyph = CALayer::layer();
            glyph.setFrame(layer.bounds());
            glyph.setContentsScale(scale);
            if let Some(image) = icon_image(self.ivars().icon, self.bounds().size.width, Some(self.ivars().color), 0.0, 0.0) {
                unsafe { glyph.setContents(Some(&image.layerContentsForContentsScale(scale))) };
            }
            let spin = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("transform.rotation.z")));
            unsafe {
                spin.setFromValue(Some(&NSNumber::new_f64(0.0)));
                spin.setToValue(Some(&NSNumber::new_f64(-2.0 * std::f64::consts::PI)));
            }
            // A slow turn says "still working" without pulling the eye from everything else.
            spin.setDuration(1.8);
            spin.setRepeatCount(f32::INFINITY);
            glyph.addAnimation_forKey(&spin, Some(&NSString::from_str("spin")));
            layer.addSublayer(&glyph);
            *self.ivars().glyph.borrow_mut() = Some(glyph);
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            // On screen the turning layer shows the icon.
            if self.ivars().glyph.borrow().is_some() && NSGraphicsContext::currentContextDrawingToScreen() {
                return;
            }
            if let Some(image) = icon_image(self.ivars().icon, self.bounds().size.width, Some(self.ivars().color), 0.0, 0.0) {
                image.drawInRect(self.bounds());
            }
        }
    }
);

impl SpinningIconView {
    pub fn new(mtm: MainThreadMarker, frame: NSRect, icon: &'static str, color: Rgb) -> Retained<SpinningIconView> {
        let this = SpinningIconView::alloc(mtm).set_ivars(SpinnerIvars { icon, color, glyph: RefCell::new(None) });
        let view: Retained<SpinningIconView> = unsafe { msg_send![super(this), initWithFrame: frame] };
        view
    }
}
