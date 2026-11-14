//! Draws parts of the widget to a PNG without showing a window, for checking the screens without real
//! accounts.

use super::avatar_editor::AvatarEditor;
use super::compact::{Character, CompactEvents, Simple};
use super::journal::Journal;
use super::details::Details;
use super::dock::{Dock, DockEvents};
use super::kit;
use super::kit::FlippedView;
use super::panel::{make_panel, screen_frame, visible_frames};
use super::rooms::Rooms;
use super::tray::Tray;
use errand_core::Language;
use errand_core::avatar::Avatar;
use errand_core::layout::{self, DockShape};
use errand_core::model::AppState;
use errand_core::view::{self, ViewContext};
use jiff::Timestamp;
use objc2::AnyThread;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSAffineTransformNSAppKitAdditions, NSBitmapImageFileType, NSBitmapImageRep, NSColorRenderingIntent, NSColorSpace, NSDeviceRGBColorSpace, NSGraphicsContext, NSView};
use objc2_foundation::NSPoint;
use objc2_foundation::{NSAffineTransform, NSDictionary, NSSize};
use std::path::Path;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PreviewKind {
    Settings,
    TaskForm,
    /// The account window.
    Dashboard,
    Dock,
    Details,
    /// The pet's morning bubble about the last day's work.
    Nudge,
    /// The character editor beside the dock.
    AvatarEditor,
    /// The cards stacked on the dock.
    Tray,
    /// The list of work rooms beside the dock.
    Rooms,
    /// One character whole, on nothing.
    Character(Avatar),
    /// The simple widget: a one-line card per task that calls, over the line of counts and batteries; open, every
    /// task's line.
    Simple { open: bool },
    /// The character-only widget; open, with the simple widget over it, as the pointer resting on it shows.
    Badge { open: bool },
}

fn capture(view: &NSView) -> Option<Retained<NSBitmapImageRep>> {
    view.layoutSubtreeIfNeeded();
    let bounds = view.bounds();
    let bitmap = view.bitmapImageRepForCachingDisplayInRect(bounds)?;
    view.cacheDisplayInRect_toBitmapImageRep(bounds, &bitmap);
    // sRGB, whatever the screen's own space, so previews compare pixel for pixel across builds and Macs.
    Some(bitmap.bitmapImageRepByConvertingToColorSpace_renderingIntent(&NSColorSpace::sRGBColorSpace(), NSColorRenderingIntent::Default).unwrap_or(bitmap))
}

fn save(bitmap: &NSBitmapImageRep, output: &Path) -> bool {
    let png = unsafe { bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) };
    png.is_some_and(|png| std::fs::write(output, png.to_vec()).is_ok())
}

fn snapshot(view: &NSView, output: &Path) -> bool {
    capture(view).is_some_and(|bitmap| save(&bitmap, output))
}

/// An empty image of this size in points, at twice as many pixels.
fn canvas(size: NSSize) -> Option<Retained<NSBitmapImageRep>> {
    let image = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(), std::ptr::null_mut(), (size.width * 2.0) as isize, (size.height * 2.0) as isize, 8, 4, true, false,
            NSDeviceRGBColorSpace, 0, 0,
        )
    }?;
    image.setSize(size);
    Some(image)
}

/// A character drawn as the widget draws it, without the circle, so the README's are the app's own.
fn character(avatar: &Avatar, output: &Path) -> bool {
    const SIDE: f64 = 200.0;
    let Some(image) = canvas(NSSize::new(SIDE, SIDE)) else { return false };
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(NSGraphicsContext::graphicsContextWithBitmapImageRep(&image).as_deref());
    // Characters are drawn downwards, as in the widget's flipped views.
    let flip = NSAffineTransform::transform();
    flip.translateXBy_yBy(0.0, SIDE);
    flip.scaleXBy_yBy(1.0, -1.0);
    flip.concat();
    // Smaller than the frame, as the body goes on below where the circle would cut it off.
    kit::draw_character(avatar, kit::rect(SIDE * 0.1, SIDE * 0.08, SIDE * 0.8, SIDE * 0.8));
    NSGraphicsContext::restoreGraphicsState_class();
    save(&image, output)
}

/// The cards above the dock, 16 points in from a clear edge, so the picture sits on any background.
fn stack(dock: &NSBitmapImageRep, cards: &NSBitmapImageRep, output: &Path) -> bool {
    let (dock_size, cards_size) = (dock.size(), cards.size());
    let size = NSSize::new(dock_size.width + 32.0, cards_size.height + 8.0 + dock_size.height + 32.0);
    let Some(image) = canvas(size) else { return false };
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(NSGraphicsContext::graphicsContextWithBitmapImageRep(&image).as_deref());
    dock.drawInRect(kit::rect(16.0, 16.0, dock_size.width, dock_size.height));
    cards.drawInRect(kit::rect(16.0, 16.0 + dock_size.height + 8.0, cards_size.width, cards_size.height));
    NSGraphicsContext::restoreGraphicsState_class();
    save(&image, output)
}

/// A hidden panel to draw in: drawn in a window as the real widget is, never shown.
fn host(mtm: MainThreadMarker, view: &NSView) -> objc2::rc::Retained<objc2_app_kit::NSPanel> {
    let panel = make_panel(mtm, view.frame().size, "preview");
    panel.setColorSpace(Some(&NSColorSpace::sRGBColorSpace()));
    panel.setContentView(Some(view));
    panel
}

pub fn render(
    mtm: MainThreadMarker,
    state: &AppState,
    now: Timestamp,
    kind: PreviewKind,
    output: &Path,
    poster: std::sync::Arc<dyn errand_core::ports::Poster>,
    language: Language,
) -> bool {
    let context = ViewContext { now, zone: errand_core::format::local_zone(), computer: "Mac", language };
    let nothing: Rc<dyn Fn()> = Rc::new(|| {});
    match kind {
        PreviewKind::Settings => {
            let mut settings = super::settings::Settings::new(mtm, poster, language);
            settings.window.setColorSpace(Some(&NSColorSpace::sRGBColorSpace()));
            settings.render(&state.tasks);
            snapshot(&settings.view, output)
        }
        PreviewKind::TaskForm => {
            let mut form = super::task_form::TaskForm::new(mtm, poster, language);
            let _panel = host(mtm, &form.view);
            form.render(&state.tasks, mtm);
            form.point(true, 430.0);
            snapshot(&form.view, output)
        }
        PreviewKind::Character(avatar) => character(&avatar, output),
        PreviewKind::Simple { open } => {
            let mut simple = Simple::new(mtm, poster, language, CompactEvents::none());
            let height = simple.render(&view::simple(state, &context, open), mtm);
            // 16 points in from a clear edge, as the cards' picture is.
            let frame = FlippedView::new(mtm, kit::rect(0.0, 0.0, layout::STACK_WIDTH + 32.0, height + 32.0));
            simple.view.setFrameOrigin(NSPoint::new(16.0, 16.0));
            frame.addSubview(&simple.view);
            let _panel = host(mtm, &frame);
            snapshot(&frame, output)
        }
        PreviewKind::Badge { open } => {
            let mut character = Character::new(mtm, poster, language, CompactEvents::none());
            let line = open.then(|| view::simple(state, &context, false));
            let size = character.render(&view::badge(state, &context), line.as_ref(), true, mtm);
            // The circle 16 points in from a clear edge; its marks stand out past it.
            let inset = (layout::BADGE_SIZE - layout::BADGE_CIRCLE) / 2.0;
            let margin = 16.0 - inset;
            let frame = FlippedView::new(mtm, kit::rect(0.0, 0.0, size.width + margin * 2.0, size.height + margin * 2.0));
            character.view.setFrameOrigin(NSPoint::new(margin, margin));
            frame.addSubview(&character.view);
            let _panel = host(mtm, &frame);
            snapshot(&frame, output)
        }
        PreviewKind::Rooms => {
            let mut rooms = Rooms::new(mtm, poster);
            let _panel = host(mtm, &rooms.view);
            rooms.render(&view::rooms(state, &context), mtm);
            // Beside a dock in its default place, as the rooms open from it.
            let shape = view::dock(state, &context).shape();
            let screen = screen_frame(&_panel, mtm);
            let placement = layout::rooms_placement(layout::default_dock_frame(screen, shape), screen, &visible_frames(mtm), shape);
            rooms.point(placement.tail_on_right, placement.tail_y);
            snapshot(&rooms.view, output)
        }
        PreviewKind::Dashboard => {
            let mut journal = Journal::new(mtm, poster, language.window_title_journal());
            journal.window.setColorSpace(Some(&NSColorSpace::sRGBColorSpace()));
            journal.render(&view::journal(state, &context));
            journal.content().is_some_and(|content| snapshot(&content, output))
        }
        PreviewKind::Dock => {
            let events = DockEvents {
                remove_member: Rc::new(|_, _, _| {}),
                toggle_details: nothing.clone(),
                toggle_tray: nothing.clone(),
                refresh: nothing.clone(),
                update: nothing.clone(),
                edit_avatar: nothing.clone(),
                drag_began: nothing.clone(),
                dragged: Rc::new(|_, _| {}),
                expand_room: Rc::new(|_| {}),
                show_rooms: nothing.clone(),
                mode_menu: Rc::new(|| None),
            };
            let mut dock = Dock::new(mtm, events, language);
            let vm = view::dock(state, &context);
            dock.view.setFrameSize(NSSize::new(layout::STACK_WIDTH, layout::dock_height(vm.shape())));
            let _panel = host(mtm, &dock.view);
            dock.render(&vm, mtm);
            snapshot(&dock.view, output)
        }
        PreviewKind::Tray => {
            let events = DockEvents {
                remove_member: Rc::new(|_, _, _| {}),
                toggle_details: nothing.clone(),
                toggle_tray: nothing.clone(),
                refresh: nothing.clone(),
                update: nothing.clone(),
                edit_avatar: nothing.clone(),
                drag_began: nothing.clone(),
                dragged: Rc::new(|_, _| {}),
                expand_room: Rc::new(|_| {}),
                show_rooms: nothing.clone(),
                mode_menu: Rc::new(|| None),
            };
            let mut dock = Dock::new(mtm, events, language);
            let vm = view::dock(state, &context);
            dock.view.setFrameSize(NSSize::new(layout::STACK_WIDTH, layout::dock_height(vm.shape())));
            let _dock_panel = host(mtm, &dock.view);
            dock.render(&vm, mtm);
            let mut tray = Tray::new(mtm, poster, language);
            let _tray_panel = host(mtm, &tray.view);
            tray.update(&view::tray(state, &context, layout::TRAY_MAX_HEIGHT), mtm);
            match (capture(&dock.view), capture(&tray.view)) {
                (Some(dock), Some(cards)) => stack(&dock, &cards, output),
                _ => false,
            }
        }
        PreviewKind::Details => {
            let mut details = Details::new(mtm, poster, language);
            let panel = host(mtm, &details.view);
            details.render(&view::details(state, &context), mtm);
            // Beside a dock in its default place, as the details first open.
            let screen = screen_frame(&panel, mtm);
            let shape = DockShape { batteries: state.shown_providers().len(), ..DockShape::FULL };
            let dock = layout::default_dock_frame(screen, shape);
            let placement = layout::detail_placement(dock, screen, &visible_frames(mtm), shape);
            details.point(placement.tail_on_right, placement.tail_y);
            snapshot(&details.view, output)
        }
        PreviewKind::Nudge => {
            let mut nudge = super::nudge::Nudge::new(mtm, poster);
            let panel = host(mtm, &nudge.view);
            let Some(vm) = view::nudge(state, &context) else { return false };
            nudge.render(&vm);
            let screen = screen_frame(&panel, mtm);
            let shape = DockShape { batteries: state.shown_providers().len(), ..DockShape::FULL };
            let dock = layout::default_dock_frame(screen, shape);
            let placement = layout::nudge_placement(dock, screen, &visible_frames(mtm), shape, None);
            nudge.point(placement.tail_on_right, placement.tail_y);
            snapshot(&nudge.view, output)
        }
        PreviewKind::AvatarEditor => {
            let mut editor = AvatarEditor::new(mtm, poster, language);
            let panel = host(mtm, &editor.view);
            editor.render(&view::avatar_editor(state, language));
            let screen = screen_frame(&panel, mtm);
            let shape = DockShape { batteries: state.shown_providers().len(), ..DockShape::FULL };
            let dock = layout::default_dock_frame(screen, shape);
            let placement = layout::avatar_editor_placement(dock, screen, &visible_frames(mtm), shape);
            editor.point(placement.tail_on_right, placement.tail_y);
            snapshot(&editor.view, output)
        }
    }
}
