//! The desktop window: the person's work by day, week and month, a report of the last weeks, and one task's
//! whole record, drawn in the widget's own cards, chips and colours. Everything it shows comes from
//! `errand_core::journal::view`; this file only lays it out and draws it.

use super::kit::{self, Weight, color, ns, paint, rect};
use super::marks::draw_app_mark;
use super::spinner::SpinningIconView;
use errand_core::avatar::Avatar;
use errand_core::journal::map::{COLOURED, MapState, MapTask, MapView, clock, clock_of};
use errand_core::journal::summary::Open;
use errand_core::journal::view::{Body, Callout, DetailView, Draft, DraftLine, JournalTab, LeftLine, MapScreen, Masthead, RecapCard, RecapView, Screen, Stat, Translate, Words, translate_label};
use jiff::civil::Date;
use errand_core::model::Msg;
use errand_core::ports::Poster;
use errand_core::theme::{self, Paint, Rgb};
use errand_core::{Language, Provider};
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAffineTransformNSAppKitAdditions, NSBitmapImageFileType, NSBitmapImageRep, NSDeviceRGBColorSpace, NSModalResponseOK, NSSavePanel,
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSApplication, NSBackingStoreType, NSBezierPath, NSColor, NSCompositingOperation, NSEvent, NSEventModifierFlags,
    NSCursor, NSGraphicsContext, NSLineBreakMode, NSLineCapStyle, NSLineJoinStyle, NSMutableParagraphStyle, NSScrollView, NSShadow, NSStringDrawing, NSStringDrawingOptions, NSStringNSExtendedStringDrawing, NSTextAlignment, NSView, NSWindow, NSWindowDelegate,
    NSTrackingArea, NSTrackingAreaOptions, NSWindowStyleMask, NSWindowTitleVisibility, NSWorkspace,
};
use objc2_foundation::{NSAffineTransform, NSAttributedStringKey, NSDictionary, NSNotification, NSNumber, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSTimer};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

pub const SIZE: (f64, f64) = (1180.0, 800.0);
const TOP: f64 = 52.0;
const PAD: f64 = 28.0;
const WIDTH: f64 = SIZE.0 - 2.0 * PAD;

const fn rgb(red: f64, green: f64, blue: f64) -> Rgb {
    Rgb { red, green, blue }
}

const GROUND: Rgb = rgb(0.953, 0.957, 0.965);
const EDGE: Rgb = rgb(0.882, 0.890, 0.902);
const HAIRLINE: Rgb = rgb(0.933, 0.941, 0.949);
const FAINT: Rgb = rgb(0.706, 0.729, 0.741);
/// What a press on a drawn spot does.
#[derive(Clone, Debug)]
enum Hit {
    Tab(JournalTab),
    Step(i64),
    /// Goes to a day, a week or a month, this far back.
    Show(JournalTab, i64),
    /// Picks the day whose summary the week shows, or the week whole; the window keeps this itself.
    Pick(Option<usize>),
    Open(String),
    Close,
    Copy(String),
    OpenTask(String),
    Translate(String, Vec<String>, Language),
    Original(bool),
    Summarize(Date),
    EditSummary(Date),
    Recap(Option<usize>),
    /// Saves one recap card, or all of them, as an image; the window does this itself.
    SaveRecap(Option<usize>),
    /// Plays the map again from its start, or stops it while it plays; the window does this itself.
    Replay,
    /// Lets go of the stretch of the map that was dragged over; the window does this itself.
    Unselect,
}

impl Hit {
    fn message(self) -> Msg {
        match self {
            Hit::Tab(tab) => Msg::JournalTab(tab),
            Hit::Step(step) => Msg::JournalStep(step),
            Hit::Show(tab, back) => Msg::JournalShow { tab, back },
            Hit::Open(key) => Msg::JournalOpen(Some(key)),
            Hit::Close => Msg::JournalOpen(None),
            Hit::Copy(text) => Msg::JournalCopy(text),
            Hit::OpenTask(url) => Msg::JournalOpenTask(url),
            Hit::Translate(key, texts, language) => Msg::JournalTranslate { key, texts, language },
            Hit::Original(original) => Msg::JournalOriginal(original),
            Hit::Summarize(date) => Msg::JournalSummarize(date),
            Hit::EditSummary(date) => Msg::JournalEditSummary(date),
            Hit::Recap(card) => Msg::JournalRecap(card),
            Hit::SaveRecap(_) | Hit::Pick(_) | Hit::Replay | Hit::Unselect => Msg::Redraw,
        }
    }
}

/// Draws, or only measures, and keeps where each pressable thing landed.
struct Pen {
    draw: bool,
    hits: Vec<(NSRect, Hit)>,
    /// Where each task is drawn, as its week on screen and its place among that week's tasks.
    spots: Vec<(NSRect, (usize, usize))>,
    /// Where a mark for work going on belongs. Nothing is drawn there: a turning icon is laid over the spot.
    spins: RefCell<Vec<Spin>>,
}

/// A spot for a turning icon, and its colour.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Spin {
    frame: NSRect,
    ink: Rgb,
}

/// The icon that turns for work going on.
const TURNING: &str = "loader-circle";

fn attributes(size: f64, weight: Weight, ink: &NSColor, kern: f64, align: NSTextAlignment, wrap: bool) -> Retained<NSDictionary<NSAttributedStringKey, AnyObject>> {
    let font = kit::font(size, weight);
    let style = NSMutableParagraphStyle::new();
    style.setLineBreakMode(if wrap { NSLineBreakMode::ByWordWrapping } else { NSLineBreakMode::ByTruncatingTail });
    style.setAlignment(align);
    let kern = NSNumber::new_f64(kern);
    let keys: [&NSAttributedStringKey; 4] = unsafe {
        [objc2_app_kit::NSFontAttributeName, objc2_app_kit::NSForegroundColorAttributeName, objc2_app_kit::NSParagraphStyleAttributeName, objc2_app_kit::NSKernAttributeName]
    };
    let values: [&AnyObject; 4] = [font.as_ref(), ink.as_ref(), style.as_ref(), kern.as_ref()];
    NSDictionary::from_slices(&keys, &values)
}

fn text_width(text: &str, size: f64, weight: Weight) -> f64 {
    let attributes = attributes(size, weight, &NSColor::blackColor(), 0.0, NSTextAlignment::Left, false);
    unsafe { ns(text).sizeWithAttributes(Some(&attributes)) }.width.ceil()
}

impl Pen {
    fn measuring() -> Pen {
        Pen { draw: false, hits: Vec::new(), spots: Vec::new(), spins: RefCell::new(Vec::new()) }
    }

    fn drawing() -> Pen {
        Pen { draw: true, ..Pen::measuring() }
    }

    /// Leaves room for the icon that turns for work going on. The view lays a turning icon over each such
    /// spot, which Core Animation turns at no cost; turning it here would draw the page again many times a
    /// second.
    fn spinner(&self, frame: NSRect, ink: Rgb) {
        self.spins.borrow_mut().push(Spin { frame, ink });
    }

    fn hit(&mut self, frame: NSRect, hit: Hit) {
        self.hits.push((frame, hit));
    }

    fn round(&self, frame: NSRect, radius: f64, fill: Option<Paint>, edge: Option<(Paint, f64)>) {
        if !self.draw {
            return;
        }
        let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(frame, radius, radius);
        if let Some(fill) = fill {
            paint(fill).setFill();
            path.fill();
        }
        if let Some((edge, width)) = edge {
            let inset = kit::inset(frame, width / 2.0, width / 2.0);
            let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(inset, (radius - width / 2.0).max(0.0), (radius - width / 2.0).max(0.0));
            paint(edge).setStroke();
            path.setLineWidth(width);
            path.stroke();
        }
    }

    /// A white panel lifted off the ground, as the widget's cards sit on the desktop.
    fn panel(&self, frame: NSRect, radius: f64, fill: Rgb, edge: Paint, edge_width: f64) {
        if !self.draw {
            return;
        }
        NSGraphicsContext::saveGraphicsState_class();
        let shadow = NSShadow::new();
        shadow.setShadowColor(Some(&paint(theme::INK.alpha(0.07))));
        shadow.setShadowBlurRadius(18.0);
        shadow.setShadowOffset(NSSize::new(0.0, -6.0));
        shadow.set();
        color(fill).setFill();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(frame, radius, radius).fill();
        NSGraphicsContext::restoreGraphicsState_class();
        self.round(frame, radius, None, Some((edge, edge_width)));
    }

    fn fill(&self, frame: NSRect, ink: Paint) {
        if self.draw {
            paint(ink).setFill();
            NSBezierPath::fillRect(frame);
        }
    }

    /// One line of text with its top left at (x, y); returns its width.
    fn text(&self, text: &str, x: f64, y: f64, size: f64, weight: Weight, ink: Paint) -> f64 {
        let width = text_width(text, size, weight);
        if self.draw {
            let attributes = attributes(size, weight, &paint(ink), 0.0, NSTextAlignment::Left, false);
            unsafe { ns(text).drawAtPoint_withAttributes(NSPoint::new(x, y), Some(&attributes)) };
        }
        width
    }

    /// Text inside a frame: cut short with an ellipsis, wrapped to as many lines as the frame holds.
    #[allow(clippy::too_many_arguments)]
    fn text_in(&self, text: &str, frame: NSRect, size: f64, weight: Weight, ink: Paint, align: NSTextAlignment, wrap: bool) {
        if !self.draw {
            return;
        }
        let attributes = attributes(size, weight, &paint(ink), 0.0, align, wrap);
        let options = NSStringDrawingOptions::UsesLineFragmentOrigin | NSStringDrawingOptions::TruncatesLastVisibleLine;
        unsafe { ns(text).drawWithRect_options_attributes_context(frame, options, Some(&attributes), None) };
    }

    /// One line inside a frame, cut in its middle when it is too long for it.
    fn text_middle(&self, text: &str, frame: NSRect, size: f64, weight: Weight, ink: Paint) {
        if !self.draw {
            return;
        }
        let font = kit::font(size, weight);
        let style = NSMutableParagraphStyle::new();
        style.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
        let ink = paint(ink);
        let keys: [&NSAttributedStringKey; 3] = unsafe { [objc2_app_kit::NSFontAttributeName, objc2_app_kit::NSForegroundColorAttributeName, objc2_app_kit::NSParagraphStyleAttributeName] };
        let values: [&AnyObject; 3] = [font.as_ref(), ink.as_ref(), style.as_ref()];
        let attributes = NSDictionary::from_slices(&keys, &values);
        let options = NSStringDrawingOptions::UsesLineFragmentOrigin | NSStringDrawingOptions::TruncatesLastVisibleLine;
        unsafe { ns(text).drawWithRect_options_attributes_context(frame, options, Some(&attributes), None) };
    }

    /// How tall wrapped text is at this width, up to `lines` lines.
    fn text_height(&self, text: &str, width: f64, size: f64, weight: Weight, lines: usize) -> f64 {
        let attributes = attributes(size, weight, &NSColor::blackColor(), 0.0, NSTextAlignment::Left, true);
        let options = NSStringDrawingOptions::UsesLineFragmentOrigin;
        let bounds = unsafe { ns(text).boundingRectWithSize_options_attributes_context(NSSize::new(width, 10_000.0), options, Some(&attributes), None) };
        let line = kit::font(size, weight).boundingRectForFont().size.height.min(size * 1.35);
        bounds.size.height.ceil().min(line * lines as f64).max(line)
    }

    fn icon(&self, name: &str, frame: NSRect, ink: Rgb) {
        if !self.draw {
            return;
        }
        if let Some(image) = kit::icon_image(name, frame.size.width, Some(ink), 0.0, 0.0) {
            unsafe { image.drawInRect_fromRect_operation_fraction_respectFlipped_hints(frame, NSRect::ZERO, NSCompositingOperation::SourceOver, 1.0, true, None) };
        }
    }

    fn app(&self, provider: Provider, frame: NSRect) {
        if self.draw {
            draw_app_mark(provider, frame);
        }
    }

    /// A rounded count or label with an optional icon; returns its width.
    #[allow(clippy::too_many_arguments)]
    fn capsule(&self, x: f64, y: f64, height: f64, icon: Option<&str>, text: &str, size: f64, fill: Paint, ink: Rgb, icon_ink: Rgb) -> f64 {
        let icon_size = (size + 1.0).round();
        let words = text_width(text, size, Weight::Semibold);
        let width = if icon.is_some() { 8.0 + icon_size + 5.0 + words + 10.0 } else { 9.0 + words + 9.0 };
        self.round(rect(x, y, width, height), height / 2.0, Some(fill), None);
        let mut cursor = x + if icon.is_some() { 8.0 } else { 9.0 };
        if let Some(icon) = icon {
            let frame = rect(cursor, y + (height - icon_size) / 2.0, icon_size, icon_size);
            if icon == TURNING { self.spinner(frame, icon_ink) } else { self.icon(icon, frame, icon_ink) }
            cursor += icon_size + 5.0;
        }
        self.text(text, cursor, y + (height - size * 1.25) / 2.0, size, Weight::Semibold, ink.alpha(1.0));
        width
    }
}

/// The colours a live state puts on a chip: its fill, its words and its icon.
fn chip_colors(callout: Callout) -> (Paint, Rgb, Rgb) {
    match callout {
        Callout::Asking => (theme::ASKING.alpha(0.12), theme::ASKING, theme::ASKING),
        Callout::New => (theme::NEW_RESULT.alpha(0.12), theme::NEW_RESULT, theme::NEW_RESULT),
        Callout::Running => (theme::RUNNING_BRIGHT.alpha(0.16), theme::RUNNING, theme::RUNNING_BRIGHT),
    }
}

/// Counts at the end of the date line, right-aligned, their figures' baseline at `baseline`. Returns where
/// the first of them begins.
fn stats(pen: &Pen, stats: &[Stat], right: f64, baseline: f64) -> f64 {
    let (mut x, mut left) = (right, right);
    for stat in stats.iter().rev() {
        let figure: f64 = stat.parts.iter().map(|part| text_width(&part.text, if part.small { 15.0 } else { 26.0 }, Weight::Bold)).sum();
        let label = text_width(&stat.label, 11.5, Weight::Semibold);
        let width = figure.max(label);
        x -= width;
        left = x;
        let ink = if stat.asking { theme::ASKING } else { theme::INK };
        pen.text(&stat.label, x, baseline - 50.0, 11.5, Weight::Semibold, if stat.asking { theme::ASKING.alpha(1.0) } else { theme::MUTED.alpha(1.0) });
        let mut cursor = x;
        for part in &stat.parts {
            let size = if part.small { 15.0 } else { 26.0 };
            // Units sit on the figures' baseline.
            let top = baseline - kit::font(size, Weight::Bold).ascender() - 4.0;
            cursor += pen.text(&part.text, cursor, top, size, Weight::Bold, ink.alpha(1.0));
        }
        x -= 28.0;
    }
    left
}

/// The big number, set tight as in the mock-ups; returns its width.
fn big_number(pen: &Pen, text: &str, x: f64, y: f64) -> f64 {
    let attributes = attributes(104.0, Weight::Bold, &color(theme::INK), -5.0, NSTextAlignment::Left, false);
    let size = unsafe { ns(text).sizeWithAttributes(Some(&attributes)) };
    if pen.draw {
        unsafe { ns(text).drawAtPoint_withAttributes(NSPoint::new(x, y), Some(&attributes)) };
    }
    size.width.ceil()
}

// The page: one map for a day, a week or a month, and the cards and the year strip that follow it.

const HERO_TOP: f64 = 120.0;
const HERO_HEIGHT: f64 = 300.0;
const ROW_TOP: f64 = HERO_TOP + HERO_HEIGHT + 12.0;
const ROW_HEIGHT: f64 = 236.0;
const YEAR_TOP: f64 = ROW_TOP + ROW_HEIGHT + 12.0;
/// The map's plot inside its card: where the days begin and end, and the band they sit in.
const X0: f64 = 150.0;
const X1: f64 = 1024.0;
const PLOT: f64 = X1 - X0;
const DAY_GAP: f64 = 8.0;
const DAY_WIDTH: f64 = (PLOT - 6.0 * DAY_GAP) / 7.0;
const BAND_TOP: f64 = 40.0;
const BAND_BOTTOM: f64 = 270.0;
/// How far a track sits from its line: the first under it, the next over it.
const TRACKS: [f64; 5] = [0.0, 18.0, -18.0, 36.0, -36.0];
/// A line in a card's list.
const ROW: f64 = 26.0;
/// The lines' colours: violet, aqua, blue and pink, which stay apart for colour-blind eyes too and keep clear
/// of the orange that means an answer is needed. A fifth line, or all the rest, is grey.
const LINE_COLOURS: [Rgb; COLOURED] = [rgb(0.384, 0.314, 0.839), rgb(0.106, 0.686, 0.478), rgb(0.165, 0.471, 0.839), rgb(0.910, 0.482, 0.643)];
const OTHER_LINE: Rgb = rgb(0.553, 0.592, 0.612);
const BAND: Rgb = rgb(0.961, 0.965, 0.973);
/// Words a step below the ink, and small words a step darker than the widget's grey so they hold on the ground.
const SECOND: Rgb = rgb(0.243, 0.282, 0.302);
const SOFT: Rgb = rgb(0.369, 0.408, 0.431);
/// The year strip, from a little work to the most.
const HEAT: [Rgb; 5] = [rgb(0.867, 0.882, 0.898), rgb(0.718, 0.745, 0.769), rgb(0.545, 0.580, 0.608), rgb(0.341, 0.384, 0.416), rgb(0.149, 0.188, 0.212)];
const NO_WORK: Rgb = rgb(0.925, 0.933, 0.945);

/// What the person picked on the page that the model does not keep.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Ui {
    /// The day whose summary the week shows; none follows today, or the last day with work.
    day: Option<usize>,
    /// The week's summary whole, instead of one of its days.
    whole_week: bool,
    /// The task the pointer rests on, as its week on screen and its place there, and where the pointer is.
    hover: Option<(usize, usize)>,
    pointer: (f64, f64),
    /// How far the map is opened while it moves from one view to the next; none once it rests where its view
    /// has it.
    zoom: Option<Zoom>,
    /// How far through the map a replay has got, from 0 at its start to 1 at its end, and a little over 1
    /// while it holds there before the page is as it was.
    replay: Option<f64>,
    /// The stretch of the map the person dragged over, as where it begins and ends across the page.
    select: Option<(f64, f64)>,
}

/// How far the map is opened on one day: 0 shows the week, 1 that day across the whole plot.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Zoom {
    open: f64,
    day: f64,
}

/// How far the map is opened when a view rests; none on a month, which is not one map.
fn rest(screen: &Screen) -> Option<Zoom> {
    screen.map.weeks.first()?;
    match screen.place.tab {
        JournalTab::Day => Some(Zoom { open: 1.0, day: screen.map.day.unwrap_or(0) as f64 }),
        JournalTab::Week => Some(Zoom { open: 0.0, day: 0.0 }),
        JournalTab::Month | JournalTab::Report => None,
    }
}

/// The date line of whichever view is on screen.
fn head(screen: &Screen) -> &Masthead {
    match &screen.body {
        Body::Day(view) => &view.masthead,
        Body::Week(view) => &view.masthead,
        Body::Month(view) => &view.masthead,
        Body::Report(view) => &view.masthead,
    }
}

/// The day whose summary shows: the day on screen, or on a week the one picked from its tabs, today, or the
/// last day with work.
fn chosen_day(page: &MapScreen, ui: Ui) -> Option<usize> {
    let map = page.weeks.first()?;
    let days: Vec<usize> = page.drafts.iter().map(|draft| draft.0).collect();
    page.day.or(ui.day.filter(|day| days.contains(day))).or_else(|| map.now.map(|now| now.0).filter(|day| days.contains(day))).or(days.last().copied())
}

/// How the page is drawn this moment: what the person picked, and what follows from it.
struct Look {
    ui: Ui,
    /// How far the map is opened; none on a month.
    zoom: Option<Zoom>,
    /// Where a replay's playhead is across the page, while the map replays.
    playhead: Option<f64>,
    /// How far the names over the work have come in behind the playhead: as far as the playhead, and on past
    /// where it stops while the replay holds at its end, so that the last names are in before it is over.
    wake: Option<f64>,
    /// For each task of the week on screen, whether the counts and the cards speak of it: those in the stretch
    /// dragged over or, while the map replays, those the playhead has passed. None when they speak of all.
    inside: Option<Vec<bool>>,
    /// When the stretch dragged over begins and ends: the day, and the minute of it to the nearest five.
    range: Option<((usize, i64), (usize, i64))>,
}

/// What of the person's picks the date line and the cards show. They are drawn again only when this changes,
/// however the map moves meanwhile.
#[derive(PartialEq)]
struct Shown {
    day: Option<usize>,
    whole_week: bool,
    hover: Option<(usize, usize)>,
    inside: Option<Vec<bool>>,
    replaying: bool,
}

impl Look {
    fn shown(&self) -> Shown {
        Shown { day: self.ui.day, whole_week: self.ui.whole_week, hover: self.ui.hover, inside: self.inside.clone(), replaying: self.playhead.is_some() }
    }

    fn new(screen: &Screen, ui: Ui) -> Look {
        let (Some(resting), Some(map)) = (rest(screen), screen.map.weeks.first()) else {
            return Look { ui: Ui { zoom: None, replay: None, select: None, ..ui }, zoom: None, playhead: None, wake: None, inside: None, range: None };
        };
        let zoom = ui.zoom.unwrap_or(resting);
        let plot = Plot { map, zoom, x: PAD, y: HERO_TOP };
        let ends = |task: &MapTask| (plot.at(task.day, task.start), plot.at(task.day, task.waiting.unwrap_or(task.end)));
        let along = |part: f64| plot.span().map(|(from, to)| from + (to - from) * part);
        let (playhead, wake) = (ui.replay.and_then(|part| along(part.min(1.0))), ui.replay.and_then(along));
        let inside = match (playhead, ui.select) {
            (Some(playhead), _) => Some(map.tasks.iter().map(|task| ends(task).1 <= playhead + 0.5).collect()),
            (None, Some((from, to))) => Some(map.tasks.iter().map(|task| ends(task).1 >= from && ends(task).0 <= to).collect()),
            (None, None) => None,
        };
        // To five minutes, which is as fine as a hand drags.
        let moment = |x: f64| {
            let (day, minute) = plot.moment(x);
            (day, ((minute / 5.0).round() * 5.0) as i64)
        };
        let range = ui.select.filter(|_| playhead.is_none()).map(|(from, to)| (moment(from), moment(to)));
        Look { ui, zoom: Some(zoom), playhead, wake, inside, range }
    }

    /// Whether the counts and the cards speak of a task just now.
    fn holds(&self, week: usize, task: usize) -> bool {
        week != 0 || self.inside.as_ref().is_none_or(|inside| inside.get(task).copied().unwrap_or(true))
    }

    /// Whether a task is drawn at full strength: with the pointer on a task, only that one, on each day it ran;
    /// otherwise all that the page speaks of.
    fn lit(&self, page: &MapScreen, week: usize, task: usize) -> bool {
        match self.ui.hover.and_then(|(week, task)| page.weeks.get(week).and_then(|map| map.tasks.get(task))) {
            Some(hovered) => page.weeks[week].tasks[task].key == hovered.key,
            None => self.holds(week, task),
        }
    }
}

/// A list's row that stands for a task: marked while the pointer rests on that task anywhere, and somewhere
/// to rest the pointer on it.
fn row_spot(pen: &mut Pen, page: &MapScreen, look: &Look, frame: NSRect, task: Option<usize>) {
    let Some(task) = task else { return };
    if look.ui.hover.is_some() && look.lit(page, 0, task) {
        pen.round(frame, 8.0, Some(theme::INK.alpha(0.055)), None);
    }
    pen.spots.push((frame, (0, task)));
}

/// Pales a row once it is drawn, where the page does not speak of its task just now: a replay has not come
/// to it yet.
fn row_rest(pen: &Pen, look: &Look, frame: NSRect, task: Option<usize>) {
    if task.is_some_and(|task| !look.holds(0, task)) {
        pen.fill(frame, theme::WHITE.alpha(0.78));
    }
}

/// Where a moment of the week falls on the map.
struct Plot<'a> {
    map: &'a MapView,
    zoom: Zoom,
    x: f64,
    y: f64,
}

impl Plot<'_> {
    fn at(&self, day: usize, minute: f64) -> f64 {
        let (first, last) = (self.map.first_hour as f64 * 60.0, self.map.last_hour as f64 * 60.0);
        let part = (minute - first) / (last - first);
        let week = X0 + day as f64 * (DAY_WIDTH + DAY_GAP) + part.clamp(0.0, 1.0) * DAY_WIDTH;
        let open = X0 + (day as f64 - self.zoom.day) * (PLOT + 64.0) + part * PLOT;
        self.x + week + (open - week) * self.zoom.open
    }

    /// How far apart the lines are.
    fn gap(&self) -> f64 {
        let count = self.map.lanes.len().max(1) as f64;
        if count > 1.0 { (184.0 / (count - 1.0)).min(74.0) } else { 74.0 }
    }

    fn lane(&self, lane: usize) -> f64 {
        let count = self.map.lanes.len().max(1) as f64;
        self.y + 158.0 - (count - 1.0) / 2.0 * self.gap() + lane as f64 * self.gap()
    }

    /// How far a track sits from its line; closer where the lines themselves are close.
    fn track(&self, track: usize) -> f64 {
        TRACKS.get(track).copied().unwrap_or(0.0) * (self.gap() / 74.0).clamp(0.62, 1.0)
    }

    /// The day, and the minute of it, that a place across the page falls on, on a map at rest.
    fn moment(&self, x: f64) -> (usize, f64) {
        let (first, last) = (self.map.first_hour as f64 * 60.0, self.map.last_hour as f64 * 60.0);
        let across = x - self.x - X0;
        let (day, part) = if self.zoom.open > 0.5 {
            (self.zoom.day.round().clamp(0.0, 6.0), across / PLOT)
        } else {
            let day = ((across + DAY_GAP / 2.0) / (DAY_WIDTH + DAY_GAP)).floor().clamp(0.0, 6.0);
            (day, (across - day * (DAY_WIDTH + DAY_GAP)) / DAY_WIDTH)
        };
        (day as usize, first + part.clamp(0.0, 1.0) * (last - first))
    }

    /// Where a replay's playhead sets out and where it stops: from just before the first work on screen to
    /// now, or to just past the last. None where there is no work on screen to play.
    fn span(&self) -> Option<(f64, f64)> {
        let (left, right) = (self.x + X0 - 8.0, self.x + X1 + 8.0);
        let ends: Vec<(f64, f64)> = self.map.tasks.iter().map(|task| (self.at(task.day, task.start), self.at(task.day, task.waiting.unwrap_or(task.end)))).filter(|(from, reach)| *reach > left && *from < right).collect();
        let first = ends.iter().map(|ends| ends.0).reduce(f64::min)?;
        let last = ends.iter().map(|ends| ends.1).fold(first, f64::max);
        let now = self.map.now.map(|(day, minute)| self.at(day, minute)).filter(|now| *now > first && *now < right);
        Some(((first - 28.0).max(left), now.unwrap_or((last + 28.0).min(right))))
    }
}

/// Words over a piece of the map's work.
struct Name<'a> {
    text: &'a str,
    left: f64,
    top: f64,
    width: f64,
    size: f64,
    weight: Weight,
    ink: Paint,
    align: NSTextAlignment,
}

/// Draws a name over the map's work. While the map replays it is kept instead, to be drawn whole rather
/// than cut off at the playhead as the work is, and only once the replay's `wake` is past it: the character
/// rides the playhead at the names' height, and leaves each name behind it instead of passing over it.
fn say<'a>(pen: &Pen, late: &mut Vec<Name<'a>>, wake: Option<f64>, name: Name<'a>) {
    let Some(wake) = wake else {
        label(pen, name.text, name.left, name.top, name.width, name.size, name.weight, name.ink, name.align);
        return;
    };
    // Clear of the character's circle, and then in over a little way.
    let past = wake - (name.left + name.width + 22.0);
    if past > 0.0 {
        late.push(Name { ink: Paint { alpha: name.ink.alpha * (past / 22.0).min(1.0), ..name.ink }, ..name });
    }
}

/// A pull request's number, under the ring that ends its work.
fn pull_tag(pen: &Pen, centre: f64, top: f64, tag: &str) {
    let width = text_width(tag, 11.0, Weight::Bold) + 13.0;
    let frame = rect(centre - width / 2.0, top, width, 16.0);
    pen.round(frame, 8.0, Some(theme::INK.alpha(1.0)), None);
    pen.text_in(tag, rect(frame.origin.x, frame.origin.y + 1.0, width, 14.0), 11.0, Weight::Bold, theme::WHITE.alpha(1.0), NSTextAlignment::Center, false);
}

/// How high the character rides at `x` as a replay passes the work: level with the piece it came to last,
/// easing over to the next as it gets there. `stops` are where each piece begins and how high it sits, in order.
fn ride(stops: &[(f64, f64)], x: f64, rest: f64) -> f64 {
    const REACH: f64 = 34.0;
    let ease = |part: f64| {
        let part = part.clamp(0.0, 1.0);
        part * part * (3.0 - 2.0 * part)
    };
    // It looks ahead, so that it is level with a piece of work by the time that work begins: at the replay's
    // end it is then over the work still running, where it rests.
    let x = x + REACH;
    // It sets out level with the first piece of work.
    let mut from = stops.first().map_or(rest, |stop| stop.1);
    let mut toward: Option<(f64, f64)> = None;
    for (at, level) in stops {
        if *at > x {
            break;
        }
        if let Some((began, to)) = toward {
            from += (to - from) * ease((at - began) / REACH);
        }
        toward = Some((*at, *level));
    }
    match toward {
        Some((began, to)) => from + (to - from) * ease((x - began) / REACH),
        None => from,
    }
}

fn line_colour(colour: Option<usize>) -> Rgb {
    colour.map_or(OTHER_LINE, |slot| LINE_COLOURS[slot % COLOURED])
}

/// A straight stroke with round ends; dotted when `dots` gives the gap between its dots.
fn stroke_line(pen: &Pen, from: (f64, f64), to: (f64, f64), width: f64, ink: Paint, dots: Option<f64>) {
    if !pen.draw {
        return;
    }
    // A stroke of no length still draws its round ends, as a dot.
    let to = if from == to { (to.0 + 0.01, to.1) } else { to };
    let path = NSBezierPath::bezierPath();
    path.moveToPoint(NSPoint::new(from.0, from.1));
    path.lineToPoint(NSPoint::new(to.0, to.1));
    path.setLineWidth(width);
    path.setLineCapStyle(NSLineCapStyle::Round);
    if let Some(gap) = dots {
        let pattern: [f64; 2] = [0.1, gap];
        unsafe { path.setLineDash_count_phase(pattern.as_ptr() as *mut f64, 2, 0.0) };
    }
    paint(ink).setStroke();
    path.stroke();
}

fn disc(pen: &Pen, x: f64, y: f64, radius: f64, fill: Option<Paint>, edge: Option<(Paint, f64)>) {
    pen.round(rect(x - radius, y - radius, radius * 2.0, radius * 2.0), radius, fill, edge);
}

/// A thin dashed rule across, as a list is parted.
fn dashed(pen: &Pen, from: f64, to: f64, y: f64, ink: Paint) {
    if !pen.draw || to <= from {
        return;
    }
    let path = NSBezierPath::bezierPath();
    path.moveToPoint(NSPoint::new(from, y));
    path.lineToPoint(NSPoint::new(to, y));
    path.setLineWidth(1.0);
    let pattern: [f64; 2] = [4.0, 3.0];
    unsafe { path.setLineDash_count_phase(pattern.as_ptr() as *mut f64, 2, 0.0) };
    paint(ink).setStroke();
    path.stroke();
}

/// The character's face in its soft circle.
fn face(pen: &Pen, screen: &Screen, frame: NSRect) {
    pen.round(frame, frame.size.width / 2.0, Some(theme::HEALTHY.alpha(0.16)), None);
    if pen.draw {
        kit::draw_avatar(&Avatar::named(&screen.avatar), frame);
    }
}

/// Words over the map, on a patch of the band's colour so a line behind them does not run through them.
#[allow(clippy::too_many_arguments)]
fn label(pen: &Pen, text: &str, left: f64, top: f64, width: f64, size: f64, weight: Weight, ink: Paint, align: NSTextAlignment) {
    pen.round(rect(left - 4.0, top - 1.0, width + 8.0, 17.0), 5.0, Some(BAND.alpha(0.92 * ink.alpha)), None);
    pen.text_in(text, rect(left, top, width, 16.0), size, weight, ink, align, false);
}

/// A small pill button with an optional icon, its right edge at `right`; returns its width.
fn pill(pen: &mut Pen, right: f64, y: f64, icon: Option<&str>, words: &str, dark: bool, hit: Hit) -> f64 {
    let words_width = text_width(words, 12.0, Weight::Semibold);
    let width = words_width + if icon.is_some() { 43.0 } else { 26.0 };
    let frame = rect(right - width, y, width, 28.0);
    let (fill, ink) = if dark { (theme::INK.alpha(1.0), theme::WHITE) } else { (theme::INK.alpha(0.07), theme::INK) };
    pen.round(frame, 14.0, Some(fill), None);
    let mut cursor = frame.origin.x + 13.0;
    if let Some(icon) = icon {
        pen.icon(icon, rect(cursor, y + 7.5, 13.0, 13.0), ink);
        cursor += 19.0;
    }
    pen.text(words, cursor, y + 6.0, 12.0, Weight::Semibold, ink.alpha(1.0));
    pen.hit(frame, hit);
    width
}

/// The button that plays the map again from its start; while it plays, the same button stops it.
fn replay_button(pen: &mut Pen, x: f64, y: f64, playing: bool, words: &Words) {
    let name = if playing { words.stop } else { words.replay };
    let frame = rect(x, y, text_width(name, 12.0, Weight::Semibold) + 38.0, 26.0);
    let ink = if playing { theme::WHITE } else { theme::INK };
    pen.round(frame, 13.0, Some(theme::INK.alpha(if playing { 1.0 } else { 0.07 })), None);
    let (mark, middle) = (x + 12.0, y + 13.0);
    if playing {
        pen.round(rect(mark, middle - 4.0, 8.0, 8.0), 2.0, Some(ink.alpha(1.0)), None);
    } else if pen.draw {
        // A triangle pointing on, its corners eased.
        let path = NSBezierPath::bezierPath();
        path.moveToPoint(NSPoint::new(mark + 0.75, middle - 4.25));
        path.lineToPoint(NSPoint::new(mark + 8.0, middle));
        path.lineToPoint(NSPoint::new(mark + 0.75, middle + 4.25));
        path.closePath();
        path.setLineJoinStyle(NSLineJoinStyle::Round);
        path.setLineWidth(1.5);
        paint(ink.alpha(1.0)).setFill();
        paint(ink.alpha(1.0)).setStroke();
        path.fill();
        path.stroke();
    }
    pen.text(name, x + 26.0, y + 5.0, 12.0, Weight::Semibold, ink.alpha(1.0));
    pen.hit(frame, Hit::Replay);
}

/// The date line: the big number, the two lines beside it, the way back and forward, and the period's counts.
fn masthead(pen: &mut Pen, masthead: &Masthead, screen: &Screen, look: &Look) {
    let big = kit::font(104.0, Weight::Bold);
    let baseline = 100.0;
    let width = big_number(pen, &masthead.big, PAD, baseline - big.ascender());
    let x = PAD + width + 14.0;
    let top_width = pen.text(&masthead.top, x, 56.0, 22.0, Weight::Semibold, theme::INK.alpha(1.0));
    let bottom_width = pen.text(&masthead.bottom, x, 86.0, 12.0, Weight::Regular, SOFT.alpha(1.0));
    let mut x = x + top_width.max(bottom_width) + 18.0;
    for (icon, step, enabled) in [("chevron-left", -1, true), ("chevron-right", 1, masthead.can_go_forward)] {
        let frame = rect(x, 78.0, 26.0, 26.0);
        pen.round(frame, 13.0, Some(theme::INK.alpha(if enabled { 0.07 } else { 0.04 })), None);
        pen.icon(icon, kit::inset(frame, 6.0, 6.0), if enabled { theme::INK } else { FAINT });
        if enabled {
            pen.hit(frame, Hit::Step(step));
        }
        x += 30.0;
    }
    // The way into the week's recap cards.
    if let Body::Week(view) = &screen.body
        && let Some(recap) = &view.recap
    {
        let words = recap.title.replace('\n', " ");
        let words_width = text_width(&words, 12.0, Weight::Semibold);
        let frame = rect(x + 8.0, 78.0, words_width + 38.0, 26.0);
        pen.round(frame, 13.0, Some(INDIGO.alpha(1.0)), None);
        pen.text(&words, frame.origin.x + 13.0, 83.0, 12.0, Weight::Semibold, theme::WHITE.alpha(1.0));
        pen.icon("chevron-right", rect(frame.origin.x + words_width + 17.0, 84.5, 13.0, 13.0), LAVENDER);
        pen.hit(frame, Hit::Recap(Some(0)));
    }
    if !masthead.stats.is_empty() {
        // For part of the map the counts are that part's: what a replay has passed, or the stretch dragged over.
        let part = look.inside.as_ref().zip(screen.map.weeks.first()).map(|(inside, map)| map.stats_of(screen.map.day, &|task| inside.get(task).copied().unwrap_or(true), screen.language));
        let shown: Vec<Stat> = match part {
            // A replay counts up to what the line said, under the names it had there.
            Some(part) if look.playhead.is_some() && part.len() == masthead.stats.len() => part.into_iter().zip(&masthead.stats).map(|(part, whole)| Stat { label: whole.label.clone(), ..part }).collect(),
            Some(part) => part,
            None => masthead.stats.clone(),
        };
        let left = stats(pen, &shown, PAD + WIDTH, 100.0);
        if look.ui.select.is_some() && look.playhead.is_none() {
            // Whose counts these are, and the way back to the whole.
            pill(pen, left - 22.0, 77.0, Some("x"), screen.words.selected, true, Hit::Unselect);
        }
        return;
    }
    let mut right = PAD + WIDTH;
    for pill in masthead.pills.iter().rev() {
        let (fill, ink, icon_ink) = pill.callout.map(chip_colors).unwrap_or((theme::MUTED.alpha(0.12), SECOND, SOFT));
        let width = Pen::measuring().capsule(0.0, 0.0, 24.0, Some(pill.icon), &pill.text, 12.0, fill, ink, icon_ink);
        right -= width;
        pen.capsule(right, 79.0, 24.0, Some(pill.icon), &pill.text, 12.0, fill, ink, icon_ink);
        right -= 6.0;
    }
}

/// The map in its card: the week's lines, or one of its days opened across the card.
fn hero(pen: &mut Pen, screen: &Screen, map: &MapView, zoom: Zoom, look: &Look) {
    let (x, y) = (PAD, HERO_TOP);
    let plot = Plot { map, zoom, x, y };
    let open = zoom.open;
    // Between the week and a day the two sets of words do not show at once: the week's go first, then the
    // day's come, with only the work moving in between.
    let (week_words, day_words) = ((1.0 - open / 0.35).clamp(0.0, 1.0), ((open - 0.65) / 0.35).clamp(0.0, 1.0));
    let playhead = look.playhead;
    // The stretch dragged over says when it begins and when it ends under its two edges, on the line the
    // hours are written on, as a chart's cursor does; the top of the map is where the work's names are.
    // Each is its words, where it begins across the page, and how wide it is.
    let select = look.ui.select.filter(|_| playhead.is_none());
    let times: Vec<(String, f64, f64)> = match (select, look.range) {
        (Some((from, to)), Some((first, last))) => {
            let clock = |moment: (usize, i64)| format!("{:02}:{:02}", moment.1 / 60, moment.1 % 60);
            let time = |text: String, centre: f64| {
                let width = text_width(&text, 11.0, Weight::Bold) + 14.0;
                (text, (centre - width / 2.0).clamp(x + X0 - 12.0, x + X1 + 12.0 - width), width)
            };
            // Too close for two: one, from when to when.
            if to - from < 104.0 { vec![time(format!("{} – {}", clock(first), clock(last)), (from + to) / 2.0)] } else { vec![time(clock(first), from), time(clock(last), to)] }
        }
        _ => Vec::new(),
    };
    let thick = 10.0 + 4.0 * open;
    let (first, last) = (map.first_hour as f64 * 60.0, map.last_hour as f64 * 60.0);
    let (clip_left, clip_right) = (x + X0 - 12.0, x + X1 + 12.0);
    if pen.draw {
        NSGraphicsContext::saveGraphicsState_class();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect(clip_left, y, clip_right - clip_left, HERO_HEIGHT), 0.0, 0.0).addClip();
    }
    // The days: a band each, its name above, and under it the day's time or, opened, the hours.
    let mut bands: Vec<NSRect> = Vec::new();
    for (index, day) in map.days.iter().enumerate() {
        let (left, right) = (plot.at(index, first) - 7.0, plot.at(index, last) + 7.0);
        if right < clip_left - 30.0 || left > clip_right + 30.0 {
            continue;
        }
        bands.push(rect(left, y + BAND_TOP, right - left, BAND_BOTTOM - BAND_TOP));
        pen.round(rect(left, y + BAND_TOP, right - left, BAND_BOTTOM - BAND_TOP), 12.0, Some(BAND.alpha(if day.future { 0.5 } else { 1.0 })), None);
        let ink = if day.today { theme::INK } else if day.future { FAINT } else { SOFT };
        pen.text_in(&format!("{} {}", day.label, day.number), rect(left, y + 14.0, right - left, 16.0), 12.0, if day.today { Weight::Bold } else { Weight::Semibold }, ink.alpha(1.0), NSTextAlignment::Center, false);
        let middle = (left + right) / 2.0;
        if !day.future && middle > clip_left && middle < clip_right {
            // A press opens the day; on the opened day, it goes back out to the week.
            let hit = if open > 0.5 { Hit::Show(JournalTab::Week, map.back) } else { Hit::Show(JournalTab::Day, day.back) };
            pen.hit(rect(middle - 34.0, y + 8.0, 68.0, 28.0), hit);
        }
        // The days' totals leave their line to a stretch's times.
        if !day.total.is_empty() && week_words > 0.0 && times.is_empty() {
            pen.text_in(&day.total, rect(left, y + 277.0, right - left, 15.0), 11.0, Weight::Medium, SOFT.alpha(week_words), NSTextAlignment::Center, false);
        }
        if open > 0.02 {
            for hour in map.first_hour..=map.last_hour {
                let at = plot.at(index, hour as f64 * 60.0);
                pen.fill(rect(at - 0.5, y + BAND_TOP + 10.0, 1.0, BAND_BOTTOM - BAND_TOP - 20.0), rgb(0.902, 0.914, 0.925).alpha(open));
                // Nor is an hour written where one of those times sits.
                if day_words <= 0.0 || times.iter().any(|(_, left, width)| at > left - 14.0 && at < left + width + 14.0) {
                    continue;
                }
                pen.text_in(&format!("{hour:02}"), rect(at - 20.0, y + 277.0, 40.0, 15.0), 11.0, Weight::Regular, SOFT.alpha(day_words), NSTextAlignment::Center, false);
            }
        }
    }
    // The lines: full colour up to now, faint for what is still to come.
    let now_x = map.now.map(|(day, minute)| plot.at(day, minute));
    let (rail_from, rail_to) = (plot.at(0, first) - 7.0, plot.at(6, last) + 7.0);
    for (index, lane) in map.lanes.iter().enumerate() {
        let (lane_y, colour) = (plot.lane(index), line_colour(lane.colour));
        // A replay's lines are in full colour only as far as it has got.
        let turn = match (look.playhead, now_x) {
            (Some(playhead), _) => playhead.clamp(rail_from, rail_to),
            (None, Some(now)) => now.clamp(rail_from, rail_to),
            (None, None) if map.back < 0 => rail_from,
            (None, None) => rail_to,
        };
        if turn > rail_from {
            stroke_line(pen, (rail_from, lane_y), (turn, lane_y), 3.0, colour.alpha(0.5), None);
        }
        if rail_to > turn {
            stroke_line(pen, (turn, lane_y), (rail_to, lane_y), 3.0, colour.alpha(0.16), None);
        }
    }
    // On the week, one name for each line and day: the work still live, or else the longest.
    let mut named: HashMap<(usize, usize), usize> = HashMap::new();
    let weight = |task: &MapTask| (matches!(task.state, MapState::Running | MapState::Asking), task.minutes);
    for (index, task) in map.tasks.iter().enumerate() {
        let slot = named.entry((task.lane, task.day)).or_insert(index);
        if weight(task) > weight(&map.tasks[*slot]) {
            *slot = index;
        }
    }
    let mut pet: Option<(f64, f64)> = None;
    // Where work still open stops, to run on from there to next week.
    let mut open_ends: Vec<(f64, f64, Rgb)> = Vec::new();
    // While the map replays: the names and the pull requests' numbers, which come whole over the playhead's cut.
    let mut names: Vec<Name> = Vec::new();
    let mut pulls: Vec<(f64, f64, String)> = Vec::new();
    // With the pointer on a task, or a stretch of the map dragged over, the map pales under a veil and that
    // task, or the work in that stretch, is drawn again over it.
    let hovered = look.ui.hover.and_then(|(week, task)| screen.map.weeks.get(week).and_then(|map| map.tasks.get(task))).map(|task| task.key.as_str());
    let mut order: Vec<Option<(usize, bool)>> = (0..map.tasks.len()).map(|index| Some((index, false))).collect();
    if hovered.is_some() || select.is_some() {
        order.push(None);
        order.extend((0..map.tasks.len()).filter(|index| hovered.map_or_else(|| look.holds(0, *index), |key| map.tasks[*index].key == key)).map(|index| Some((index, true))));
    }
    // A replay shows the work only as far as it has got.
    if let (true, Some(playhead)) = (pen.draw, playhead) {
        NSGraphicsContext::saveGraphicsState_class();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect(clip_left, y, (playhead - clip_left).max(0.0), HERO_HEIGHT), 0.0, 0.0).addClip();
    }
    // Where each piece of work on screen begins and how high it sits, for the character to ride past.
    let mut stops: Vec<(f64, f64)> = Vec::new();
    for step in order {
        let Some((index, again)) = step else {
            // The veil is the bands' own colour, band by band, so the days stay where they are.
            for band in &bands {
                pen.round(*band, 12.0, Some(BAND.alpha(0.8)), None);
            }
            // Between the days the lines pale too.
            for pair in bands.windows(2) {
                let from = pair[0].origin.x + pair[0].size.width;
                pen.fill(rect(from, y + BAND_TOP, pair[1].origin.x - from, BAND_BOTTOM - BAND_TOP), theme::WHITE.alpha(0.8));
            }
            if let Some((from, to)) = select {
                pen.round(rect(from, y + BAND_TOP, to - from, BAND_BOTTOM - BAND_TOP), 10.0, Some(theme::INK.alpha(0.045)), Some((theme::INK.alpha(0.3), 1.0)));
            }
            continue;
        };
        let task = &map.tasks[index];
        let lane_y = plot.lane(task.lane);
        let offset = plot.track(task.track);
        let ty = lane_y + offset;
        let (from, to) = (plot.at(task.day, task.start), plot.at(task.day, task.end));
        let reach = plot.at(task.day, task.waiting.unwrap_or(task.end));
        let on_card = reach >= clip_left - 90.0 && from <= clip_right + 90.0;
        if on_card && !again {
            stops.push((from, ty));
        }
        // Off the card, or still ahead of a replay.
        if !on_card || playhead.is_some_and(|playhead| from - 16.0 > playhead) {
            continue;
        }
        let colour = line_colour(map.lanes[task.lane].colour);
        let live = map.now.is_some() && matches!(task.state, MapState::Running | MapState::Asking);
        // A track beside the line leaves it just before the work and joins it again just after.
        if offset != 0.0 && pen.draw {
            let path = NSBezierPath::bezierPath();
            path.moveToPoint(NSPoint::new(from - 15.0, lane_y));
            path.curveToPoint_controlPoint1_controlPoint2(NSPoint::new(from, ty), NSPoint::new(from - 6.0, lane_y), NSPoint::new(from - 9.0, ty));
            path.lineToPoint(NSPoint::new(reach, ty));
            path.curveToPoint_controlPoint1_controlPoint2(NSPoint::new(reach + 15.0, lane_y), NSPoint::new(reach + 9.0, ty), NSPoint::new(reach + 6.0, lane_y));
            path.setLineWidth(3.0);
            path.setLineCapStyle(NSLineCapStyle::Round);
            paint(colour.alpha(0.5)).setStroke();
            path.stroke();
        }
        if live && task.state == MapState::Running {
            disc(pen, reach, ty, thick / 2.0 + 5.0, Some(colour.alpha(0.2)), None);
        }
        // The band's colour under the work keeps it clear of the line it sits on. Thick is the agent working;
        // the paler stretches between are the person's turn.
        stroke_line(pen, (from, ty), (to, ty), thick + 4.0, BAND.alpha(1.0), None);
        if task.work.len() > 1 {
            stroke_line(pen, (from, ty), (to, ty), thick, colour.alpha(0.34), None);
        }
        for (begin, end) in &task.work {
            stroke_line(pen, (plot.at(task.day, *begin), ty), (plot.at(task.day, *end), ty), thick, colour.alpha(1.0), None);
        }
        if to - from > thick * 0.9 {
            disc(pen, from, ty, thick * 0.23, Some(theme::WHITE.alpha(1.0)), None);
        }
        if let Some(number) = task.pull {
            disc(pen, to, ty, thick / 2.0 + 2.5, Some(theme::WHITE.alpha(1.0)), Some((theme::INK.alpha(1.0), 2.5)));
            disc(pen, to, ty, (thick / 2.0 - 2.0).max(1.6), Some(theme::INK.alpha(1.0)), None);
            let tag = format!("PR #{number}");
            match playhead {
                None => pull_tag(pen, to, ty + thick / 2.0 + 7.0, &tag),
                // A replay shows it once the work has ended.
                Some(playhead) if to <= playhead => pulls.push((to, ty + thick / 2.0 + 7.0, tag)),
                Some(_) => {}
            }
        }
        match task.state {
            MapState::Open => disc(pen, to, ty, thick / 2.0 + 1.5, Some(theme::WHITE.alpha(1.0)), Some((colour.alpha(1.0), 2.5))),
            MapState::Asking => {
                if reach - 12.0 > to + thick / 2.0 + 4.0 {
                    stroke_line(pen, (to + thick / 2.0 + 4.0, ty), (reach - 12.0, ty), 3.0, theme::ASKING.alpha(1.0), Some(7.0));
                }
                disc(pen, reach, ty, 8.5, Some(theme::WHITE.alpha(1.0)), Some((theme::ASKING.alpha(1.0), 2.0)));
                pen.text_in("?", rect(reach - 8.0, ty - 7.5, 16.0, 14.0), 11.0, Weight::Bold, theme::ASKING.alpha(1.0), NSTextAlignment::Center, false);
            }
            MapState::Running | MapState::Done => {}
        }
        // What a replay has not come to yet does not run on to next week, and has no character over it.
        if live && !again && playhead.is_none_or(|playhead| reach <= playhead + 0.5) {
            open_ends.push((reach + 14.0, ty, colour));
            if task.state == MapState::Running {
                pet = Some((reach, ty));
            }
        }
        let (day_left, day_right) = (plot.at(task.day, first), plot.at(task.day, last));
        let title_width = text_width(&task.title, 12.0, Weight::Semibold);
        if live && task.state == MapState::Running {
            // The character sits over the running work, so its name ends just before the character.
            let width = title_width.min(150.0).min(reach - 26.0 - day_left - 2.0);
            if width >= 30.0 {
                say(pen, &mut names, look.wake, Name { text: &task.title, left: reach - 26.0 - width, top: ty - thick / 2.0 - 22.0, width, size: 12.0, weight: Weight::Semibold, ink: theme::INK.alpha(1.0), align: NSTextAlignment::Right });
            }
        } else {
            if week_words > 0.0 && named.get(&(task.lane, task.day)) == Some(&index) {
                let width = title_width.min(DAY_WIDTH - 6.0);
                // Over the highest track in use beside it.
                let up = map.tasks.iter().filter(|other| other.lane == task.lane && other.day == task.day && (other.start - task.start).abs() < 240.0).map(|other| plot.track(other.track)).fold(0.0, f64::min);
                let left = ((from + reach) / 2.0 - width / 2.0).min(day_right - width - 2.0).max(day_left + 2.0);
                say(pen, &mut names, look.wake, Name { text: &task.title, left, top: lane_y + up - thick / 2.0 - 22.0, width, size: 12.0, weight: Weight::Semibold, ink: theme::INK.alpha(week_words), align: NSTextAlignment::Center });
            }
            if day_words > 0.0 {
                let next = map.tasks.iter().filter(|other| other.lane == task.lane && other.day == task.day && other.track == task.track && other.start > task.start).map(|other| plot.at(other.day, other.start)).fold(day_right + 8.0, f64::min);
                let room = (next - from - 8.0).min(250.0);
                if room >= 30.0 {
                    let width = title_width.min(room);
                    let top = if offset > 0.0 { ty + thick / 2.0 + 4.0 } else { ty - thick / 2.0 - 22.0 };
                    let left = from - thick / 2.0 + 1.0;
                    say(pen, &mut names, look.wake, Name { text: &task.title, left, top, width, size: 12.0, weight: Weight::Semibold, ink: theme::INK.alpha(day_words), align: NSTextAlignment::Left });
                    let time_width = text_width(&task.worked, 11.0, Weight::Regular);
                    if !task.worked.is_empty() && title_width + time_width + 12.0 < room {
                        say(pen, &mut names, look.wake, Name { text: &task.worked, left: left + width + 8.0, top, width: time_width, size: 11.0, weight: Weight::Regular, ink: SOFT.alpha(day_words), align: NSTextAlignment::Left });
                    }
                }
            }
        }
        // Work a replay has not come to is not there to press yet.
        if !again && playhead.is_none_or(|playhead| from <= playhead) {
            let frame = rect(from - 9.0, ty - 14.0, reach - from + 18.0, 28.0);
            pen.hit(frame, Hit::Open(task.key.clone()));
            pen.spots.push((frame, (0, index)));
        }
    }
    if let (true, Some(_)) = (pen.draw, playhead) {
        NSGraphicsContext::restoreGraphicsState_class();
    }
    for name in names {
        label(pen, name.text, name.left, name.top, name.width, name.size, name.weight, name.ink, name.align);
    }
    for (centre, top, tag) in pulls {
        pull_tag(pen, centre, top, &tag);
    }
    if let Some(playhead) = playhead {
        // The character rides the playhead, level with the work it is passing.
        pen.fill(rect(playhead - 0.6, y + BAND_TOP + 6.0, 1.25, BAND_BOTTOM - BAND_TOP - 12.0), theme::INK.alpha(0.5));
        stops.sort_by(|a, b| a.0.total_cmp(&b.0));
        pet = Some((playhead, ride(&stops, playhead, y + 158.0)));
    } else if let Some(now) = now_x {
        pen.fill(rect(now - 0.6, y + BAND_TOP + 6.0, 1.25, BAND_BOTTOM - BAND_TOP - 12.0), theme::INK.alpha(0.5));
        let width = text_width(&map.now_text, 11.0, Weight::Bold);
        let count = map.lanes.len();
        let top = if count > 1 && plot.lane(count - 1) > y + BAND_BOTTOM - 38.0 { (plot.lane(count - 1) + plot.lane(count - 2)) / 2.0 - 8.0 } else { y + BAND_BOTTOM - 26.0 };
        label(pen, &map.now_text, now - 8.0 - width, top, width, 11.0, Weight::Bold, theme::INK.alpha(1.0), NSTextAlignment::Left);
    }
    if pen.draw {
        NSGraphicsContext::restoreGraphicsState_class();
    }
    if map.tasks.is_empty() {
        pen.text_in(&map.empty, rect(x + X0, y + 150.0, PLOT, 18.0), 14.0, Weight::Regular, SOFT.alpha(1.0), NSTextAlignment::Center, false);
    }
    for (text, left, width) in &times {
        pen.round(rect(*left, y + 275.0, *width, 18.0), 9.0, Some(theme::INK.alpha(1.0)), None);
        pen.text_in(text, rect(*left, y + 277.0, *width, 14.0), 11.0, Weight::Bold, theme::WHITE.alpha(1.0), NSTextAlignment::Center, false);
    }
    // Work still open runs on, dotted, to where the next week begins.
    if map.carried > 0 && open < 0.98 {
        let fade = 1.0 - open;
        let middle = y + 158.0;
        // As tall as the work still live, whether or not a replay has come to it yet.
        let levels: Vec<f64> = map.tasks.iter().filter(|task| map.now.is_some() && matches!(task.state, MapState::Running | MapState::Asking)).map(|task| plot.lane(task.lane) + plot.track(task.track)).collect();
        let (top, bottom) = (levels.iter().copied().fold(middle, f64::min) - 22.0, levels.iter().copied().fold(middle, f64::max) + 22.0);
        let left = x + X1 + 22.0;
        for (from, end_y, colour) in &open_ends {
            if left - 6.0 > *from {
                stroke_line(pen, (*from, *end_y), (left - 6.0, *end_y), 3.0, colour.alpha(0.85 * fade), Some(7.0));
            }
        }
        let box_width = (text_width(&map.carried_words.0, 11.0, Weight::Bold) + 18.0).clamp(58.0, 72.0);
        pen.round(rect(left, top, box_width, bottom - top), 12.0, Some(GROUND.alpha(fade)), None);
        let centre = (top + bottom) / 2.0;
        pen.text_in(&map.carried_words.0, rect(left, centre - 15.0, box_width, 15.0), 11.0, Weight::Bold, theme::INK.alpha(fade), NSTextAlignment::Center, false);
        pen.text_in(&map.carried_words.1, rect(left, centre + 1.0, box_width, 15.0), 11.0, Weight::Regular, SOFT.alpha(fade), NSTextAlignment::Center, false);
    }
    if plot.span().is_some() {
        replay_button(pen, x + 16.0, y + 10.0, playhead.is_some(), &screen.words);
    }
    // Each line's name at its start, with its week or, on an opened day, that day.
    if let Some(note) = &map.split {
        // Over the first line, or under the last where the button over the lines is in the way.
        let top = plot.lane(0) - 42.0;
        pen.text(note, x + 20.0, if top < y + 42.0 { y + 279.0 } else { top }, 11.0, Weight::Regular, SOFT.alpha(1.0));
    }
    let day_notes = if open > 0.5 { map.days.get(zoom.day.round() as usize).map(|day| &day.notes) } else { None };
    for (index, lane) in map.lanes.iter().enumerate() {
        let lane_y = plot.lane(index);
        pen.round(rect(x + 20.0, lane_y - 2.0, 14.0, 4.0), 2.0, Some(line_colour(lane.colour).alpha(1.0)), None);
        pen.text_middle(&lane.name, rect(x + 42.0, lane_y - 9.0, 100.0, 16.0), 12.0, Weight::Semibold, theme::INK.alpha(1.0));
        let note = day_notes.and_then(|notes| notes.get(index)).unwrap_or(&lane.note);
        pen.text_in(note, rect(x + 42.0, lane_y + 7.0, 104.0, 15.0), 11.0, Weight::Regular, SOFT.alpha(1.0), NSTextAlignment::Left, false);
    }
    // The character sits where the work is going on now.
    if let Some((pet_x, pet_y)) = pet
        && pet_x > clip_left - 20.0
        && pet_x < clip_right + 20.0
    {
        disc(pen, pet_x, pet_y - 27.0, 19.5, Some(theme::WHITE.alpha(1.0)), Some((theme::INK.alpha(0.14), 1.0)));
        face(pen, screen, rect(pet_x - 17.0, pet_y - 44.0, 34.0, 34.0));
    }
}

/// A month as its weeks, each a small map; a press opens the week.
fn month_hero(pen: &mut Pen, weeks: &[MapView]) {
    let (x, y) = (PAD, HERO_TOP);
    let Some(head) = weeks.first() else { return };
    for (index, day) in head.days.iter().enumerate() {
        pen.text_in(&day.label, rect(x + X0 + index as f64 * (DAY_WIDTH + DAY_GAP), y + 14.0, DAY_WIDTH, 16.0), 12.0, Weight::Semibold, SOFT.alpha(1.0), NSTextAlignment::Center, false);
    }
    let row = (232.0 / weeks.len() as f64).min(56.0);
    for (index, map) in weeks.iter().enumerate() {
        let (top, height) = (y + 42.0 + index as f64 * row, row - 8.0);
        let middle = top + height / 2.0;
        let mut name_x = x + 20.0;
        if map.back == 0 {
            disc(pen, x + 24.0, middle - if map.tally.is_empty() { 0.0 } else { 8.0 }, 3.0, Some(theme::INK.alpha(1.0)), None);
            name_x += 14.0;
        }
        pen.text(&map.range, name_x, middle - if map.tally.is_empty() { 8.0 } else { 16.0 }, 12.0, Weight::Semibold, theme::INK.alpha(1.0));
        pen.text(&map.tally, x + 20.0, middle + 1.0, 11.0, Weight::Regular, SOFT.alpha(1.0));
        let (first, last) = (map.first_hour as f64 * 60.0, map.last_hour as f64 * 60.0);
        for (day, head) in map.days.iter().enumerate() {
            pen.round(rect(x + X0 + day as f64 * (DAY_WIDTH + DAY_GAP), top, DAY_WIDTH, height), 9.0, Some(BAND.alpha(if head.future { 0.5 } else { 1.0 })), None);
        }
        let count = map.lanes.len().max(1) as f64;
        let gap = if count > 1.0 { ((height - 14.0) / (count - 1.0)).min(10.0) } else { 0.0 };
        for task in &map.tasks {
            let ty = middle + (task.lane as f64 - (count - 1.0) / 2.0) * gap;
            let at = |minute: f64| x + X0 + task.day as f64 * (DAY_WIDTH + DAY_GAP) + 7.0 + ((minute - first) / (last - first)).clamp(0.0, 1.0) * (DAY_WIDTH - 14.0);
            stroke_line(pen, (at(task.start), ty), (at(task.waiting.unwrap_or(task.end)), ty), 5.0, line_colour(map.lanes[task.lane].colour).alpha(1.0), None);
        }
        if map.back >= 0 {
            pen.hit(rect(x + 12.0, top - 4.0, WIDTH - 24.0, height + 8.0), Hit::Show(JournalTab::Week, map.back));
        }
    }
}

/// A left-over item: its state's mark, its title, then why. `still` draws running work's mark at rest, for a
/// row that is paled and so not to draw the eye.
fn left_line(pen: &mut Pen, line: &LeftLine, x: f64, y: f64, width: f64, still: bool) {
    let ink = match line.open {
        Open::Asking => {
            pen.icon("message-circle-question-mark", rect(x, y + 5.5, 16.0, 16.0), theme::ASKING);
            theme::ASKING
        }
        Open::Running => {
            let frame = rect(x, y + 5.5, 16.0, 16.0);
            if still { pen.icon(TURNING, frame, theme::RUNNING_BRIGHT) } else { pen.spinner(frame, theme::RUNNING_BRIGHT) }
            theme::INK
        }
        Open::Todo => {
            pen.round(rect(x + 2.0, y + 7.5, 12.0, 12.0), 4.0, None, Some((SOFT.alpha(1.0), 1.75)));
            theme::INK
        }
    };
    let title_width = text_width(&line.title, 14.0, Weight::Bold).min(width * 0.5);
    pen.text_in(&line.title, rect(x + 25.0, y + 4.0, title_width, 18.0), 14.0, Weight::Bold, ink.alpha(1.0), NSTextAlignment::Left, false);
    pen.text_in(&format!(" · {}", line.text), rect(x + 25.0 + title_width, y + 4.0, width - 25.0 - title_width, 18.0), 14.0, Weight::Regular, theme::INK.alpha(1.0), NSTextAlignment::Left, false);
    if let Some(key) = &line.key {
        pen.hit(rect(x - 6.0, y, width + 12.0, ROW), Hit::Open(key.clone()));
    }
}

/// One of the week's days, or the whole week, as a tab over the summary; returns its width.
fn tab(pen: &mut Pen, x: f64, y: f64, name: &str, on: bool, hit: Hit) -> f64 {
    let width = (text_width(name, 12.0, Weight::Semibold) + 16.0).max(28.0);
    let frame = rect(x, y, width, 22.0);
    if on {
        pen.round(frame, 11.0, Some(theme::INK.alpha(1.0)), None);
    }
    pen.text_in(name, rect(x, y + 3.0, width, 16.0), 12.0, Weight::Semibold, if on { theme::WHITE.alpha(1.0) } else { SOFT.alpha(1.0) }, NSTextAlignment::Center, false);
    pen.hit(frame, hit);
    width
}

/// Tasks as a list in a card: each with its line's colour, its name and when it ran, and how many more
/// there are where they do not all fit. `days` says which day each ran, for a list that spans several.
#[allow(clippy::too_many_arguments)]
fn task_rows(pen: &mut Pen, page: &MapScreen, look: &Look, map: &MapView, tasks: &[(usize, &MapTask)], frame: NSRect, days: bool, words: &Words) {
    let (x, width) = (frame.origin.x, frame.size.width);
    let (right, bottom) = (x + width, frame.origin.y + frame.size.height);
    let mut row_y = frame.origin.y;
    let room = (((bottom - row_y) / ROW).floor().max(1.0)) as usize;
    let shown = if tasks.len() > room { room - 1 } else { tasks.len() };
    for (at, task) in tasks.iter().take(shown) {
        let row = rect(x - 6.0, row_y, width + 12.0, ROW);
        row_spot(pen, page, look, row, Some(*at));
        pen.round(rect(x + 3.0, row_y + 11.5, 10.0, 4.0), 2.0, Some(line_colour(map.lanes[task.lane].colour).alpha(1.0)), None);
        let time = if days { format!("{} {}", map.days[task.day].label, task.time) } else { task.time.clone() };
        let time_width = text_width(&time, 11.0, Weight::Semibold);
        pen.text(&time, right - time_width, row_y + 6.5, 11.0, Weight::Semibold, SOFT.alpha(1.0));
        pen.text_in(&task.title, rect(x + 25.0, row_y + 4.0, right - time_width - 12.0 - (x + 25.0), 18.0), 14.0, Weight::Regular, theme::INK.alpha(1.0), NSTextAlignment::Left, false);
        pen.hit(row, Hit::Open(task.key.clone()));
        row_rest(pen, look, row, Some(*at));
        row_y += ROW;
    }
    if tasks.len() > shown {
        pen.text(&words.more_lines.replace("{}", &(tasks.len() - shown).to_string()), x + 25.0, row_y + 5.5, 12.0, Weight::Regular, SOFT.alpha(1.0));
    }
}

/// The summary card: the day on screen; on the week, the day picked from its tabs or the week whole; on the
/// month, a line for each week. With a stretch of the map dragged over, the work in that stretch.
fn summary(pen: &mut Pen, screen: &Screen, look: &Look, frame: NSRect) {
    let (words, page) = (&screen.words, &screen.map);
    let (x, top, width) = (frame.origin.x + 18.0, frame.origin.y + 14.0, frame.size.width - 36.0);
    let (right, bottom) = (x + width, frame.origin.y + frame.size.height - 12.0);
    face(pen, screen, rect(x, top + 1.0, 26.0, 26.0));
    let title_x = x + 34.0;
    let week = match &screen.body {
        Body::Week(view) => Some(view),
        _ => None,
    };
    if page.day.is_none() && week.is_none() {
        pen.text(&page.title, title_x, top + 5.0, 15.0, Weight::Bold, theme::INK.alpha(1.0));
        let mut row_y = top + 36.0;
        for map in &page.weeks {
            pen.text_in(&map.range, rect(x, row_y + 5.5, 96.0, 16.0), 12.0, Weight::Semibold, SOFT.alpha(1.0), NSTextAlignment::Left, false);
            let tally_width = text_width(&map.tally, 11.0, Weight::Semibold);
            pen.text(&map.tally, right - tally_width, row_y + 6.5, 11.0, Weight::Semibold, SOFT.alpha(1.0));
            let (text, ink) = if map.tasks.is_empty() { (&map.empty, SOFT) } else { (&map.top, theme::INK) };
            pen.text_in(text, rect(x + 104.0, row_y + 4.0, right - tally_width - 12.0 - (x + 104.0), 18.0), 14.0, Weight::Regular, ink.alpha(1.0), NSTextAlignment::Left, false);
            if map.back >= 0 {
                pen.hit(rect(x - 6.0, row_y, width + 12.0, ROW), Hit::Show(JournalTab::Week, map.back));
            }
            row_y += ROW;
        }
        return;
    }
    let Some(map) = page.weeks.first() else { return };
    // A stretch dragged over on the map: the work in it, in place of a day's summary.
    if let (Some(_), Some(inside), None) = (look.ui.select, &look.inside, look.playhead) {
        let title_width = pen.text(words.selected, title_x, top + 5.0, 15.0, Weight::Bold, theme::INK.alpha(1.0));
        let tasks: Vec<(usize, &MapTask)> = map.tasks.iter().enumerate().filter(|(index, task)| page.day.is_none_or(|day| day == task.day) && inside.get(*index).copied().unwrap_or(false)).collect();
        pen.capsule(title_x + title_width + 8.0, top + 4.0, 20.0, None, &format!("{} {}", words.tasks, tasks.len()), 11.0, GROUND.alpha(1.0), SECOND, SOFT);
        if tasks.is_empty() {
            pen.text_in(words.nothing_selected, rect(x, top + 39.0, width, 18.0), 14.0, Weight::Regular, SOFT.alpha(1.0), NSTextAlignment::Left, false);
        }
        task_rows(pen, page, look, map, &tasks, rect(x, top + 36.0, width, bottom - top - 36.0), page.day.is_none(), words);
        return;
    }
    let days: Vec<usize> = page.drafts.iter().map(|draft| draft.0).collect();
    let whole = week.is_some() && look.ui.whole_week;
    let chosen = chosen_day(page, look.ui);
    let mut lines_top = top + 36.0;
    if week.is_some() && !days.is_empty() {
        let mut tab_x = x;
        for day in &days {
            tab_x += tab(pen, tab_x, lines_top, &map.days[*day].label, !whole && chosen == Some(*day), Hit::Pick(Some(*day))) + 4.0;
        }
        tab(pen, tab_x + 6.0, lines_top, words.whole_week, whole, Hit::Pick(None));
        lines_top += 28.0;
    }
    // The week whole: a line for each day, and the report to paste.
    if let (true, Some(view)) = (whole, week) {
        pen.text(&view.title, title_x, top + 5.0, 15.0, Weight::Bold, theme::INK.alpha(1.0));
        if !view.report.is_empty() {
            pill(pen, right, top, Some("copy"), words.copy_week, true, Hit::Copy(view.report.clone()));
        }
        let mut row_y = lines_top;
        for day in &view.days {
            if row_y + ROW > bottom {
                break;
            }
            pen.text_in(&day.day, rect(x, row_y + 5.0, 22.0, 17.0), 12.0, Weight::Bold, theme::INK.alpha(1.0), NSTextAlignment::Left, false);
            let mut edge = right;
            if let Some(note) = &day.note {
                let chip = Pen::measuring().capsule(0.0, 0.0, 20.0, None, note, 11.0, GROUND.alpha(1.0), SECOND, SOFT);
                pen.capsule(edge - chip, row_y + 3.5, 20.0, None, note, 11.0, GROUND.alpha(1.0), SECOND, SOFT);
                edge -= chip + 10.0;
            }
            let rest = day.lines.len() + day.more;
            let text = match day.lines.first() {
                Some(line) if rest > 1 => format!("{} {}", line.text, words.more_lines.replace("{}", &(rest - 1).to_string())),
                Some(line) => line.text.clone(),
                None => String::new(),
            };
            pen.text_in(&text, rect(x + 30.0, row_y + 4.0, edge - x - 30.0, 18.0), 14.0, Weight::Regular, theme::INK.alpha(1.0), NSTextAlignment::Left, false);
            if let Some(index) = map.days.iter().position(|head| head.date == day.date) {
                pen.hit(rect(x - 6.0, row_y, width + 12.0, ROW), Hit::Pick(Some(index)));
            }
            row_y += ROW;
        }
        return;
    }
    let Some(day) = chosen else {
        pen.text(words.week_summary, title_x, top + 5.0, 15.0, Weight::Bold, theme::INK.alpha(1.0));
        pen.text_in(if screen.loading { words.loading } else { words.empty }, rect(x, lines_top + 2.0, width, 18.0), 14.0, Weight::Regular, SOFT.alpha(1.0), NSTextAlignment::Left, false);
        return;
    };
    let head = &map.days[day];
    let title_width = pen.text(&head.title, title_x, top + 5.0, 15.0, Weight::Bold, theme::INK.alpha(1.0));
    let Some((_, draft)) = page.drafts.iter().find(|draft| draft.0 == day) else { return };
    let card = match draft {
        Draft::Ready(card) => card,
        other => {
            // No summary yet: what stands in for it, and the day's tasks as they are.
            let (note, ink, action): (&str, Rgb, Option<&String>) = match other {
                Draft::Nothing(note) => (note, SOFT, None),
                Draft::Offer(note, button) => (note, theme::INK, Some(button)),
                Draft::Making(note) => (note, theme::RUNNING, None),
                Draft::Failed(why, button) => (why, theme::STALLED, button.as_ref()),
                Draft::Ready(_) => unreachable!(),
            };
            if let Some(action) = action {
                pill(pen, right, top, Some("refresh-cw"), action, true, Hit::Summarize(head.date));
            }
            let mut note_x = x;
            if matches!(other, Draft::Making(_)) {
                pen.spinner(rect(x, lines_top + 5.0, 13.0, 13.0), theme::RUNNING_BRIGHT);
                note_x += 19.0;
            }
            pen.text_in(note, rect(note_x, lines_top + 3.0, right - note_x, 18.0), 14.0, Weight::Regular, ink.alpha(1.0), NSTextAlignment::Left, false);
            let tasks: Vec<(usize, &MapTask)> = map.tasks.iter().enumerate().filter(|(_, task)| task.day == day).collect();
            task_rows(pen, page, look, map, &tasks, rect(x, lines_top + ROW, width, bottom - lines_top - ROW), false, words);
            return;
        }
    };
    pen.capsule(title_x + title_width + 8.0, top + 4.0, 20.0, None, &card.chip, 11.0, GROUND.alpha(1.0), SECOND, SOFT);
    let mut edge = right;
    edge -= pill(pen, edge, top, Some("copy"), words.copy_report, true, Hit::Copy(card.report.clone())) + 6.0;
    pill(pen, edge, top, Some("pencil"), words.edit, false, Hit::EditSummary(card.date));
    enum Row<'a> {
        Stale(&'a str, bool),
        Done(&'a DraftLine),
        Heading,
        Left(&'a LeftLine),
    }
    let mut rows: Vec<Row> = Vec::new();
    if let Some((note, redo)) = &card.stale {
        rows.push(Row::Stale(note, *redo));
    }
    rows.extend(card.done.iter().map(Row::Done));
    if !card.left.is_empty() {
        rows.push(Row::Heading);
        rows.extend(card.left.iter().map(Row::Left));
    }
    let height = |row: &Row| if matches!(row, Row::Heading) { 14.0 } else { ROW };
    let mut shown = rows.len();
    if rows.iter().map(height).sum::<f64>() > bottom - lines_top {
        // Too many for the card: as many as fit over a line that says how many more there are.
        let mut used = 0.0;
        shown = rows.iter().take_while(|row| {
            used += height(row);
            used + 20.0 <= bottom - lines_top
        }).count();
        if shown > 0 && matches!(rows[shown - 1], Row::Heading) {
            shown -= 1;
        }
    }
    let hidden = rows[shown..].iter().filter(|row| !matches!(row, Row::Heading)).count();
    let mut row_y = lines_top;
    for row in rows.iter().take(shown) {
        match row {
            Row::Stale(note, redo) => {
                let bar = rect(x, row_y + 1.0, width, ROW - 3.0);
                pen.round(bar, 9.0, Some(GROUND.alpha(1.0)), None);
                let frame = rect(x + 10.0, row_y + 7.0, 13.0, 13.0);
                if *redo { pen.icon("refresh-cw", frame, SOFT) } else { pen.spinner(frame, SOFT) }
                pen.text(note, x + 30.0, row_y + 5.0, 12.0, Weight::Medium, SECOND.alpha(1.0));
                if *redo {
                    let redo_width = text_width(words.redo, 12.0, Weight::Bold);
                    pen.text(words.redo, right - redo_width - 12.0, row_y + 5.0, 12.0, Weight::Bold, theme::INK.alpha(1.0));
                    pen.hit(bar, Hit::Summarize(card.date));
                }
            }
            Row::Done(line) => {
                let at = line.key.as_deref().and_then(|key| map.tasks.iter().position(|task| task.key == key && task.day == day));
                row_spot(pen, page, look, rect(x - 6.0, row_y, width + 12.0, ROW), at);
                pen.icon("circle-check", rect(x, row_y + 5.5, 16.0, 16.0), SECOND);
                let colour = line.key.as_deref().and_then(|key| map.tasks.iter().find(|task| task.key == key && task.day == day)).map_or(OTHER_LINE, |task| line_colour(map.lanes[task.lane].colour));
                pen.round(rect(x + 25.0, row_y + 11.5, 10.0, 4.0), 2.0, Some(colour.alpha(1.0)), None);
                pen.text_in(&line.project, rect(x + 41.0, row_y + 5.5, 88.0, 16.0), 12.0, Weight::Semibold, SOFT.alpha(1.0), NSTextAlignment::Left, false);
                let mark_width = text_width(&line.mark, 11.0, Weight::Semibold);
                pen.text(&line.mark, right - mark_width, row_y + 6.5, 11.0, Weight::Semibold, SOFT.alpha(1.0));
                pen.text_in(&line.text, rect(x + 137.0, row_y + 4.0, right - mark_width - 12.0 - (x + 137.0), 18.0), 14.0, Weight::Regular, theme::INK.alpha(1.0), NSTextAlignment::Left, false);
                if let Some(key) = &line.key {
                    pen.hit(rect(x - 6.0, row_y, width + 12.0, ROW), Hit::Open(key.clone()));
                }
                row_rest(pen, look, rect(x - 6.0, row_y, width + 12.0, ROW), at);
            }
            Row::Heading => {
                let words_width = text_width(words.left, 11.0, Weight::Bold);
                dashed(pen, x + words_width + 8.0, right, row_y + 8.5, rgb(0.851, 0.867, 0.882).alpha(1.0));
                pen.text(words.left, x, row_y + 1.0, 11.0, Weight::Bold, SOFT.alpha(1.0));
            }
            Row::Left(line) => {
                let at = line.key.as_deref().and_then(|key| map.tasks.iter().position(|task| task.key == key && task.day == day));
                row_spot(pen, page, look, rect(x - 6.0, row_y, width + 12.0, ROW), at);
                left_line(pen, line, x, row_y, width, at.is_some_and(|task| !look.holds(0, task)));
                row_rest(pen, look, rect(x - 6.0, row_y, width + 12.0, ROW), at);
            }
        }
        row_y += height(row);
    }
    if hidden > 0 {
        pen.text(&words.more_lines.replace("{}", &hidden.to_string()), x, row_y + 3.0, 12.0, Weight::Regular, SOFT.alpha(1.0));
    } else if row_y + 20.0 <= bottom {
        // Who wrote it and where it is kept, while there is room under the lines.
        pen.text_in(&card.footer, rect(x, bottom - 15.0, width, 15.0), 11.0, Weight::Regular, SOFT.alpha(1.0), NSTextAlignment::Left, false);
    }
}

/// The kinds of work on screen: a square for each task in its line's colour, so the count can be seen and any
/// one of them opened.
fn kinds(pen: &mut Pen, screen: &Screen, look: &Look, frame: NSRect) {
    let (words, page) = (&screen.words, &screen.map);
    let (x, top, width) = (frame.origin.x + 16.0, frame.origin.y + 14.0, frame.size.width - 32.0);
    pen.text(words.kinds, x, top + 5.0, 15.0, Weight::Bold, theme::INK.alpha(1.0));
    // A kind's tasks, gathered across every week on screen.
    let mut rows: Vec<(String, Vec<(usize, usize)>)> = Vec::new();
    for (week, map) in page.weeks.iter().enumerate() {
        for row in map.kinds(page.day, screen.language) {
            let tasks = row.tasks.iter().map(|task| (week, *task));
            match rows.iter_mut().find(|entry| entry.0 == row.name) {
                Some(entry) => entry.1.extend(tasks),
                None => rows.push((row.name.clone(), tasks.collect())),
            }
        }
    }
    rows.sort_by(|a, b| (a.0 == words.no_summary).cmp(&(b.0 == words.no_summary)).then(b.1.len().cmp(&a.1.len())));
    let total: usize = rows.iter().map(|row| row.1.len()).sum();
    // The counts are of the tasks the page speaks of just now; the squares of the rest stay where they are, paler.
    let held = |tasks: &[(usize, usize)]| tasks.iter().filter(|(week, at)| look.holds(*week, *at)).count();
    let count = format!("{} {}", words.tasks, rows.iter().map(|row| held(&row.1)).sum::<usize>());
    pen.text(&count, x + width - text_width(&count, 12.0, Weight::Semibold), top + 7.0, 12.0, Weight::Semibold, SOFT.alpha(1.0));
    if rows.is_empty() {
        pen.text_in(if screen.loading { words.loading } else { words.empty }, rect(x, top + 42.0, width, 18.0), 12.0, Weight::Regular, SOFT.alpha(1.0), NSTextAlignment::Left, false);
        return;
    }
    // Many tasks, as on a month, take smaller squares.
    let (size, gap, least) = if total > 34 { (8.0, 2.0, 22.0) } else { (13.0, 3.0, ROW) };
    let names = rows.iter().map(|row| text_width(&row.0, 12.0, Weight::Regular)).fold(48.0, f64::max).min(104.0);
    let (marks_x, marks_width) = (x + names + 14.0, width - names - 14.0 - 30.0);
    let across = (((marks_width + gap) / (size + gap)).floor().max(1.0)) as usize;
    let bottom = frame.origin.y + frame.size.height - 10.0;
    let mut row_y = top + 40.0;
    for (name, tasks) in &rows {
        let down = tasks.len().div_ceil(across);
        let height = (down as f64 * (size + gap) + 8.0).max(least);
        if row_y + height > bottom {
            break;
        }
        pen.text_in(name, rect(x, row_y + (height - 16.0) / 2.0, names + 2.0, 16.0), 12.0, Weight::Regular, theme::INK.alpha(1.0), NSTextAlignment::Left, false);
        let marks_top = row_y + (height - (down as f64 * (size + gap) - gap)) / 2.0;
        for (index, (week, at)) in tasks.iter().enumerate() {
            let map = &page.weeks[*week];
            let task = &map.tasks[*at];
            let cell = rect(marks_x + (index % across) as f64 * (size + gap), marks_top + (index / across) as f64 * (size + gap), size, size);
            let strength = if look.lit(page, *week, *at) { 1.0 } else { 0.18 };
            pen.round(cell, if size < 10.0 { 2.5 } else { 4.0 }, Some(line_colour(map.lanes[task.lane].colour).alpha(strength)), None);
            let around = kit::inset(cell, -gap / 2.0, -gap / 2.0);
            pen.hit(around, Hit::Open(task.key.clone()));
            pen.spots.push((around, (*week, *at)));
        }
        let number = held(tasks).to_string();
        pen.text(&number, x + width - text_width(&number, 12.0, Weight::Bold), row_y + (height - 16.0) / 2.0, 12.0, Weight::Bold, theme::INK.alpha(1.0));
        row_y += height;
    }
}

/// Part of a circle from one minute of the day to another, midnight at the top.
#[allow(clippy::too_many_arguments)]
fn dial_arc(pen: &Pen, centre: (f64, f64), radius: f64, from: f64, to: f64, width: f64, ink: Paint) {
    if !pen.draw {
        return;
    }
    let point = |minute: f64| {
        let angle = minute / 1440.0 * std::f64::consts::TAU;
        NSPoint::new(centre.0 + radius * angle.sin(), centre.1 - radius * angle.cos())
    };
    let path = NSBezierPath::bezierPath();
    path.moveToPoint(point(from));
    let steps = (((to - from) / 6.0).ceil() as usize).max(1);
    for step in 1..=steps {
        path.lineToPoint(point(from + (to - from) * step as f64 / steps as f64));
    }
    path.setLineWidth(width);
    path.setLineCapStyle(NSLineCapStyle::Round);
    path.setLineJoinStyle(NSLineJoinStyle::Round);
    paint(ink).setStroke();
    path.stroke();
}

/// When the work was asked for, around a 24-hour dial: a ring for each line, as on the map.
fn dial(pen: &mut Pen, screen: &Screen, look: &Look, frame: NSRect) {
    let (words, page) = (&screen.words, &screen.map);
    let (x, top) = (frame.origin.x + 16.0, frame.origin.y + 14.0);
    pen.text(words.hours, x, top + 5.0, 15.0, Weight::Bold, theme::INK.alpha(1.0));
    const RINGS: [f64; 4] = [64.0, 53.0, 42.0, 31.0];
    let centre = (x + 90.0, top + 124.0);
    let point = |minute: f64, radius: f64| {
        let angle = minute / 1440.0 * std::f64::consts::TAU;
        (centre.0 + radius * angle.sin(), centre.1 - radius * angle.cos())
    };
    let clock = clock(&page.weeks, page.day, screen.language);
    // Its words are of the tasks the page speaks of just now; the arcs of the rest stay, paler.
    let part = look.inside.as_ref().map(|_| clock_of(&page.weeks, page.day, &|week, task| look.holds(week, task), screen.language));
    let said = part.as_ref().unwrap_or(&clock);
    let ring_of = |arc: &errand_core::journal::map::ClockArc| page.weeks[arc.week].tasks[arc.task].lane.min(RINGS.len() - 1);
    let mut used = [false; 4];
    clock.arcs.iter().for_each(|arc| used[ring_of(arc)] = true);
    if !used.contains(&true) {
        used[0] = true;
    }
    for (ring, radius) in RINGS.iter().enumerate() {
        if used[ring] {
            dial_arc(pen, centre, *radius, 0.0, 1440.0, 7.0, NO_WORK.alpha(1.0));
        }
    }
    for hour in 0..24 {
        let (inner, outer) = (point(hour as f64 * 60.0, 72.0), point(hour as f64 * 60.0, if hour % 6 == 0 { 77.0 } else { 74.5 }));
        stroke_line(pen, inner, outer, 1.0, FAINT.alpha(1.0), None);
    }
    for (hour, name) in [(0.0, "0"), (6.0, "6"), (12.0, "12"), (18.0, "18")] {
        let at = point(hour * 60.0, 86.0);
        pen.text_in(name, rect(at.0 - 12.0, at.1 - 8.0, 24.0, 15.0), 11.0, Weight::Regular, SOFT.alpha(1.0), NSTextAlignment::Center, false);
    }
    // One day's work is solid; a week's or a month's days lie over each other, darker where they agree.
    let strength = if page.day.is_some() { 1.0 } else { 0.62 };
    for arc in &clock.arcs {
        let map = &page.weeks[arc.week];
        let colour = line_colour(map.lanes[map.tasks[arc.task].lane].colour);
        let lit = if look.lit(page, arc.week, arc.task) { 1.0 } else { 0.2 };
        dial_arc(pen, centre, RINGS[ring_of(arc)], arc.from, arc.to.max(arc.from + 10.0), 7.0, colour.alpha(strength * lit));
    }
    if let Some(minute) = page.weeks.iter().find_map(|map| map.now.filter(|now| page.day.is_none_or(|day| day == now.0)).map(|now| now.1)) {
        stroke_line(pen, point(minute, 22.0), point(minute, 71.0), 1.25, theme::INK.alpha(0.5), None);
    }
    if said.peak.is_empty() {
        return;
    }
    let side = centre.0 + 100.0;
    pen.text(words.peak, side, top + 78.0, 11.0, Weight::Semibold, SOFT.alpha(1.0));
    pen.text(&said.peak, side, top + 92.0, 26.0, Weight::Bold, theme::INK.alpha(1.0));
    let mut line_y = top + 134.0;
    for (name, time) in &said.halves {
        let name_width = pen.text(name, side, line_y, 12.0, Weight::Semibold, SECOND.alpha(1.0));
        pen.text(time, side + name_width + 5.0, line_y, 12.0, Weight::Regular, SECOND.alpha(1.0));
        line_y += 18.0;
    }
}

/// Where the three cards under the map are: the summary, the kinds of work, and the clock.
fn card_frames() -> [NSRect; 3] {
    let (kinds_width, dial_width) = (292.0, 300.0);
    let summary_width = WIDTH - kinds_width - dial_width - 24.0;
    [rect(PAD, ROW_TOP, summary_width, ROW_HEIGHT), rect(PAD + summary_width + 12.0, ROW_TOP, kinds_width, ROW_HEIGHT), rect(PAD + summary_width + kinds_width + 24.0, ROW_TOP, dial_width, ROW_HEIGHT)]
}

/// A part of the page that is drawn in a view of its own, so that when it moves or changes it alone is drawn
/// again: the cards' soft shadows and all the words elsewhere are what drawing the page costs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    /// The date line and its counts.
    Masthead,
    /// The map, inside its card.
    Map,
    Summary,
    Kinds,
    Dial,
}

/// How far inside its card a part's view lies, which keeps it clear of the card's round corners.
const PART_INSET: f64 = 6.0;

impl Part {
    const ALL: [Part; 5] = [Part::Masthead, Part::Map, Part::Summary, Part::Kinds, Part::Dial];

    /// Where the part's view is on the page. Each lies wholly on the bare ground or wholly inside its white
    /// card, so it can fill itself and wait on nothing under it.
    fn frame(self) -> NSRect {
        let cards = card_frames();
        match self {
            // Down to where the map's card begins to cast its shadow.
            Part::Masthead => rect(0.0, 0.0, SIZE.0, HERO_TOP - 12.0),
            Part::Map => kit::inset(rect(PAD, HERO_TOP, WIDTH, HERO_HEIGHT), PART_INSET, PART_INSET),
            Part::Summary => kit::inset(cards[0], PART_INSET, PART_INSET),
            Part::Kinds => kit::inset(cards[1], PART_INSET, PART_INSET),
            Part::Dial => kit::inset(cards[2], PART_INSET, PART_INSET),
        }
    }

    fn ground(self) -> Rgb {
        if self == Part::Masthead { GROUND } else { theme::WHITE }
    }
}

/// What of the page one pass over it takes in.
#[derive(Clone, Copy, PartialEq)]
enum Pass {
    /// All of it, as it is measured and found its way around.
    Whole,
    /// What lies under the parts and never moves: the empty cards, and the year.
    Ground,
    Part(Part),
}

/// The year as a strip of its weeks, darker where the agents worked longer; a press goes to that week.
fn year_strip(pen: &mut Pen, screen: &Screen) {
    let year = &screen.map.year;
    let (x, y) = (PAD, YEAR_TOP);
    let (cell, gap, left) = (17.0, 2.9, PAD + 58.0);
    pen.text(&year.label, x + 4.0, y + 25.0, 12.0, Weight::Bold, theme::INK.alpha(1.0));
    for (index, name) in &year.months {
        pen.text(name, left + *index as f64 * (cell + gap), y + 3.0, 11.0, Weight::Regular, SOFT.alpha(1.0));
    }
    for (index, week) in year.cells.iter().enumerate() {
        let frame = rect(left + index as f64 * (cell + gap), y + 24.0, cell, cell);
        match week.back {
            Some(back) => {
                let fill = if week.level == 0 { NO_WORK } else { HEAT[(week.level as usize - 1).min(HEAT.len() - 1)] };
                pen.round(frame, 4.5, Some(fill.alpha(1.0)), None);
                pen.hit(frame, Hit::Show(JournalTab::Week, back));
            }
            None => pen.round(frame, 4.5, None, Some((EDGE.alpha(1.0), 1.0))),
        }
    }
    // A ring round the weeks on screen.
    let shown: Vec<usize> = screen.map.weeks.iter().filter_map(|map| usize::try_from(year.current as i64 - map.back).ok()).filter(|index| *index < year.cells.len()).collect();
    if let (Some(first), Some(last)) = (shown.iter().min(), shown.iter().max()) {
        let frame = rect(left + *first as f64 * (cell + gap) - 2.5, y + 21.5, (last - first + 1) as f64 * (cell + gap) - gap + 5.0, cell + 5.0);
        pen.round(frame, 6.5, None, Some((theme::INK.alpha(1.0), 1.75)));
    }
}

/// Lays out the page, or the part of it one pass takes in: the date line, the map, the cards under it and
/// the year.
fn body(pen: &mut Pen, screen: &Screen, ui: Ui, pass: Pass) {
    let look = Look::new(screen, ui);
    let takes = |part: Part| pass == Pass::Whole || pass == Pass::Part(part);
    let cards = card_frames();
    // The map first: where a task is drawn in several places, its place on the map is the first of them.
    if takes(Part::Map) {
        match (look.zoom, screen.map.weeks.first()) {
            (Some(zoom), Some(map)) => hero(pen, screen, map, zoom, &look),
            _ => month_hero(pen, &screen.map.weeks),
        }
    }
    if takes(Part::Masthead) {
        masthead(pen, head(screen), screen, &look);
    }
    if takes(Part::Summary) {
        summary(pen, screen, &look, cards[0]);
    }
    if takes(Part::Kinds) {
        kinds(pen, screen, &look, cards[1]);
    }
    if takes(Part::Dial) {
        dial(pen, screen, &look, cards[2]);
    }
    if pass == Pass::Ground {
        for frame in [rect(PAD, HERO_TOP, WIDTH, HERO_HEIGHT), cards[0], cards[1], cards[2]] {
            pen.panel(frame, 18.0, theme::WHITE, EDGE.alpha(1.0), 1.0);
        }
    }
    if pass == Pass::Whole || pass == Pass::Ground {
        year_strip(pen, screen);
    }
}

/// How far the tip's view reaches past the tip itself, for its shadow.
const TIP_MARGIN: f64 = 26.0;

/// What the pointer rests on, beside it: the line it is on, its name, when it ran, and what it left. Drawn at
/// `at` where one is given; returns where on the page the tip belongs, beside the pointer.
fn tip(pen: &mut Pen, screen: &Screen, look: &Look, at: Option<(f64, f64)>) -> Option<NSRect> {
    let (week, index) = look.ui.hover?;
    let map = screen.map.weeks.get(week)?;
    let task = map.tasks.get(index)?;
    let words = &screen.words;
    let lane = &map.lanes[task.lane];
    let top = format!("{} · {}", lane.name, task.provider.title());
    let mut chips: Vec<(String, bool)> = Vec::new();
    match task.state {
        MapState::Asking => chips.push((Callout::Asking.word(screen.language).to_owned(), true)),
        MapState::Running => chips.push((Callout::Running.word(screen.language).to_owned(), false)),
        MapState::Open => chips.push((words.left.to_owned(), false)),
        MapState::Done => {}
    }
    if task.commits > 0 {
        chips.push((format!("{} {}", words.commits, task.commits), false));
    }
    if let Some(number) = task.pull {
        chips.push((format!("PR #{number}"), false));
    }
    let chips_width: f64 = chips.iter().map(|chip| Pen::measuring().capsule(0.0, 0.0, 20.0, None, &chip.0, 11.0, GROUND.alpha(1.0), SECOND, SOFT) + 4.0).sum();
    let width = text_width(&task.title, 14.0, Weight::Bold).min(260.0).max(text_width(&top, 11.0, Weight::Semibold).min(240.0) + 18.0).max(text_width(&task.time, 12.0, Weight::Regular)).max(chips_width) + 24.0;
    let height = if chips.is_empty() { 74.0 } else { 102.0 };
    let (pointer_x, pointer_y) = look.ui.pointer;
    let on_page = rect((pointer_x + 16.0).min(SIZE.0 - PAD - width).max(PAD), if pointer_y + 18.0 + height > SIZE.1 - TOP - 8.0 { pointer_y - height - 12.0 } else { pointer_y + 18.0 }, width, height);
    let (x, y) = at.unwrap_or((on_page.origin.x, on_page.origin.y));
    pen.panel(rect(x, y, width, height), 12.0, theme::WHITE, EDGE.alpha(1.0), 1.0);
    pen.round(rect(x + 12.0, y + 17.0, 12.0, 4.0), 2.0, Some(line_colour(lane.colour).alpha(1.0)), None);
    pen.text_middle(&top, rect(x + 30.0, y + 11.0, width - 42.0, 15.0), 11.0, Weight::Semibold, SOFT.alpha(1.0));
    pen.text_in(&task.title, rect(x + 12.0, y + 28.0, width - 24.0, 19.0), 14.0, Weight::Bold, theme::INK.alpha(1.0), NSTextAlignment::Left, false);
    pen.text_in(&task.time, rect(x + 12.0, y + 50.0, width - 24.0, 16.0), 12.0, Weight::Regular, SECOND.alpha(1.0), NSTextAlignment::Left, false);
    let mut chip_x = x + 12.0;
    for (text, asking) in &chips {
        let (fill, ink) = if *asking { (theme::ASKING.alpha(0.12), theme::ASKING) } else { (GROUND.alpha(1.0), SECOND) };
        chip_x += pen.capsule(chip_x, y + 72.0, 20.0, None, text, 11.0, fill, ink, ink) + 4.0;
    }
    Some(on_page)
}

const INDIGO: Rgb = rgb(0.169, 0.137, 0.314);
const NAVY: Rgb = rgb(0.075, 0.192, 0.361);
const PINE: Rgb = rgb(0.059, 0.239, 0.227);
const LAVENDER: Rgb = rgb(0.812, 0.780, 0.937);
const SKY: Rgb = rgb(0.612, 0.780, 1.0);
const MINT: Rgb = rgb(0.561, 0.890, 0.824);
const PALE: Rgb = rgb(0.839, 0.902, 1.0);
const PEACH: Rgb = rgb(1.0, 0.788, 0.541);

/// A recap card's size; saved images are drawn at the same size, twice as many pixels.
const CARD: (f64, f64) = (340.0, 604.0);

/// One recap card in `frame`, `CARD` in size, with its place among `total` as the bar along the top.
fn recap_card(pen: &Pen, card: &RecapCard, frame: NSRect, index: usize, total: usize, avatar: &str) {
    let (ground, accent) = match card {
        RecapCard::Cover { .. } | RecapCard::Kind { .. } | RecapCard::Carried { .. } => (INDIGO, LAVENDER),
        RecapCard::Finished { .. } | RecapCard::Longest { .. } => (NAVY, SKY),
        RecapCard::Busiest { .. } => (PINE, MINT),
    };
    let (x, y, width, height) = (frame.origin.x, frame.origin.y, frame.size.width, frame.size.height);
    let white = theme::WHITE;
    pen.round(frame, 30.0, Some(ground.alpha(1.0)), None);
    if pen.draw {
        NSGraphicsContext::saveGraphicsState_class();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(frame, 30.0, 30.0).addClip();
    }
    let (ix, iw) = (x + 28.0, width - 56.0);
    // Where the card sits among the rest.
    let gap = 5.0;
    let segment = (iw - gap * (total.max(1) - 1) as f64) / total.max(1) as f64;
    for step in 0..total {
        pen.round(rect(ix + step as f64 * (segment + gap), y + 22.0, segment, 4.0), 2.0, Some(white.alpha(if step <= index { 1.0 } else { 0.28 })), None);
    }
    let label_y = y + 56.0;
    let face = |frame: NSRect| {
        if pen.draw {
            kit::draw_character(&Avatar::named(avatar), frame);
        }
    };
    match card {
        RecapCard::Cover { label, title, span, chips } => {
            face(rect(x + 50.0, y + height - 330.0, 290.0, 290.0));
            pen.text(label, ix, label_y, 17.0, Weight::Bold, accent.alpha(1.0));
            pen.text_in(title, rect(ix, label_y + 28.0, iw, 120.0), 48.0, Weight::Bold, white.alpha(1.0), NSTextAlignment::Left, true);
            let title_height = pen.text_height(title, iw, 48.0, Weight::Bold, 2);
            pen.text(span, ix, label_y + 34.0 + title_height, 17.0, Weight::Semibold, accent.alpha(1.0));
            let mut cx = ix;
            for chip in chips {
                cx += pen.capsule(cx, y + height - 52.0, 32.0, None, chip, 14.0, rgb(0.243, 0.204, 0.439).alpha(1.0), white, white) + 8.0;
            }
        }
        RecapCard::Finished { label, count, versus, waiting, note } => {
            pen.text(label, ix, label_y, 17.0, Weight::Bold, accent.alpha(1.0));
            pen.text(&count.to_string(), ix - 6.0, label_y + 22.0, 150.0, Weight::Bold, white.alpha(1.0));
            pen.text(versus, ix, label_y + 210.0, 17.0, Weight::Semibold, PALE.alpha(1.0));
            let shown = (*count).min(32);
            let dots = shown + (*waiting).min(32 - shown);
            let rows = dots.div_ceil(8);
            let dots_top = y + height - 44.0 - rows as f64 * 34.0 - if note.is_empty() { 0.0 } else { 24.0 };
            for dot in 0..dots {
                let frame = rect(ix + (dot % 8) as f64 * 34.0, dots_top + (dot / 8) as f64 * 34.0, 24.0, 24.0);
                if dot < shown {
                    pen.round(frame, 12.0, Some(SKY.alpha(1.0)), None);
                } else {
                    pen.round(frame, 12.0, None, Some((PEACH.alpha(1.0), 2.5)));
                }
            }
            if !note.is_empty() {
                pen.text(note, ix, y + height - 56.0, 14.0, Weight::Regular, PALE.alpha(1.0));
            }
        }
        RecapCard::Kind { label, top, of, kinds } => {
            pen.text(label, ix, label_y, 17.0, Weight::Bold, accent.alpha(1.0));
            pen.text_in(top, rect(ix, label_y + 28.0, iw, 130.0), 52.0, Weight::Bold, white.alpha(1.0), NSTextAlignment::Left, true);
            let top_height = pen.text_height(top, iw, 52.0, Weight::Bold, 2);
            pen.text(of, ix, label_y + 34.0 + top_height, 22.0, Weight::Bold, accent.alpha(1.0));
            let most = kinds.iter().map(|(_, count)| *count).max().unwrap_or(1).max(1) as f64;
            let mut ky = y + height - 36.0 - kinds.len() as f64 * 44.0;
            for (rank, (name, count)) in kinds.iter().enumerate() {
                let ink = if rank == 0 { white } else { accent };
                pen.text(name, ix, ky, 15.0, Weight::Semibold, ink.alpha(1.0));
                let number = count.to_string();
                pen.text(&number, ix + iw - text_width(&number, 15.0, Weight::Semibold), ky, 15.0, Weight::Semibold, ink.alpha(1.0));
                pen.round(rect(ix, ky + 24.0, iw, 10.0), 5.0, Some(white.alpha(0.12)), None);
                pen.round(rect(ix, ky + 24.0, (iw * *count as f64 / most).max(6.0), 10.0), 5.0, Some(if rank == 0 { white.alpha(1.0) } else { rgb(0.655, 0.588, 0.890).alpha(1.0) }), None);
                ky += 44.0;
            }
        }
        RecapCard::Busiest { label, day, note, days } => {
            pen.text(label, ix, label_y, 17.0, Weight::Bold, accent.alpha(1.0));
            pen.text(day, ix, label_y + 26.0, 64.0, Weight::Bold, white.alpha(1.0));
            pen.text(note, ix, label_y + 110.0, 19.0, Weight::Bold, rgb(0.788, 0.949, 0.918).alpha(1.0));
            let count = days.len().max(1) as f64;
            let column = (iw - 14.0 * (count - 1.0)) / count;
            let floor = y + height - 56.0;
            for (index, (name, share, busiest)) in days.iter().enumerate() {
                let cx = ix + index as f64 * (column + 14.0);
                let bar_height = (share * 190.0).max(4.0);
                pen.round(rect(cx, floor - bar_height, column, bar_height), 7.0, Some(if *busiest { white.alpha(1.0) } else { MINT.alpha(0.45) }), None);
                let name_width = text_width(name, 14.0, Weight::Bold);
                pen.text(name, cx + (column - name_width) / 2.0, floor + 10.0, 14.0, Weight::Bold, if *busiest { white.alpha(1.0) } else { accent.alpha(1.0) });
            }
        }
        RecapCard::Longest { label, provider, project, title, time, span, bar } => {
            pen.text(label, ix, label_y, 17.0, Weight::Bold, accent.alpha(1.0));
            pen.app(*provider, rect(ix, label_y + 34.0, 30.0, 30.0));
            pen.text(&format!("{} · {project}", provider.title()), ix + 40.0, label_y + 40.0, 15.0, Weight::Semibold, PALE.alpha(1.0));
            pen.text_in(title, rect(ix, label_y + 78.0, iw, 110.0), 40.0, Weight::Bold, white.alpha(1.0), NSTextAlignment::Left, true);
            let floor = y + height - 44.0;
            let mut tx = ix;
            for part in time {
                let size = if part.small { 32.0 } else { 66.0 };
                let top = floor - 64.0 - kit::font(size, Weight::Bold).ascender();
                tx += pen.text(&part.text, tx, top, size, Weight::Bold, white.alpha(1.0));
            }
            if let Some((start, end)) = bar {
                pen.round(rect(ix, floor - 30.0, iw, 12.0), 6.0, Some(white.alpha(0.12)), None);
                pen.round(rect(ix + start * iw, floor - 30.0, ((end - start) * iw).max(8.0), 12.0), 6.0, Some(SKY.alpha(1.0)), None);
                pen.text("08", ix, floor - 10.0, 13.0, Weight::Regular, PALE.alpha(1.0));
                pen.text("19", ix + iw - text_width("19", 13.0, Weight::Regular), floor - 10.0, 13.0, Weight::Regular, PALE.alpha(1.0));
            }
            let span_width = text_width(span, 13.0, Weight::Regular);
            pen.text(span, ix + (iw - span_width) / 2.0, floor - 10.0, 13.0, Weight::Regular, PALE.alpha(1.0));
        }
        RecapCard::Carried { label, count, items, .. } => {
            face(rect(x + width - 150.0, y + height - 190.0, 170.0, 170.0));
            pen.text(label, ix, label_y, 17.0, Weight::Bold, accent.alpha(1.0));
            pen.text(&count.to_string(), ix - 4.0, label_y + 22.0, 120.0, Weight::Bold, white.alpha(1.0));
            let mut iy = label_y + 170.0;
            for (title, text, asking) in items.iter().take(3) {
                let block = rect(ix, iy, iw, 64.0);
                pen.round(block, 16.0, Some(white.alpha(0.12)), None);
                pen.text_in(title, rect(ix + 14.0, iy + 11.0, iw - 28.0, 20.0), 15.0, Weight::Bold, if *asking { PEACH.alpha(1.0) } else { white.alpha(1.0) }, NSTextAlignment::Left, false);
                pen.text_in(text, rect(ix + 14.0, iy + 34.0, iw - 28.0, 20.0), 14.0, Weight::Medium, white.alpha(0.85), NSTextAlignment::Left, false);
                iy += 74.0;
            }
        }
    }
    if pen.draw {
        NSGraphicsContext::restoreGraphicsState_class();
    }
}

/// The recap over the week: the window dimmed, one card at full size, and the ways to move and save.
fn recap_overlay(pen: &mut Pen, view: &RecapView, screen: &Screen) {
    pen.fill(rect(0.0, 0.0, SIZE.0, SIZE.1), rgb(0.094, 0.078, 0.188).alpha(0.9));
    pen.hit(rect(0.0, TOP, SIZE.0, SIZE.1 - TOP), Hit::Recap(None));
    let frame = rect((SIZE.0 - CARD.0) / 2.0, TOP + 18.0, CARD.0, CARD.1);
    if pen.draw {
        NSGraphicsContext::saveGraphicsState_class();
        let shadow = NSShadow::new();
        shadow.setShadowColor(Some(&paint(rgb(0.0, 0.0, 0.0).alpha(0.35))));
        shadow.setShadowBlurRadius(40.0);
        shadow.setShadowOffset(NSSize::new(0.0, -20.0));
        shadow.set();
        color(INDIGO).setFill();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(frame, 30.0, 30.0).fill();
        NSGraphicsContext::restoreGraphicsState_class();
    }
    if let Some(card) = view.cards.get(view.index) {
        recap_card(pen, card, frame, view.index, view.cards.len(), &screen.avatar);
    }
    pen.hit(frame, Hit::Recap(Some((view.index + 1) % view.cards.len().max(1))));
    let arrow = |pen: &mut Pen, x: f64, icon: &str, target: Option<usize>, bright: bool| {
        let button = rect(x, TOP + 18.0 + CARD.1 / 2.0 - 22.0, 44.0, 44.0);
        pen.round(button, 22.0, Some(if bright { theme::WHITE.alpha(1.0) } else { theme::WHITE.alpha(0.16) }), None);
        pen.icon(icon, kit::inset(button, 13.0, 13.0), if bright { NAVY } else { theme::WHITE });
        if let Some(target) = target {
            pen.hit(button, Hit::Recap(Some(target)));
        }
    };
    let last = view.cards.len().saturating_sub(1);
    arrow(pen, frame.origin.x - 68.0, "chevron-left", view.index.checked_sub(1), false);
    arrow(pen, frame.origin.x + CARD.0 + 24.0, "chevron-right", (view.index < last).then_some(view.index + 1), view.index < last);
    let close = rect(SIZE.0 - 60.0, TOP + 18.0, 36.0, 36.0);
    pen.round(close, 18.0, Some(theme::WHITE.alpha(0.16)), None);
    pen.icon("x", kit::inset(close, 10.0, 10.0), theme::WHITE);
    pen.hit(close, Hit::Recap(None));
    // Below the card: where it is among the rest, and saving.
    let by = TOP + 18.0 + CARD.1 + 14.0;
    let one = text_width(&view.save_one, 13.0, Weight::Bold) + 44.0;
    let all = text_width(&view.save_all, 13.0, Weight::Bold) + 44.0;
    let position = text_width(&view.position, 12.5, Weight::Regular);
    let mut bx = (SIZE.0 - (position + 18.0 + one + 10.0 + all)) / 2.0;
    pen.text(&view.position, bx, by + 9.0, 12.5, Weight::Regular, theme::WHITE.alpha(0.7));
    bx += position + 18.0;
    for (words, width, which, bright) in [(&view.save_one, one, Some(view.index), false), (&view.save_all, all, None, true)] {
        let button = rect(bx, by, width, 36.0);
        pen.round(button, 18.0, Some(if bright { theme::WHITE.alpha(1.0) } else { theme::WHITE.alpha(0.16) }), None);
        pen.icon("download", rect(bx + 15.0, by + 11.0, 14.0, 14.0), if bright { theme::INK } else { theme::WHITE });
        pen.text(words, bx + 35.0, by + 9.0, 13.0, Weight::Bold, if bright { theme::INK.alpha(1.0) } else { theme::WHITE.alpha(1.0) });
        pen.hit(button, Hit::SaveRecap(which));
        bx += width + 10.0;
    }
}

/// Draws one recap card, or all of them side by side, into a PNG the person places with a save panel.
fn save_recap(mtm: MainThreadMarker, screen: &Screen, which: Option<usize>) {
    let Some(view) = &screen.recap else { return };
    let cards: Vec<(usize, &RecapCard)> = match which {
        Some(index) => view.cards.get(index).map(|card| vec![(index, card)]).unwrap_or_default(),
        None => view.cards.iter().enumerate().collect(),
    };
    if cards.is_empty() {
        return;
    }
    let (margin, gap) = (32.0, 24.0);
    let size = NSSize::new(margin * 2.0 + cards.len() as f64 * CARD.0 + (cards.len() - 1) as f64 * gap, margin * 2.0 + CARD.1);
    let Some(image) = (unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(), std::ptr::null_mut(), (size.width * 2.0) as isize, (size.height * 2.0) as isize, 8, 4, true, false, NSDeviceRGBColorSpace, 0, 0,
        )
    }) else {
        return;
    };
    image.setSize(size);
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(NSGraphicsContext::graphicsContextWithBitmapImageRep(&image).as_deref());
    // Cards are drawn downwards, as in the window's flipped views.
    let flip = NSAffineTransform::transform();
    flip.translateXBy_yBy(0.0, size.height);
    flip.scaleXBy_yBy(1.0, -1.0);
    flip.concat();
    color(GROUND).setFill();
    NSBezierPath::fillRect(rect(0.0, 0.0, size.width, size.height));
    let pen = Pen::drawing();
    for (slot, (index, card)) in cards.iter().enumerate() {
        recap_card(&pen, card, rect(margin + slot as f64 * (CARD.0 + gap), margin, CARD.0, CARD.1), *index, view.cards.len(), &screen.avatar);
    }
    NSGraphicsContext::restoreGraphicsState_class();
    let Some(png) = (unsafe { image.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }) else { return };
    let bytes = png.to_vec();
    let panel = NSSavePanel::savePanel(mtm);
    let name = match which {
        Some(index) => format!("errand-recap-{}.png", index + 1),
        None => "errand-recap.png".to_owned(),
    };
    panel.setNameFieldStringValue(&ns(&name));
    let chosen = panel.clone();
    let completion = block2::RcBlock::new(move |result| {
        if result == NSModalResponseOK
            && let Some(path) = chosen.URL().and_then(|url| url.to_file_path())
        {
            let _ = std::fs::write(path, &bytes);
        }
    });
    panel.beginWithCompletionHandler(&completion);
}

/// The title bar: views in the middle, usage and copying on the right. The window's own buttons sit on the left.
fn top_bar(pen: &mut Pen, screen: &Screen) {
    let widths: Vec<f64> = screen.tabs.iter().map(|(tab, name)| text_width(name, 12.0, if *tab == screen.place.tab { Weight::Semibold } else { Weight::Medium }) + 32.0).collect();
    let total = widths.iter().sum::<f64>() + 2.0 * (widths.len() - 1) as f64 + 4.0;
    let mut x = (SIZE.0 - total) / 2.0;
    pen.round(rect(x, 12.0, total, 30.0), 15.0, Some(theme::INK.alpha(0.07)), None);
    x += 2.0;
    for ((tab, name), width) in screen.tabs.iter().zip(&widths) {
        let frame = rect(x, 14.0, *width, 26.0);
        let selected = *tab == screen.place.tab;
        if selected {
            pen.panel(frame, 13.0, theme::WHITE, theme::WHITE.alpha(0.0), 0.0);
        }
        pen.text_in(name, rect(frame.origin.x, frame.origin.y + 5.0, frame.size.width, 16.0), 12.0, if selected { Weight::Semibold } else { Weight::Medium }, if selected { theme::INK.alpha(1.0) } else { theme::MUTED.alpha(1.0) }, NSTextAlignment::Center, false);
        pen.hit(frame, Hit::Tab(*tab));
        x += width + 2.0;
    }
    let mut right = SIZE.0 - 16.0;
    if screen.place.tab != JournalTab::Report {
        let words = text_width(screen.words.copy, 12.0, Weight::Medium);
        let frame = rect(right - (words + 40.0), 13.0, words + 40.0, 26.0);
        pen.round(frame, 13.0, Some(theme::INK.alpha(0.07)), None);
        pen.icon("copy", rect(frame.origin.x + 11.0, frame.origin.y + 6.5, 13.0, 13.0), theme::INK);
        pen.text(screen.words.copy, frame.origin.x + 29.0, frame.origin.y + 5.0, 12.0, Weight::Medium, theme::INK.alpha(1.0));
        pen.hit(frame, Hit::Copy(screen.log.clone()));
        right = frame.origin.x - 16.0;
    }
    for battery in screen.usage.iter().rev() {
        let percent = text_width(&battery.percent, 11.5, Weight::Semibold);
        right -= percent;
        pen.text(&battery.percent, right, 18.0, 11.5, Weight::Semibold, theme::INK.alpha(1.0));
        right -= 5.0 + 34.0;
        let body = rect(right, 18.0, 34.0, 16.0);
        pen.round(body, 5.0, Some(rgb(0.98, 0.98, 0.98).alpha(1.0)), None);
        if let Some(level) = battery.level {
            let tint = if battery.low { theme::LOW } else { theme::HEALTHY };
            pen.round(rect(body.origin.x + 1.5, body.origin.y + 1.5, (31.0 * level).max(2.0), 13.0), 3.5, Some(tint.alpha(0.45)), None);
        }
        pen.round(body, 5.0, None, Some((rgb(0.8, 0.8, 0.8).alpha(1.0), 1.5)));
        right -= 5.0 + 16.0;
        pen.app(battery.provider, rect(right, 18.0, 16.0, 16.0));
        right -= 12.0;
    }
}

/// One task's whole record, in a panel over the dimmed window.
fn detail(pen: &mut Pen, view: &DetailView, screen: &Screen) {
    let words = &screen.words;
    pen.fill(rect(0.0, 0.0, SIZE.0, SIZE.1), theme::INK.alpha(0.16));
    let panel = detail_frame();
    if pen.draw {
        NSGraphicsContext::saveGraphicsState_class();
        let shadow = NSShadow::new();
        shadow.setShadowColor(Some(&paint(theme::INK.alpha(0.18))));
        shadow.setShadowBlurRadius(40.0);
        shadow.setShadowOffset(NSSize::new(0.0, -18.0));
        shadow.set();
        color(theme::WHITE).setFill();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(panel, 16.0, 16.0).fill();
        NSGraphicsContext::restoreGraphicsState_class();
        pen.round(panel, 16.0, None, Some((EDGE.alpha(1.0), 1.0)));
        NSGraphicsContext::saveGraphicsState_class();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(panel, 16.0, 16.0).addClip();
    }
    // Presses on the panel do nothing unless they land on its own buttons; the rest of the window closes it.
    pen.hit(rect(0.0, TOP, SIZE.0, SIZE.1 - TOP), Hit::Close);
    let (x, width) = (panel.origin.x + 20.0, panel.size.width - 40.0);
    let mut y = panel.origin.y + 18.0;
    pen.app(view.provider, rect(x, y + 2.0, 22.0, 22.0));
    pen.text_in(&view.title, rect(x + 31.0, y, width - 31.0 - 34.0, 24.0), 18.0, Weight::Semibold, theme::INK.alpha(1.0), NSTextAlignment::Left, false);
    let close = rect(x + width - 26.0, y, 26.0, 26.0);
    pen.round(close, 13.0, Some(theme::INK.alpha(0.07)), None);
    pen.icon("x", kit::inset(close, 7.0, 7.0), theme::INK);
    y += 34.0;
    let mut cursor = x;
    let mut meta: Vec<(&str, &str)> = vec![("folder", view.project.as_str())];
    if let Some(branch) = &view.branch {
        meta.push(("git-branch", branch));
    }
    meta.push(("clock", view.time.as_str()));
    for (icon, text) in meta {
        pen.icon(icon, rect(cursor, y + 2.0, 12.0, 12.0), theme::MUTED);
        cursor += 16.0 + pen.text(text, cursor + 16.0, y, 11.5, Weight::Regular, theme::MUTED.alpha(1.0)) + 12.0;
    }
    y += 26.0;
    let mut button_x = x;
    if let Some(url) = &view.url {
        let words_width = text_width(&view.open_label, 12.0, Weight::Semibold);
        let frame = rect(button_x, y, words_width + 42.0, 28.0);
        pen.round(frame, 14.0, Some(theme::INK.alpha(1.0)), None);
        pen.text(&view.open_label, frame.origin.x + 12.0, y + 6.0, 12.0, Weight::Semibold, theme::WHITE.alpha(1.0));
        pen.icon("external-link", rect(frame.origin.x + 18.0 + words_width, y + 8.0, 12.0, 12.0), theme::WHITE);
        pen.hit(frame, Hit::OpenTask(url.clone()));
        button_x += frame.size.width + 6.0;
    }
    let summary = std::iter::once(view.title.clone()).chain(view.turns.iter().filter(|turn| !turn.result.is_empty()).map(|turn| format!("- {}", turn.result))).collect::<Vec<_>>().join("\n");
    let words_width = text_width(words.copy_summary, 12.0, Weight::Medium);
    let copy = rect(button_x, y, words_width + 38.0, 28.0);
    pen.round(copy, 14.0, Some(theme::INK.alpha(0.07)), None);
    pen.icon("copy", rect(copy.origin.x + 11.0, y + 8.0, 12.0, 12.0), theme::INK);
    pen.text(words.copy_summary, copy.origin.x + 28.0, y + 6.0, 12.0, Weight::Medium, theme::INK.alpha(1.0));
    pen.hit(copy, Hit::Copy(summary));
    // The counts sit at the right; where longer words leave no room beside the buttons, the first ones go.
    let mut right = x + width;
    for pill in view.pills.iter().rev() {
        let pill_width = Pen::measuring().capsule(0.0, 0.0, 22.0, None, pill, 11.0, theme::MUTED.alpha(0.12), theme::MUTED, theme::MUTED);
        if right - pill_width < copy.origin.x + copy.size.width + 10.0 {
            break;
        }
        right -= pill_width;
        pen.capsule(right, y + 3.0, 22.0, None, pill, 11.0, theme::MUTED.alpha(0.12), theme::MUTED, theme::MUTED);
        right -= 6.0;
    }
    y += 42.0;
    pen.fill(rect(panel.origin.x, y, panel.size.width, 1.0), HAIRLINE.alpha(1.0));
    if pen.draw {
        NSGraphicsContext::restoreGraphicsState_class();
    }
    // Presses are checked last-first: the panel's buttons, then the panel itself, which keeps it open, then the
    // dimmed window around it, which closes it.
    let buttons: Vec<(NSRect, Hit)> = pen.hits.drain(1..).collect();
    pen.hits.push((panel, Hit::Open(view.key.clone())));
    pen.hits.extend(buttons);
    pen.hits.push((close, Hit::Close));
}

/// Where the task panel sits, and how tall its fixed header is; its body scrolls under the header.
const DETAIL_WIDTH: f64 = 500.0;
const DETAIL_TOP: f64 = 60.0;
const DETAIL_HEADER: f64 = 18.0 + 34.0 + 26.0 + 42.0 + 1.0;

fn detail_frame() -> NSRect {
    rect(SIZE.0 - 10.0 - DETAIL_WIDTH, DETAIL_TOP, DETAIL_WIDTH, SIZE.1 - DETAIL_TOP - 10.0)
}

/// The scrolling part of a task's panel: each request and its answer, the commits and the changed files.
/// Returns how tall it is.
fn detail_body(pen: &mut Pen, view: &DetailView, words: &Words, language: Language) -> f64 {
    let (x, width) = (20.0, DETAIL_WIDTH - 40.0);
    let mut y = 16.0;
    // Each request and what came back, in order.
    pen.text(words.flow, x, y, 12.5, Weight::Semibold, theme::INK.alpha(1.0));
    // Answers written in another language can be translated, and shown as written again.
    if let Some(translate) = &view.translate {
        let label = translate_label(translate, language);
        let working = matches!(translate, Translate::Working);
        let frame = rect(x + width - text_width(label, 11.5, Weight::Medium) - 22.0, y - 4.0, text_width(label, 11.5, Weight::Medium) + 22.0, 24.0);
        pen.round(frame, 12.0, Some(theme::INK.alpha(if working { 0.04 } else { 0.07 })), None);
        pen.text(label, frame.origin.x + 11.0, frame.origin.y + 4.5, 11.5, Weight::Medium, if working { theme::MUTED.alpha(1.0) } else { theme::INK.alpha(1.0) });
        let hit = match translate {
            Translate::Offer(texts) | Translate::Failed(_, texts) => Some(Hit::Translate(view.key.clone(), texts.clone(), language)),
            Translate::Shown => Some(Hit::Original(true)),
            Translate::Original => Some(Hit::Original(false)),
            Translate::Working => None,
        };
        if let Some(hit) = hit {
            pen.hit(frame, hit);
        }
    }
    y += 26.0;
    if let Some(Translate::Failed(why, _)) = &view.translate {
        let height = pen.text_height(why, width, 11.5, Weight::Regular, 3);
        pen.text_in(why, rect(x, y - 4.0, width, height), 11.5, Weight::Regular, theme::STALLED.alpha(1.0), NSTextAlignment::Left, true);
        y += height + 8.0;
    }
    let content_x = x + 64.0;
    let content_width = width - 64.0;
    for (index, turn) in view.turns.iter().enumerate() {
        let ask_height = pen.text_height(&format!("“{}”", turn.ask), content_width - 22.0, 12.5, Weight::Regular, 3);
        let result_height = if turn.result.is_empty() { 0.0 } else { pen.text_height(&turn.result, content_width - 18.0, 12.0, Weight::Regular, 4) };
        let marks_height = if turn.commits.is_empty() { 0.0 } else { 26.0 };
        let block = 12.0 + ask_height + 6.0 + result_height + marks_height + 16.0;
        pen.text(&turn.time, x, y + 6.0, 11.0, Weight::Regular, theme::MUTED.alpha(1.0));
        let last = index == view.turns.len() - 1;
        pen.round(rect(x + 47.0, y + 9.0, 9.0, 9.0), 4.5, Some(if last { theme::INK.alpha(1.0) } else { theme::WHITE.alpha(1.0) }), Some((theme::INK.alpha(1.0), 1.5)));
        if !last {
            pen.fill(rect(x + 51.0, y + 21.0, 1.5, block - 12.0), rgb(0.89, 0.902, 0.914).alpha(1.0));
        }
        pen.round(rect(content_x, y, (text_width(&format!("“{}”", turn.ask), 12.5, Weight::Regular) + 22.0).min(content_width), ask_height + 12.0), 12.0, Some(GROUND.alpha(1.0)), None);
        pen.text_in(&format!("“{}”", turn.ask), rect(content_x + 11.0, y + 6.0, content_width - 22.0, ask_height), 12.5, Weight::Regular, theme::INK.alpha(1.0), NSTextAlignment::Left, true);
        let mut ty = y + ask_height + 18.0;
        if !turn.result.is_empty() {
            pen.icon("corner-down-right", rect(content_x, ty + 2.0, 12.0, 12.0), theme::MUTED);
            pen.text_in(&turn.result, rect(content_x + 18.0, ty, content_width - 18.0, result_height), 12.0, Weight::Regular, rgb(0.243, 0.282, 0.302).alpha(1.0), NSTextAlignment::Left, true);
            ty += result_height + 6.0;
        }
        let mut chip_x = content_x + 18.0;
        for sha in &turn.commits {
            let chip_width = pen.capsule(chip_x, ty, 20.0, Some("git-commit-horizontal"), sha, 11.0, theme::MUTED.alpha(0.12), rgb(0.243, 0.282, 0.302), theme::MUTED);
            chip_x += chip_width + 6.0;
        }
        y += block;
    }
    // What it left behind.
    if !view.commits.is_empty() {
        y += 4.0;
        pen.text(&format!("{} {}", words.commits, view.commits.len()), x, y, 12.5, Weight::Semibold, theme::INK.alpha(1.0));
        y += 22.0;
        for (sha, message, time) in &view.commits {
            pen.fill(rect(x, y, width, 1.0), HAIRLINE.alpha(1.0));
            let sha_width = pen.text(sha, x, y + 6.0, 11.5, Weight::Regular, theme::MUTED.alpha(1.0));
            let time_width = text_width(time, 11.0, Weight::Regular);
            pen.text(time, x + width - time_width, y + 6.0, 11.0, Weight::Regular, theme::MUTED.alpha(1.0));
            pen.text_in(message, rect(x + sha_width + 10.0, y + 5.0, width - sha_width - time_width - 20.0, 17.0), 12.5, Weight::Regular, theme::INK.alpha(1.0), NSTextAlignment::Left, false);
            y += 26.0;
        }
    }
    if !view.files.is_empty() {
        y += 14.0;
        pen.text(&format!("{} {}", words.files, view.files.len()), x, y, 12.5, Weight::Semibold, theme::INK.alpha(1.0));
        y += 22.0;
        for file in &view.files {
            pen.fill(rect(x, y, width, 1.0), HAIRLINE.alpha(1.0));
            pen.icon("file", rect(x, y + 6.0, 12.0, 12.0), theme::MUTED);
            let diff_width = text_width(&file.diff, 11.0, Weight::Regular);
            pen.text(&file.diff, x + width - diff_width, y + 5.0, 11.0, Weight::Regular, theme::MUTED.alpha(1.0));
            let mut end = x + width - diff_width - 10.0;
            if file.created {
                let chip = Pen::measuring().capsule(0.0, 0.0, 18.0, None, words.new_file, 10.5, theme::MUTED.alpha(0.12), theme::MUTED, theme::MUTED);
                end -= chip;
                pen.capsule(end, y + 3.0, 18.0, None, words.new_file, 10.5, theme::MUTED.alpha(0.12), theme::MUTED, theme::MUTED);
                end -= 8.0;
            }
            pen.text_in(&file.path, rect(x + 20.0, y + 4.0, end - x - 20.0, 17.0), 11.5, Weight::Regular, theme::INK.alpha(1.0), NSTextAlignment::Left, false);
            y += 24.0;
        }
    }
    y + 20.0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layer {
    Top,
    /// The page: its ground, its empty cards and the year. Every press and key on the page comes to it.
    Canvas,
    /// One of the page's parts, over the canvas.
    Part(Part),
    /// What the pointer rests on, beside the pointer, over the page.
    Tip,
    Overlay,
    /// The scrolling body inside the task panel.
    DetailBody,
}

/// Something on the page that moves by itself for a while.
#[derive(Clone, Copy)]
enum Motion {
    /// The map opening on a day, closing back to its week, or sliding from one day to the next.
    Zoom { from: Zoom, to: Zoom, began: Instant },
    /// The map drawn again from its start, over this many seconds.
    Replay { began: Instant, seconds: f64 },
}

/// How long the map takes from one view to the next, and how long a replay that has run out stays before the
/// page is as it was.
const ZOOM_SECONDS: f64 = 0.46;
const REPLAY_HOLD: f64 = 0.7;
/// How fast a replay's playhead goes, in points a second, and the least and the most a replay lasts.
const REPLAY_SPEED: f64 = 150.0;
const REPLAY_SECONDS: (f64, f64) = (2.4, 7.0);

/// A press on the map that may turn into a drag over a stretch of it.
struct Press {
    /// Where across the page it began.
    x: f64,
    /// What it pressed, which is done when the button comes up without a drag.
    hit: Option<Hit>,
    moved: bool,
}

pub struct LayerIvars {
    layer: Layer,
    screen: RefCell<Option<Rc<Screen>>>,
    /// The canvas's parts and its tip, which it has drawn again as each needs.
    parts: RefCell<Vec<Retained<JournalLayer>>>,
    tip: RefCell<Option<Retained<JournalLayer>>>,
    hits: RefCell<Vec<(NSRect, Hit)>>,
    /// Where the canvas drew each task, for the pointer to rest on.
    spots: RefCell<Vec<(NSRect, (usize, usize))>>,
    /// What the person picked on the page; only the canvas reads it.
    ui: Cell<Ui>,
    /// The turning icons laid over this layer's drawing, each with the spot it was made for.
    spinners: RefCell<Vec<(Spin, Retained<SpinningIconView>)>>,
    /// What is moving on the canvas, and the clock that redraws it meanwhile.
    motion: Cell<Option<Motion>>,
    timer: RefCell<Option<Retained<NSTimer>>>,
    press: RefCell<Option<Press>>,
    poster: Arc<dyn Poster>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandJournalLayer"]
    #[ivars = LayerIvars]
    pub struct JournalLayer;

    impl JournalLayer {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        /// The page and its parts fill themselves, so none of them waits on what lies under it.
        #[unsafe(method(isOpaque))]
        fn is_opaque(&self) -> bool {
            matches!(self.ivars().layer, Layer::Canvas | Layer::Part(_))
        }

        /// The page takes the window's keys.
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            self.ivars().layer == Layer::Canvas
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if !self.key(event) {
                let _: () = unsafe { msg_send![super(self), keyDown: event] };
            }
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let ivars = self.ivars();
            let ground = match ivars.layer {
                Layer::Top | Layer::Canvas => Some(GROUND),
                Layer::Part(part) => Some(part.ground()),
                Layer::DetailBody => Some(theme::WHITE),
                Layer::Overlay | Layer::Tip => None,
            };
            if let Some(ground) = ground {
                color(ground).setFill();
                NSBezierPath::fillRect(self.bounds());
            }
            let screen = ivars.screen.borrow();
            let Some(screen) = screen.as_ref() else { return };
            let mut pen = Pen::drawing();
            match ivars.layer {
                Layer::Top => top_bar(&mut pen, screen),
                Layer::Canvas => body(&mut pen, screen, ivars.ui.get(), Pass::Ground),
                Layer::Part(part) => body(&mut pen, screen, ivars.ui.get(), Pass::Part(part)),
                Layer::Tip => {
                    tip(&mut pen, screen, &Look::new(screen, ivars.ui.get()), Some((TIP_MARGIN, TIP_MARGIN)));
                }
                Layer::Overlay => {
                    if let Some(view) = &screen.recap {
                        recap_overlay(&mut pen, view, screen);
                    } else if let Some(view) = &screen.detail {
                        detail(&mut pen, view, screen);
                    }
                }
                Layer::DetailBody => {
                    if let Some(view) = &screen.detail {
                        detail_body(&mut pen, view, &screen.words, screen.language);
                    }
                }
            }
            *ivars.spots.borrow_mut() = std::mem::take(&mut pen.spots);
            *ivars.hits.borrow_mut() = pen.hits;
        }

        #[unsafe(method(updateTrackingAreas))]
        fn update_tracking_areas(&self) {
            let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
            if self.ivars().layer == Layer::Canvas {
                track_pointer(self);
            }
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            let ivars = self.ivars();
            let ui = ivars.ui.get();
            // Under a task's panel or the recap cards the page does not answer the pointer.
            let covered = ivars.screen.borrow().as_ref().is_none_or(|screen| screen.detail.is_some() || screen.recap.is_some());
            let point = self.convertPoint_fromView(event.locationInWindow(), None);
            // Nor does a replay, which shows the work in its own order.
            let hover = if covered || ui.replay.is_some() { None } else { self.spot(point) };
            if hover != ui.hover || hover.is_some() {
                self.set_ui(Ui { hover, pointer: (point.x, point.y), ..ui });
            }
            if !covered && self.hit(point).is_some() {
                NSCursor::pointingHandCursor().set();
            } else if !covered && self.on_map(point) {
                // The bare map can be dragged over.
                NSCursor::crosshairCursor().set();
            } else {
                NSCursor::arrowCursor().set();
            }
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            let ui = self.ivars().ui.get();
            if ui.hover.is_some() {
                self.set_ui(Ui { hover: None, ..ui });
            }
            NSCursor::arrowCursor().set();
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let point = self.convertPoint_fromView(event.locationInWindow(), None);
            let hit = self.hit(point);
            if self.ivars().layer == Layer::Canvas {
                // A press stops a replay, and on the replay's own button does only that.
                let playing = self.ivars().ui.get().replay.is_some();
                if playing || matches!(hit, Some(Hit::Replay)) {
                    self.replay();
                }
                if matches!(hit, Some(Hit::Replay)) {
                    return;
                }
                if matches!(hit, Some(Hit::Unselect)) {
                    self.unselect();
                    return;
                }
                // A press on the map may turn into a drag over a stretch of it, so what it pressed waits for
                // the button to come up.
                if self.on_map(point) && matches!(hit, None | Some(Hit::Open(_))) {
                    *self.ivars().press.borrow_mut() = Some(Press { x: point.x, hit, moved: false });
                    return;
                }
            }
            if let Some(Hit::Pick(day)) = hit {
                self.set_ui(Ui { day, whole_week: day.is_none(), ..self.ivars().ui.get() });
                return;
            }
            if let Some(Hit::SaveRecap(which)) = hit {
                if let Some(screen) = self.ivars().screen.borrow().as_ref() {
                    save_recap(self.mtm(), screen, which);
                }
                return;
            }
            if let Some(hit) = hit {
                // A press inside the panel that names the panel itself keeps it open.
                if matches!(&hit, Hit::Open(key) if self.ivars().layer == Layer::Overlay && self.ivars().screen.borrow().as_ref().and_then(|screen| screen.detail.as_ref()).is_some_and(|detail| &detail.key == key)) {
                    return;
                }
                self.ivars().poster.post(hit.message());
            }
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            let ivars = self.ivars();
            let point = self.convertPoint_fromView(event.locationInWindow(), None);
            let mut press = ivars.press.borrow_mut();
            let Some(press) = press.as_mut() else { return };
            let x = point.x.clamp(PAD + X0 - 10.0, PAD + X1 + 10.0);
            // A hand that only trembles has pressed, not dragged.
            if !press.moved && (x - press.x).abs() < 5.0 {
                return;
            }
            press.moved = true;
            self.set_ui(Ui { select: Some((x.min(press.x), x.max(press.x))), hover: None, ..ivars.ui.get() });
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: &NSEvent) {
            let Some(press) = self.ivars().press.borrow_mut().take() else { return };
            if press.moved {
                return;
            }
            match press.hit {
                Some(hit) => self.ivars().poster.post(hit.message()),
                // A press on the bare map lets go of the stretch picked before.
                None => self.unselect(),
            }
        }

        #[unsafe(method(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> *mut NSView {
            // The title bar passes presses outside its own controls through to the window's buttons.
            let local = match unsafe { self.superview() } {
                Some(parent) => self.convertPoint_fromView(point, Some(&parent)),
                None => point,
            };
            let inside = kit::contains(self.bounds(), local);
            let owned = match self.ivars().layer {
                Layer::Top => self.ivars().hits.borrow().iter().any(|(frame, _)| kit::contains(*frame, local)),
                Layer::Overlay => self.ivars().screen.borrow().as_ref().is_some_and(|screen| screen.detail.is_some() || screen.recap.is_some()) && local.y >= TOP,
                Layer::Canvas | Layer::DetailBody => true,
                // Presses on the page's parts and its tip are the page's own.
                Layer::Part(_) | Layer::Tip => false,
            };
            // The panel's scrolling body is a subview of the overlay, so it is asked first.
            if inside && owned { unsafe { msg_send![super(self), hitTest: point] } } else { std::ptr::null_mut() }
        }
    }
);

/// Has the view told as the pointer moves over it, comes and goes.
fn track_pointer(view: &NSView) {
    for area in view.trackingAreas().iter() {
        view.removeTrackingArea(&area);
    }
    let options = NSTrackingAreaOptions::MouseEnteredAndExited | NSTrackingAreaOptions::MouseMoved | NSTrackingAreaOptions::ActiveAlways | NSTrackingAreaOptions::InVisibleRect;
    let owner: &AnyObject = view.as_ref();
    let area = unsafe { NSTrackingArea::initWithRect_options_owner_userInfo(NSTrackingArea::alloc(), NSRect::ZERO, options, Some(owner), None) };
    view.addTrackingArea(&area);
}

impl JournalLayer {
    fn new(mtm: MainThreadMarker, frame: NSRect, layer: Layer, poster: Arc<dyn Poster>) -> Retained<JournalLayer> {
        let this = JournalLayer::alloc(mtm).set_ivars(LayerIvars {
            layer,
            screen: RefCell::new(None),
            parts: RefCell::new(Vec::new()),
            tip: RefCell::new(None),
            hits: RefCell::new(Vec::new()),
            spots: RefCell::new(Vec::new()),
            ui: Cell::new(Ui::default()),
            spinners: RefCell::new(Vec::new()),
            motion: Cell::new(None),
            timer: RefCell::new(None),
            press: RefCell::new(None),
            poster,
        });
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }

    /// Whether a point is on the days of the map, where a drag picks a stretch of them: on a day's or a
    /// week's map at rest.
    fn on_map(&self, point: NSPoint) -> bool {
        let ivars = self.ivars();
        let resting = ivars.ui.get().zoom.is_none() && ivars.screen.borrow().as_ref().is_some_and(|screen| rest(screen).is_some());
        ivars.layer == Layer::Canvas && resting && kit::contains(rect(PAD + X0 - 10.0, HERO_TOP + BAND_TOP, PLOT + 20.0, BAND_BOTTOM - BAND_TOP), point)
    }

    /// What a press at a point of the page would do. The parts lie over the page, so they are asked first.
    fn hit(&self, point: NSPoint) -> Option<Hit> {
        let find = |layer: &JournalLayer| layer.ivars().hits.borrow().iter().rev().find(|(frame, _)| kit::contains(*frame, point)).map(|(_, hit)| hit.clone());
        self.ivars().parts.borrow().iter().find_map(|part| find(part)).or_else(|| find(self))
    }

    /// The task drawn at a point of the page, for the pointer to rest on.
    fn spot(&self, point: NSPoint) -> Option<(usize, usize)> {
        let find = |layer: &JournalLayer| layer.ivars().spots.borrow().iter().rev().find(|(frame, _)| kit::contains(*frame, point)).map(|(_, task)| *task);
        self.ivars().parts.borrow().iter().find_map(|part| find(part)).or_else(|| find(self))
    }

    /// Lays a turning icon over each spot this part's drawing leaves for one, and takes away those no longer
    /// wanted. Asked whenever what the part shows may have changed; icons already in place keep turning.
    fn lay_spinners(&self) {
        let ivars = self.ivars();
        let Layer::Part(part) = ivars.layer else { return };
        let wanted: Vec<Spin> = match ivars.screen.borrow().as_ref() {
            Some(screen) => {
                let mut pen = Pen::measuring();
                body(&mut pen, screen, ivars.ui.get(), Pass::Part(part));
                pen.spins.into_inner()
            }
            None => Vec::new(),
        };
        let mut laid = ivars.spinners.borrow_mut();
        if laid.len() == wanted.len() && laid.iter().zip(&wanted).all(|(have, want)| have.0 == *want) {
            return;
        }
        for (_, view) in laid.drain(..) {
            view.removeFromSuperview();
        }
        for spin in wanted {
            let view = SpinningIconView::new(self.mtm(), spin.frame, TURNING, spin.ink);
            self.addSubview(&view);
            laid.push((spin, view));
        }
    }

    /// Takes what the person picked, or how far a motion has got, and has drawn again only the parts of the
    /// page that show it: the map for every step, the date line and the cards only when what they count or
    /// mark changes. The tip is moved to the pointer.
    fn set_ui(&self, ui: Ui) {
        let ivars = self.ivars();
        let before = ivars.ui.replace(ui);
        let moved = (before.zoom, before.replay, before.select, before.hover) != (ui.zoom, ui.replay, ui.select, ui.hover);
        let counted = ivars.screen.borrow().as_ref().is_some_and(|screen| Look::new(screen, before).shown() != Look::new(screen, ui).shown());
        for part in ivars.parts.borrow().iter() {
            part.ivars().ui.set(ui);
            if if part.ivars().layer == Layer::Part(Part::Map) { moved } else { counted } {
                part.setNeedsDisplay(true);
                part.lay_spinners();
            }
        }
        self.place_tip(before.hover != ui.hover);
    }

    /// Puts the tip beside the pointer, or away while the pointer rests on no task. `changed` when it speaks
    /// of another task than before and so is drawn again; following the pointer, it is only moved.
    fn place_tip(&self, changed: bool) {
        let ivars = self.ivars();
        let Some(view) = ivars.tip.borrow().clone() else { return };
        let ui = ivars.ui.get();
        view.ivars().ui.set(ui);
        let frame = ivars.screen.borrow().as_ref().and_then(|screen| tip(&mut Pen::measuring(), screen, &Look::new(screen, ui), None));
        match frame {
            Some(frame) => {
                view.setFrame(kit::inset(frame, -TIP_MARGIN, -TIP_MARGIN));
                view.setHidden(false);
                if changed {
                    view.setNeedsDisplay(true);
                }
            }
            None => view.setHidden(true),
        }
    }

    /// Sets something moving, and the clock that redraws the page while it does.
    fn start(&self, motion: Motion) {
        let ivars = self.ivars();
        ivars.motion.set(Some(motion));
        if ivars.timer.borrow().is_some() {
            return;
        }
        // The clock does not keep the page alive.
        let layer = Weak::new(self);
        let block = block2::RcBlock::new(move |_| {
            if let Some(layer) = layer.load() {
                layer.tick();
            }
        });
        let timer = unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(1.0 / 60.0, true, &block) };
        *ivars.timer.borrow_mut() = Some(timer);
    }

    /// Stops what is moving and its clock, and leaves the page as it is drawn.
    fn halt(&self) {
        let ivars = self.ivars();
        ivars.motion.set(None);
        if let Some(timer) = ivars.timer.borrow_mut().take() {
            timer.invalidate();
        }
    }

    /// One step of what is moving.
    fn tick(&self) {
        let ivars = self.ivars();
        let mut ui = ivars.ui.get();
        match ivars.motion.get() {
            Some(Motion::Zoom { from, to, began }) => {
                let part = began.elapsed().as_secs_f64() / ZOOM_SECONDS;
                if part >= 1.0 {
                    ui.zoom = None;
                    self.halt();
                } else {
                    // Slow away, quick through the middle, slow home.
                    let eased = if part < 0.5 { 4.0 * part.powi(3) } else { 1.0 - (2.0 - 2.0 * part).powi(3) / 2.0 };
                    ui.zoom = Some(Zoom { open: from.open + (to.open - from.open) * eased, day: from.day + (to.day - from.day) * eased });
                }
            }
            Some(Motion::Replay { began, seconds }) => {
                let part = began.elapsed().as_secs_f64() / seconds;
                if part >= 1.0 + REPLAY_HOLD / seconds {
                    ui.replay = None;
                    self.halt();
                } else {
                    // Past its end it holds there, and only the names still come in.
                    ui.replay = Some(part);
                }
            }
            None => self.halt(),
        }
        self.set_ui(ui);
    }

    /// Plays the map again from its start, or stops it where it is playing.
    fn replay(&self) {
        let ivars = self.ivars();
        let ui = ivars.ui.get();
        if ui.replay.is_some() {
            self.halt();
            self.set_ui(Ui { replay: None, ..ui });
            return;
        }
        // Only a day's or a week's map at rest, with work on screen and nothing over it, has anything to play.
        let seconds = ivars.screen.borrow().as_ref().and_then(|screen| {
            let map = screen.map.weeks.first().filter(|_| screen.detail.is_none() && screen.recap.is_none())?;
            let (from, to) = Plot { map, zoom: rest(screen)?, x: PAD, y: HERO_TOP }.span()?;
            Some(((to - from) / REPLAY_SPEED).clamp(REPLAY_SECONDS.0, REPLAY_SECONDS.1))
        });
        let Some(seconds) = seconds.filter(|_| ui.zoom.is_none()) else { return };
        self.set_ui(Ui { replay: Some(0.0), select: None, hover: None, ..ui });
        self.start(Motion::Replay { began: Instant::now(), seconds });
    }

    /// Lets go of the stretch of the map that was dragged over.
    fn unselect(&self) {
        let ui = self.ivars().ui.get();
        if ui.select.is_some() {
            self.set_ui(Ui { select: None, ..ui });
        }
    }

    /// Starts the page afresh for another view or period. Where the map was open on the same week before, it
    /// moves from how it stood then to how the new view has it.
    fn turn_to(&self, from: Option<Zoom>, to: Option<Zoom>) {
        self.halt();
        *self.ivars().press.borrow_mut() = None;
        let mut ui = Ui::default();
        if let (Some(from), Some(to)) = (from, to)
            && from != to
            && !NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
        {
            // A closed map can open on any day; a map that closes stays on the day it was open on.
            let from = if from.open < 0.01 { Zoom { day: to.day, ..from } } else { from };
            let to = if to.open < 0.01 { Zoom { day: from.day, ..to } } else { to };
            ui.zoom = Some(from);
            self.set_ui(ui);
            self.start(Motion::Zoom { from, to, began: Instant::now() });
        } else {
            self.set_ui(ui);
        }
    }

    /// What a key does on the page: the arrows step back and forward, 1, 2 and 3 show the day, the week and
    /// the month, the space bar plays the map again, and Escape leaves the innermost thing. False for a key
    /// the page has no use for.
    fn key(&self, event: &NSEvent) -> bool {
        enum Act {
            Post(Hit),
            Replay,
            Unselect,
            Nothing,
        }
        const ESCAPE: u16 = 53;
        const LEFT: u16 = 123;
        const RIGHT: u16 = 124;
        let ivars = self.ivars();
        if event.modifierFlags().intersects(NSEventModifierFlags::Command | NSEventModifierFlags::Control | NSEventModifierFlags::Option) {
            return false;
        }
        let typed = event.charactersIgnoringModifiers().map(|typed| typed.to_string()).unwrap_or_default();
        let code = event.keyCode();
        let ui = ivars.ui.get();
        // What to do is settled before anything is done, as doing it may draw the page again.
        let act = {
            let screen = ivars.screen.borrow();
            let Some(screen) = screen.as_ref() else { return false };
            let page = &screen.map;
            if let Some(recap) = &screen.recap {
                // Over the recap the arrows turn its cards.
                match code {
                    ESCAPE => Act::Post(Hit::Recap(None)),
                    LEFT if recap.index > 0 => Act::Post(Hit::Recap(Some(recap.index - 1))),
                    RIGHT if recap.index + 1 < recap.cards.len() => Act::Post(Hit::Recap(Some(recap.index + 1))),
                    LEFT | RIGHT => Act::Nothing,
                    _ => return false,
                }
            } else if screen.detail.is_some() {
                match code {
                    ESCAPE => Act::Post(Hit::Close),
                    _ => return false,
                }
            } else {
                match (code, typed.as_str()) {
                    (ESCAPE, _) if ui.replay.is_some() => Act::Replay,
                    (ESCAPE, _) if ui.select.is_some() => Act::Unselect,
                    (ESCAPE, _) => Act::Nothing,
                    (LEFT, _) => Act::Post(Hit::Step(-1)),
                    (RIGHT, _) if head(screen).can_go_forward => Act::Post(Hit::Step(1)),
                    (RIGHT, _) => Act::Nothing,
                    (_, "1") => match (screen.place.tab, page.weeks.first(), chosen_day(page, ui)) {
                        (JournalTab::Day, ..) => Act::Nothing,
                        // A week opens on the day its summary shows.
                        (JournalTab::Week, Some(map), Some(day)) if !map.days[day].future => Act::Post(Hit::Show(JournalTab::Day, map.days[day].back)),
                        _ => Act::Post(Hit::Tab(JournalTab::Day)),
                    },
                    (_, "2") => match (screen.place.tab, page.weeks.first()) {
                        (JournalTab::Week, _) => Act::Nothing,
                        // A day closes back to the week it is in.
                        (JournalTab::Day, Some(map)) => Act::Post(Hit::Show(JournalTab::Week, map.back)),
                        _ => Act::Post(Hit::Tab(JournalTab::Week)),
                    },
                    (_, "3") if screen.place.tab == JournalTab::Month => Act::Nothing,
                    (_, "3") => Act::Post(Hit::Tab(JournalTab::Month)),
                    (_, " ") => Act::Replay,
                    _ => return false,
                }
            }
        };
        match act {
            Act::Post(hit) => ivars.poster.post(hit.message()),
            Act::Replay => self.replay(),
            Act::Unselect => self.unselect(),
            Act::Nothing => {}
        }
        true
    }

    fn show(&self, screen: &Rc<Screen>) {
        let ivars = self.ivars();
        // What the pointer rested on may not be there any more, or not in the same place.
        let ui = Ui { hover: None, ..ivars.ui.get() };
        ivars.ui.set(ui);
        let before = ivars.screen.borrow_mut().replace(screen.clone());
        // The canvas itself draws only the cards' ground and the year, which seldom change; their shadows
        // are the dearest thing on the page to draw.
        let same_ground = ivars.layer == Layer::Canvas && before.is_some_and(|before| before.map.year == screen.map.year && before.map.weeks.iter().map(|map| map.back).eq(screen.map.weeks.iter().map(|map| map.back)));
        if !same_ground {
            self.setNeedsDisplay(true);
        }
        // The page's parts and its tip show what the page shows.
        for part in ivars.parts.borrow().iter().chain(ivars.tip.borrow().iter()) {
            part.ivars().ui.set(ui);
            *part.ivars().screen.borrow_mut() = Some(screen.clone());
            part.setNeedsDisplay(true);
            part.lay_spinners();
        }
        self.place_tip(true);
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErrandJournalDelegate"]
    #[ivars = Arc<dyn Poster>]
    struct JournalDelegate;

    unsafe impl NSObjectProtocol for JournalDelegate {}

    unsafe impl NSWindowDelegate for JournalDelegate {
        #[unsafe(method(windowWillClose:))]
        fn will_close(&self, _notification: &NSNotification) {
            self.ivars().post(Msg::JournalClosed);
        }
    }
);

/// A moment of the page that a person only passes through, for a preview to hold still.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pose {
    /// The pointer resting on the task worked on longest.
    Hover,
    /// The map half-way between a week and one of its days.
    Zoom,
    /// A replay part of the way through.
    Replay,
    /// A stretch of the map dragged over.
    Select,
}

/// What on the page is moving or picked, for a preview that drives the window to see.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Standing {
    pub zoom: Option<(f64, f64)>,
    pub replay: Option<f64>,
    pub select: Option<(f64, f64)>,
}

pub struct Journal {
    pub window: Retained<NSWindow>,
    top: Retained<JournalLayer>,
    canvas: Retained<JournalLayer>,
    overlay: Retained<JournalLayer>,
    scroll: Retained<NSScrollView>,
    detail_scroll: Retained<NSScrollView>,
    detail_body: Retained<JournalLayer>,
    detail_key: Option<String>,
    _delegate: Retained<JournalDelegate>,
    shown: Option<Rc<Screen>>,
    tab: Cell<Option<(JournalTab, i64)>>,
}

impl Journal {
    pub fn new(mtm: MainThreadMarker, poster: Arc<dyn Poster>, title: &str) -> Journal {
        let (width, height) = SIZE;
        let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Miniaturizable | NSWindowStyleMask::FullSizeContentView;
        let window: Retained<NSWindow> = unsafe {
            msg_send![NSWindow::alloc(mtm), initWithContentRect: rect(0.0, 0.0, width, height), styleMask: style, backing: NSBackingStoreType::Buffered, defer: false]
        };
        window.setTitle(&ns(title));
        window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        window.setTitlebarAppearsTransparent(true);
        window.setAppearance(NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua }).as_deref());
        window.setBackgroundColor(Some(&color(GROUND)));
        unsafe { window.setReleasedWhenClosed(false) };
        let content = kit::FlippedView::new(mtm, rect(0.0, 0.0, width, height));
        window.setContentView(Some(&content));
        let scroll = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), rect(0.0, TOP, width, height - TOP));
        scroll.setDrawsBackground(false);
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);
        let canvas = JournalLayer::new(mtm, rect(0.0, 0.0, width, height - TOP), Layer::Canvas, poster.clone());
        // Each part of the page that moves or changes is a view of its own over the canvas, each kept as it
        // was drawn, so that only what changes is drawn again.
        canvas.setWantsLayer(true);
        let parts: Vec<Retained<JournalLayer>> = Part::ALL
            .iter()
            .map(|part| {
                let view = JournalLayer::new(mtm, part.frame(), Layer::Part(*part), poster.clone());
                // It draws in the page's own coordinates.
                view.setBoundsOrigin(part.frame().origin);
                canvas.addSubview(&view);
                view
            })
            .collect();
        let tip = JournalLayer::new(mtm, NSRect::ZERO, Layer::Tip, poster.clone());
        tip.setHidden(true);
        canvas.addSubview(&tip);
        *canvas.ivars().parts.borrow_mut() = parts;
        *canvas.ivars().tip.borrow_mut() = Some(tip);
        scroll.setDocumentView(Some(&canvas));
        content.addSubview(&scroll);
        let top = JournalLayer::new(mtm, rect(0.0, 0.0, width, TOP), Layer::Top, poster.clone());
        content.addSubview(&top);
        let overlay = JournalLayer::new(mtm, rect(0.0, 0.0, width, height), Layer::Overlay, poster.clone());
        overlay.setHidden(true);
        content.addSubview(&overlay);
        // The task panel's body scrolls under its header, inside the overlay; the bottom stays clear of the
        // panel's rounded corners.
        let panel = detail_frame();
        let body_frame = rect(panel.origin.x + 1.0, panel.origin.y + DETAIL_HEADER, panel.size.width - 2.0, panel.size.height - DETAIL_HEADER - 10.0);
        let detail_scroll = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), body_frame);
        detail_scroll.setDrawsBackground(false);
        detail_scroll.setHasVerticalScroller(true);
        detail_scroll.setAutohidesScrollers(true);
        let detail_body = JournalLayer::new(mtm, rect(0.0, 0.0, body_frame.size.width, body_frame.size.height), Layer::DetailBody, poster.clone());
        detail_scroll.setDocumentView(Some(&detail_body));
        overlay.addSubview(&detail_scroll);
        let delegate: Retained<JournalDelegate> = {
            let this = JournalDelegate::alloc(mtm).set_ivars(poster);
            unsafe { msg_send![super(this), init] }
        };
        window.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        window.center();
        Journal { window, top, canvas, overlay, scroll, detail_scroll, detail_body, detail_key: None, _delegate: delegate, shown: None, tab: Cell::new(None) }
    }

    pub fn render(&mut self, screen: &Screen) {
        if self.shown.as_deref() == Some(screen) {
            return;
        }
        // One copy, which every view of the window shows.
        let shared = Rc::new(screen.clone());
        self.top.show(&shared);
        self.canvas.show(&shared);
        self.overlay.show(&shared);
        self.overlay.setHidden(screen.detail.is_none() && screen.recap.is_none());
        self.detail_scroll.setHidden(screen.detail.is_none() || screen.recap.is_some());
        if let Some(detail) = &screen.detail {
            let visible = self.detail_scroll.frame().size;
            let height = detail_body(&mut Pen::measuring(), detail, &screen.words, screen.language).max(visible.height);
            self.detail_body.setFrameSize(NSSize::new(visible.width, height));
            self.detail_body.show(&shared);
            // Another task's record starts at its top.
            if self.detail_key.as_deref() != Some(detail.key.as_str()) {
                self.detail_key = Some(detail.key.clone());
                let clip = self.detail_scroll.contentView();
                clip.scrollToPoint(NSPoint::new(0.0, 0.0));
                self.detail_scroll.reflectScrolledClipView(&clip);
            }
        } else {
            self.detail_key = None;
        }
        // A new view or period starts at its top, and within one week the map moves from where it stood.
        let place = (screen.place.tab, screen.place.back);
        if self.tab.get() != Some(place) {
            self.tab.set(Some(place));
            let week = |screen: &Screen| screen.map.weeks.first().map(|map| map.monday);
            let from = self.shown.as_deref().filter(|shown| week(shown) == week(screen)).and_then(|shown| self.canvas.ivars().ui.get().zoom.or(rest(shown)));
            self.canvas.turn_to(from, rest(screen));
            let clip = self.scroll.contentView();
            clip.scrollToPoint(NSPoint::new(0.0, 0.0));
            self.scroll.reflectScrolledClipView(&clip);
        }
        self.shown = Some(shared);
    }

    pub fn show(&self, mtm: MainThreadMarker) {
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
        self.window.makeKeyAndOrderFront(None);
        self.focus();
    }

    /// Sends the window's keys to the page.
    pub fn focus(&self) {
        self.window.makeFirstResponder(Some(&self.canvas));
    }

    /// Where on the page the task worked on longest is drawn on the map, and which task it is.
    fn longest(&self) -> Option<(NSRect, (usize, usize))> {
        let screen = self.shown.as_deref()?;
        let map = screen.map.weeks.first()?;
        let mut pen = Pen::measuring();
        body(&mut pen, screen, Ui::default(), Pass::Whole);
        // Among the tasks on screen. The first place each is drawn is on the map, and of equals the last wins,
        // so they are gone through backwards.
        pen.spots.iter().rev().filter(|(_, at)| at.0 == 0).max_by_key(|(_, at)| map.tasks[at.1].minutes).copied()
    }

    /// Holds the page at a moment a person only passes through, as a preview shows it.
    pub fn pose(&self, pose: Pose) {
        let Some(screen) = self.shown.as_deref() else { return };
        let ui = self.canvas.ivars().ui.get();
        let posed = match pose {
            Pose::Hover => {
                let Some((frame, at)) = self.longest() else { return };
                Ui { hover: Some(at), pointer: (frame.origin.x + frame.size.width / 2.0, frame.origin.y + frame.size.height / 2.0), ..ui }
            }
            // Half-way between the week and the day its summary shows.
            Pose::Zoom => Ui { zoom: Some(Zoom { open: 0.5, day: chosen_day(&screen.map, ui).unwrap_or(0) as f64 }), ..ui },
            Pose::Replay => Ui { replay: Some(0.62), ..ui },
            Pose::Select => Ui { select: Some((PAD + X0 + PLOT * 0.52, PAD + X0 + PLOT * 0.86)), ..ui },
        };
        self.canvas.set_ui(posed);
    }

    /// The middle of the task worked on longest, as the map draws it, in the window's own coordinates.
    pub fn longest_task(&self) -> Option<NSPoint> {
        let (frame, _) = self.longest()?;
        Some(self.canvas.convertPoint_toView(NSPoint::new(frame.origin.x + frame.size.width / 2.0, frame.origin.y + frame.size.height / 2.0), None))
    }

    /// A place on the map's days, this far across them and just above their foot, in the window's own
    /// coordinates.
    pub fn map_point(&self, across: f64) -> NSPoint {
        self.canvas.convertPoint_toView(NSPoint::new(PAD + X0 + PLOT * across, HERO_TOP + BAND_BOTTOM - 5.0), None)
    }

    /// How long each part of the page takes to draw once, in milliseconds. The map's is what every step of
    /// a motion costs.
    pub fn drawing_times(&self) -> Vec<(String, f64)> {
        let time = |view: &JournalLayer| {
            let bitmap = view.bitmapImageRepForCachingDisplayInRect(view.bounds())?;
            let began = Instant::now();
            view.cacheDisplayInRect_toBitmapImageRep(view.bounds(), &bitmap);
            Some(began.elapsed().as_secs_f64() * 1000.0)
        };
        self.canvas.ivars().parts.borrow().iter().filter_map(|part| Some((format!("{:?}", part.ivars().layer), time(part)?))).collect()
    }

    /// How the map stands just now: how far it is opened and on which day while it moves, how far a replay
    /// has got, and the stretch dragged over.
    pub fn standing(&self) -> Standing {
        let ui = self.canvas.ivars().ui.get();
        Standing { zoom: ui.zoom.map(|zoom| (zoom.open, zoom.day)), replay: ui.replay, select: ui.select }
    }

    /// The window's content as drawn, for previews.
    pub fn content(&self) -> Option<Retained<NSView>> {
        self.window.contentView()
    }
}
