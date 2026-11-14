//! One service's remaining allowance as a battery whose liquid fill gently ripples. On screen the liquid is
//! Core Animation layers: the ripple loops and the level slides without any work in the app per frame.
//! Snapshots and Reduce Motion draw it still.

use super::kit::{self, Weight, color, digits_font, grey, icon_image, label, ns, rect};
use super::marks::AppMarkView;
use errand_core::Language;
use errand_core::theme::{self, Rgb};
use errand_core::usage::Mood;
use errand_core::view::BatteryVm;
use errand_core::{Provider, layout};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSAccessibilityLevelIndicatorRole, NSBezierPath, NSGraphicsContext, NSImageView, NSTextAlignment, NSTextField, NSView,
    NSWorkspace,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGColor, CGMutablePath, CGPath};
use objc2_foundation::{NSArray, NSNumber, NSPoint, NSRect};
use objc2_quartz_core::{
    CABasicAnimation, CAGradientLayer, CAKeyframeAnimation, CALayer, CAMediaTiming, CAMediaTimingFunction, CAShapeLayer, CATransaction,
};
use std::cell::{Cell, RefCell};
use std::f64::consts::PI;

const CAP_WIDTH: f64 = 4.0;
const CAP_GAP: f64 = 2.0;
const INSET: f64 = 2.5;

/// Shallower near empty or full, where a big wave would read as a different level.
fn amplitude(level: f64, width: f64) -> f64 {
    (level.min(1.0 - level) * width / 6.0).clamp(0.4, 1.8)
}

/// The liquid's edge around x = `edge`: two sines of different length keep the ripple from looking
/// mechanical. Both turn a whole number of times per cycle, so the loop joins without a jump.
fn surface(bounds: NSRect, edge: f64, phase: f64, amplitude: f64, back: bool, closed: bool) -> Vec<(f64, f64)> {
    let wavelength = bounds.size.height * 1.7;
    let (sign, shift, scale) = if back { (-1.0, 2.1, 1.4) } else { (1.0, 0.0, 1.0) };
    let (top, bottom) = (bounds.origin.y, bounds.origin.y + bounds.size.height);
    let mut points = Vec::new();
    let mut y = top - 1.0;
    while y <= bottom + 1.5 {
        let x = edge
            + amplitude * scale * ((2.0 * PI * y / wavelength + sign * phase + shift).sin() + 0.35 * (2.0 * PI * y / (wavelength * 0.55) - sign * 2.0 * phase).sin());
        points.push((x, y));
        y += 1.5;
    }
    if closed {
        points.push((bounds.origin.x - 1.0, bottom + 1.5));
        points.push((bounds.origin.x - 1.0, top - 1.0));
    }
    points
}

fn cg_path(points: &[(f64, f64)], closed: bool) -> objc2_core_foundation::CFRetained<CGMutablePath> {
    let path = CGMutablePath::new();
    for (index, (x, y)) in points.iter().enumerate() {
        unsafe {
            if index == 0 {
                CGMutablePath::move_to_point(Some(&path), std::ptr::null(), *x, *y);
            } else {
                CGMutablePath::add_line_to_point(Some(&path), std::ptr::null(), *x, *y);
            }
        }
    }
    if closed {
        CGMutablePath::close_subpath(Some(&path));
    }
    path
}

/// Core Foundation objects are Objective-C objects too; arrays of paths and colours hold them as such.
fn as_object<T>(value: &T) -> &AnyObject {
    unsafe { &*(value as *const T).cast::<AnyObject>() }
}

fn cg_color(rgb: Rgb, alpha: f64) -> objc2_core_foundation::CFRetained<CGColor> {
    kit::paint(rgb.alpha(alpha)).CGColor().into()
}

pub struct LiquidIvars {
    level: Cell<f64>,
    has_level: Cell<bool>,
    color: Cell<Rgb>,
    /// Its x is the level; the shapes inside keep their edge at x = 0.
    slider: RefCell<Option<Retained<CALayer>>>,
    shapes: RefCell<Vec<Retained<CAShapeLayer>>>,
    ripple_level: Cell<f64>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandLiquidView"]
    #[ivars = LiquidIvars]
    pub struct LiquidView;

    impl LiquidView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(viewDidMoveToWindow))]
        fn view_did_move_to_window(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidMoveToWindow] };
            let center = NSWorkspace::sharedWorkspace().notificationCenter();
            unsafe {
                center.removeObserver(self);
                if self.window().is_some() {
                    center.addObserver_selector_name_object(
                        self,
                        sel!(rebuild),
                        Some(objc2_app_kit::NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification),
                        None,
                    );
                }
            }
            self.rebuild_layers();
        }

        #[unsafe(method(rebuild))]
        fn rebuild(&self) {
            self.rebuild_layers();
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let ivars = self.ivars();
            let animates = ivars.slider.borrow().is_some();
            if (animates && NSGraphicsContext::currentContextDrawingToScreen()) || !ivars.has_level.get() || ivars.level.get() < 0.002 {
                return;
            }
            let bounds = self.bounds();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(bounds, self.radius(), self.radius()).addClip();
            let (level, tint) = (ivars.level.get(), ivars.color.get());
            let edge = bounds.size.width * level;
            let height = amplitude(level, bounds.size.width);
            for layer in 0..3 {
                let points = surface(bounds, edge, 0.0, height, layer == 0, layer != 2);
                let path = NSBezierPath::bezierPath();
                for (index, (x, y)) in points.iter().enumerate() {
                    if index == 0 { path.moveToPoint(NSPoint::new(*x, *y)) } else { path.lineToPoint(NSPoint::new(*x, *y)) }
                }
                if layer == 2 {
                    kit::paint(tint.alpha(0.75)).setStroke();
                    path.setLineWidth(1.2);
                    path.stroke();
                } else {
                    path.closePath();
                    kit::paint(tint.alpha(if layer == 1 { 0.36 } else { 0.20 })).setFill();
                    path.fill();
                }
            }
        }
    }
);

impl LiquidView {
/// Recreates the liquid's layers, or leaves it drawn still under Reduce Motion or off screen.
fn rebuild_layers(&self) {
        if let Some(slider) = self.ivars().slider.borrow_mut().take() {
            slider.removeFromSuperlayer();
        }
        self.ivars().shapes.borrow_mut().clear();
        self.setNeedsDisplay(true);
        if self.window().is_none() || NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion() {
            return;
        }
        self.setWantsLayer(true);
        let Some(layer) = self.layer() else { return };
        layer.setMasksToBounds(true);
        layer.setCornerRadius(self.radius());
        let slider = CALayer::layer();
        slider.setAnchorPoint(CGPoint::new(0.0, 0.0));
        slider.setBounds(layer.bounds());
        let level = if self.ivars().has_level.get() { self.ivars().level.get() * self.bounds().size.width } else { 0.0 };
        slider.setPosition(CGPoint::new(level, 0.0));
        slider.setHidden(!self.ivars().has_level.get());
        let shapes: Vec<Retained<CAShapeLayer>> = (0..3).map(|_| CAShapeLayer::layer()).collect();
        shapes[2].setFillColor(None);
        shapes[2].setLineWidth(1.2);
        for shape in &shapes {
            shape.setFrame(slider.bounds());
            slider.addSublayer(shape);
        }
        layer.addSublayer(&slider);
        *self.ivars().slider.borrow_mut() = Some(slider);
        *self.ivars().shapes.borrow_mut() = shapes;
        self.ivars().ripple_level.set(-1.0);
        self.apply_color();
        self.apply_ripple();
    }


    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<LiquidView> {
        let this = LiquidView::alloc(mtm).set_ivars(LiquidIvars {
            level: Cell::new(0.0),
            has_level: Cell::new(false),
            color: Cell::new(theme::HEALTHY),
            slider: RefCell::new(None),
            shapes: RefCell::new(Vec::new()),
            ripple_level: Cell::new(-1.0),
        });
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }

    /// The battery's inner curve: its body radius less the inset around the liquid.
    fn radius(&self) -> f64 {
        (10.0f64).min((self.bounds().size.height + INSET * 2.0) * 0.32) - INSET
    }

    fn apply_color(&self) {
        let tint = self.ivars().color.get();
        let shapes = self.ivars().shapes.borrow();
        if let [back, front, edge] = shapes.as_slice() {
            back.setFillColor(Some(&cg_color(tint, 0.20)));
            front.setFillColor(Some(&cg_color(tint, 0.36)));
            edge.setStrokeColor(Some(&cg_color(tint, 0.75)));
        }
    }

    /// The ripple's shape depends on the level, so it is rebuilt only when the level moves.
    fn apply_ripple(&self) {
        let ivars = self.ivars();
        let level = ivars.level.get();
        if ivars.slider.borrow().is_none() || (ivars.ripple_level.get() - level).abs() < 0.005 {
            return;
        }
        ivars.ripple_level.set(level);
        let width = self.bounds().size.width;
        let height = amplitude(level, width);
        // In the slider the edge sits at x = 0, so the liquid reaches a full width to its left.
        let bounds = rect(-width - 2.0, 0.0, width + 2.0, self.bounds().size.height);
        let frames = 36;
        for (index, shape) in ivars.shapes.borrow().iter().enumerate() {
            let (back, closed) = (index == 0, index != 2);
            let paths: Vec<_> = (0..=frames)
                .map(|frame| cg_path(&surface(bounds, 0.0, 2.0 * PI * f64::from(frame) / f64::from(frames), height, back, closed), closed))
                .collect();
            let first: &CGPath = &paths[0];
            shape.setPath(Some(first));
            let objects: Vec<&AnyObject> = paths.iter().map(|path| as_object::<CGPath>(path)).collect();
            let ripple = CAKeyframeAnimation::animationWithKeyPath(Some(&ns("path")));
            unsafe { ripple.setValues(Some(&NSArray::from_slice(&objects))) };
            ripple.setDuration(3.0);
            ripple.setRepeatCount(f32::INFINITY);
            shape.addAnimation_forKey(&ripple, Some(&ns("ripple")));
        }
    }

    fn set_level(&self, level: f64, visible: bool, tint: Rgb) {
        let ivars = self.ivars();
        let rises = ivars.has_level.get() && visible && self.window().is_some_and(|window| window.isVisible());
        let slider = ivars.slider.borrow().clone();
        let from = slider.as_ref().map_or(0.0, |slider| unsafe { slider.presentationLayer() }.unwrap_or_else(|| slider.clone()).position().x);
        ivars.level.set(level);
        ivars.has_level.set(visible);
        ivars.color.set(tint);
        self.setNeedsDisplay(true);
        let Some(slider) = slider else { return };
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        slider.setHidden(!visible);
        slider.setPosition(CGPoint::new(level * self.bounds().size.width, 0.0));
        self.apply_color();
        CATransaction::commit();
        self.apply_ripple();
        let to = slider.position().x;
        if rises && (from - to).abs() > 0.5 {
            // New values rise into place with an exponential ease-out.
            let rise = CABasicAnimation::animationWithKeyPath(Some(&ns("position.x")));
            unsafe {
                rise.setFromValue(Some(&NSNumber::new_f64(from)));
                rise.setToValue(Some(&NSNumber::new_f64(to)));
            }
            rise.setDuration(1.1);
            rise.setTimingFunction(Some(&CAMediaTimingFunction::functionWithControlPoints(0.16, 1.0, 0.3, 1.0)));
            slider.addAnimation_forKey(&rise, Some(&ns("rise")));
        }
    }

    /// A reset sends one light band through the refilled liquid.
    fn shimmer(&self) {
        let ivars = self.ivars();
        if ivars.slider.borrow().is_none() || !ivars.has_level.get() {
            return;
        }
        let Some(layer) = self.layer() else { return };
        let band = CAGradientLayer::layer();
        let colors = [grey(1.0, 0.0).CGColor(), grey(1.0, 0.7).CGColor(), grey(1.0, 0.0).CGColor()];
        let objects: Vec<&AnyObject> = colors.iter().map(|color| as_object::<CGColor>(color)).collect();
        unsafe { band.setColors(Some(&NSArray::from_slice(&objects))) };
        band.setStartPoint(CGPoint::new(0.0, 0.5));
        band.setEndPoint(CGPoint::new(1.0, 0.5));
        band.setFrame(CGRect::new(CGPoint::new(-40.0, 0.0), CGSize::new(40.0, self.bounds().size.height)));
        layer.addSublayer(&band);
        CATransaction::begin();
        let finished_band = band.clone();
        let finished = block2::RcBlock::new(move || finished_band.removeFromSuperlayer());
        unsafe { CATransaction::setCompletionBlock(Some(&finished)) };
        let sweep = CABasicAnimation::animationWithKeyPath(Some(&ns("position.x")));
        unsafe {
            sweep.setFromValue(Some(&NSNumber::new_f64(-20.0)));
            sweep.setToValue(Some(&NSNumber::new_f64(ivars.level.get() * self.bounds().size.width - 20.0)));
            sweep.setFillMode(objc2_quartz_core::kCAFillModeForwards);
        }
        sweep.setDuration(1.6);
        sweep.setRemovedOnCompletion(false);
        band.addAnimation_forKey(&sweep, Some(&ns("sweep")));
        CATransaction::commit();
    }
}

pub struct BatteryIvars {
    icon_size: f64,
    liquid: RefCell<Option<Retained<LiquidView>>>,
    percent: RefCell<Option<Retained<NSTextField>>>,
    warning: RefCell<Option<Retained<NSImageView>>>,
    detail: RefCell<Option<Retained<NSTextField>>>,
    mood: Cell<Option<Mood>>,
    shown: RefCell<Option<BatteryVm>>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandBatteryView"]
    #[ivars = BatteryIvars]
    pub struct BatteryView;

    impl BatteryView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let body = self.body();
            let radius = (10.0f64).min(body.size.height * 0.32);
            let shell = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(body, radius, radius);
            kit::color(theme::SHELL).setFill();
            shell.fill();
            let cap = rect(body.origin.x + body.size.width + CAP_GAP, body.origin.y + body.size.height / 2.0 - body.size.height * 0.17, CAP_WIDTH, body.size.height * 0.34);
            kit::color(theme::OUTLINE).setFill();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(cap, 1.5, 1.5).fill();
            kit::color(theme::OUTLINE).setStroke();
            shell.setLineWidth(1.0);
            shell.stroke();
        }
    }
);

impl BatteryView {
    pub fn new(mtm: MainThreadMarker, frame: NSRect, provider: Provider, language: Language) -> Retained<BatteryView> {
        let height = frame.size.height;
        let icon = (24.0f64).min(height - 12.0);
        let this = BatteryView::alloc(mtm).set_ivars(BatteryIvars {
            icon_size: icon,
            liquid: RefCell::new(None),
            percent: RefCell::new(None),
            warning: RefCell::new(None),
            detail: RefCell::new(None),
            mood: Cell::new(None),
            shown: RefCell::new(None),
        });
        let view: Retained<BatteryView> = unsafe { msg_send![super(this), initWithFrame: frame] };
        let liquid = LiquidView::new(mtm, kit::inset(view.body(), INSET, INSET));
        view.addSubview(&liquid);
        view.addSubview(&AppMarkView::new(mtm, rect(0.0, (height - icon) / 2.0, icon, icon), provider));
        let percent = label(language.checking(), 13.0, Weight::Semibold, theme::INK, mtm);
        percent.setFont(Some(&digits_font((18.0f64).min((height * 0.43).round()), Weight::Semibold)));
        view.addSubview(&percent);
        let warning = NSImageView::imageViewWithImage(&icon_image("battery-warning", 13.0, None, 0.0, 0.0).expect("a vendored icon"), mtm);
        warning.setFrame(rect(0.0, 0.0, 13.0, 13.0));
        warning.setContentTintColor(Some(&color(theme::LOW)));
        warning.setAccessibilityElement(false);
        warning.setHidden(true);
        view.addSubview(&warning);
        let detail = label(language.checking_reset(), 10.5, Weight::Regular, theme::MUTED, mtm);
        detail.setAlignment(NSTextAlignment::Right);
        view.addSubview(&detail);
        view.setAccessibilityElement(true);
        view.setAccessibilityRole(Some(unsafe { NSAccessibilityLevelIndicatorRole }));
        view.setAccessibilityLabel(Some(&ns(&language.usage_left(provider))));
        *view.ivars().liquid.borrow_mut() = Some(liquid);
        *view.ivars().percent.borrow_mut() = Some(percent);
        *view.ivars().warning.borrow_mut() = Some(warning);
        *view.ivars().detail.borrow_mut() = Some(detail);
        view.layout_labels();
        view
    }

    fn body(&self) -> NSRect {
        let left = self.ivars().icon_size + 8.0;
        let bounds = self.bounds();
        rect(left + 0.5, 0.5, bounds.size.width - CAP_WIDTH - CAP_GAP - 1.0 - left, bounds.size.height - 1.0)
    }

    fn layout_labels(&self) {
        let (Some(percent), Some(warning), Some(detail)) =
            (self.ivars().percent.borrow().clone(), self.ivars().warning.borrow().clone(), self.ivars().detail.borrow().clone())
        else {
            return;
        };
        let body = self.body();
        let height = self.bounds().size.height;
        let font = percent.font().unwrap_or_else(|| digits_font(13.0, Weight::Semibold));
        let width = kit::text_width(&percent.stringValue().to_string(), &font).ceil() + 5.0;
        let line = (font.ascender() - font.descender()).ceil() + 2.0;
        percent.setFrame(rect(body.origin.x + 9.0, ((height - line) / 2.0).round(), width, line));
        let mut x = percent.frame().origin.x + width + 2.0;
        if !warning.isHidden() {
            warning.setFrameOrigin(NSPoint::new(x, ((height - 13.0) / 2.0).round()));
            x += 17.0;
        }
        detail.setFrame(rect(x + 4.0, ((height - 15.0) / 2.0).round(), body.origin.x + body.size.width - 10.0 - x - 4.0, 15.0));
    }

    pub fn update(&self, vm: &BatteryVm) {
        if self.ivars().shown.borrow().as_ref() == Some(vm) {
            return;
        }
        if let Some(liquid) = self.ivars().liquid.borrow().as_ref() {
            liquid.set_level(vm.level.unwrap_or(0.0), vm.level.is_some(), vm.color);
            if vm.mood == Mood::Reset && self.ivars().mood.get() != Some(Mood::Reset) {
                liquid.shimmer();
            }
        }
        self.ivars().mood.set(Some(vm.mood));
        if let Some(percent) = self.ivars().percent.borrow().as_ref() {
            percent.setStringValue(&ns(&vm.percent));
            percent.setTextColor(Some(&color(vm.percent_color)));
        }
        if let Some(warning) = self.ivars().warning.borrow().as_ref() {
            warning.setHidden(!vm.low);
        }
        if let Some(detail) = self.ivars().detail.borrow().as_ref() {
            detail.setStringValue(&ns(&vm.detail));
            detail.setTextColor(Some(&color(vm.detail_color)));
        }
        self.setToolTip(Some(&ns(&vm.tooltip)));
        unsafe { self.setAccessibilityValue(Some(&NSNumber::new_i64(vm.accessibility_value))) };
        self.setAccessibilityValueDescription(Some(&ns(&vm.accessibility_description)));
        self.layout_labels();
        *self.ivars().shown.borrow_mut() = Some(vm.clone());
    }

    /// How full the liquid is drawn, 0-1.
    pub fn shown_level(&self) -> f64 {
        self.ivars().liquid.borrow().as_ref().filter(|liquid| liquid.ivars().has_level.get()).map_or(0.0, |liquid| liquid.ivars().level.get())
    }
}

/// The width a battery gets in the dock.
pub fn dock_battery_width() -> f64 {
    layout::STACK_WIDTH - layout::DOCK_PADDING * 2.0 - layout::REFRESH_SIZE - 6.0
}

