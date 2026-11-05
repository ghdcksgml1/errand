//! The person's character: a short, round body in one of five shapes, wearing clothes, with ears, a sprout
//! or a hat on top or nothing at all. A seed picks every part, so a seed always draws the same character,
//! and once someone picks parts themselves the character is kept as those parts instead. Only the clothes
//! and the hat take a colour of their own; the body, lines and cheeks are everyone's.
//!
//! The character is drawn whole on a square [`FRAME`] units wide and shows through the circle inscribed in
//! it, which cuts the body off below the chest: the face and the top of the clothes are what show.

use crate::icons::path::{self, PathOp, Point};
use crate::theme::{self, Paint, Rgb};

/// The side of the square the layers are drawn on; the picture is the circle inscribed in it.
pub const FRAME: f64 = 100.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Dome,
    Square,
    Triangle,
    Cloud,
    Hexagon,
}

/// What sits on top of the body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Top {
    Bare,
    BearEars,
    CatEars,
    Sprout,
    /// In the clothes' colour.
    Beanie,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Eyes {
    Dots,
    Sparkly,
    Happy,
    Sleepy,
    Squint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mouth {
    Smile,
    Cat,
    Round,
    Grin,
    Flat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outfit {
    Tee,
    Stripes,
    Collar,
    Hoodie,
    Overalls,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Avatar {
    pub shape: Shape,
    pub top: Top,
    pub eyes: Eyes,
    pub mouth: Mouth,
    pub outfit: Outfit,
    /// Pink cheeks.
    pub blush: bool,
    /// One of [`CLOTHES`].
    pub clothes: Rgb,
}

/// What an editor changes, one option at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Shape,
    Top,
    Eyes,
    Mouth,
    Outfit,
    Cheeks,
}

pub const PARTS: [Part; 6] = [Part::Shape, Part::Top, Part::Eyes, Part::Mouth, Part::Outfit, Part::Cheeks];

/// Starts a character kept as its parts, so it is never taken for a seed.
const CODE: &str = "avatar:";

pub const SHAPES: [Shape; 5] = [Shape::Dome, Shape::Square, Shape::Triangle, Shape::Cloud, Shape::Hexagon];
pub const TOPS: [Top; 5] = [Top::Bare, Top::BearEars, Top::CatEars, Top::Sprout, Top::Beanie];
/// A triangle's point leaves no room for ears.
const POINTED_TOPS: [Top; 3] = [Top::Bare, Top::Sprout, Top::Beanie];
pub const EYES: [Eyes; 5] = [Eyes::Dots, Eyes::Sparkly, Eyes::Happy, Eyes::Sleepy, Eyes::Squint];
pub const MOUTHS: [Mouth; 5] = [Mouth::Smile, Mouth::Cat, Mouth::Round, Mouth::Grin, Mouth::Flat];
pub const OUTFITS: [Outfit; 5] = [Outfit::Tee, Outfit::Stripes, Outfit::Collar, Outfit::Hoodie, Outfit::Overalls];

// How each option is written in a kept character; a changed word would lose the characters people picked.
impl Shape {
    fn code(self) -> &'static str {
        match self {
            Shape::Dome => "dome",
            Shape::Square => "square",
            Shape::Triangle => "triangle",
            Shape::Cloud => "cloud",
            Shape::Hexagon => "hexagon",
        }
    }
}

impl Top {
    fn code(self) -> &'static str {
        match self {
            Top::Bare => "bare",
            Top::BearEars => "bear",
            Top::CatEars => "cat",
            Top::Sprout => "sprout",
            Top::Beanie => "beanie",
        }
    }
}

impl Eyes {
    fn code(self) -> &'static str {
        match self {
            Eyes::Dots => "dots",
            Eyes::Sparkly => "sparkly",
            Eyes::Happy => "happy",
            Eyes::Sleepy => "sleepy",
            Eyes::Squint => "squint",
        }
    }
}

impl Mouth {
    fn code(self) -> &'static str {
        match self {
            Mouth::Smile => "smile",
            Mouth::Cat => "cat",
            Mouth::Round => "round",
            Mouth::Grin => "grin",
            Mouth::Flat => "flat",
        }
    }
}

impl Outfit {
    fn code(self) -> &'static str {
        match self {
            Outfit::Tee => "tee",
            Outfit::Stripes => "stripes",
            Outfit::Collar => "collar",
            Outfit::Hoodie => "hoodie",
            Outfit::Overalls => "overalls",
        }
    }
}

/// What the clothes can be dyed.
pub const CLOTHES: [Rgb; 12] = [
    hex(0x3B82F6),
    hex(0x0EA5E9),
    hex(0x14B8A6),
    hex(0x22C55E),
    hex(0x84CC16),
    hex(0xEAB308),
    hex(0xF97316),
    hex(0xEF4444),
    hex(0xEC4899),
    hex(0xA855F7),
    hex(0x6366F1),
    hex(0x64748B),
];

/// Everyone's colours: the body, its lines, the cheeks and inner ears, and the sprout.
pub const BODY: Rgb = hex(0xFFF9EE);
pub const INK: Rgb = hex(0x2E2926);
pub const PINK: Rgb = hex(0xFF8FA3);
pub const LEAF: Rgb = hex(0x7BC67E);

const fn hex(value: u32) -> Rgb {
    Rgb { red: ((value >> 16) & 0xFF) as f64 / 255.0, green: ((value >> 8) & 0xFF) as f64 / 255.0, blue: (value & 0xFF) as f64 / 255.0 }
}

/// The shade of the clothes for folds, the hood and the hat's band, and the tint for stripes, strings and buttons.
pub fn shades(clothes: Rgb) -> (Rgb, Rgb) {
    (clothes.blend(0.2, Rgb::grey(0.0)), clothes.blend(0.6, theme::WHITE))
}

impl Avatar {
    /// The character a seed draws. FNV-1a and mulberry32 are written out here rather than taken from std, whose
    /// hashers may change, so a seed keeps its character from one build to the next and on every platform.
    pub fn from_seed(seed: &str) -> Avatar {
        let mut random = Mulberry32(fnv1a(seed.as_bytes()));
        let shape = random.pick(&SHAPES);
        let top = random.pick(Avatar::tops(shape));
        let eyes = random.pick(&EYES);
        let mouth = random.pick(&MOUTHS);
        let outfit = random.pick(&OUTFITS);
        let blush = random.chance(0.6);
        let clothes = random.pick(&CLOTHES);
        Avatar { shape, top, eyes, mouth, outfit, blush, clothes }
    }

    /// The character a profile names: the parts someone picked, as [`Avatar::code`] writes them, or else the
    /// character a seed draws.
    pub fn named(text: &str) -> Avatar {
        Avatar::parse(text).unwrap_or_else(|| Avatar::from_seed(text))
    }

    /// The parts as text, to keep a character someone picked; [`Avatar::named`] reads it back.
    pub fn code(&self) -> String {
        let [red, green, blue] = [self.clothes.red, self.clothes.green, self.clothes.blue].map(|channel| (channel * 255.0).round() as u8);
        let cheeks = if self.blush { "blush" } else { "plain" };
        let (shape, top, eyes, mouth, outfit) = (self.shape.code(), self.top.code(), self.eyes.code(), self.mouth.code(), self.outfit.code());
        format!("{CODE}{shape},{top},{eyes},{mouth},{outfit},{cheeks},{red:02X}{green:02X}{blue:02X}")
    }

    /// The parts a code names, or None for anything else, a seed included: for codes from elsewhere, such as a
    /// room's, that must not be drawn as a seed when they are not one.
    pub fn parse(text: &str) -> Option<Avatar> {
        let fields: Vec<&str> = text.strip_prefix(CODE)?.split(',').collect();
        let [shape, top, eyes, mouth, outfit, cheeks, clothes] = fields[..] else { return None };
        let colour = (clothes.len() == 6 && clothes.bytes().all(|digit| digit.is_ascii_hexdigit())).then(|| u32::from_str_radix(clothes, 16).ok())??;
        Some(Avatar {
            shape: *SHAPES.iter().find(|option| option.code() == shape)?,
            top: *TOPS.iter().find(|option| option.code() == top)?,
            eyes: *EYES.iter().find(|option| option.code() == eyes)?,
            mouth: *MOUTHS.iter().find(|option| option.code() == mouth)?,
            outfit: *OUTFITS.iter().find(|option| option.code() == outfit)?,
            blush: match cheeks {
                "blush" => true,
                "plain" => false,
                _ => return None,
            },
            clothes: hex(colour),
        })
    }

    /// The tops a body can wear: a triangle's point leaves no room for ears.
    pub fn tops(shape: Shape) -> &'static [Top] {
        if shape == Shape::Triangle { &POINTED_TOPS } else { &TOPS }
    }

    /// The character with one part moved `by` options along, round and round. A body turned into a triangle
    /// takes its ears off.
    pub fn step(self, part: Part, by: isize) -> Avatar {
        fn along<T: Copy + PartialEq>(options: &[T], current: T, by: isize) -> T {
            let at = options.iter().position(|option| *option == current).unwrap_or(0) as isize;
            options[(at + by).rem_euclid(options.len() as isize) as usize]
        }
        let mut next = self;
        match part {
            Part::Shape => {
                next.shape = along(&SHAPES, self.shape, by);
                if !Avatar::tops(next.shape).contains(&next.top) {
                    next.top = Top::Bare;
                }
            }
            Part::Top => next.top = along(Avatar::tops(self.shape), self.top, by),
            Part::Eyes => next.eyes = along(&EYES, self.eyes, by),
            Part::Mouth => next.mouth = along(&MOUTHS, self.mouth, by),
            Part::Outfit => next.outfit = along(&OUTFITS, self.outfit, by),
            Part::Cheeks => next.blush = self.blush != (by.rem_euclid(2) == 1),
        }
        next
    }

    /// How to draw the character on the [`FRAME`] square, back to front.
    pub fn layers(&self) -> Vec<Layer> {
        let body = Body::of(self.shape);
        let (dark, light) = shades(self.clothes);
        let mut canvas = Canvas::default();
        // Ears share the body's outline: every part is stroked at twice the outline's width before any is filled,
        // so only the outer half of each stroke shows, around all of them together.
        let (ears, inner_ears) = self.ears(&body);
        let silhouette: Vec<&Vec<PathOp>> = ears.iter().chain(&body.parts).collect();
        for part in &silhouette {
            canvas.stroke((*part).clone(), INK, 5.0);
        }
        for part in &silhouette {
            canvas.fill((*part).clone(), BODY);
        }
        for part in inner_ears {
            canvas.fill(part, PINK);
        }
        for part in &body.parts {
            canvas.fill(part.clone(), BODY);
        }
        self.dress(&mut canvas, &body, dark, light);
        let sleeves = if self.outfit == Outfit::Overalls { BODY } else { self.clothes };
        canvas.outlined(tilted(14.0, 84.0, 6.5, 9.5, 18.0), sleeves, 2.4);
        canvas.outlined(tilted(86.0, 84.0, 6.5, 9.5, -18.0), sleeves, 2.4);
        self.face(&mut canvas);
        self.hat_or_sprout(&mut canvas, &body, dark);
        canvas.layers.into_iter().map(Layer::through_camera).collect()
    }

    /// The ears, then their pink insides; none where the head has no room for them.
    fn ears(&self, body: &Body) -> (Vec<Vec<PathOp>>, Vec<Vec<PathOp>>) {
        let Some(y) = body.ear else { return (Vec::new(), Vec::new()) };
        match self.top {
            Top::BearEars => (vec![circle(28.0, y + 1.0, 10.0), circle(72.0, y + 1.0, 10.0)], vec![circle(28.0, y + 1.0, 5.0), circle(72.0, y + 1.0, 5.0)]),
            Top::CatEars => (
                vec![svg(&format!("M17 {}L24 {}L39 {}Z", y + 8.0, y - 14.0, y + 2.0)), svg(&format!("M61 {}L76 {}L83 {}Z", y + 2.0, y - 14.0, y + 8.0))],
                vec![svg(&format!("M22 {}L25 {}L32 {}Z", y + 3.0, y - 7.0, y + 1.0)), svg(&format!("M68 {}L75 {}L78 {}Z", y + 1.0, y - 7.0, y + 3.0))],
            ),
            Top::Bare | Top::Sprout | Top::Beanie => (Vec::new(), Vec::new()),
        }
    }

    /// The clothes from y 70 down, inside the body, then what laps over its edge: a collar's points or a hood.
    fn dress(&self, canvas: &mut Canvas, body: &Body, dark: Rgb, light: Rgb) {
        let clothes = self.clothes;
        let shirt = |edge: &str| svg(&format!("{edge}V140H-20Z"));
        canvas.inside(body.outline(), |canvas| match self.outfit {
            Outfit::Tee | Outfit::Stripes => {
                let edge = "M-20 70H37Q50 81 63 70H120";
                canvas.fill(shirt(edge), clothes);
                if self.outfit == Outfit::Stripes {
                    for y in [81.0, 90.0, 99.0] {
                        canvas.fill(rect(-20.0, y, 140.0, 4.2), light);
                    }
                }
                canvas.stroke(svg(edge), INK, 2.2);
            }
            Outfit::Collar => {
                let edge = "M-20 70H40L50 82L60 70H120";
                canvas.fill(shirt(edge), clothes);
                canvas.stroke(svg(edge), INK, 2.2);
                canvas.fill(circle(50.0, 88.0, 1.9), light);
            }
            Outfit::Hoodie => {
                let edge = "M-20 71H120";
                canvas.fill(shirt(edge), clothes);
                canvas.stroke(svg(edge), INK, 2.2);
            }
            Outfit::Overalls => {
                let straps = "M37 74L6 60M63 74L94 60";
                canvas.stroke(svg(straps), INK, 7.0);
                canvas.stroke(svg(straps), clothes, 4.2);
                canvas.outlined(svg("M-20 84H35V75Q35 71 39 71H61Q65 71 65 75V84H120V140H-20Z"), clothes, 2.2);
                canvas.fill(circle(39.5, 75.5, 1.9), light);
                canvas.fill(circle(60.5, 75.5, 1.9), light);
            }
        });
        match self.outfit {
            Outfit::Collar => {
                canvas.outlined(svg("M39 68.5L50 82L34 79Z"), theme::WHITE, 2.0);
                canvas.outlined(svg("M61 68.5L66 79L50 82Z"), theme::WHITE, 2.0);
            }
            Outfit::Hoodie => {
                let hood = "M24 70Q50 87 76 70";
                canvas.stroke(svg(hood), INK, 12.4);
                canvas.stroke(svg(hood), dark, 8.0);
                canvas.stroke(svg("M44 79L43 90M56 79L57 90"), light, 2.4);
            }
            Outfit::Tee | Outfit::Stripes | Outfit::Overalls => {}
        }
    }

    fn face(&self, canvas: &mut Canvas) {
        const LINE: f64 = 2.6;
        match self.eyes {
            Eyes::Dots => {
                canvas.fill(oval(38.0, 50.0, 3.3, 3.3), INK);
                canvas.fill(oval(62.0, 50.0, 3.3, 3.3), INK);
            }
            Eyes::Sparkly => {
                canvas.fill(oval(38.0, 50.0, 3.6, 4.6), INK);
                canvas.fill(oval(62.0, 50.0, 3.6, 4.6), INK);
                canvas.fill(oval(39.2, 48.3, 1.4, 1.4), theme::WHITE);
                canvas.fill(oval(63.2, 48.3, 1.4, 1.4), theme::WHITE);
            }
            Eyes::Happy => canvas.stroke(svg("M33.5 51.8Q38 46 42.5 51.8M57.5 51.8Q62 46 66.5 51.8"), INK, LINE),
            Eyes::Sleepy => canvas.stroke(svg("M33.5 49.5Q38 54 42.5 49.5M57.5 49.5Q62 54 66.5 49.5"), INK, LINE),
            Eyes::Squint => canvas.stroke(svg("M35 46.5L40.5 50L35 53.5M65 46.5L59.5 50L65 53.5"), INK, LINE),
        }
        if self.blush {
            canvas.add(oval(28.0, 57.5, 5.5, 3.2), Some(PINK.alpha(0.55)), None);
            canvas.add(oval(72.0, 57.5, 5.5, 3.2), Some(PINK.alpha(0.55)), None);
        }
        match self.mouth {
            Mouth::Smile => canvas.stroke(svg("M45 57.5Q50 62.5 55 57.5"), INK, LINE),
            Mouth::Cat => canvas.stroke(svg("M44 57.5Q47 61.5 50 57.5Q53 61.5 56 57.5"), INK, LINE),
            Mouth::Round => canvas.fill(oval(50.0, 59.5, 2.6, 3.0), INK),
            Mouth::Grin => {
                canvas.fill(svg("M44.5 57H55.5Q55.5 64 50 64Q44.5 64 44.5 57Z"), INK);
                canvas.fill(oval(50.0, 62.2, 3.0, 1.6), PINK);
            }
            Mouth::Flat => canvas.stroke(svg("M46.5 59H53.5"), INK, LINE),
        }
    }

    fn hat_or_sprout(&self, canvas: &mut Canvas, body: &Body, dark: Rgb) {
        let top = body.top;
        match self.top {
            Top::Sprout => {
                canvas.stroke(svg(&format!("M50 {}V{}", top + 2.0, top - 7.0)), INK, 2.2);
                canvas.outlined(tilted(44.5, top - 9.5, 6.0, 3.4, 20.0), LEAF, 2.0);
                canvas.outlined(tilted(55.5, top - 9.5, 6.0, 3.4, -20.0), LEAF, 2.0);
            }
            Top::Beanie => {
                let (clothes, brim) = (self.clothes, body.brim);
                canvas.outlined(circle(50.0, top - 6.0, 5.5), dark, 2.4);
                // The hat is the head itself grown by half a stroke, kept above the brim, under a band.
                canvas.inside(rect(-20.0, -20.0, 140.0, brim + 20.0), |canvas| {
                    for part in &body.parts {
                        canvas.stroke(part.clone(), INK, 10.0);
                    }
                    for part in &body.parts {
                        canvas.add(part.clone(), Some(clothes.alpha(1.0)), Some(Stroke { paint: clothes.alpha(1.0), width: 5.0 }));
                    }
                });
                for (y, height, ink, width) in [(brim - 7.5, 9.0, INK, 12.0), (brim - 6.25, 6.5, dark, 7.0)] {
                    canvas.inside(rect(-20.0, y, 140.0, height), |canvas| {
                        for part in &body.parts {
                            canvas.add(part.clone(), Some(ink.alpha(1.0)), Some(Stroke { paint: ink.alpha(1.0), width }));
                        }
                    });
                }
            }
            Top::Bare | Top::BearEars | Top::CatEars => {}
        }
    }
}

/// One step of drawing a character: the path filled and then stroked with round caps and joins, all of it
/// inside `clip` when there is one.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub path: Vec<PathOp>,
    pub fill: Option<Paint>,
    pub stroke: Option<Stroke>,
    pub clip: Option<Vec<PathOp>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stroke {
    pub paint: Paint,
    pub width: f64,
}

impl Layer {
    /// Shrinks the character a little about its chest, so ears, sprouts and pompoms stay inside the circle.
    fn through_camera(self) -> Layer {
        const ZOOM: f64 = 0.92;
        let look = |point: Point| Point::new(50.0 + (point.x - 50.0) * ZOOM, 55.0 + (point.y - 55.0) * ZOOM);
        Layer {
            path: map(&self.path, look),
            fill: self.fill,
            stroke: self.stroke.map(|stroke| Stroke { width: stroke.width * ZOOM, ..stroke }),
            clip: self.clip.map(|clip| map(&clip, look)),
        }
    }
}

/// A body shape in its parts, and where things sit on it.
struct Body {
    parts: Vec<Vec<PathOp>>,
    /// The top of the head, where a sprout grows and a pompom sits.
    top: f64,
    /// Where ears join the head, when there is room for them.
    ear: Option<f64>,
    /// The beanie's lower edge.
    brim: f64,
}

impl Body {
    fn of(shape: Shape) -> Body {
        // Every body is 80 wide, its sides rounding into the bottom at 106, well below the circle.
        let base = "V96Q90 106 80 106H20Q10 106 10 96Z";
        let (parts, top, ear, brim) = match shape {
            Shape::Dome => (vec![svg(&format!("M10 96V56A40 40 0 0 1 90 56{base}"))], 16.0, Some(22.6), 33.0),
            Shape::Square => (vec![svg(&format!("M10 96V36Q10 20 26 20H74Q90 20 90 36{base}"))], 20.0, Some(20.0), 37.0),
            Shape::Triangle => (vec![svg(&format!("M10 96V80Q10 70 15 62L42 22Q50 9 58 22L85 62Q90 70 90 80{base}"))], 15.5, None, 33.0),
            Shape::Cloud => {
                let parts = vec![svg(&format!("M10 96V42H90{base}")), circle(24.0, 42.0, 14.0), circle(50.0, 30.0, 17.0), circle(76.0, 42.0, 14.0)];
                (parts, 13.0, Some(28.6), 30.0)
            }
            Shape::Hexagon => {
                let hexagon = format!("M10 96V46Q10 42 12.5 39L27.5 21Q30 18 34 18H66Q70 18 72.5 21L87.5 39Q90 42 90 46{base}");
                (vec![svg(&hexagon)], 18.0, Some(21.0), 35.0)
            }
        };
        Body { parts, top, ear, brim }
    }

    /// The parts as one shape, to draw inside. They all run clockwise, so where they overlap stays inside.
    fn outline(&self) -> Vec<PathOp> {
        self.parts.concat()
    }
}

/// Layers in the order they are added, each kept inside the clip in force when it was added.
#[derive(Default)]
struct Canvas {
    layers: Vec<Layer>,
    clip: Option<Vec<PathOp>>,
}

impl Canvas {
    fn add(&mut self, path: Vec<PathOp>, fill: Option<Paint>, stroke: Option<Stroke>) {
        self.layers.push(Layer { path, fill, stroke, clip: self.clip.clone() });
    }

    fn fill(&mut self, path: Vec<PathOp>, color: Rgb) {
        self.add(path, Some(color.alpha(1.0)), None);
    }

    fn stroke(&mut self, path: Vec<PathOp>, color: Rgb, width: f64) {
        self.add(path, None, Some(Stroke { paint: color.alpha(1.0), width }));
    }

    /// Filled with `color` and outlined in ink.
    fn outlined(&mut self, path: Vec<PathOp>, color: Rgb, width: f64) {
        self.add(path, Some(color.alpha(1.0)), Some(Stroke { paint: INK.alpha(1.0), width }));
    }

    fn inside(&mut self, clip: Vec<PathOp>, draw: impl FnOnce(&mut Canvas)) {
        self.clip = Some(clip);
        draw(self);
        self.clip = None;
    }
}

fn svg(data: &str) -> Vec<PathOp> {
    path::parse(data)
}

/// A circle as two clockwise arcs, the way the bodies run, so a cloud's bumps and body make one shape.
fn circle(cx: f64, cy: f64, r: f64) -> Vec<PathOp> {
    svg(&format!("M{} {cy}A{r} {r} 0 1 1 {} {cy}A{r} {r} 0 1 1 {} {cy}Z", cx - r, cx + r, cx - r))
}

fn oval(cx: f64, cy: f64, rx: f64, ry: f64) -> Vec<PathOp> {
    path::oval(cx - rx, cy - ry, rx * 2.0, ry * 2.0)
}

/// An oval turned `degrees` clockwise about its centre.
fn tilted(cx: f64, cy: f64, rx: f64, ry: f64, degrees: f64) -> Vec<PathOp> {
    let (sin, cos) = degrees.to_radians().sin_cos();
    map(&oval(cx, cy, rx, ry), |point| Point::new(cx + (point.x - cx) * cos - (point.y - cy) * sin, cy + (point.x - cx) * sin + (point.y - cy) * cos))
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> Vec<PathOp> {
    path::rounded_rect(x, y, width, height, 0.0, 0.0)
}

fn map(ops: &[PathOp], move_point: impl Fn(Point) -> Point) -> Vec<PathOp> {
    ops.iter()
        .map(|op| match *op {
            PathOp::MoveTo(to) => PathOp::MoveTo(move_point(to)),
            PathOp::LineTo(to) => PathOp::LineTo(move_point(to)),
            PathOp::CubicTo(to, first, second) => PathOp::CubicTo(move_point(to), move_point(first), move_point(second)),
            PathOp::Close => PathOp::Close,
        })
        .collect()
}

fn fnv1a(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811C_9DC5, |hash, byte| (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193))
}

/// Tommy Ettinger's mulberry32: a small generator whose every output follows from the seed.
struct Mulberry32(u32);

impl Mulberry32 {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_add(0x6D2B_79F5);
        let mut t = self.0;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        t ^ (t >> 14)
    }

    fn pick<T: Copy>(&mut self, options: &[T]) -> T {
        options[((u64::from(self.next()) * options.len() as u64) >> 32) as usize]
    }

    /// True with this likelihood.
    fn chance(&mut self, likelihood: f64) -> bool {
        f64::from(self.next()) / 4_294_967_296.0 < likelihood
    }
}
