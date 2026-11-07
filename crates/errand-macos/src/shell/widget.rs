//! The floating widget: the dock in the bottom-right corner, the task cards stacked on it, and the usage
//! details or the character editor beside it. Its place survives relaunch and comes back into view when a
//! monitor goes away.

use super::avatar_editor::AvatarEditor;
use super::details::Details;
use super::rooms::Rooms;
use super::dock::{Dock, DockEvents};
use super::tray::Tray;
use super::kit::ns;
use super::panel::{TypingPanel, make_panel, make_typing_panel, primary_height, screen_frame, to_cocoa, to_core, visible_frames};
use errand_core::Language;
use errand_core::layout::{self, DockShape, Rect, STACK_WIDTH};
use errand_core::model::{AppState, Msg};
use errand_core::ports::Poster;
use errand_core::view::{self, ViewContext};
use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplicationDidChangeScreenParametersNotification, NSPanel};
use objc2_foundation::{NSArray, NSNotificationCenter, NSNumber, NSPoint, NSSize, NSUserDefaults};
use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::sync::Arc;

const POSITION_KEY: &str = "FloatingPetsPosition";

pub struct Widget {
    mtm: MainThreadMarker,
    pub panel: Retained<NSPanel>,
    pub dock: Dock,
    pub details_panel: Retained<NSPanel>,
    details: Details,
    pub avatar_editor_panel: Retained<NSPanel>,
    avatar_editor: AvatarEditor,
    pub tray_panel: Retained<NSPanel>,
    pub tray: Tray,
    pub rooms_panel: Retained<TypingPanel>,
    rooms: Rooms,
    poster: Arc<dyn Poster>,
    defaults: Retained<NSUserDefaults>,
    /// The dock's frame when a drag began, in core coordinates.
    drag_origin: Option<Rect>,
}

impl Widget {
    pub fn new(mtm: MainThreadMarker, poster: Arc<dyn Poster>, defaults: Retained<NSUserDefaults>, language: Language) -> Rc<RefCell<Widget>> {
        Rc::new_cyclic(|this: &Weak<RefCell<Widget>>| {
            let post = |msg: fn() -> Msg| {
                let poster = poster.clone();
                Rc::new(move || poster.post(msg())) as Rc<dyn Fn()>
            };
            let began = this.clone();
            let dragged = this.clone();
            let events = DockEvents {
                toggle_details: post(|| Msg::ToggleDetails),
                toggle_tray: post(|| Msg::ToggleTray),
                refresh: post(|| Msg::AskUsage),
                update: post(|| Msg::InstallUpdate),
                show_rooms: post(|| Msg::ToggleRooms),
                expand_room: {
                    let poster = poster.clone();
                    Rc::new(move |room: String| poster.post(Msg::ExpandRoom(room)))
                },
                remove_member: {
                    let poster = poster.clone();
                    Rc::new(move |room, member, name| poster.post(Msg::AskRemoveMember { room, member, name }))
                },
                drag_began: {
                    let poster = poster.clone();
                    Rc::new(move || {
                        if let Some(widget) = began.upgrade() {
                            let mut widget = widget.borrow_mut();
                            widget.drag_origin = Some(widget.dock_frame());
                        }
                        poster.post(Msg::CloseDetails);
                        poster.post(Msg::CloseAvatarEditor);
                        poster.post(Msg::CloseRooms);
                    })
                },
                dragged: {
                    let poster = poster.clone();
                    Rc::new(move |delta: NSPoint, done: bool| {
                        let Some(widget) = dragged.upgrade() else { return };
                        let mut widget = widget.borrow_mut();
                        let Some(start) = widget.drag_origin else { return };
                        // AppKit's y grows upward; the core's downward.
                        widget.move_to(Rect { x: start.x + delta.x, y: start.y - delta.y, ..start }, done);
                        if done {
                            widget.drag_origin = None;
                            poster.post(Msg::Redraw);
                        }
                    })
                },
                edit_avatar: post(|| Msg::ToggleAvatarEditor),
            };
            let dock = Dock::new(mtm, events, language);
            let panel = make_panel(mtm, NSSize::new(STACK_WIDTH, layout::dock_height(DockShape::FULL)), language.window_title_usage());
            panel.setContentView(Some(&dock.view));
            let details = Details::new(mtm, poster.clone(), language);
            let details_panel = make_panel(mtm, details.view.frame().size, language.details_title());
            details_panel.setContentView(Some(&details.view));
            let avatar_editor = AvatarEditor::new(mtm, poster.clone(), language);
            let avatar_editor_panel = make_typing_panel(mtm, avatar_editor.view.frame().size, language.avatar_editor_title());
            avatar_editor_panel.allow_typing();
            let avatar_editor_panel = Retained::into_super(avatar_editor_panel);
            avatar_editor_panel.setContentView(Some(&avatar_editor.view));
            let tray = Tray::new(mtm, poster.clone(), language);
            let tray_panel = make_panel(mtm, NSSize::new(STACK_WIDTH, 120.0), language.tray_window_title());
            tray_panel.setContentView(Some(&tray.view));
            let rooms = Rooms::new(mtm, poster.clone());
            let rooms_panel = make_typing_panel(mtm, rooms.view.frame().size, language.rooms_title());
            rooms_panel.setContentView(Some(&rooms.view));
            let mut widget = Widget {
                mtm,
                panel,
                dock,
                details_panel,
                details,
                avatar_editor_panel,
                avatar_editor,
                tray_panel,
                tray,
                rooms_panel,
                rooms,
                poster: poster.clone(),
                defaults,
                drag_origin: None,
            };
            let stored = widget.stored_origin();
            let frame = match stored {
                Some(origin) => to_core(super::kit::rect(origin.x, origin.y, STACK_WIDTH, layout::dock_height(DockShape::FULL)), primary_height(mtm)),
                None => layout::default_dock_frame(widget.main_screen(), DockShape::FULL),
            };
            widget.move_to(frame, false);
            let screens = this.clone();
            let redraw = poster.clone();
            let block = block2::RcBlock::new(move |_| {
                if let Some(widget) = screens.upgrade() {
                    let mut widget = widget.borrow_mut();
                    let frame = widget.dock_frame();
                    widget.move_to(frame, false);
                }
                redraw.post(Msg::Redraw);
            });
            unsafe {
                NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                    Some(NSApplicationDidChangeScreenParametersNotification),
                    None,
                    None,
                    &block,
                );
            }
            RefCell::new(widget)
        })
    }

    fn main_screen(&self) -> Rect {
        screen_frame(&self.panel, self.mtm)
    }

    /// The saved bottom-left corner, in AppKit coordinates as the Objective-C build stored it.
    fn stored_origin(&self) -> Option<NSPoint> {
        let stored = self.defaults.arrayForKey(&ns(POSITION_KEY))?;
        let numbers: Vec<f64> = stored.iter().filter_map(|value| value.downcast::<NSNumber>().ok()).map(|number| number.doubleValue()).collect();
        match numbers.as_slice() {
            [x, y] if x.is_finite() && y.is_finite() => Some(NSPoint::new(*x, *y)),
            _ => None,
        }
    }

    pub fn dock_frame(&self) -> Rect {
        to_core(self.panel.frame(), primary_height(self.mtm))
    }

    /// Moves the dock, kept on a connected screen; `persist` saves the place for the next launch.
    pub fn move_to(&mut self, frame: Rect, persist: bool) {
        let frame = layout::constrained_frame(frame, &visible_frames(self.mtm));
        let cocoa = to_cocoa(frame, primary_height(self.mtm));
        self.panel.setFrame_display(cocoa, true);
        if self.details_panel.isVisible() {
            self.place_details();
        }
        if self.avatar_editor_panel.isVisible() {
            self.place_avatar_editor();
        }
        if self.rooms_panel.isVisible() {
            self.place_rooms();
        }
        // While dragging the cards only follow; the room for them is measured again once the drag ends.
        if self.tray_panel.isVisible() {
            self.place_tray();
        }
        if persist {
            let origin = [NSNumber::new_f64(cocoa.origin.x), NSNumber::new_f64(cocoa.origin.y)];
            unsafe { self.defaults.setObject_forKey(Some(&NSArray::from_retained_slice(&origin)), &ns(POSITION_KEY)) };
        }
    }

    /// The details beside the dock, on the side with more room, the tail aimed at the batteries.
    fn place_details(&self) {
        let dock = self.dock_frame();
        let placement = layout::detail_placement(dock, self.main_screen(), &visible_frames(self.mtm), self.dock.shape());
        self.details_panel.setFrame_display(to_cocoa(placement.frame, primary_height(self.mtm)), true);
        self.details.point(placement.tail_on_right, placement.tail_y);
    }

    /// The character editor beside the dock, the tail aimed at the person's character.
    /// Puts the editor away; a nickname still being typed is kept as it stands, as if Return were pressed.
    fn close_avatar_editor(&self) {
        if self.avatar_editor_panel.isVisible() {
            self.avatar_editor_panel.makeFirstResponder(None);
            self.avatar_editor_panel.orderOut(None);
        }
    }

    fn place_avatar_editor(&self) {
        let placement = layout::avatar_editor_placement(self.dock_frame(), self.main_screen(), &visible_frames(self.mtm), self.dock.shape());
        self.avatar_editor_panel.setFrame_display(to_cocoa(placement.frame, primary_height(self.mtm)), true);
        self.avatar_editor.point(placement.tail_on_right, placement.tail_y);
    }

    /// The list of rooms beside the dock, the tail aimed at what opens it.
    fn place_rooms(&self) {
        let placement = layout::rooms_placement(self.dock_frame(), self.main_screen(), &visible_frames(self.mtm), self.dock.shape());
        self.rooms_panel.setFrame_display(to_cocoa(placement.frame, primary_height(self.mtm)), true);
        self.rooms.point(placement.tail_on_right, placement.tail_y);
    }

    fn place_tray(&self) {
        let height = self.tray_panel.frame().size.height;
        let frame = layout::tray_frame(self.dock_frame(), self.main_screen(), height);
        self.tray_panel.setFrame_display(to_cocoa(frame, primary_height(self.mtm)), true);
    }

    pub fn fade_card(&self, key: &str) {
        self.tray.fade(key, self.tray_panel.isVisible(), self.poster.clone());
    }

    pub fn reset_position(&mut self) {
        let frame = layout::default_dock_frame(self.main_screen(), self.dock.shape());
        self.move_to(frame, true);
    }

    pub fn render(&mut self, state: &AppState, context: &ViewContext) {
        let dock = view::dock(state, context);
        let shape = dock.shape();
        self.dock.render(&dock, self.mtm);
        // The dock keeps its bottom edge, growing upward when the task header appears or a room opens, and
        // shrinking when a service is hidden.
        let frame = self.dock_frame();
        let height = layout::dock_height(shape);
        if frame.height != height {
            self.move_to(Rect { y: frame.bottom() - height, height, ..frame }, false);
        }
        if state.widget.hidden {
            self.panel.orderOut(None);
            self.details_panel.orderOut(None);
            self.close_avatar_editor();
            self.tray_panel.orderOut(None);
            self.rooms_panel.orderOut(None);
            return;
        }
        if !self.panel.isVisible() {
            self.panel.orderFrontRegardless();
        }
        self.panel.invalidateShadow();
        if state.details_open {
            self.details.render(&view::details(state, context), self.mtm);
            self.place_details();
            if !self.details_panel.isVisible() {
                self.details_panel.orderFrontRegardless();
            }
        } else {
            self.details_panel.orderOut(None);
        }
        if state.avatar_editor_open {
            self.avatar_editor.render(&view::avatar_editor(state, context.language));
            self.place_avatar_editor();
            if !self.avatar_editor_panel.isVisible() {
                self.avatar_editor_panel.orderFrontRegardless();
            }
        } else {
            self.close_avatar_editor();
        }
        if state.rooms.open {
            self.rooms.render(&view::rooms(state, context), self.mtm);
            self.place_rooms();
            if !self.rooms_panel.isVisible() {
                self.rooms_panel.orderFrontRegardless();
            }
            // A form just shown takes typing in its first field; the list gives typing back.
            if let Some(focus) = self.rooms.take_focus() {
                self.rooms_panel.set_typing(Some(&focus));
            } else if !self.rooms.typing() {
                self.rooms_panel.set_typing(None);
            }
        } else {
            self.rooms_panel.set_typing(None);
            self.rooms_panel.orderOut(None);
        }
        if state.tray_visible() {
            let max = layout::tray_max_height(self.dock_frame(), self.main_screen());
            let height = self.tray.update(&view::tray(state, context, max), self.mtm);
            self.tray_panel.setContentSize(NSSize::new(STACK_WIDTH, height));
            self.place_tray();
            self.tray_panel.invalidateShadow();
            if !self.tray_panel.isVisible() {
                self.tray_panel.orderFrontRegardless();
            }
        } else {
            self.tray_panel.orderOut(None);
        }
    }
}
