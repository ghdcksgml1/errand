//! Small AppKit helpers every view uses: colours, fonts, labels, Lucide icons, characters and button actions.

use block2::RcBlock;
use errand_core::avatar::{self, Avatar};
use errand_core::icons::{self, Element, PathOp};
use errand_core::theme::{Paint, Rgb};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAffineTransformNSAppKitAdditions, NSBezierPath, NSColor, NSControl, NSFont, NSFontWeight, NSGraphicsContext, NSImage, NSLineBreakMode,
    NSLineCapStyle, NSLineJoinStyle, NSStringDrawing, NSTextField, NSView,
};
use objc2_foundation::{NSAffineTransform, NSPoint, NSRect, NSSize, NSString};

pub fn color(rgb: Rgb) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(rgb.red, rgb.green, rgb.blue, 1.0)
}

pub fn paint(paint: Paint) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(paint.color.red, paint.color.green, paint.color.blue, paint.alpha)
}

pub fn grey(white: f64, alpha: f64) -> Retained<NSColor> {
    NSColor::colorWithWhite_alpha(white, alpha)
}

#[derive(Clone, Copy)]
pub enum Weight {
    Regular,
    Medium,
    Semibold,
    Bold,
}

fn weight(weight: Weight) -> NSFontWeight {
    unsafe {
        match weight {
            Weight::Regular => objc2_app_kit::NSFontWeightRegular,
            Weight::Medium => objc2_app_kit::NSFontWeightMedium,
            Weight::Semibold => objc2_app_kit::NSFontWeightSemibold,
            Weight::Bold => objc2_app_kit::NSFontWeightBold,
        }
    }
}

pub fn font(size: f64, style: Weight) -> Retained<NSFont> {
    NSFont::systemFontOfSize_weight(size, weight(style))
}

/// Digits that line up as they change.
pub fn digits_font(size: f64, style: Weight) -> Retained<NSFont> {
    NSFont::monospacedDigitSystemFontOfSize_weight(size, weight(style))
}

pub fn ns(text: &str) -> Retained<NSString> {
    NSString::from_str(text)
}

pub fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

pub fn inset(frame: NSRect, dx: f64, dy: f64) -> NSRect {
    rect(frame.origin.x + dx, frame.origin.y + dy, frame.size.width - 2.0 * dx, frame.size.height - 2.0 * dy)
}

/// A one-line label that truncates its tail.
pub fn label(text: &str, size: f64, style: Weight, ink: Rgb, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let label = NSTextField::labelWithString(&ns(text), mtm);
    label.setFont(Some(&font(size, style)));
    label.setTextColor(Some(&color(ink)));
    label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    label
}

/// Width a label needs for its whole text; truncating labels under-report their intrinsic width.
pub fn fitting_width(label: &NSTextField) -> f64 {
    text_width(&label.stringValue().to_string(), &label.font().unwrap_or_else(|| font(13.0, Weight::Regular))).ceil() + 5.0
}

pub fn text_width(text: &str, font: &NSFont) -> f64 {
    text_size(text, font).width
}

pub fn text_size(text: &str, font: &NSFont) -> NSSize {
    let attributes = attributes(font, None);
    unsafe { ns(text).sizeWithAttributes(Some(&attributes)) }
}

pub fn attributes(font: &NSFont, ink: Option<&NSColor>) -> Retained<objc2_foundation::NSDictionary<objc2_foundation::NSAttributedStringKey, AnyObject>> {
    let mut keys: Vec<&objc2_foundation::NSAttributedStringKey> = vec![unsafe { objc2_app_kit::NSFontAttributeName }];
    let mut values: Vec<&AnyObject> = vec![font.as_ref()];
    if let Some(ink) = ink {
        keys.push(unsafe { objc2_app_kit::NSForegroundColorAttributeName });
        values.push(ink.as_ref());
    }
    objc2_foundation::NSDictionary::from_slices(&keys, &values)
}

/// Draws `text` with its top left at `point` in a flipped view.
pub fn draw_text(text: &str, point: NSPoint, font: &NSFont, ink: &NSColor) {
    unsafe { ns(text).drawAtPoint_withAttributes(point, Some(&attributes(font, Some(ink)))) };
}

/// A Lucide icon as one stroked path on its 24-unit grid. Ovals and rounded rectangles use AppKit's own
/// shapes, as the Objective-C build drew them.
pub fn icon_path(name: &str) -> Option<Retained<NSBezierPath>> {
    let elements = icons::elements(name)?;
    let path = NSBezierPath::bezierPath();
    for element in elements {
        match *element {
            Element::Path(data) => append_ops(&path, &icons::path::parse(data)),
            Element::Circle { cx, cy, r } => path.appendBezierPathWithOvalInRect(rect(cx - r, cy - r, r * 2.0, r * 2.0)),
            Element::Ellipse { cx, cy, rx, ry } => path.appendBezierPathWithOvalInRect(rect(cx - rx, cy - ry, rx * 2.0, ry * 2.0)),
            Element::Rect { x, y, width, height, rx, ry } => path.appendBezierPathWithRoundedRect_xRadius_yRadius(rect(x, y, width, height), rx, ry),
            Element::Line { .. } | Element::Polyline(_) | Element::Polygon(_) => {
                append_ops(&path, &icons::element_outline(element));
            }
        }
    }
    path.setLineWidth(icons::STROKE);
    path.setLineCapStyle(NSLineCapStyle::Round);
    path.setLineJoinStyle(NSLineJoinStyle::Round);
    Some(path)
}

pub fn append_ops(path: &NSBezierPath, ops: &[PathOp]) {
    let point = |p: icons::Point| NSPoint::new(p.x, p.y);
    for op in ops {
        match *op {
            PathOp::MoveTo(to) => path.moveToPoint(point(to)),
            PathOp::LineTo(to) => path.lineToPoint(point(to)),
            PathOp::CubicTo(to, first, second) => path.curveToPoint_controlPoint1_controlPoint2(point(to), point(first), point(second)),
            PathOp::Close => path.closePath(),
        }
    }
}

/// An icon image `size` points tall, with `leading` and `trailing` space beside it. Without a colour it is a
/// template image that buttons and image views tint.
pub fn icon_image(name: &str, size: f64, ink: Option<Rgb>, leading: f64, trailing: f64) -> Option<Retained<NSImage>> {
    let shape = icon_path(name)?;
    let stroke = ink.map(color);
    let handler = RcBlock::new(move |_: NSRect| -> Bool {
        let transform = NSAffineTransform::transform();
        transform.translateXBy_yBy(leading, 0.0);
        transform.scaleBy(size / icons::GRID);
        transform.concat();
        stroke.clone().unwrap_or_else(NSColor::blackColor).setStroke();
        shape.stroke();
        Bool::YES
    });
    let image = NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(leading + size + trailing, size), true, &handler);
    image.setTemplate(ink.is_none());
    image.setAccessibilityDescription(Some(&ns(name)));
    Some(image)
}

/// Draws a character in the circle inscribed in `frame` of a flipped view; the circle cuts off the rest.
pub fn draw_avatar(avatar: &Avatar, frame: NSRect) {
    NSGraphicsContext::saveGraphicsState_class();
    NSBezierPath::bezierPathWithOvalInRect(frame).addClip();
    draw_character(avatar, frame);
    NSGraphicsContext::restoreGraphicsState_class();
}

/// The character whole, as no circle cuts it.
pub fn draw_character(avatar: &Avatar, frame: NSRect) {
    let shape = |ops: &[PathOp]| {
        let path = NSBezierPath::bezierPath();
        append_ops(&path, ops);
        path
    };
    NSGraphicsContext::saveGraphicsState_class();
    let transform = NSAffineTransform::transform();
    transform.translateXBy_yBy(frame.origin.x, frame.origin.y);
    transform.scaleBy(frame.size.width / avatar::FRAME);
    transform.concat();
    for layer in avatar.layers() {
        if let Some(clip) = &layer.clip {
            NSGraphicsContext::saveGraphicsState_class();
            shape(clip).addClip();
        }
        let path = shape(&layer.path);
        if let Some(fill) = layer.fill {
            paint(fill).setFill();
            path.fill();
        }
        if let Some(stroke) = layer.stroke {
            paint(stroke.paint).setStroke();
            path.setLineWidth(stroke.width);
            path.setLineCapStyle(NSLineCapStyle::Round);
            path.setLineJoinStyle(NSLineJoinStyle::Round);
            path.stroke();
        }
        if layer.clip.is_some() {
            NSGraphicsContext::restoreGraphicsState_class();
        }
    }
    NSGraphicsContext::restoreGraphicsState_class();
}

define_class!(
    /// Runs a Rust closure as a control's or menu item's action. Controls hold their target weakly, so the
    /// view that owns the control keeps its `Action` too.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandAction"]
    #[ivars = Box<dyn Fn()>]
    pub struct Action;

    impl Action {
        #[unsafe(method(perform:))]
        fn perform(&self, _sender: Option<&AnyObject>) {
            (self.ivars())();
        }
    }
);

impl Action {
    pub fn new(mtm: MainThreadMarker, run: impl Fn() + 'static) -> Retained<Action> {
        let this = Action::alloc(mtm).set_ivars(Box::new(run) as Box<dyn Fn()>);
        unsafe { msg_send![super(this), init] }
    }

    pub fn attach(&self, control: &NSControl) {
        unsafe {
            control.setTarget(Some(self));
            control.setAction(Some(sel!(perform:)));
        }
    }
}

define_class!(
    /// A plain container whose y axis grows downward, as all of Errand's layouts are written.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandFlippedView"]
    pub struct FlippedView;

    impl FlippedView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

impl FlippedView {
    pub fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<FlippedView> {
        unsafe { msg_send![FlippedView::alloc(mtm), initWithFrame: frame] }
    }
}

/// The icon on a state chip or count, and the chip's own height.
pub const CHIP_ICON: f64 = 11.0;

pub fn contains(frame: NSRect, point: NSPoint) -> bool {
    point.x >= frame.origin.x && point.y >= frame.origin.y && point.x < frame.origin.x + frame.size.width && point.y < frame.origin.y + frame.size.height
}

define_class!(
    /// A flipped container VoiceOver can press, like the task header.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandPressableView"]
    #[ivars = std::cell::RefCell<Option<std::rc::Rc<dyn Fn()>>>]
    pub struct PressableView;

    impl PressableView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn accessibility_perform_press(&self) -> bool {
            let press = self.ivars().borrow().clone();
            press.map(|press| press()).is_some()
        }
    }
);

impl PressableView {
    pub fn new(mtm: MainThreadMarker, frame: NSRect, press: Option<std::rc::Rc<dyn Fn()>>) -> Retained<PressableView> {
        let this = PressableView::alloc(mtm).set_ivars(std::cell::RefCell::new(press));
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }
}

/// Removes every subview, for views rebuilt from a new view model.
pub fn clear(view: &NSView) {
    for child in view.subviews().iter() {
        child.removeFromSuperview();
    }
}
