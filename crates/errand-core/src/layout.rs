//! Sizes and placement of the floating widget. Points, with the origin at the top left and y growing
//! downward; the macOS shell converts from AppKit's bottom-left origin.

use crate::Provider;

pub const STACK_WIDTH: f64 = 304.0;
/// Between the dock and the cards above or below it.
pub const STACK_GAP: f64 = 8.0;
pub const DOCK_PADDING: f64 = 10.0;
pub const HEADER_HEIGHT: f64 = 38.0;
pub const REFRESH_SIZE: f64 = 24.0;
pub const BATTERY_HEIGHT: f64 = 30.0;
pub const BATTERY_GAP: f64 = 6.0;
pub const PROFILE_HEIGHT: f64 = 44.0;
pub const AVATAR_SIZE: f64 = 24.0;
pub const DETAIL_CARD_WIDTH: f64 = 248.0;
pub const DETAIL_CARD_GAP: f64 = 8.0;
pub const DETAIL_HEIGHT: f64 = 356.0;
pub const POPOVER_TAIL: f64 = 12.0;
/// The character editor's body, without its tail.
pub const AVATAR_EDITOR_WIDTH: f64 = 300.0;
pub const AVATAR_EDITOR_HEIGHT: f64 = 348.0;
/// About four cards plus a peek of the next; the rest scroll instead of covering the desktop.
pub const TRAY_MAX_HEIGHT: f64 = 560.0;
pub const CARD_HEIGHT: f64 = 118.0;
/// Added when a finished card shows what to do next.
pub const ACTION_HEIGHT: f64 = 18.0;
pub const COMPACT_HEIGHT: f64 = 62.0;
/// A sub-task's line, inside the card it came from.
pub const SUB_TASK_HEIGHT: f64 = 32.0;
/// Under a card's last sub-task.
pub const SUB_TASKS_FOOT: f64 = 4.0;
pub const CARD_GAP: f64 = 8.0;
pub const MORE_BAR_HEIGHT: f64 = 30.0;
/// From the default position, how far the dock sits from the screen's right and bottom edges.
pub const SCREEN_MARGIN: f64 = 24.0;
/// The simple widget's own line: the counts and a small battery per service.
pub const STRIP_HEIGHT: f64 = 40.0;
/// A task's one-line card over the simple widget's line, and the space between two of them.
pub const LINE_HEIGHT: f64 = 36.0;
pub const LINE_GAP: f64 = 6.0;
/// Between the last one-line card and the line under it.
pub const LINES_FOOT: f64 = 8.0;
/// The character-only widget: its circle, with room around it for the badges on its edge.
pub const BADGE_SIZE: f64 = 64.0;
pub const BADGE_CIRCLE: f64 = 56.0;

/// How many one-line cards fit over the simple widget's line in `height`; the counts still count the rest.
pub fn lines_that_fit(height: f64) -> usize {
    ((height - STRIP_HEIGHT - LINES_FOOT + LINE_GAP) / (LINE_HEIGHT + LINE_GAP)).floor().max(0.0) as usize
}

/// The simple widget with this many one-line cards over its line.
pub fn simple_height(lines: usize) -> f64 {
    match lines {
        0 => STRIP_HEIGHT,
        count => count as f64 * LINE_HEIGHT + (count - 1) as f64 * LINE_GAP + LINES_FOOT + STRIP_HEIGHT,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect { x, y, width, height }
    }
    pub fn right(&self) -> f64 {
        self.x + self.width
    }
    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }
    pub fn mid_x(&self) -> f64 {
        self.x + self.width / 2.0
    }
    pub fn contains(&self, other: &Rect) -> bool {
        other.x >= self.x && other.y >= self.y && other.right() <= self.right() && other.bottom() <= self.bottom()
    }
    fn intersection_area(&self, other: &Rect) -> f64 {
        let width = self.right().min(other.right()) - self.x.max(other.x);
        let height = self.bottom().min(other.bottom()) - self.y.max(other.y);
        if width > 0.0 && height > 0.0 { width * height } else { 0.0 }
    }
}

/// What the dock holds: the task header while there are tasks, a battery per service shown, and under the
/// profile a tab per room the person is in, with thin rows for the open room's people.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DockShape {
    pub header: bool,
    pub batteries: usize,
    /// The person is in a room, so the room tabs show.
    pub tabs: bool,
    /// Rows under the tabs: the open room's people, or a word in their place.
    pub rows: usize,
}

impl DockShape {
    /// Every service's battery, without the header or rooms.
    pub const FULL: DockShape = DockShape { header: false, batteries: Provider::ALL.len(), tabs: false, rows: 0 };
}

pub const ROOM_TAB_HEIGHT: f64 = 22.0;
/// Between the avatar on the profile row and the room tabs under it.
pub const TABS_GAP: f64 = 12.0;
pub const MEMBER_ROW_HEIGHT: f64 = 26.0;
/// The most rows a room takes on the dock; past them, the last counts everyone else.
pub const MEMBER_ROWS: usize = 6;
/// Around the rows, inside their panel.
pub const MEMBERS_INSET: f64 = 4.0;

pub fn battery_top(shape: DockShape) -> f64 {
    if shape.header { HEADER_HEIGHT + 8.0 } else { DOCK_PADDING }
}

pub fn battery_bottom(shape: DockShape) -> f64 {
    let count = shape.batteries.max(1) as f64;
    battery_top(shape) + count * BATTERY_HEIGHT + (count - 1.0) * BATTERY_GAP
}

/// The profile row closes the dock under the batteries.
pub fn profile_top(shape: DockShape) -> f64 {
    battery_bottom(shape) + DOCK_PADDING
}

/// The room tabs, under the profile row's avatar.
pub fn tabs_top(shape: DockShape) -> f64 {
    profile_top(shape) + (PROFILE_HEIGHT + AVATAR_SIZE) / 2.0 + TABS_GAP
}

/// The open room's people, on a panel under the tabs.
pub fn members_top(shape: DockShape) -> f64 {
    tabs_top(shape) + ROOM_TAB_HEIGHT + 6.0
}

pub fn members_height(rows: usize) -> f64 {
    rows as f64 * MEMBER_ROW_HEIGHT + 2.0 * MEMBERS_INSET
}

/// The dock is taller by the task header while there are tasks, by each battery shown, and by the rooms.
pub fn dock_height(shape: DockShape) -> f64 {
    match (shape.tabs, shape.rows) {
        (false, _) => profile_top(shape) + PROFILE_HEIGHT,
        (true, 0) => tabs_top(shape) + ROOM_TAB_HEIGHT + DOCK_PADDING,
        (true, rows) => members_top(shape) + members_height(rows) + DOCK_PADDING,
    }
}

/// The usage details popover's body, one card per service shown, without its tail.
pub fn detail_body_width(cards: usize) -> f64 {
    let count = cards.max(1) as f64;
    24.0 + count * DETAIL_CARD_WIDTH + (count - 1.0) * DETAIL_CARD_GAP
}

/// The dock's frame in the bottom-right corner of `screen`.
pub fn default_dock_frame(screen: Rect, shape: DockShape) -> Rect {
    let height = dock_height(shape);
    Rect::new(screen.right() - STACK_WIDTH - SCREEN_MARGIN, screen.bottom() - SCREEN_MARGIN - height, STACK_WIDTH, height)
}

/// Moves `frame` inside the visible screen it overlaps most, or the first screen when it overlaps none,
/// so a position from a disconnected monitor comes back into view.
pub fn constrained_frame(frame: Rect, visible: &[Rect]) -> Rect {
    let Some(first) = visible.first() else { return frame };
    let mut selected = *first;
    let mut best = 0.0;
    for screen in visible {
        let area = frame.intersection_area(screen);
        if area > best {
            selected = *screen;
            best = area;
        }
    }
    Rect {
        x: selected.x.max(frame.x.min(selected.right() - frame.width)),
        y: selected.y.max(frame.y.min(selected.bottom() - frame.height)),
        ..frame
    }
}

/// The cards stack on top of the dock, or hang below it when the dock is parked near the top of the screen.
pub fn cards_go_above(dock: Rect, screen: Rect) -> bool {
    let above = dock.y - screen.y - STACK_GAP;
    let below = screen.bottom() - dock.bottom() - STACK_GAP;
    above >= 240.0 || above >= below
}

/// The most height the cards may take on the side they go.
pub fn tray_max_height(dock: Rect, screen: Rect) -> f64 {
    let room = if cards_go_above(dock, screen) { dock.y - screen.y - STACK_GAP * 2.0 } else { screen.bottom() - dock.bottom() - STACK_GAP * 2.0 };
    TRAY_MAX_HEIGHT.min(room.max(118.0))
}

pub fn tray_frame(dock: Rect, screen: Rect, height: f64) -> Rect {
    let y = if cards_go_above(dock, screen) { dock.y - STACK_GAP - height } else { dock.bottom() + STACK_GAP };
    Rect::new(dock.x, y, STACK_WIDTH, height)
}

/// The usage details beside the dock, on the side with more room.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DetailPlacement {
    pub frame: Rect,
    /// The tail points right, at a dock to the details' right.
    pub tail_on_right: bool,
    /// The tail's height in the details, level with the middle of the batteries.
    pub tail_y: f64,
}

/// A bubble this size beside the dock, on the side with more room, its bottom level with the dock's and its
/// tail aimed `aim` down the dock.
pub fn popover_placement(dock: Rect, screen: Rect, visible: &[Rect], width: f64, height: f64, aim: f64) -> DetailPlacement {
    let to_left = dock.mid_x() > screen.mid_x();
    let x = if to_left { dock.x - 4.0 - width } else { dock.right() + 4.0 };
    let frame = constrained_frame(Rect::new(x, dock.bottom() - height, width, height), visible);
    DetailPlacement { frame, tail_on_right: to_left, tail_y: dock.y + aim - frame.y }
}

/// The usage details, their tail at the middle of the batteries.
pub fn detail_placement(dock: Rect, screen: Rect, visible: &[Rect], shape: DockShape) -> DetailPlacement {
    let aim = (battery_top(shape) + battery_bottom(shape)) / 2.0;
    popover_placement(dock, screen, visible, detail_body_width(shape.batteries) + POPOVER_TAIL, DETAIL_HEIGHT, aim)
}

/// The work rooms' bubble, without its tail.
pub const ROOMS_WIDTH: f64 = 340.0;
pub const ROOMS_HEIGHT: f64 = 500.0;

/// The list of work rooms, its tail at what opens it: the tabs' row in a room, the profile row outside one.
pub fn rooms_placement(dock: Rect, screen: Rect, visible: &[Rect], shape: DockShape) -> DetailPlacement {
    let aim = if shape.tabs { tabs_top(shape) + ROOM_TAB_HEIGHT / 2.0 } else { profile_top(shape) + PROFILE_HEIGHT / 2.0 };
    popover_placement(dock, screen, visible, ROOMS_WIDTH + POPOVER_TAIL, ROOMS_HEIGHT, aim)
}

/// The pet's morning bubble, without its tail.
pub const NUDGE_WIDTH: f64 = 316.0;
pub const NUDGE_HEIGHT: f64 = 118.0;

/// The morning bubble, its tail at the character: in the profile row on the full widget, else where `aim` says.
pub fn nudge_placement(dock: Rect, screen: Rect, visible: &[Rect], shape: DockShape, aim: Option<f64>) -> DetailPlacement {
    let aim = aim.unwrap_or(profile_top(shape) + PROFILE_HEIGHT / 2.0);
    popover_placement(dock, screen, visible, NUDGE_WIDTH + POPOVER_TAIL, NUDGE_HEIGHT, aim)
}

/// The character editor, its tail at the person's character.
pub fn avatar_editor_placement(dock: Rect, screen: Rect, visible: &[Rect], shape: DockShape) -> DetailPlacement {
    let aim = profile_top(shape) + PROFILE_HEIGHT / 2.0;
    popover_placement(dock, screen, visible, AVATAR_EDITOR_WIDTH + POPOVER_TAIL, AVATAR_EDITOR_HEIGHT, aim)
}

/// A card's full height: taller when a finished card says what to do next.
pub fn card_height(action_shown: bool) -> f64 {
    CARD_HEIGHT + if action_shown { ACTION_HEIGHT } else { 0.0 }
}

/// What a card's sub-tasks add under it.
pub fn sub_tasks_height(count: usize) -> f64 {
    if count == 0 { 0.0 } else { count as f64 * SUB_TASK_HEIGHT + SUB_TASKS_FOOT }
}

/// How the cards fit in the room they have.
#[derive(Clone, Debug, PartialEq)]
pub struct TrayFold {
    /// Each card's own height after folding, without its sub-tasks; folded cards are `COMPACT_HEIGHT`.
    pub heights: Vec<f64>,
    /// All cards stacked.
    pub content_height: f64,
    /// The cards' visible part.
    pub viewport_height: f64,
    /// A "N개 더 있어요" bar under whole cards when they still do not fit.
    pub more_bar: bool,
}

impl TrayFold {
    pub fn total_height(&self) -> f64 {
        self.viewport_height + if self.more_bar { MORE_BAR_HEIGHT + CARD_GAP } else { 0.0 }
    }
    pub fn compact(&self, index: usize) -> bool {
        self.heights[index] == COMPACT_HEIGHT
    }
}

/// From the oldest request up, cards fold to their title and request until the stack fits. Folding goes by
/// position, never by state, so opening or finishing a card does not resize it under the pointer. If that
/// is still too tall, only whole cards show, since a card sliced at the edge reads as broken. `sub_tasks` says how
/// many lines each card holds; they stay when it folds. Only a card too tall for the room by itself is sliced.
pub fn fold_cards(full_heights: &[f64], sub_tasks: &[usize], max_height: f64) -> TrayFold {
    let mut heights = full_heights.to_vec();
    if heights.is_empty() {
        return TrayFold { heights, content_height: 0.0, viewport_height: 0.0, more_bar: false };
    }
    let lines: Vec<f64> = sub_tasks.iter().copied().map(sub_tasks_height).collect();
    let mut full: f64 = heights.iter().sum::<f64>() + lines.iter().sum::<f64>() + CARD_GAP * (heights.len() - 1) as f64;
    for height in heights.iter_mut().rev() {
        if full <= max_height {
            break;
        }
        full -= *height - COMPACT_HEIGHT;
        *height = COMPACT_HEIGHT;
    }
    if full <= max_height {
        return TrayFold { heights, content_height: full, viewport_height: full, more_bar: false };
    }
    let room = max_height - (MORE_BAR_HEIGHT + CARD_GAP);
    let whole: Vec<f64> = heights.iter().zip(&lines).map(|(height, lines)| height + lines).collect();
    let mut viewport = whole[0];
    for height in &whole[1..] {
        if viewport + CARD_GAP + height > room {
            break;
        }
        viewport += CARD_GAP + height;
    }
    TrayFold { heights, content_height: full, viewport_height: viewport.min(room), more_bar: true }
}
