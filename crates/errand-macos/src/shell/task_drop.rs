//! One native drop destination wraps every widget size without intercepting mouse drags or clicks.
use super::kit::{self, ns, rect};
use errand_core::{
    model::Msg,
    ports::Poster,
    tasks::{self, Input},
};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::*;
use objc2_foundation::{NSArray, NSDictionary, NSObjectProtocol, NSPoint, NSRect, NSURL};
use std::{cell::Cell, sync::Arc};

define_class!(
    #[unsafe(super(NSView))] #[thread_kind = MainThreadOnly] #[name = "ErrandDropOutline"] #[ivars = Cell<bool>]
    struct Outline;
    impl Outline {
        #[unsafe(method(hitTest:))] fn hit_test(&self, _point: NSPoint) -> *mut NSView { std::ptr::null_mut() }
        #[unsafe(method(drawRect:))] fn draw(&self, _dirty: NSRect) {
            if !self.ivars().get() { return }
            NSColor::colorWithCalibratedRed_green_blue_alpha(0.40, 0.32, 0.78, 0.16).setFill();
            let bounds = self.bounds();
            let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect(2.0, 2.0, bounds.size.width - 4.0, bounds.size.height - 4.0), 14.0, 14.0);
            path.fill(); NSColor::colorWithCalibratedRed_green_blue_alpha(0.40, 0.32, 0.78, 1.0).setStroke(); path.setLineWidth(3.0); path.stroke();
        }
    }
);
pub struct DropIvars {
    poster: Arc<dyn Poster>,
    outline: Retained<Outline>,
}
define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandTaskDropView"]
    #[ivars = DropIvars]
    pub struct DropView;
    impl DropView {
        #[unsafe(method(isFlipped))]
        fn flipped(&self) -> bool { true }
        #[unsafe(method(resizeSubviewsWithOldSize:))]
        fn resize_contents(&self, _old: objc2_foundation::NSSize) {
            // NSPanel used to size the mode view directly. Keep that exact sizing through this wrapper.
            for view in self.subviews() { view.setFrame(self.bounds()); }
        }
        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, event: &NSEvent) { if let Some(view) = self.subviews().firstObject() { view.mouseEntered(event); } }
        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, event: &NSEvent) { if let Some(view) = self.subviews().firstObject() { view.mouseExited(event); } }
    }
    unsafe impl NSObjectProtocol for DropView {}
    unsafe impl NSDraggingDestination for DropView {
        #[unsafe(method(draggingEntered:))]
        fn entered(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            let valid = sender
                .draggingPasteboard()
                .availableTypeFromArray(&types())
                .is_some();
            self.highlight(valid);
            if valid {
                NSDragOperation::Copy
            } else {
                NSDragOperation::None
            }
        }
        #[unsafe(method(draggingExited:))]
        fn exited(&self, _sender: Option<&ProtocolObject<dyn NSDraggingInfo>>) {
            self.highlight(false);
        }
        #[unsafe(method(prepareForDragOperation:))]
        fn prepare(&self, _sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            true
        }
        #[unsafe(method(performDragOperation:))]
        fn perform(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            let inputs = read_inputs(&sender.draggingPasteboard());
            self.highlight(false);
            if inputs.is_empty() {
                false
            } else {
                self.ivars()
                    .poster
                    .post(Msg::Tasks(tasks::Msg::Drop(inputs)));
                true
            }
        }
        #[unsafe(method(concludeDragOperation:))]
        fn conclude(&self, _sender: Option<&ProtocolObject<dyn NSDraggingInfo>>) {
            self.highlight(false);
        }
    }
);
fn types() -> Retained<NSArray<NSPasteboardType>> {
    unsafe {
        NSArray::from_slice(&[
            NSPasteboardTypeFileURL,
            NSPasteboardTypePNG,
            NSPasteboardTypeTIFF,
        ])
    }
}
/// File URLs are decoded by Foundation (spaces, Unicode and percent signs remain literal).
pub fn read_inputs(pasteboard: &NSPasteboard) -> Vec<Input> {
    let mut inputs = vec![];
    if let Some(items) = pasteboard.pasteboardItems() {
        for item in items {
            if let Some(value) = item.stringForType(unsafe { NSPasteboardTypeFileURL })
                && let Some(url) = NSURL::URLWithString(&value)
                && url.isFileURL()
                && let Some(path) = url.to_file_path()
            {
                inputs.push(Input::File(path));
            }
        }
    }
    if inputs.is_empty()
        && let Some(value) = pasteboard.stringForType(unsafe { NSPasteboardTypeFileURL })
        && let Some(url) = NSURL::URLWithString(&value) && url.isFileURL()
        && let Some(path) = url.to_file_path() {
        inputs.push(Input::File(path));
    }
    if !inputs.is_empty() { return inputs; }
    if let Some(data) = pasteboard.dataForType(unsafe { NSPasteboardTypePNG }) {
        if data.len() as u64 <= tasks::MAX_FILE_BYTES {
            inputs.push(Input::Png(data.to_vec()));
        }
    } else if let Some(data) = pasteboard.dataForType(unsafe { NSPasteboardTypeTIFF })
        && data.len() as u64 <= tasks::MAX_FILE_BYTES
        && let Some(bitmap) = NSBitmapImageRep::initWithData(NSBitmapImageRep::alloc(), &data)
        && let Some(png) = unsafe {
            bitmap.representationUsingType_properties(
                NSBitmapImageFileType::PNG,
                &NSDictionary::new(),
            )
        }
    {
        if png.len() as u64 <= tasks::MAX_FILE_BYTES {
            inputs.push(Input::Png(png.to_vec()));
        }
    }
    inputs
}
impl DropView {
    pub fn new(mtm: MainThreadMarker, poster: Arc<dyn Poster>, content: &NSView) -> Retained<Self> {
        let outline = Outline::alloc(mtm).set_ivars(Cell::new(false));
        let outline: Retained<Outline> =
            unsafe { msg_send![super(outline), initWithFrame: content.bounds()] };
        outline.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        let this = Self::alloc(mtm).set_ivars(DropIvars { poster, outline });
        let view: Retained<Self> =
            unsafe { msg_send![super(this), initWithFrame: content.frame()] };
        view.registerForDraggedTypes(&types());
        view.setToolTip(Some(&ns(
            "파일이나 이미지를 놓아 작업 등록 / Drop files or images to add a task",
        )));
        view.hold(content);
        view
    }
    pub fn hold(&self, content: &NSView) {
        kit::clear(self);
        content.setFrame(self.bounds());
        // Modes lay themselves out before the panel resizes; do not apply that size change twice.
        content.setAutoresizingMask(NSAutoresizingMaskOptions::empty());
        self.addSubview(content);
        self.ivars().outline.setFrame(self.bounds());
        self.addSubview(&self.ivars().outline);
    }
    fn highlight(&self, on: bool) {
        if self.ivars().outline.ivars().replace(on) != on {
            self.ivars()
                .poster
                .post(Msg::Tasks(tasks::Msg::Dragging(on)));
        }
        self.ivars().outline.setNeedsDisplay(true);
    }
}
