//! SVG path data as move, line and cubic-curve steps, the primitives every platform's path type has.
//! Quadratic curves and arcs become cubics the way the SVG implementation notes describe.

use std::f64::consts::{FRAC_PI_2, PI};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Point {
        Point { x, y }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathOp {
    MoveTo(Point),
    LineTo(Point),
    /// End point, then the two control points.
    CubicTo(Point, Point, Point),
    Close,
}

/// Parses SVG path data. Parsing stops at the first malformed command, keeping what came before.
pub fn parse(data: &str) -> Vec<PathOp> {
    let mut scanner = Scanner { text: data.as_bytes(), index: 0 };
    let mut ops = Vec::new();
    let (mut command, mut previous) = (0u8, 0u8);
    let (mut current, mut start, mut control) = (Point::default(), Point::default(), Point::default());
    while scanner.skip_separators() {
        let fresh = scanner.peek().is_ascii_alphabetic();
        if fresh {
            command = scanner.advance();
        } else if command == 0 {
            break;
        }
        let relative = command.is_ascii_lowercase();
        let origin = if relative { current } else { Point::default() };
        let at = |x: f64, y: f64| Point::new(origin.x + x, origin.y + y);
        let upper = command.to_ascii_uppercase();
        let count = match upper {
            b'Z' => 0,
            b'H' | b'V' => 1,
            b'M' | b'L' | b'T' => 2,
            b'S' | b'Q' => 4,
            b'C' => 6,
            b'A' => 7,
            _ => break,
        };
        // Numbers after a close have no command to belong to.
        if upper == b'Z' && !fresh {
            break;
        }
        let mut values = [0.0; 7];
        let (mut large, mut sweep) = (false, false);
        for (item, value) in values.iter_mut().enumerate().take(count) {
            let read = match (upper, item) {
                (b'A', 3) => scanner.flag().map(|flag| large = flag),
                (b'A', 4) => scanner.flag().map(|flag| sweep = flag),
                _ => scanner.number().map(|number| *value = number),
            };
            if read.is_none() {
                return ops;
            }
        }
        let v = values;
        match upper {
            b'Z' => {
                ops.push(PathOp::Close);
                current = start;
                previous = b'Z';
                continue;
            }
            b'M' => {
                current = at(v[0], v[1]);
                start = current;
                ops.push(PathOp::MoveTo(current));
                // Further pairs after a move are lines.
                command = if relative { b'l' } else { b'L' };
            }
            b'L' | b'H' | b'V' => {
                let end = match upper {
                    b'H' => Point::new(origin.x + v[0], current.y),
                    b'V' => Point::new(current.x, origin.y + v[0]),
                    _ => at(v[0], v[1]),
                };
                ops.push(PathOp::LineTo(end));
                current = end;
            }
            b'C' | b'S' => {
                let first = if upper == b'C' {
                    at(v[0], v[1])
                } else if matches!(previous, b'C' | b'S') {
                    Point::new(2.0 * current.x - control.x, 2.0 * current.y - control.y)
                } else {
                    current
                };
                let offset = if upper == b'C' { 2 } else { 0 };
                control = at(v[offset], v[offset + 1]);
                let end = at(v[offset + 2], v[offset + 3]);
                ops.push(PathOp::CubicTo(end, first, control));
                current = end;
            }
            b'Q' | b'T' => {
                let quad = if upper == b'Q' {
                    at(v[0], v[1])
                } else if matches!(previous, b'Q' | b'T') {
                    Point::new(2.0 * current.x - control.x, 2.0 * current.y - control.y)
                } else {
                    current
                };
                let offset = if upper == b'Q' { 2 } else { 0 };
                let end = at(v[offset], v[offset + 1]);
                let toward = |from: Point| Point::new(from.x + 2.0 / 3.0 * (quad.x - from.x), from.y + 2.0 / 3.0 * (quad.y - from.y));
                ops.push(PathOp::CubicTo(end, toward(current), toward(end)));
                control = quad;
                current = end;
            }
            _ => {
                let end = at(v[5], v[6]);
                append_arc(&mut ops, current, v[0], v[1], v[2], large, sweep, end);
                current = end;
            }
        }
        previous = upper;
    }
    ops
}

struct Scanner<'a> {
    text: &'a [u8],
    index: usize,
}

impl Scanner<'_> {
    fn peek(&self) -> u8 {
        self.text[self.index]
    }
    fn advance(&mut self) -> u8 {
        self.index += 1;
        self.text[self.index - 1]
    }
    /// Skips whitespace and commas; false at the end.
    fn skip_separators(&mut self) -> bool {
        while self.index < self.text.len() && (self.text[self.index].is_ascii_whitespace() || self.text[self.index] == b'\x0b' || self.text[self.index] == b',') {
            self.index += 1;
        }
        self.index < self.text.len()
    }
    /// "1.35.09" is two numbers: a second dot starts the next one.
    fn number(&mut self) -> Option<f64> {
        if !self.skip_separators() {
            return None;
        }
        let text = self.text;
        let start = self.index;
        let mut cursor = start;
        if matches!(text[cursor], b'-' | b'+') {
            cursor += 1;
        }
        let (mut digits, mut dot) = (false, false);
        while cursor < text.len() && (text[cursor].is_ascii_digit() || (text[cursor] == b'.' && !dot)) {
            if text[cursor] == b'.' {
                dot = true;
            } else {
                digits = true;
            }
            cursor += 1;
        }
        if !digits {
            return None;
        }
        if cursor < text.len() && matches!(text[cursor], b'e' | b'E') {
            let mut exponent = cursor + 1;
            if exponent < text.len() && matches!(text[exponent], b'-' | b'+') {
                exponent += 1;
            }
            if exponent < text.len() && text[exponent].is_ascii_digit() {
                cursor = exponent;
                while cursor < text.len() && text[cursor].is_ascii_digit() {
                    cursor += 1;
                }
            }
        }
        self.index = cursor;
        std::str::from_utf8(&text[start..cursor]).ok()?.parse().ok()
    }
    fn flag(&mut self) -> Option<bool> {
        if !self.skip_separators() || !matches!(self.peek(), b'0' | b'1') {
            return None;
        }
        Some(self.advance() == b'1')
    }
}

/// An SVG arc as cubic curves, from the endpoint to the centre parameterisation.
#[allow(clippy::too_many_arguments)]
fn append_arc(ops: &mut Vec<PathOp>, from: Point, rx: f64, ry: f64, degrees: f64, large: bool, sweep: bool, to: Point) {
    if from == to {
        return;
    }
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx == 0.0 || ry == 0.0 {
        ops.push(PathOp::LineTo(to));
        return;
    }
    let phi = degrees * PI / 180.0;
    let (sin_phi, cos_phi) = phi.sin_cos();
    let (dx, dy) = ((from.x - to.x) / 2.0, (from.y - to.y) / 2.0);
    let (x1, y1) = (cos_phi * dx + sin_phi * dy, -sin_phi * dx + cos_phi * dy);
    let scale = x1 * x1 / (rx * rx) + y1 * y1 / (ry * ry);
    if scale > 1.0 {
        rx *= scale.sqrt();
        ry *= scale.sqrt();
    }
    let numerator = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let denominator = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let coefficient = if large != sweep { 1.0 } else { -1.0 } * (numerator / denominator).max(0.0).sqrt();
    let (cx1, cy1) = (coefficient * rx * y1 / ry, -coefficient * ry * x1 / rx);
    let cx = cos_phi * cx1 - sin_phi * cy1 + (from.x + to.x) / 2.0;
    let cy = sin_phi * cx1 + cos_phi * cy1 + (from.y + to.y) / 2.0;
    let start = ((y1 - cy1) / ry).atan2((x1 - cx1) / rx);
    let end = ((-y1 - cy1) / ry).atan2((-x1 - cx1) / rx);
    let mut delta = end - start;
    if sweep && delta < 0.0 {
        delta += 2.0 * PI;
    } else if !sweep && delta > 0.0 {
        delta -= 2.0 * PI;
    }
    let segments = ((delta.abs() / FRAC_PI_2 - 1e-9).ceil() as i64).max(1);
    let step = delta / segments as f64;
    let handle = 4.0 / 3.0 * (step / 4.0).tan();
    let point = |ux: f64, uy: f64| Point::new(cos_phi * rx * ux - sin_phi * ry * uy + cx, sin_phi * rx * ux + cos_phi * ry * uy + cy);
    for segment in 0..segments {
        let a = start + segment as f64 * step;
        let b = a + step;
        let first = point(a.cos() - handle * a.sin(), a.sin() + handle * a.cos());
        let second = point(b.cos() + handle * b.sin(), b.sin() - handle * b.cos());
        let end = if segment == segments - 1 { to } else { point(b.cos(), b.sin()) };
        ops.push(PathOp::CubicTo(end, first, second));
    }
}

/// The handle length that makes four cubics a circle.
const KAPPA: f64 = 0.552_284_749_830_793_6;

/// An ellipse in the rect, as four cubic quarter-arcs.
pub fn oval(x: f64, y: f64, width: f64, height: f64) -> Vec<PathOp> {
    let (rx, ry) = (width / 2.0, height / 2.0);
    let (cx, cy) = (x + rx, y + ry);
    let (kx, ky) = (rx * KAPPA, ry * KAPPA);
    vec![
        PathOp::MoveTo(Point::new(cx + rx, cy)),
        PathOp::CubicTo(Point::new(cx, cy + ry), Point::new(cx + rx, cy + ky), Point::new(cx + kx, cy + ry)),
        PathOp::CubicTo(Point::new(cx - rx, cy), Point::new(cx - kx, cy + ry), Point::new(cx - rx, cy + ky)),
        PathOp::CubicTo(Point::new(cx, cy - ry), Point::new(cx - rx, cy - ky), Point::new(cx - kx, cy - ry)),
        PathOp::CubicTo(Point::new(cx + rx, cy), Point::new(cx + kx, cy - ry), Point::new(cx + rx, cy - ky)),
        PathOp::Close,
    ]
}

/// A rectangle whose corners are quarter-ellipses, radii clamped to half its sides.
pub fn rounded_rect(x: f64, y: f64, width: f64, height: f64, rx: f64, ry: f64) -> Vec<PathOp> {
    let (rx, ry) = (rx.clamp(0.0, width / 2.0), ry.clamp(0.0, height / 2.0));
    let (right, bottom) = (x + width, y + height);
    if rx == 0.0 || ry == 0.0 {
        return vec![
            PathOp::MoveTo(Point::new(x, y)),
            PathOp::LineTo(Point::new(right, y)),
            PathOp::LineTo(Point::new(right, bottom)),
            PathOp::LineTo(Point::new(x, bottom)),
            PathOp::Close,
        ];
    }
    let (kx, ky) = (rx * KAPPA, ry * KAPPA);
    vec![
        PathOp::MoveTo(Point::new(x + rx, y)),
        PathOp::LineTo(Point::new(right - rx, y)),
        PathOp::CubicTo(Point::new(right, y + ry), Point::new(right - rx + kx, y), Point::new(right, y + ry - ky)),
        PathOp::LineTo(Point::new(right, bottom - ry)),
        PathOp::CubicTo(Point::new(right - rx, bottom), Point::new(right, bottom - ry + ky), Point::new(right - rx + kx, bottom)),
        PathOp::LineTo(Point::new(x + rx, bottom)),
        PathOp::CubicTo(Point::new(x, bottom - ry), Point::new(x + rx - kx, bottom), Point::new(x, bottom - ry + ky)),
        PathOp::LineTo(Point::new(x, y + ry)),
        PathOp::CubicTo(Point::new(x + rx, y), Point::new(x, y + ry - ky), Point::new(x + rx - kx, y)),
        PathOp::Close,
    ]
}

/// The tight box around the drawn curve, not its control points: (top left, bottom right).
pub fn bounds(ops: &[PathOp]) -> Option<(Point, Point)> {
    let mut low = Point::new(f64::INFINITY, f64::INFINITY);
    let mut high = Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut include = |point: Point| {
        low = Point::new(low.x.min(point.x), low.y.min(point.y));
        high = Point::new(high.x.max(point.x), high.y.max(point.y));
    };
    let (mut current, mut start) = (Point::default(), Point::default());
    for op in ops {
        match *op {
            PathOp::MoveTo(point) => {
                include(point);
                current = point;
                start = point;
            }
            PathOp::LineTo(point) => {
                include(point);
                current = point;
            }
            PathOp::CubicTo(end, first, second) => {
                include(end);
                for t in extrema(current.x, first.x, second.x, end.x).into_iter().chain(extrema(current.y, first.y, second.y, end.y)) {
                    include(cubic_point(current, first, second, end, t));
                }
                current = end;
            }
            PathOp::Close => current = start,
        }
    }
    (low.x <= high.x).then_some((low, high))
}

fn cubic_point(p0: Point, p1: Point, p2: Point, p3: Point, t: f64) -> Point {
    let u = 1.0 - t;
    let blend = |a: f64, b: f64, c: f64, d: f64| u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d;
    Point::new(blend(p0.x, p1.x, p2.x, p3.x), blend(p0.y, p1.y, p2.y, p3.y))
}

/// Where one coordinate of a cubic turns, strictly inside the curve.
fn extrema(p0: f64, p1: f64, p2: f64, p3: f64) -> Vec<f64> {
    let (d0, d1, d2) = (p1 - p0, p2 - p1, p3 - p2);
    let (a, b, c) = (d0 - 2.0 * d1 + d2, 2.0 * (d1 - d0), d0);
    let roots = if a.abs() < 1e-12 {
        if b.abs() < 1e-12 { vec![] } else { vec![-c / b] }
    } else {
        let discriminant = b * b - 4.0 * a * c;
        if discriminant < 0.0 {
            vec![]
        } else {
            let root = discriminant.sqrt();
            vec![(-b + root) / (2.0 * a), (-b - root) / (2.0 * a)]
        }
    };
    roots.into_iter().filter(|t| *t > 0.0 && *t < 1.0).collect()
}
