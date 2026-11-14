//! Draws parts of the widget to a PNG without showing a window, for checking the screens without real
//! accounts.

use super::avatar_editor::AvatarEditor;
use super::compact::{Character, CompactEvents, Simple};
use super::journal::Journal;
pub use super::journal::Pose;
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
use errand_core::model::{AppState, Msg, update};
use errand_core::ports::Poster;
use errand_core::view::{self, ViewContext};
use jiff::Timestamp;
use objc2::AnyThread;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSAffineTransformNSAppKitAdditions, NSBitmapImageFileType, NSBitmapImageRep, NSColorRenderingIntent, NSColorSpace, NSDeviceRGBColorSpace, NSEvent, NSEventModifierFlags, NSEventType,
    NSGraphicsContext, NSView,
};
use objc2_foundation::NSPoint;
use objc2_foundation::{NSAffineTransform, NSDate, NSDictionary, NSRunLoop, NSSize};
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PreviewKind {
    Settings,
    TaskForm,
    /// The account window.
    Dashboard,
    /// The same held at a moment a person only passes through.
    DashboardPose(Pose),
    /// The same pressed, dragged over and typed at as a person would, with a picture after each step.
    DashboardDrive,
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

/// What the window sends while it is driven, kept to be handled as the runtime would handle it.
#[derive(Default)]
struct Kept(Mutex<Vec<Msg>>);

impl Poster for Kept {
    fn post(&self, msg: Msg) {
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(msg);
    }
}

/// Drives the desktop window as a person would, with the app's own clock running between the steps: the keys
/// that open a day, step to the day before and close back to the week, a drag over a stretch of the map, the
/// replay, and a press on a task. Each step goes through the window as a real key or mouse event does. A
/// picture after each is saved beside `output`, numbered, and a line says how the step left the page. True
/// when every step left it as it should.
fn drive(mtm: MainThreadMarker, state: &AppState, context: &ViewContext, output: &Path, language: Language) -> bool {
    let kept = Arc::new(Kept::default());
    let mut state = state.clone();
    state.journal.place.tab = errand_core::journal::view::JournalTab::Week;
    let mut journal = Journal::new(mtm, kept.clone(), language.window_title_journal());
    journal.window.setColorSpace(Some(&NSColorSpace::sRGBColorSpace()));
    journal.render(&view::journal(&state, context));
    journal.focus();
    let stem = output.with_extension("");
    let (mut step, mut failed) = (0, 0);
    let wait = |seconds: f64| NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(seconds));
    let key = |journal: &Journal, code: u16, typed: &str| {
        let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            NSEventType::KeyDown, NSPoint::new(0.0, 0.0), NSEventModifierFlags::empty(), 0.0, journal.window.windowNumber(), None, &kit::ns(typed), &kit::ns(typed), false, code,
        );
        if let Some(event) = event {
            journal.window.sendEvent(&event);
        }
    };
    // A press goes to whatever the window finds under it, and the drag and the release that follow to the same.
    let pressed: std::cell::RefCell<Option<Retained<NSView>>> = std::cell::RefCell::new(None);
    let mouse = |journal: &Journal, kind: NSEventType, at: NSPoint| {
        let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
            kind, at, NSEventModifierFlags::empty(), 0.0, journal.window.windowNumber(), None, 0, 1, if kind == NSEventType::LeftMouseUp { 0.0 } else { 1.0 },
        );
        let Some(event) = event else { return };
        if kind == NSEventType::LeftMouseDown {
            let content = journal.content();
            let frame = content.as_ref().and_then(|content| unsafe { content.superview() });
            *pressed.borrow_mut() = content.zip(frame).and_then(|(content, frame)| content.hitTest(frame.convertPoint_fromView(at, None)));
        }
        let target = pressed.borrow().clone();
        match (target, kind) {
            (Some(target), NSEventType::LeftMouseDown) => target.mouseDown(&event),
            (Some(target), NSEventType::LeftMouseDragged) => target.mouseDragged(&event),
            (Some(target), _) => target.mouseUp(&event),
            (None, _) => {}
        }
    };
    // What the window sent is handled as the runtime would handle it, and the window drawn again.
    let settle = |journal: &mut Journal, state: &mut AppState| -> Vec<String> {
        let sent: Vec<Msg> = std::mem::take(&mut *kept.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
        let said = sent
            .iter()
            .map(|msg| match msg {
                Msg::JournalShow { tab, back } => format!("show {tab:?} {back} back"),
                Msg::JournalStep(step) => format!("step {step}"),
                Msg::JournalTab(tab) => format!("tab {tab:?}"),
                Msg::JournalOpen(key) => format!("open {}", key.as_deref().unwrap_or("nothing")),
                Msg::JournalRecap(card) => format!("recap {card:?}"),
                _ => "something else".to_owned(),
            })
            .collect();
        for msg in sent {
            update(state, msg, context.now);
        }
        journal.render(&view::journal(state, context));
        said
    };
    let mut check = |journal: &Journal, name: &str, pass: bool, said: &[String]| {
        step += 1;
        if !pass {
            failed += 1;
        }
        let saved = journal.content().is_some_and(|content| snapshot(&content, &stem.with_file_name(format!("{}-{step:02}-{name}.png", stem.file_name().and_then(|name| name.to_str()).unwrap_or("drive")))));
        // How long each part takes to draw: the map's is what every step of a motion costs.
        let times = journal.drawing_times().iter().map(|(part, time)| format!("{part} {time:.1}")).collect::<Vec<_>>().join(", ");
        println!("{} {step:02} {name}: {:?} sent {said:?}{}\n        drawing, ms: {times}", if pass { "ok  " } else { "FAIL" }, journal.standing(), if saved { "" } else { ", not saved" });
    };
    use errand_core::journal::view::JournalTab;
    let tab = |state: &AppState| (state.journal.place.tab, state.journal.place.back);

    // The first turns of the run loop are taken up with the app's own start.
    wait(0.3);
    check(&journal, "week", journal.standing().zoom.is_none(), &[]);
    // 1 opens the day the week's summary shows; the map is part of the way there a moment later.
    key(&journal, 18, "1");
    let said = settle(&mut journal, &mut state);
    wait(0.2);
    let opening = journal.standing().zoom;
    check(&journal, "opening", tab(&state).0 == JournalTab::Day && opening.is_some_and(|(open, _)| open > 0.02 && open < 0.98), &said);
    wait(0.5);
    check(&journal, "day", journal.standing().zoom.is_none(), &[]);
    // The left arrow goes to the day before; within the week the map slides there.
    let before = tab(&state).1;
    key(&journal, 123, "\u{f702}");
    let said = settle(&mut journal, &mut state);
    wait(0.2);
    let sliding = journal.standing().zoom;
    check(&journal, "sliding", tab(&state) == (JournalTab::Day, before + 1) && sliding.is_some_and(|(open, _)| open > 0.99), &said);
    wait(0.5);
    // 2 closes the day back to its week.
    key(&journal, 19, "2");
    let said = settle(&mut journal, &mut state);
    wait(0.2);
    let closing = journal.standing().zoom;
    check(&journal, "closing", tab(&state).0 == JournalTab::Week && closing.is_some_and(|(open, _)| open > 0.02 && open < 0.98), &said);
    wait(0.5);
    check(&journal, "week-again", journal.standing().zoom.is_none(), &[]);
    // A drag over the map picks that stretch of it; a press that does not move does not.
    let (from, to) = (journal.map_point(0.52), journal.map_point(0.86));
    mouse(&journal, NSEventType::LeftMouseDown, from);
    mouse(&journal, NSEventType::LeftMouseDragged, NSPoint::new(from.x + 2.0, from.y));
    let trembled = journal.standing().select.is_none();
    mouse(&journal, NSEventType::LeftMouseDragged, to);
    mouse(&journal, NSEventType::LeftMouseUp, to);
    let said = settle(&mut journal, &mut state);
    check(&journal, "dragged", trembled && journal.standing().select.is_some_and(|(left, right)| (right - left - (to.x - from.x)).abs() < 1.0) && said.is_empty(), &said);
    // Escape lets the stretch go.
    key(&journal, 53, "\u{1b}");
    check(&journal, "let-go", journal.standing().select.is_none(), &[]);
    // The space bar plays the map again; it runs on by the clock, and the space bar stops it.
    key(&journal, 49, " ");
    wait(0.1);
    let early = journal.standing().replay;
    wait(0.9);
    let later = journal.standing().replay;
    check(&journal, "replaying", early.zip(later).is_some_and(|(early, later)| later > early + 0.08 && later < 1.0), &[]);
    // It holds a moment at its end, and then the page is as it was, without a press.
    let until = |done: &dyn Fn(Option<f64>) -> bool| (0..100).any(|_| done(journal.standing().replay) || { wait(0.1); false });
    let held = until(&|replay| replay.is_none_or(|part| part >= 1.0)) && journal.standing().replay.is_some();
    check(&journal, "replay-held", held, &[]);
    check(&journal, "replay-over", until(&|replay| replay.is_none()), &[]);
    // Played again, the space bar stops it where it is.
    key(&journal, 49, " ");
    wait(0.3);
    let playing = journal.standing().replay.is_some();
    key(&journal, 49, " ");
    check(&journal, "stopped", playing && journal.standing().replay.is_none(), &[]);
    // A press on a task opens it once the button comes up.
    if let Some(at) = journal.longest_task() {
        mouse(&journal, NSEventType::LeftMouseDown, at);
        let waiting = kept.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).is_empty();
        mouse(&journal, NSEventType::LeftMouseUp, at);
        let said = settle(&mut journal, &mut state);
        check(&journal, "opened", waiting && state.journal.place.open.is_some(), &said);
        // Escape closes its panel.
        key(&journal, 53, "\u{1b}");
        let said = settle(&mut journal, &mut state);
        check(&journal, "closed", state.journal.place.open.is_none(), &said);
    }
    println!("{step} steps, {failed} failed");
    failed == 0
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
        PreviewKind::Dashboard | PreviewKind::DashboardPose(_) => {
            let mut journal = Journal::new(mtm, poster, language.window_title_journal());
            journal.window.setColorSpace(Some(&NSColorSpace::sRGBColorSpace()));
            journal.render(&view::journal(state, &context));
            if let PreviewKind::DashboardPose(pose) = kind {
                journal.pose(pose);
            }
            journal.content().is_some_and(|content| snapshot(&content, output))
        }
        PreviewKind::DashboardDrive => drive(mtm, state, &context, output, language),
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
