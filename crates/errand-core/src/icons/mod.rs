//! The Lucide line icons Errand draws (ISC license, in `data.rs`). Each platform strokes these shapes
//! itself; arrows, dingbats and emoji never stand in for them.

mod data;
pub mod path;

pub use path::{PathOp, Point};

/// Lucide's own geometry: a 24-unit grid with a 2-unit round stroke.
pub const GRID: f64 = 24.0;
pub const STROKE: f64 = 2.0;

/// One SVG element of an icon, on the 24-unit grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Element {
    /// SVG path data; see [`path::parse`].
    Path(&'static str),
    Circle { cx: f64, cy: f64, r: f64 },
    Ellipse { cx: f64, cy: f64, rx: f64, ry: f64 },
    Rect { x: f64, y: f64, width: f64, height: f64, rx: f64, ry: f64 },
    Line { x1: f64, y1: f64, x2: f64, y2: f64 },
    Polyline(&'static [(f64, f64)]),
    Polygon(&'static [(f64, f64)]),
}

/// The elements of a Lucide icon, or None for a name Errand does not carry.
pub fn elements(name: &str) -> Option<&'static [Element]> {
    data::ICONS.iter().find(|(icon, _)| *icon == name).map(|(_, elements)| *elements)
}

pub fn names() -> impl Iterator<Item = &'static str> {
    data::ICONS.iter().map(|(name, _)| *name)
}

/// An icon as one outline, for platforms that stroke paths only and for checking its extent.
pub fn outline(name: &str) -> Option<Vec<PathOp>> {
    Some(elements(name)?.iter().flat_map(element_outline).collect())
}

/// One element as path steps. Ovals and rounded corners become the same four-arc cubic curves AppKit draws.
pub fn element_outline(element: &Element) -> Vec<PathOp> {
    match *element {
        Element::Path(data) => path::parse(data),
        Element::Circle { cx, cy, r } => path::oval(cx - r, cy - r, r * 2.0, r * 2.0),
        Element::Ellipse { cx, cy, rx, ry } => path::oval(cx - rx, cy - ry, rx * 2.0, ry * 2.0),
        Element::Rect { x, y, width, height, rx, ry } => path::rounded_rect(x, y, width, height, rx, ry),
        Element::Line { x1, y1, x2, y2 } => vec![PathOp::MoveTo(Point::new(x1, y1)), PathOp::LineTo(Point::new(x2, y2))],
        Element::Polyline(points) | Element::Polygon(points) => {
            let Some(((x, y), rest)) = points.split_first() else { return Vec::new() };
            let mut ops = vec![PathOp::MoveTo(Point::new(*x, *y))];
            ops.extend(rest.iter().map(|(x, y)| PathOp::LineTo(Point::new(*x, *y))));
            if matches!(element, Element::Polygon(_)) {
                ops.push(PathOp::Close);
            }
            ops
        }
    }
}
