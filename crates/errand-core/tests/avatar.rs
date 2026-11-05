//! The person's character: what a seed draws, and how it is drawn.

use errand_core::avatar::{self, Avatar, BODY, CLOTHES, EYES, Eyes, FRAME, INK, LEAF, MOUTHS, Mouth, OUTFITS, Outfit, PARTS, PINK, Part, SHAPES, Shape, TOPS, Top};
use errand_core::icons::{PathOp, Point};
use errand_core::theme::{self, Rgb};
use std::fmt::Debug;

/// Every way a character can look, in one colour of clothes.
fn every_character(clothes: Rgb) -> Vec<Avatar> {
    let mut all = Vec::new();
    for shape in SHAPES {
        for top in TOPS.into_iter().filter(|top| shape != Shape::Triangle || !matches!(top, Top::BearEars | Top::CatEars)) {
            for eyes in EYES {
                for mouth in MOUTHS {
                    for outfit in OUTFITS {
                        for blush in [false, true] {
                            all.push(Avatar { shape, top, eyes, mouth, outfit, blush, clothes });
                        }
                    }
                }
            }
        }
    }
    all
}

fn points(ops: &[PathOp]) -> Vec<Point> {
    ops.iter()
        .flat_map(|op| match *op {
            PathOp::MoveTo(to) | PathOp::LineTo(to) => vec![to],
            PathOp::CubicTo(to, first, second) => vec![to, first, second],
            PathOp::Close => vec![],
        })
        .collect()
}

#[test]
fn a_seed_always_draws_the_same_character() {
    // Drawn once and kept: a different hash or order of picks would hand everyone someone else's character.
    let cases = [
        ("졸린 수달", Shape::Triangle, Top::Sprout, Eyes::Dots, Mouth::Cat, Outfit::Stripes, false, 0),
        ("바쁜 너구리", Shape::Triangle, Top::Beanie, Eyes::Happy, Mouth::Grin, Outfit::Stripes, true, 10),
        ("조용한 부엉이", Shape::Dome, Top::CatEars, Eyes::Sparkly, Mouth::Grin, Outfit::Tee, true, 2),
        ("한결", Shape::Square, Top::BearEars, Eyes::Squint, Mouth::Smile, Outfit::Tee, false, 6),
        ("", Shape::Cloud, Top::CatEars, Eyes::Sleepy, Mouth::Round, Outfit::Overalls, true, 10),
        ("seed", Shape::Hexagon, Top::Bare, Eyes::Dots, Mouth::Grin, Outfit::Tee, true, 4),
    ];
    for (seed, shape, top, eyes, mouth, outfit, blush, clothes) in cases {
        assert_eq!(Avatar::from_seed(seed), Avatar { shape, top, eyes, mouth, outfit, blush, clothes: CLOTHES[clothes] }, "{seed:?}");
    }
}

#[test]
fn every_part_turns_up_and_a_triangle_never_has_ears() {
    let drawn: Vec<Avatar> = (0..2000).map(|seed| Avatar::from_seed(&seed.to_string())).collect();
    fn turns_up<T: PartialEq + Debug>(drawn: &[Avatar], options: &[T], part: impl Fn(&Avatar) -> T) {
        for option in options {
            assert!(drawn.iter().any(|avatar| part(avatar) == *option), "no seed drew {option:?}");
        }
    }
    turns_up(&drawn, &SHAPES, |avatar| avatar.shape);
    turns_up(&drawn, &TOPS, |avatar| avatar.top);
    turns_up(&drawn, &EYES, |avatar| avatar.eyes);
    turns_up(&drawn, &MOUTHS, |avatar| avatar.mouth);
    turns_up(&drawn, &OUTFITS, |avatar| avatar.outfit);
    turns_up(&drawn, &[false, true], |avatar| avatar.blush);
    turns_up(&drawn, &CLOTHES, |avatar| avatar.clothes);
    assert!(drawn.iter().all(|avatar| avatar.shape != Shape::Triangle || !matches!(avatar.top, Top::BearEars | Top::CatEars)));
}

#[test]
fn a_picked_character_reads_back_from_its_code() {
    for clothes in CLOTHES {
        for character in every_character(clothes) {
            assert_eq!(Avatar::named(&character.code()), character, "{}", character.code());
        }
    }
    assert_eq!(Avatar::from_seed("한결").code(), "avatar:square,bear,squint,smile,tee,plain,F97316");
}

#[test]
fn anything_but_a_whole_code_is_a_seed() {
    let near_misses = [
        "졸린 수달",
        "",
        "avatar:",
        "avatar:square,bear,squint,smile,tee,plain",
        "avatar:square,bear,squint,smile,tee,plain,F97316,more",
        "avatar:blob,bear,squint,smile,tee,plain,F97316",
        "avatar:square,bear,squint,smile,tee,pink,F97316",
        "avatar:square,bear,squint,smile,tee,plain,+F9731",
    ];
    for text in near_misses {
        assert_eq!(Avatar::named(text), Avatar::from_seed(text), "{text:?}");
    }
}

#[test]
fn a_part_steps_round_and_a_triangle_drops_its_ears() {
    let bear = Avatar { shape: Shape::Square, top: Top::BearEars, ..Avatar::from_seed("한결") };
    let triangle = bear.step(Part::Shape, 1);
    assert_eq!((triangle.shape, triangle.top), (Shape::Triangle, Top::Bare), "a square turned triangle takes its ears off");
    let tops: Vec<Top> = (1..=3).map(|by| triangle.step(Part::Top, by).top).collect();
    assert_eq!(tops, [Top::Sprout, Top::Beanie, Top::Bare], "a triangle's tops skip the ears");
    for part in PARTS {
        // Going round passes the triangle, which would take the ears off, so the body goes round bare-headed.
        let start = if part == Part::Shape { Avatar { top: Top::Bare, ..bear } } else { bear };
        let lap = if part == Part::Cheeks { 2 } else { 5 };
        assert_eq!((0..lap).fold(start, |avatar, _| avatar.step(part, 1)), start, "{part:?} comes round after {lap} steps");
        assert_eq!(start.step(part, 1).step(part, -1), start, "{part:?} steps back where it was");
    }
}

#[test]
fn only_the_clothes_and_the_hat_take_a_colour() {
    let clothes = CLOTHES[3];
    let (dark, light) = avatar::shades(clothes);
    let everyones = [BODY, INK, PINK, LEAF, theme::WHITE];
    for character in every_character(clothes) {
        let layers = character.layers();
        let colours: Vec<Rgb> = layers.iter().flat_map(|layer| layer.fill.into_iter().chain(layer.stroke.map(|stroke| stroke.paint))).map(|paint| paint.color).collect();
        assert!(colours.iter().all(|colour| everyones.contains(colour) || [clothes, dark, light].contains(colour)), "{character:?}");
        assert!(colours.contains(&clothes), "{character:?} wears its colour");
    }
}

#[test]
fn nothing_above_the_middle_is_cut_off_by_the_circle() {
    // Below the middle the circle cuts the body off on purpose; above it, ears, sprouts and hats must fit.
    let middle = FRAME / 2.0;
    for character in every_character(CLOTHES[0]) {
        for layer in character.layers().iter().filter(|layer| layer.clip.is_none()) {
            let reach = layer.stroke.map_or(0.0, |stroke| stroke.width / 2.0);
            for point in points(&layer.path).into_iter().filter(|point| point.y < middle) {
                assert!((point.x - middle).hypot(point.y - middle) + reach <= middle, "{character:?} pokes out at {point:?}");
            }
        }
    }
}
