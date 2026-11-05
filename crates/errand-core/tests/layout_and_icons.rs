//! Ported from the geometry and icon checks of Tests/FloatingPetsTests.m, plus the tray folding that
//! DCSessionTrayView did inline.

use errand_core::icons::{self, PathOp, Point, path};
use errand_core::layout::{self, COMPACT_HEIGHT, DockShape, Rect};

const BARE: DockShape = DockShape::FULL;
const WITH_TASKS: DockShape = DockShape { header: true, ..DockShape::FULL };

// AppKit's frames from the original test; the rules are symmetric in y, so they hold in either orientation.
fn screens() -> [Rect; 2] {
    [Rect::new(0.0, 0.0, 1440.0, 900.0), Rect::new(-1280.0, 0.0, 1280.0, 800.0)]
}

#[test]
fn the_dock_stays_on_a_connected_screen() {
    let moved = layout::constrained_frame(Rect::new(-900.0, 100.0, 300.0, 150.0), &screens());
    assert_eq!(moved.x, -900.0, "a valid position on a monitor left of the primary is preserved");
    let lost = layout::constrained_frame(Rect::new(9000.0, -500.0, 300.0, 150.0), &screens());
    assert!(screens()[0].contains(&lost), "a disconnected screen's position recovers to an available monitor: {lost:?}");
    let edge = layout::constrained_frame(Rect::new(1390.0, 890.0, 300.0, 150.0), &screens());
    assert!(screens()[0].contains(&edge), "the dock is kept within the usable display: {edge:?}");
    assert_eq!(layout::constrained_frame(Rect::new(5.0, 5.0, 1.0, 1.0), &[]), Rect::new(5.0, 5.0, 1.0, 1.0));
}

#[test]
fn the_dock_grows_by_its_header_and_sits_in_the_corner() {
    assert_eq!(layout::dock_height(BARE), 10.0 + 30.0 * 2.0 + 6.0 + 10.0 + 44.0);
    assert_eq!(layout::dock_height(WITH_TASKS) - layout::dock_height(BARE), 38.0 + 8.0 - 10.0);
    let screen = Rect::new(0.0, 25.0, 1440.0, 875.0);
    let dock = layout::default_dock_frame(screen, BARE);
    assert_eq!((dock.right(), dock.bottom()), (1440.0 - 24.0, 900.0 - 24.0));
}

#[test]
fn the_dock_grows_by_its_room_tabs_and_the_open_rooms_rows() {
    let tabs = DockShape { tabs: true, ..BARE };
    assert_eq!(layout::tabs_top(tabs), layout::profile_top(BARE) + 34.0 + 12.0, "the tabs sit 12 points under the avatar");
    assert_eq!(layout::dock_height(tabs) - layout::dock_height(BARE), 2.0 + 22.0 + 10.0, "the profile row's own 10 points under the avatar, 2 more, the tabs and the dock's padding");
    let open = DockShape { rows: 3, ..tabs };
    assert_eq!(layout::dock_height(open) - layout::dock_height(tabs), 6.0 + 3.0 * 26.0 + 8.0, "thin rows on a panel under the tabs");
    assert_eq!(layout::dock_height(DockShape { rows: 3, ..BARE }), layout::dock_height(BARE), "no rows without tabs");
}

#[test]
fn a_hidden_service_takes_its_battery_and_card_along() {
    let one = DockShape { batteries: 1, ..BARE };
    assert_eq!(layout::dock_height(BARE) - layout::dock_height(one), 30.0 + 6.0, "one battery and its gap fewer");
    assert_eq!(layout::detail_body_width(2) - layout::detail_body_width(1), 248.0 + 8.0, "one card and its gap fewer");
    assert_eq!(layout::dock_height(DockShape { batteries: 0, ..BARE }), layout::dock_height(one), "never less than one battery");
}

#[test]
fn cards_go_where_there_is_room() {
    let screen = Rect::new(0.0, 0.0, 1440.0, 900.0);
    let low = Rect::new(1100.0, 700.0, 304.0, 130.0);
    assert!(layout::cards_go_above(low, screen));
    let tray = layout::tray_frame(low, screen, 200.0);
    assert_eq!((tray.x, tray.bottom()), (low.x, low.y - layout::STACK_GAP), "the cards sit on top of the dock");
    let high = Rect::new(1100.0, 30.0, 304.0, 130.0);
    assert!(!layout::cards_go_above(high, screen));
    assert_eq!(layout::tray_frame(high, screen, 200.0).y, high.bottom() + layout::STACK_GAP, "parked at the top, the cards hang below");
    assert_eq!(layout::tray_max_height(low, screen), 560.0);
    assert_eq!(layout::tray_max_height(Rect::new(1100.0, 60.0, 304.0, 830.0), screen), 118.0, "a sliver of room still shows a card");
}

#[test]
fn details_open_on_the_side_with_room_and_point_at_the_batteries() {
    let screen = Rect::new(0.0, 0.0, 1440.0, 900.0);
    let dock = Rect::new(1112.0, 700.0, 304.0, 176.0);
    let placement = layout::detail_placement(dock, screen, &[screen], BARE);
    assert!(placement.tail_on_right && placement.frame.right() <= dock.x, "a dock on the right opens its details to the left");
    assert!(screen.contains(&placement.frame));
    let middle = dock.y + (layout::battery_top(BARE) + layout::battery_bottom(BARE)) / 2.0;
    assert_eq!(placement.frame.y + placement.tail_y, middle);
    let left = layout::detail_placement(Rect::new(20.0, 700.0, 304.0, 176.0), screen, &[screen], BARE);
    assert!(!left.tail_on_right && left.frame.x >= 20.0 + 304.0);
    let narrow = layout::detail_placement(dock, screen, &[screen], DockShape { batteries: 1, ..BARE });
    assert_eq!(placement.frame.width - narrow.frame.width, 256.0, "with one service the details hold one card");
    assert_eq!(narrow.frame.right(), placement.frame.right(), "and still end beside the dock");
}

#[test]
fn crowded_cards_fold_oldest_first_then_show_whole_cards_only() {
    let roomy = layout::fold_cards(&[118.0, 136.0], 560.0);
    assert_eq!((roomy.content_height, roomy.viewport_height, roomy.more_bar), (262.0, 262.0, false));
    assert!(!roomy.compact(0) && !roomy.compact(1));

    let heights = [118.0; 5];
    let folded = layout::fold_cards(&heights, 500.0);
    assert_eq!(folded.heights, [118.0, 118.0, COMPACT_HEIGHT, COMPACT_HEIGHT, COMPACT_HEIGHT], "the oldest requests fold first");
    assert!(!folded.more_bar && folded.total_height() <= 500.0);

    let many = layout::fold_cards(&[118.0; 10], 560.0);
    assert!(many.more_bar && many.heights.iter().all(|height| *height == COMPACT_HEIGHT));
    assert!(many.total_height() <= 560.0, "{many:?}");
    let shown = (many.viewport_height + layout::CARD_GAP) / (COMPACT_HEIGHT + layout::CARD_GAP);
    assert_eq!(shown.fract(), 0.0, "no card is sliced at the edge: {many:?}");
    assert!(layout::fold_cards(&[], 560.0).total_height() == 0.0);
}

#[test]
fn every_icon_draws_inside_lucides_grid() {
    let names: Vec<&str> = icons::names().collect();
    assert_eq!(names.len(), 26);
    for name in names {
        let outline = icons::outline(name).unwrap();
        let (low, high) = path::bounds(&outline).unwrap();
        assert!(outline.len() > 1 && low.x >= -0.5 && low.y >= -0.5 && high.x <= 24.5 && high.y <= 24.5, "{name}: {low:?} {high:?}");
    }
    let (low, high) = path::bounds(&icons::outline("refresh-cw").unwrap()).unwrap();
    for (value, expected) in [(low.x, 3.0), (low.y, 3.0), (high.x, 21.0), (high.y, 21.0)] {
        assert!((value - expected).abs() < 0.5, "arcs land on the grid: {low:?} {high:?}");
    }
    assert!(icons::elements("no-such-icon").is_none());
}

#[test]
fn path_data_parses_like_svg() {
    assert_eq!(
        path::parse("M1 2 3 4h1v-1z"),
        [
            PathOp::MoveTo(Point::new(1.0, 2.0)),
            PathOp::LineTo(Point::new(3.0, 4.0)),
            PathOp::LineTo(Point::new(4.0, 4.0)),
            PathOp::LineTo(Point::new(4.0, 3.0)),
            PathOp::Close,
        ],
        "pairs after a move are lines"
    );
    assert_eq!(path::parse("M0 0L1.35.09"), [PathOp::MoveTo(Point::new(0.0, 0.0)), PathOp::LineTo(Point::new(1.35, 0.09))]);
    assert_eq!(path::parse("M0 0Z 5"), [PathOp::MoveTo(Point::new(0.0, 0.0)), PathOp::Close], "stray numbers after a close end parsing");
    assert_eq!(path::parse("M0 0 L x"), [PathOp::MoveTo(Point::new(0.0, 0.0))], "a malformed command keeps what came before");
    let arc = path::parse("M0 0A5 5 0 0 1 10 0");
    let (low, high) = path::bounds(&arc).unwrap();
    assert!((low.y + 5.0).abs() < 0.01 && high.y.abs() < 1e-9 && (high.x - 10.0).abs() < 1e-9, "a sweep half circle bulges upward: {low:?} {high:?}");
}
