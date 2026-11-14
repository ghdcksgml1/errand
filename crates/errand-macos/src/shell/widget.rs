//! The floating widget: the dock in the bottom-right corner, the task cards stacked on it, and the usage
//! details or the character editor beside it. Its place survives relaunch and comes back into view when a
//! monitor goes away. On a small screen it can be one line instead, or the person's character alone; it keeps
//! the side of the screen it sits nearer, so changing size never moves it away from its corner.

use super::avatar_editor::AvatarEditor;
use super::compact::{self, Character, CompactEvents, Simple};
use super::details::Details;
use super::dock::{Dock, DockEvents};
use super::kit::{self, Action, ns};
use super::panel::{TypingPanel, make_panel, make_typing_panel, primary_height, screen_frame, to_cocoa, to_core, visible_frames};
use super::rooms::Rooms;
use super::task_drop::DropView;
use super::task_form::{self, TaskForm};
use super::tray::Tray;
use dispatch2::{DispatchQueue, DispatchTime};
use errand_core::Language;
use errand_core::layout::{self, BADGE_SIZE, DockShape, Rect, STACK_WIDTH, STRIP_HEIGHT};
use errand_core::model::{AppState, Msg, WidgetMode};
use errand_core::ports::Poster;
use errand_core::view::{self, MenuAction, MenuItemVm, ViewContext};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSApplicationDidChangeScreenParametersNotification, NSEvent, NSMenu, NSPanel, NSView};
use objc2_foundation::{NSArray, NSNotificationCenter, NSNumber, NSPoint, NSSize, NSUserDefaults};
use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::Duration;

const POSITION_KEY: &str = "FloatingPetsPosition";
/// The widget keeps its right edge where it changes size; false, its left.
const ANCHOR_KEY: &str = "WidgetAnchoredRight";

pub struct Widget {
    mtm: MainThreadMarker,
    pub panel: Retained<NSPanel>,
    pub dock: Dock,
    drop_view: Retained<DropView>,
    pub task_panel: Retained<TypingPanel>,
    task_form: TaskForm,
    /// The one-line widget, and the character-only one that opens into it.
    simple: Simple,
    character: Character,
    /// What the panel holds, as last drawn.
    mode: WidgetMode,
    /// The edge kept when the widget changes size: the side of the screen it sits nearer.
    anchor_right: bool,
    /// The widget's right-click menu, as the last render had it, and the targets of the menu on show.
    mode_items: Rc<RefCell<Vec<MenuItemVm>>>,
    _menu_actions: Rc<RefCell<Vec<Retained<Action>>>>,
    pub details_panel: Retained<NSPanel>,
    details: Details,
    pub avatar_editor_panel: Retained<NSPanel>,
    avatar_editor: AvatarEditor,
    pub tray_panel: Retained<NSPanel>,
    pub tray: Tray,
    pub rooms_panel: Retained<TypingPanel>,
    rooms: Rooms,
    /// The pet's morning bubble about the last day's work.
    pub nudge_panel: Retained<NSPanel>,
    nudge: super::nudge::Nudge,
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
            let drag_began: Rc<dyn Fn()> = {
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
            };
            let drag: Rc<dyn Fn(NSPoint, bool)> = {
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
            };
            let mode_items: Rc<RefCell<Vec<MenuItemVm>>> = Rc::new(RefCell::new(Vec::new()));
            let menu_actions: Rc<RefCell<Vec<Retained<Action>>>> = Rc::new(RefCell::new(Vec::new()));
            let mode_menu: Rc<dyn Fn() -> Option<Retained<NSMenu>>> = {
                let (items, actions, poster) = (mode_items.clone(), menu_actions.clone(), poster.clone());
                let on_action: Rc<dyn Fn(MenuAction)> = Rc::new(move |action| {
                    if let MenuAction::SetWidgetMode(mode) = action {
                        poster.post(Msg::SetWidgetMode(mode));
                    }
                });
                Rc::new(move || Some(compact::mode_menu(&items.borrow(), &on_action, &actions, mtm)))
            };
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
                drag_began: drag_began.clone(),
                dragged: drag.clone(),
                edit_avatar: post(|| Msg::ToggleAvatarEditor),
                mode_menu: mode_menu.clone(),
            };
            let compact_events = CompactEvents {
                toggle_list: post(|| Msg::ToggleLines),
                toggle_details: post(|| Msg::ToggleDetails),
                edit_avatar: post(|| Msg::ToggleAvatarEditor),
                drag_began,
                dragged: drag,
                menu: mode_menu,
                hover: {
                    let (poster, widget) = (poster.clone(), this.clone());
                    Rc::new(move |on: bool| {
                        if on {
                            poster.post(Msg::PointerOnBadge(true));
                            return;
                        }
                        // A moment's grace: the pointer may only be crossing a gap, or the widget opening under it.
                        let Some(frame) = widget.upgrade().and_then(|widget| widget.try_borrow().ok().map(|widget| widget.panel.frame())) else { return };
                        let poster = poster.clone();
                        let when = DispatchTime::try_from(Duration::from_millis(350)).unwrap_or(DispatchTime::NOW);
                        let _ = DispatchQueue::main().after(when, move || {
                            if !kit::contains(frame, NSEvent::mouseLocation()) {
                                poster.post(Msg::PointerOnBadge(false));
                            }
                        });
                    })
                },
            };
            let dock = Dock::new(mtm, events, language);
            let simple = Simple::new(mtm, poster.clone(), language, compact_events.clone());
            let character = Character::new(mtm, poster.clone(), language, compact_events);
            let panel = make_panel(mtm, NSSize::new(STACK_WIDTH, layout::dock_height(DockShape::FULL)), language.window_title_usage());
            let drop_view = DropView::new(mtm, poster.clone(), &dock.view, language);
            panel.setContentView(Some(&drop_view));
            let task_form = TaskForm::new(mtm, poster.clone(), language);
            let task_panel = make_typing_panel(
                mtm,
                task_form.view.frame().size,
                errand_core::tasks::words(language, "작업 등록", "Add a task"),
            );
            task_panel.setContentView(Some(&task_form.view));
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
            let nudge = super::nudge::Nudge::new(mtm, poster.clone());
            let nudge_panel = make_panel(mtm, nudge.view.frame().size, if language == Language::Korean { "어제 한 일" } else { "Yesterday's work" });
            nudge_panel.setContentView(Some(&nudge.view));
            let mut widget = Widget {
                mtm,
                panel,
                dock,
                drop_view,
                task_panel,
                task_form,
                simple,
                character,
                mode: WidgetMode::Full,
                anchor_right: true,
                mode_items,
                _menu_actions: menu_actions,
                details_panel,
                details,
                avatar_editor_panel,
                avatar_editor,
                tray_panel,
                tray,
                rooms_panel,
                rooms,
                nudge_panel,
                nudge,
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
            widget.anchor_right = widget.stored_anchor().unwrap_or_else(|| widget.dock_frame().mid_x() > widget.main_screen().mid_x());
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

    /// The saved bottom-left corner, in AppKit coordinates as the Objective-C build stored it. It is the full dock's
    /// corner whatever size the widget is, so an older Errand puts the dock where this one left it.
    fn stored_origin(&self) -> Option<NSPoint> {
        let stored = self.defaults.arrayForKey(&ns(POSITION_KEY))?;
        let numbers: Vec<f64> = stored.iter().filter_map(|value| value.downcast::<NSNumber>().ok()).map(|number| number.doubleValue()).collect();
        match numbers.as_slice() {
            [x, y] if x.is_finite() && y.is_finite() => Some(NSPoint::new(*x, *y)),
            _ => None,
        }
    }

    fn stored_anchor(&self) -> Option<bool> {
        let stored = self.defaults.objectForKey(&ns(ANCHOR_KEY))?;
        Some(stored.downcast::<NSNumber>().ok()?.boolValue())
    }

    pub fn dock_frame(&self) -> Rect {
        to_core(self.panel.frame(), primary_height(self.mtm))
    }

    /// Where the widget sits across the screen: the dock's middle, or the character's, which keeps its own side.
    fn sits_right(&self, frame: Rect, screen: Rect) -> bool {
        let middle = match self.mode {
            WidgetMode::Character if self.anchor_right => frame.right() - BADGE_SIZE / 2.0,
            WidgetMode::Character => frame.x + BADGE_SIZE / 2.0,
            _ => frame.mid_x(),
        };
        middle > screen.mid_x()
    }

    /// Moves the dock, kept on a connected screen; `persist` saves the place for the next launch.
    pub fn move_to(&mut self, frame: Rect, persist: bool) {
        let mut frame = layout::constrained_frame(frame, &visible_frames(self.mtm));
        let primary = primary_height(self.mtm);
        self.panel.setFrame_display(to_cocoa(frame, primary), true);
        if persist {
            // Let go across the middle of the screen, the widget keeps the other edge from now on. The character
            // stays where it was let go; the simple widget open over it swaps sides around it.
            let right = self.sits_right(frame, self.main_screen());
            if right != self.anchor_right {
                if self.mode == WidgetMode::Character {
                    let circle = if self.anchor_right { frame.right() - BADGE_SIZE } else { frame.x };
                    frame.x = if right { circle + BADGE_SIZE - frame.width } else { circle };
                    frame = layout::constrained_frame(frame, &visible_frames(self.mtm));
                    self.panel.setFrame_display(to_cocoa(frame, primary), true);
                }
                self.anchor_right = right;
            }
        }
        if self.details_panel.isVisible() {
            self.place_details();
        }
        if self.nudge_panel.isVisible() {
            self.place_nudge();
        }
        if self.avatar_editor_panel.isVisible() {
            self.place_avatar_editor();
        }
        if self.task_panel.isVisible() {
            self.place_tasks();
        }
        if self.rooms_panel.isVisible() {
            self.place_rooms();
        }
        // While dragging the cards only follow; the room for them is measured again once the drag ends.
        if self.tray_panel.isVisible() {
            self.place_tray();
        }
        if persist {
            let cocoa = to_cocoa(frame, primary);
            let x = if self.anchor_right { cocoa.origin.x + cocoa.size.width - STACK_WIDTH } else { cocoa.origin.x };
            let origin = [NSNumber::new_f64(x), NSNumber::new_f64(cocoa.origin.y)];
            unsafe {
                self.defaults.setObject_forKey(Some(&NSArray::from_retained_slice(&origin)), &ns(POSITION_KEY));
                self.defaults.setObject_forKey(Some(&NSNumber::new_bool(self.anchor_right)), &ns(ANCHOR_KEY));
            }
        }
    }

    /// How far down the smaller widgets a bubble's tail points: at their line, or at the character.
    fn compact_aim(&self) -> Option<f64> {
        let height = self.dock_frame().height;
        match self.mode {
            WidgetMode::Full => None,
            WidgetMode::Simple => Some(height - STRIP_HEIGHT / 2.0),
            WidgetMode::Character => Some(height - BADGE_SIZE / 2.0),
        }
    }

    /// The details beside the dock, on the side with more room, the tail aimed at the batteries.
    fn place_details(&self) {
        let (dock, screen, visible, shape) = (self.dock_frame(), self.main_screen(), visible_frames(self.mtm), self.dock.shape());
        let placement = match self.compact_aim() {
            None => layout::detail_placement(dock, screen, &visible, shape),
            Some(aim) => layout::popover_placement(dock, screen, &visible, layout::detail_body_width(shape.batteries) + layout::POPOVER_TAIL, layout::DETAIL_HEIGHT, aim),
        };
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

    /// The morning bubble beside the widget, the tail at the character.
    fn place_nudge(&self) {
        let (dock, screen, visible) = (self.dock_frame(), self.main_screen(), visible_frames(self.mtm));
        let placement = layout::nudge_placement(dock, screen, &visible, self.dock.shape(), self.compact_aim());
        self.nudge_panel.setFrame_display(to_cocoa(placement.frame, primary_height(self.mtm)), true);
        self.nudge.point(placement.tail_on_right, placement.tail_y);
    }

    fn place_avatar_editor(&self) {
        let (dock, screen, visible) = (self.dock_frame(), self.main_screen(), visible_frames(self.mtm));
        let placement = match self.compact_aim() {
            None => layout::avatar_editor_placement(dock, screen, &visible, self.dock.shape()),
            Some(aim) => layout::popover_placement(dock, screen, &visible, layout::AVATAR_EDITOR_WIDTH + layout::POPOVER_TAIL, layout::AVATAR_EDITOR_HEIGHT, aim),
        };
        self.avatar_editor_panel.setFrame_display(to_cocoa(placement.frame, primary_height(self.mtm)), true);
        self.avatar_editor.point(placement.tail_on_right, placement.tail_y);
    }

    /// The list of rooms beside the dock, the tail aimed at what opens it.
    fn place_rooms(&self) {
        let (dock, screen, visible) = (self.dock_frame(), self.main_screen(), visible_frames(self.mtm));
        let placement = match self.compact_aim() {
            None => layout::rooms_placement(dock, screen, &visible, self.dock.shape()),
            Some(aim) => layout::popover_placement(dock, screen, &visible, layout::ROOMS_WIDTH + layout::POPOVER_TAIL, layout::ROOMS_HEIGHT, aim),
        };
        self.rooms_panel.setFrame_display(to_cocoa(placement.frame, primary_height(self.mtm)), true);
        self.rooms.point(placement.tail_on_right, placement.tail_y);
    }

    fn place_tasks(&self) {
        let dock = self.dock_frame();
        let placement = layout::popover_placement(
            dock,
            self.main_screen(),
            &visible_frames(self.mtm),
            task_form::WIDTH + layout::POPOVER_TAIL,
            task_form::HEIGHT,
            self.compact_aim().unwrap_or(dock.height / 2.0),
        );
        self.task_panel
            .setFrame_display(to_cocoa(placement.frame, primary_height(self.mtm)), true);
        self.task_form
            .point(placement.tail_on_right, placement.tail_y);
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
        let full = layout::default_dock_frame(self.main_screen(), self.dock.shape());
        let frame = self.dock_frame();
        self.anchor_right = true;
        self.move_to(Rect { x: full.right() - frame.width, y: full.bottom() - frame.height, ..frame }, true);
    }

    /// The panel holds the dock, the line or the character.
    fn hold(&mut self, mode: WidgetMode) {
        if self.mode == mode {
            return;
        }
        let view: &NSView = match mode {
            WidgetMode::Full => &self.dock.view,
            WidgetMode::Simple => &self.simple.view,
            WidgetMode::Character => &self.character.view,
        };
        self.drop_view.hold(view);
        self.mode = mode;
    }

    /// How much of the screen above the widget's foot the one-line cards may take.
    fn room_for_lines(&self, under: f64) -> f64 {
        let (frame, screen) = (self.dock_frame(), self.main_screen());
        (frame.bottom() - screen.y - layout::STACK_GAP - under).min(layout::TRAY_MAX_HEIGHT)
    }

    pub fn render(&mut self, state: &AppState, context: &ViewContext) {
        let dock = view::dock(state, context);
        let shape = dock.shape();
        // The dock is drawn whatever shows, so the bubbles beside the smaller widgets still know its batteries.
        self.dock.render(&dock, self.mtm);
        *self.mode_items.borrow_mut() = view::widget_modes(state, context.language);
        self.hold(state.widget.mode);
        let size = match state.widget.mode {
            WidgetMode::Full => NSSize::new(STACK_WIDTH, layout::dock_height(shape)),
            WidgetMode::Simple => {
                let mut line = view::simple(state, context, state.lines_open);
                line.lines.truncate(layout::lines_that_fit(self.room_for_lines(0.0)));
                NSSize::new(STACK_WIDTH, self.simple.render(&line, self.mtm))
            }
            WidgetMode::Character => {
                let line = state.badge_open.then(|| {
                    let mut line = view::simple(state, context, state.lines_open);
                    line.lines.truncate(layout::lines_that_fit(self.room_for_lines(BADGE_SIZE + layout::STACK_GAP)));
                    line
                });
                self.character.render(&view::badge(state, context), line.as_ref(), self.anchor_right, self.mtm)
            }
        };
        // The widget keeps its bottom edge and its side's, growing upward when the task header appears, a room
        // opens or a line comes, and shrinking when a service is hidden.
        let frame = self.dock_frame();
        if frame.width != size.width || frame.height != size.height {
            let x = if self.anchor_right { frame.right() - size.width } else { frame.x };
            self.move_to(Rect { x, y: frame.bottom() - size.height, width: size.width, height: size.height }, false);
        }
        if state.widget.hidden {
            self.panel.orderOut(None);
            self.details_panel.orderOut(None);
            self.close_avatar_editor();
            self.tray_panel.orderOut(None);
            self.rooms_panel.orderOut(None);
            self.nudge_panel.orderOut(None);
            self.task_panel.set_typing(None);
            self.task_panel.orderOut(None);
            return;
        }
        if !self.panel.isVisible() {
            self.panel.orderFrontRegardless();
        }
        self.panel.invalidateShadow();
        if state.details_open && !state.tasks.open() {
            self.details.render(&view::details(state, context), self.mtm);
            self.place_details();
            if !self.details_panel.isVisible() {
                self.details_panel.orderFrontRegardless();
            }
        } else {
            self.details_panel.orderOut(None);
        }
        if state.avatar_editor_open && !state.tasks.open() {
            self.avatar_editor.render(&view::avatar_editor(state, context.language));
            self.place_avatar_editor();
            if !self.avatar_editor_panel.isVisible() {
                self.avatar_editor_panel.orderFrontRegardless();
            }
        } else {
            self.close_avatar_editor();
        }
        if state.rooms.open && !state.tasks.open() {
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
        if state.tasks.open() {
            let focus = self.task_form.render(&state.tasks, self.mtm);
            self.place_tasks();
            if !self.task_panel.isVisible() {
                self.task_panel.orderFrontRegardless();
            }
            self.task_panel.allow_typing();
            if focus {
                self.task_panel.set_typing(Some(self.task_form.focus()));
            }
        } else {
            self.task_form.render(&state.tasks, self.mtm);
            self.task_panel.set_typing(None);
            self.task_panel.orderOut(None);
        }
        // The morning bubble gives way to anything else open beside the widget.
        let busy = state.tasks.open() || state.details_open || state.avatar_editor_open || state.rooms.open;
        match view::nudge(state, context).filter(|_| !busy) {
            Some(vm) => {
                self.nudge.render(&vm);
                self.place_nudge();
                if !self.nudge_panel.isVisible() {
                    self.nudge_panel.orderFrontRegardless();
                }
            }
            None => self.nudge_panel.orderOut(None),
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
