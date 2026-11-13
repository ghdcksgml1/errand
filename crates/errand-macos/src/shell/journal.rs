//! The desktop window: the person's work by day, week and month, a report of the last weeks, and one task's
//! whole record, drawn in the widget's own cards, chips and colours. Everything it shows comes from
//! `errand_core::journal::view`; this file only lays it out and draws it.

use super::kit::{self, Weight, color, ns, paint, rect};
use super::marks::draw_app_mark;
use errand_core::avatar::Avatar;
use errand_core::journal::view::{Bar, Body, Callout, CalendarCell, DayView, DetailView, HourAxis, Masthead, MonthView, ReportSpan, ReportView, Screen, WeekView, JournalTab};
use errand_core::model::Msg;
use errand_core::ports::Poster;
use errand_core::theme::{self, Paint, Rgb};
use errand_core::{Language, Provider};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSApplication, NSBackingStoreType, NSBezierPath, NSColor, NSCompositingOperation, NSEvent,
    NSGraphicsContext, NSLineBreakMode, NSMutableParagraphStyle, NSScrollView, NSShadow, NSStringDrawing, NSStringDrawingOptions, NSStringNSExtendedStringDrawing, NSTextAlignment, NSView, NSWindow, NSWindowDelegate,
    NSWindowStyleMask, NSWindowTitleVisibility,
};
use objc2_foundation::{NSAttributedStringKey, NSDictionary, NSNotification, NSNumber, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize};
use std::cell::{Cell, RefCell};
use std::sync::Arc;

pub const SIZE: (f64, f64) = (1180.0, 760.0);
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
const CODEX: Rgb = rgb(0.22, 0.55, 0.95);
const CLAUDE: Rgb = rgb(0.80, 0.42, 0.32);
const ASKING_FILL: Rgb = rgb(0.984, 0.961, 0.929);
const NEW_FILL: Rgb = rgb(0.933, 0.957, 0.988);

fn service(provider: Provider) -> Rgb {
    match provider {
        Provider::Codex => CODEX,
        Provider::Claude => CLAUDE,
    }
}

/// What a press on a drawn spot does.
#[derive(Clone, Debug)]
enum Hit {
    Tab(JournalTab),
    Step(i64),
    Project(Option<String>),
    Span(ReportSpan),
    Open(String),
    Close,
    Copy(String),
    OpenTask(String),
}

impl Hit {
    fn message(self) -> Msg {
        match self {
            Hit::Tab(tab) => Msg::JournalTab(tab),
            Hit::Step(step) => Msg::JournalStep(step),
            Hit::Project(project) => Msg::JournalProject(project),
            Hit::Span(span) => Msg::JournalSpan(span),
            Hit::Open(key) => Msg::JournalOpen(Some(key)),
            Hit::Close => Msg::JournalOpen(None),
            Hit::Copy(text) => Msg::JournalCopy(text),
            Hit::OpenTask(url) => Msg::JournalOpenTask(url),
        }
    }
}

/// Draws, or only measures, and keeps where each pressable thing landed.
struct Pen {
    draw: bool,
    hits: Vec<(NSRect, Hit)>,
}

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
        Pen { draw: false, hits: Vec::new() }
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
            self.icon(icon, rect(cursor, y + (height - icon_size) / 2.0, icon_size, icon_size), icon_ink);
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

/// A card's fill and edge, as the widget draws the same state.
fn card_colors(callout: Option<Callout>) -> (Rgb, Paint, f64) {
    match callout {
        Some(Callout::Asking) => (ASKING_FILL, theme::ASKING.alpha(0.55), 1.5),
        Some(Callout::New) => (NEW_FILL, theme::NEW_RESULT.alpha(0.55), 1.5),
        Some(Callout::Running) => (theme::WHITE, theme::RUNNING_BRIGHT.alpha(0.7), 1.25),
        None => (theme::WHITE, EDGE.alpha(1.0), 1.0),
    }
}

fn masthead(pen: &mut Pen, masthead: &Masthead, screen: &Screen, spans: bool) {
    let big = kit::font(104.0, Weight::Bold);
    let baseline = 100.0;
    let width = big_number(pen, &masthead.big, PAD, baseline - big.ascender());
    let x = PAD + width + 14.0;
    let top_width = pen.text(&masthead.top, x, 56.0, 22.0, Weight::Semibold, theme::INK.alpha(1.0));
    let bottom_width = pen.text(&masthead.bottom, x, 86.0, 12.0, Weight::Regular, theme::MUTED.alpha(1.0));
    let mut x = x + top_width.max(bottom_width) + 18.0;
    if spans {
        for (span, name) in &screen.spans {
            let selected = *span == screen.place.span;
            let words = text_width(name, 12.0, if selected { Weight::Semibold } else { Weight::Medium });
            let frame = rect(x, 78.0, words + 24.0, 26.0);
            if selected {
                pen.round(frame, 13.0, Some(theme::INK.alpha(0.09)), None);
            } else {
                pen.round(frame, 13.0, Some(theme::WHITE.alpha(1.0)), Some((rgb(0.84, 0.84, 0.84).alpha(1.0), 1.0)));
            }
            pen.text(name, x + 12.0, 83.0, 12.0, if selected { Weight::Semibold } else { Weight::Medium }, theme::INK.alpha(1.0));
            pen.hit(frame, Hit::Span(*span));
            x += words + 30.0;
        }
    } else {
        for (index, (icon, step, enabled)) in [("chevron-left", -1, true), ("chevron-right", 1, masthead.can_go_forward)].into_iter().enumerate() {
            let frame = rect(x + index as f64 * 30.0, 78.0, 26.0, 26.0);
            pen.round(frame, 13.0, Some(theme::INK.alpha(if enabled { 0.07 } else { 0.04 })), None);
            pen.icon(icon, kit::inset(frame, 6.0, 6.0), if enabled { theme::INK } else { FAINT });
            if enabled {
                pen.hit(frame, Hit::Step(step));
            }
        }
    }
    // The person's character closes the line, as on the widget's profile row.
    let face = rect(PAD + WIDTH - 64.0, 40.0, 64.0, 64.0);
    pen.round(face, 32.0, Some(theme::HEALTHY.alpha(0.14)), None);
    if pen.draw {
        kit::draw_avatar(&Avatar::named(&screen.avatar), face);
    }
    let mut right = face.origin.x - 14.0;
    for pill in masthead.pills.iter().rev() {
        let (fill, ink, icon_ink) = pill.callout.map(chip_colors).unwrap_or((theme::MUTED.alpha(0.12), theme::MUTED, theme::MUTED));
        let width = Pen::measuring().capsule(0.0, 0.0, 24.0, Some(pill.icon), &pill.text, 12.0, fill, ink, icon_ink);
        right -= width;
        pen.capsule(right, 79.0, 24.0, Some(pill.icon), &pill.text, 12.0, fill, ink, icon_ink);
        right -= 6.0;
    }
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

fn legend(pen: &Pen, right: f64, y: f64, filled: bool) {
    let mut x = right;
    for provider in [Provider::Claude, Provider::Codex] {
        let name = provider.title();
        let words = text_width(name, 11.5, Weight::Regular);
        x -= words;
        pen.text(name, x, y, 11.5, Weight::Regular, theme::MUTED.alpha(1.0));
        x -= 15.0;
        let swatch = rect(x, y + 3.0, 10.0, 10.0);
        if filled {
            pen.round(swatch, 3.0, Some(service(provider).alpha(1.0)), None);
        } else {
            pen.round(swatch, 3.0, Some(service(provider).alpha(0.3)), Some((service(provider).alpha(0.6), 1.0)));
        }
        x -= 14.0;
    }
}

fn hour_labels(pen: &Pen, axis: &HourAxis, x: f64, width: f64, y: f64) {
    let span = (axis.last - axis.first) as f64;
    for hour in axis.first..=axis.last {
        let place = x + (hour - axis.first) as f64 / span * width;
        let label = format!("{hour:02}");
        let words = text_width(&label, 10.5, Weight::Regular);
        let left = if hour == axis.first { place } else if hour == axis.last { place - words } else { place - words / 2.0 };
        pen.text(&label, left, y, 10.5, Weight::Regular, theme::MUTED.alpha(1.0));
    }
}

/// A stretch of work: the service's colour when finished, the live state's outline while it runs.
fn bar(pen: &mut Pen, item: &Bar, frame: NSRect, with_icon: bool) {
    match item.callout {
        Some(Callout::Running) => pen.round(frame, 6.0, Some(theme::WHITE.alpha(1.0)), Some((theme::RUNNING_BRIGHT.alpha(0.7), 1.25))),
        Some(Callout::Asking) => pen.round(frame, 6.0, Some(ASKING_FILL.alpha(1.0)), Some((theme::ASKING.alpha(0.55), 1.5))),
        _ => pen.round(frame, 6.0, Some(service(item.provider).alpha(0.18)), Some((service(item.provider).alpha(0.5), 1.0))),
    }
    if with_icon && frame.size.width >= 24.0 {
        let icon = (frame.size.height - 12.0).min(16.0);
        pen.app(item.provider, rect(frame.origin.x + 5.0, frame.origin.y + (frame.size.height - icon) / 2.0, icon, icon));
    }
    pen.hit(frame, Hit::Open(item.key.clone()));
}

fn day(pen: &mut Pen, view: &DayView, screen: &Screen) -> f64 {
    masthead(pen, &view.masthead, screen, false);
    let words = &screen.words;
    // The day, hour by hour.
    let top = 120.0;
    let lane = 46.0;
    let plot = top + 12.0 + 18.0 + 8.0;
    let lanes_top = plot + 20.0;
    let height = 12.0 + 18.0 + 8.0 + 20.0 + view.lanes as f64 * lane + 22.0 + 12.0;
    pen.panel(rect(PAD, top, WIDTH, height), 16.0, theme::WHITE, EDGE.alpha(1.0), 1.0);
    pen.text(words.by_hour, PAD + 18.0, top + 12.0, 12.5, Weight::Semibold, theme::INK.alpha(1.0));
    legend(pen, PAD + WIDTH - 18.0, top + 12.0, false);
    let (x, width) = (PAD + 18.0, WIDTH - 36.0);
    hour_labels(pen, &view.axis, x, width, plot);
    let span = (view.axis.last - view.axis.first) as f64;
    for hour in view.axis.first..=view.axis.last {
        pen.fill(rect(x + (hour - view.axis.first) as f64 / span * width, plot + 16.0, 1.0, view.lanes as f64 * lane + 6.0), rgb(0.937, 0.937, 0.937).alpha(1.0));
    }
    for item in &view.bars {
        let left = x + item.start * width;
        let frame = rect(left, lanes_top + item.lane as f64 * lane, ((item.end - item.start) * width).max(8.0), 28.0);
        bar(pen, item, frame, item.callout != Some(Callout::Running));
        let inside = text_width(&item.label, 11.0, Weight::Semibold) + 34.0 <= frame.size.width;
        let ink = match item.callout {
            Some(Callout::Running) => theme::RUNNING,
            Some(Callout::Asking) => theme::ASKING,
            _ => theme::INK,
        };
        if item.callout == Some(Callout::Running) {
            pen.icon("loader-circle", rect(frame.origin.x + 7.0, frame.origin.y + 8.0, 12.0, 12.0), theme::RUNNING_BRIGHT);
        }
        if inside {
            let start = frame.origin.x + if item.callout == Some(Callout::Running) { 24.0 } else { 27.0 };
            pen.text_in(&item.label, rect(start, frame.origin.y + 6.0, frame.size.width - (start - frame.origin.x) - 6.0, 16.0), 11.0, Weight::Semibold, ink.alpha(1.0), NSTextAlignment::Left, false);
        } else {
            // Below the bar, as far as the next bar in its lane allows.
            let next = view.bars.iter().filter(|other| other.lane == item.lane && other.start > item.start).map(|other| x + other.start * width).fold(x + width, f64::min);
            let room = (next - frame.origin.x - 8.0).min(150.0);
            if room >= 28.0 {
                pen.text_in(&item.label, rect(frame.origin.x, frame.origin.y + 31.0, room, 15.0), 11.0, Weight::Medium, ink.alpha(1.0), NSTextAlignment::Left, false);
            }
        }
    }
    if let Some(now) = view.axis.now {
        let place = x + now * width;
        pen.fill(rect(place - 0.75, plot + 14.0, 1.5, view.lanes as f64 * lane + 10.0), theme::INK.alpha(1.0));
        let words_width = text_width(&view.axis.now_text, 10.5, Weight::Semibold);
        let capsule = rect((place - words_width / 2.0 - 7.0).clamp(x, x + width - words_width - 14.0), lanes_top + view.lanes as f64 * lane + 2.0, words_width + 14.0, 18.0);
        pen.round(capsule, 9.0, Some(theme::INK.alpha(1.0)), None);
        pen.text(&view.axis.now_text, capsule.origin.x + 7.0, capsule.origin.y + 2.0, 10.5, Weight::Semibold, theme::WHITE.alpha(1.0));
    }
    // Every task in one grid, newest first; projects filter it.
    let mut y = top + height + 16.0;
    let mut x = PAD;
    for (project, name, count) in &view.filters {
        let selected = *project == screen.place.project;
        let label = format!("{name}  {count}");
        let words_width = text_width(&label, 12.0, if selected { Weight::Semibold } else { Weight::Medium });
        let frame = rect(x, y, words_width + 24.0, 26.0);
        if selected {
            pen.round(frame, 13.0, Some(theme::INK.alpha(0.09)), None);
        } else {
            pen.round(frame, 13.0, Some(theme::WHITE.alpha(1.0)), Some((rgb(0.84, 0.84, 0.84).alpha(1.0), 1.0)));
        }
        let name_width = pen.text(name, x + 12.0, y + 5.0, 12.0, if selected { Weight::Semibold } else { Weight::Medium }, theme::INK.alpha(1.0));
        pen.text(&count.to_string(), x + 12.0 + name_width + 6.0, y + 5.0, 12.0, Weight::Medium, theme::MUTED.alpha(1.0));
        pen.hit(frame, Hit::Project(project.clone()));
        x += frame.size.width + 6.0;
    }
    y += 36.0;
    if view.cards.is_empty() {
        let note = if screen.loading { words.loading } else { words.empty };
        pen.text_in(note, rect(PAD, y + 30.0, WIDTH, 20.0), 13.0, Weight::Regular, theme::MUTED.alpha(1.0), NSTextAlignment::Center, false);
        return y + 80.0;
    }
    let card_width = ((WIDTH - 20.0) / 3.0).floor();
    for (index, card) in view.cards.iter().enumerate() {
        let frame = rect(PAD + (index % 3) as f64 * (card_width + 10.0), y + (index / 3) as f64 * 122.0, card_width, 112.0);
        task_card(pen, card, frame);
    }
    y + view.cards.len().div_ceil(3) as f64 * 122.0 + 14.0
}

fn task_card(pen: &mut Pen, card: &errand_core::journal::view::Card, frame: NSRect) {
    let (fill, edge, edge_width) = card_colors(card.callout);
    pen.panel(frame, 16.0, fill, edge, edge_width);
    let (x, y, width) = (frame.origin.x + 14.0, frame.origin.y + 11.0, frame.size.width - 28.0);
    pen.app(card.provider, rect(x, y + 1.0, 18.0, 18.0));
    let mut title_end = x + width;
    if let Some(callout) = card.callout {
        let (fill, ink, icon_ink) = chip_colors(callout);
        let chip = Pen::measuring().capsule(0.0, 0.0, 19.0, Some(callout.state().icon()), callout.word(Language::Korean), 11.0, fill, ink, icon_ink);
        let word = callout_word(card, callout);
        let chip = chip.max(Pen::measuring().capsule(0.0, 0.0, 19.0, Some(callout.state().icon()), word, 11.0, fill, ink, icon_ink));
        title_end -= chip;
        pen.capsule(title_end, y + 1.0, 19.0, Some(callout.state().icon()), word, 11.0, fill, ink, icon_ink);
        title_end -= 8.0;
    }
    pen.text_in(&card.title, rect(x + 25.0, y + 1.0, title_end - x - 25.0, 19.0), 13.5, Weight::Semibold, theme::INK.alpha(1.0), NSTextAlignment::Left, false);
    pen.text_in(&format!("“{}”", card.ask), rect(x, y + 25.0, width, 18.0), 12.5, Weight::Regular, theme::INK.alpha(1.0), NSTextAlignment::Left, false);
    let (ink, weight, icon) = match card.callout {
        Some(Callout::Asking) => (theme::ASKING, Weight::Semibold, "corner-down-right"),
        Some(Callout::Running) => (theme::RUNNING, Weight::Semibold, "timer"),
        _ if card.line.starts_with("작업 중") || card.line.starts_with("Working") => (theme::RUNNING, Weight::Semibold, "timer"),
        _ => (theme::MUTED, Weight::Regular, "corner-down-right"),
    };
    pen.icon(icon, rect(x, y + 50.0, 12.0, 12.0), ink);
    pen.text_in(&card.line, rect(x + 18.0, y + 47.0, width - 18.0, 17.0), 11.5, weight, ink.alpha(1.0), NSTextAlignment::Left, false);
    let mut cursor = x;
    for (icon, words) in [("clock", card.time.as_str()), ("folder", card.project.as_str()), ("git-commit-horizontal", card.marks.as_str())] {
        if words.is_empty() {
            continue;
        }
        pen.icon(icon, rect(cursor, y + 73.0, 11.0, 11.0), theme::MUTED);
        cursor += 15.0 + pen.text(words, cursor + 15.0, y + 70.0, 10.5, Weight::Regular, theme::MUTED.alpha(1.0)) + 12.0;
    }
    pen.hit(frame, Hit::Open(card.key.clone()));
}

/// The chip's word in the card's own language: the card's line tells which one the view used.
fn callout_word(card: &errand_core::journal::view::Card, callout: Callout) -> &'static str {
    let english = card.time.ends_with(" on") || card.marks.starts_with("Commits");
    callout.word(if english { Language::English } else { Language::Korean })
}

fn week(pen: &mut Pen, view: &WeekView, screen: &Screen) -> f64 {
    masthead(pen, &view.masthead, screen, false);
    let words = &screen.words;
    let top = 120.0;
    let label = 56.0;
    let rows: Vec<f64> = view.rows.iter().map(|row| if row.bars.is_empty() && !row.today { 14.0 } else { 4.0 + row.lanes as f64 * 16.0 + (row.lanes.saturating_sub(1)) as f64 * 2.0 }).collect();
    let height = 12.0 + 18.0 + 6.0 + 14.0 + 6.0 + rows.iter().sum::<f64>() + 5.0 * (rows.len() - 1) as f64 + 12.0;
    pen.panel(rect(PAD, top, WIDTH, height), 16.0, theme::WHITE, EDGE.alpha(1.0), 1.0);
    pen.text(words.by_weekday, PAD + 18.0, top + 12.0, 12.5, Weight::Semibold, theme::INK.alpha(1.0));
    legend(pen, PAD + WIDTH - 18.0, top + 12.0, false);
    let (x, width) = (PAD + 18.0 + label, WIDTH - 36.0 - label);
    hour_labels(pen, &view.axis, x, width, top + 36.0);
    let mut y = top + 56.0;
    for (row, row_height) in view.rows.iter().zip(&rows) {
        let quiet = row.bars.is_empty() && !row.today;
        let ink = if quiet { FAINT } else { theme::INK };
        pen.text(&row.label, PAD + 18.0, y + (row_height - 15.0) / 2.0, if quiet { 10.5 } else { 11.5 }, if row.today { Weight::Bold } else { Weight::Medium }, ink.alpha(1.0));
        pen.round(rect(x, y, width, *row_height), 5.0, Some(theme::INK.alpha(0.03)), None);
        for item in &row.bars {
            let frame = rect(x + item.start * width, y + 2.0 + item.lane as f64 * 18.0, ((item.end - item.start) * width).max(4.0), 16.0);
            bar(pen, item, frame, false);
        }
        if let Some(now) = row.now {
            pen.fill(rect(x + now * width - 0.75, y - 3.0, 1.5, row_height + 6.0), theme::INK.alpha(1.0));
        }
        y += row_height + 5.0;
    }
    // Projects, a card each with its work as lines; the cards flow three to a row.
    let mut y = top + height + 16.0;
    if view.projects.is_empty() {
        let note = if screen.loading { words.loading } else { words.empty };
        pen.text_in(note, rect(PAD, y + 30.0, WIDTH, 20.0), 13.0, Weight::Regular, theme::MUTED.alpha(1.0), NSTextAlignment::Center, false);
        return y + 80.0;
    }
    let card_width = ((WIDTH - 20.0) / 3.0).floor();
    for row in view.projects.chunks(3) {
        let tallest = row.iter().map(|project| 46.0 + project.lines.len() as f64 * 28.0).fold(0.0, f64::max);
        for (index, project) in row.iter().enumerate() {
            let frame = rect(PAD + index as f64 * (card_width + 10.0), y, card_width, 46.0 + project.lines.len() as f64 * 28.0);
            pen.panel(frame, 16.0, theme::WHITE, EDGE.alpha(1.0), 1.0);
            let (cx, cy, cw) = (frame.origin.x + 14.0, frame.origin.y + 4.0, frame.size.width - 28.0);
            pen.icon("folder", rect(cx, cy + 11.0, 14.0, 14.0), theme::MUTED);
            let name = pen.text(&project.name, cx + 20.0, cy + 9.0, 13.5, Weight::Semibold, theme::INK.alpha(1.0));
            pen.capsule(cx + 26.0 + name, cy + 9.0, 18.0, None, &project.count.to_string(), 11.0, theme::MUTED.alpha(0.12), theme::MUTED, theme::MUTED);
            let marks = text_width(&project.marks, 11.5, Weight::Regular);
            pen.text(&project.marks, cx + cw - marks, cy + 10.0, 11.5, Weight::Regular, theme::MUTED.alpha(1.0));
            for (line_index, line) in project.lines.iter().enumerate() {
                let ly = cy + 36.0 + line_index as f64 * 28.0;
                pen.fill(rect(frame.origin.x, ly, frame.size.width, 1.0), theme::INK.alpha(0.06));
                pen.app(line.provider, rect(cx, ly + 7.0, 14.0, 14.0));
                let when = text_width(&line.when, 11.0, Weight::Regular);
                pen.text(&line.when, cx + cw - when, ly + 7.0, 11.0, Weight::Regular, theme::MUTED.alpha(1.0));
                let mut end = cx + cw - when - 10.0;
                if let Some(pull) = &line.pull_request {
                    let words = text_width(pull, 11.0, Weight::Regular);
                    end -= words;
                    pen.text(pull, end, ly + 7.0, 11.0, Weight::Regular, theme::MUTED.alpha(1.0));
                    end -= 8.0;
                }
                if let Some(callout) = line.callout {
                    let (fill, ink, icon_ink) = chip_colors(callout);
                    let word = callout.word(if line.when.is_ascii() { Language::English } else { Language::Korean });
                    let chip = Pen::measuring().capsule(0.0, 0.0, 18.0, None, word, 10.5, fill, ink, icon_ink);
                    end -= chip;
                    pen.capsule(end, ly + 5.0, 18.0, None, word, 10.5, fill, ink, icon_ink);
                    end -= 8.0;
                }
                pen.text_in(&line.title, rect(cx + 21.0, ly + 5.0, end - cx - 21.0, 18.0), 12.5, Weight::Semibold, theme::INK.alpha(1.0), NSTextAlignment::Left, false);
                pen.hit(rect(frame.origin.x, ly, frame.size.width, 28.0), Hit::Open(line.key.clone()));
            }
        }
        y += tallest + 10.0;
    }
    y + 4.0
}

fn dot(pen: &Pen, frame: NSRect, provider: Provider, callout: Option<Callout>) {
    match callout {
        Some(Callout::Running) => pen.round(frame, frame.size.width / 2.0, Some(theme::WHITE.alpha(1.0)), Some((theme::RUNNING_BRIGHT.alpha(1.0), 1.5))),
        Some(Callout::Asking) => pen.round(frame, frame.size.width / 2.0, Some(theme::WHITE.alpha(1.0)), Some((theme::ASKING.alpha(1.0), 1.5))),
        _ => pen.round(frame, frame.size.width / 2.0, Some(service(provider).alpha(0.8)), None),
    }
}

fn month(pen: &mut Pen, view: &MonthView, screen: &Screen) -> f64 {
    masthead(pen, &view.masthead, screen, false);
    let words = &screen.words;
    let top = 120.0;
    let calendar_width = WIDTH - 334.0;
    let weeks = view.cells.len() / 7;
    let (full, short) = (124.0, 30.0);
    let grid_height = (0..weeks).map(|week| if week < view.full_rows { full } else { short }).sum::<f64>() + 4.0 * (weeks - 1) as f64;
    let height = 10.0 + 18.0 + 6.0 + grid_height + 10.0 + 16.0 + 14.0;
    pen.panel(rect(PAD, top, calendar_width, height), 16.0, theme::WHITE, EDGE.alpha(1.0), 1.0);
    let (gx, gw) = (PAD + 14.0, calendar_width - 28.0);
    let cell_width = ((gw - 24.0) / 7.0).floor();
    for (index, name) in view.weekdays.iter().enumerate() {
        pen.text(name, gx + index as f64 * (cell_width + 4.0) + 9.0, top + 10.0, 11.0, Weight::Regular, theme::MUTED.alpha(1.0));
    }
    let mut y = top + 34.0;
    for week in 0..weeks {
        let row_height = if week < view.full_rows { full } else { short };
        for weekday in 0..7 {
            let cell: &CalendarCell = &view.cells[week * 7 + weekday];
            let frame = rect(gx + weekday as f64 * (cell_width + 4.0), y, cell_width, row_height);
            let Some(number) = cell.day else { continue };
            if !cell.lines.is_empty() {
                pen.round(frame, 10.0, Some(theme::INK.alpha(0.035)), None);
            }
            if cell.today {
                pen.round(frame, 10.0, None, Some((theme::INK.alpha(1.0), 1.5)));
                pen.round(rect(frame.origin.x + 6.0, frame.origin.y + 5.0, 20.0, 20.0), 10.0, Some(theme::INK.alpha(1.0)), None);
            }
            let ink = if cell.today { theme::WHITE } else if cell.future { FAINT } else { theme::INK };
            pen.text_in(&number.to_string(), rect(frame.origin.x + 6.0, frame.origin.y + 7.0, 20.0, 16.0), 12.0, Weight::Semibold, ink.alpha(1.0), NSTextAlignment::Center, false);
            if !cell.lines.is_empty() {
                let count = cell.lines.len().to_string();
                let width = text_width(&count, 11.0, Weight::Regular);
                pen.text(&count, frame.origin.x + frame.size.width - 8.0 - width, frame.origin.y + 7.0, 11.0, Weight::Regular, theme::MUTED.alpha(1.0));
            }
            let room = ((row_height - 32.0) / 15.0).floor().max(0.0) as usize;
            let shown = if cell.lines.len() > room { room.saturating_sub(1) } else { cell.lines.len() };
            for (index, line) in cell.lines.iter().take(shown).enumerate() {
                let ly = frame.origin.y + 30.0 + index as f64 * 15.0;
                dot(pen, rect(frame.origin.x + 8.0, ly + 4.0, 7.0, 7.0), line.provider, line.callout);
                pen.text_in(&line.title, rect(frame.origin.x + 20.0, ly, frame.size.width - 26.0, 15.0), 10.5, Weight::Regular, theme::INK.alpha(1.0), NSTextAlignment::Left, false);
                pen.hit(rect(frame.origin.x, ly, frame.size.width, 15.0), Hit::Open(line.key.clone()));
            }
            if cell.lines.len() > shown {
                let more = words.more_lines.replace("{}", &(cell.lines.len() - shown).to_string());
                pen.text(&more, frame.origin.x + 8.0, frame.origin.y + 30.0 + shown as f64 * 15.0, 10.0, Weight::Regular, theme::MUTED.alpha(1.0));
            }
        }
        y += row_height + 4.0;
    }
    // The legend under the calendar.
    let mut lx = gx + 4.0;
    for (provider, name) in [(Provider::Codex, "Codex"), (Provider::Claude, "Claude")] {
        dot(pen, rect(lx, y + 9.0, 7.0, 7.0), provider, None);
        lx += 12.0 + pen.text(name, lx + 12.0, y + 5.0, 10.5, Weight::Regular, theme::MUTED.alpha(1.0)) + 14.0;
    }
    for callout in [Callout::Running, Callout::Asking] {
        dot(pen, rect(lx, y + 9.0, 7.0, 7.0), Provider::Codex, Some(callout));
        let english = view.weekdays[0] == "Sun";
        lx += 12.0 + pen.text(callout.word(if english { Language::English } else { Language::Korean }), lx + 12.0, y + 5.0, 10.5, Weight::Regular, theme::MUTED.alpha(1.0)) + 14.0;
    }
    // Projects and pull requests this month.
    let rx = PAD + calendar_width + 14.0;
    let rw = 320.0;
    let projects_height = 40.0 + view.projects.len() as f64 * 66.0 + 8.0;
    pen.panel(rect(rx, top, rw, projects_height), 16.0, theme::WHITE, EDGE.alpha(1.0), 1.0);
    pen.text(words.projects, rx + 14.0, top + 13.0, 12.5, Weight::Semibold, theme::INK.alpha(1.0));
    let mut py = top + 40.0;
    for project in &view.projects {
        split_row(pen, project, rx, py, rw);
        py += 66.0;
    }
    let pulls_top = top + projects_height + 14.0;
    let pulls_height = 40.0 + view.pull_requests.len().max(1) as f64 * 30.0 + 6.0;
    pen.panel(rect(rx, pulls_top, rw, pulls_height), 16.0, theme::WHITE, EDGE.alpha(1.0), 1.0);
    let title = pen.text(words.pull_requests, rx + 14.0, pulls_top + 13.0, 12.5, Weight::Semibold, theme::INK.alpha(1.0));
    pen.capsule(rx + 20.0 + title, pulls_top + 12.0, 18.0, None, &view.pull_requests.len().to_string(), 11.0, theme::MUTED.alpha(0.12), theme::MUTED, theme::MUTED);
    for (index, pull) in view.pull_requests.iter().enumerate() {
        let ry = pulls_top + 40.0 + index as f64 * 30.0;
        pen.fill(rect(rx, ry, rw, 1.0), theme::INK.alpha(0.06));
        pen.text(&pull.number, rx + 14.0, ry + 7.0, 12.0, Weight::Semibold, theme::NEW_RESULT.alpha(1.0));
        let when = text_width(&pull.when, 11.0, Weight::Regular);
        pen.text(&pull.when, rx + rw - 14.0 - when, ry + 8.0, 11.0, Weight::Regular, theme::MUTED.alpha(1.0));
        pen.text_in(&pull.title, rect(rx + 52.0, ry + 7.0, rw - 80.0 - when, 17.0), 12.5, Weight::Regular, theme::INK.alpha(1.0), NSTextAlignment::Left, false);
        pen.hit(rect(rx, ry, rw, 30.0), Hit::OpenTask(pull.url.clone()));
    }
    (top + height).max(pulls_top + pulls_height) + 24.0
}

fn split_row(pen: &Pen, project: &errand_core::journal::view::ProjectSplit, x: f64, y: f64, width: f64) {
    pen.fill(rect(x, y, width, 1.0), theme::INK.alpha(0.06));
    pen.icon("folder", rect(x + 14.0, y + 12.0, 14.0, 14.0), theme::MUTED);
    let name = pen.text(&project.name, x + 34.0, y + 10.0, 13.0, Weight::Semibold, theme::INK.alpha(1.0));
    pen.capsule(x + 40.0 + name, y + 10.0, 18.0, None, &project.count.to_string(), 11.0, theme::MUTED.alpha(0.12), theme::MUTED, theme::MUTED);
    let marks = text_width(&project.marks, 11.5, Weight::Regular);
    pen.text(&project.marks, x + width - 14.0 - marks, y + 11.0, 11.5, Weight::Regular, theme::MUTED.alpha(1.0));
    let bar_width = width - 28.0;
    let codex = (project.codex as f64 / project.count.max(1) as f64 * bar_width).round();
    if project.codex > 0 {
        pen.round(rect(x + 14.0, y + 35.0, codex - if project.claude > 0 { 1.0 } else { 0.0 }, 6.0), 3.0, Some(CODEX.alpha(0.75)), None);
    }
    if project.claude > 0 {
        pen.round(rect(x + 14.0 + codex + if project.codex > 0 { 1.0 } else { 0.0 }, y + 35.0, bar_width - codex - if project.codex > 0 { 1.0 } else { 0.0 }, 6.0), 3.0, Some(CLAUDE.alpha(0.75)), None);
    }
    pen.text(&format!("Codex {} · Claude {}", project.codex, project.claude), x + 14.0, y + 46.0, 10.5, Weight::Regular, theme::MUTED.alpha(1.0));
}

fn report(pen: &mut Pen, view: &ReportView, screen: &Screen) -> f64 {
    masthead(pen, &view.masthead, screen, true);
    let words = &screen.words;
    // A year of work, a square a day.
    let top = 120.0;
    let height = 12.0 + 18.0 + 8.0 + 14.0 + 4.0 + 136.0 + 14.0;
    pen.panel(rect(PAD, top, WIDTH, height), 16.0, theme::WHITE, EDGE.alpha(1.0), 1.0);
    let title = pen.text(words.history, PAD + 18.0, top + 12.0, 12.5, Weight::Semibold, theme::INK.alpha(1.0));
    let sub = if view.since.is_empty() { words.history_span.to_owned() } else { format!("{} · {}", words.history_span, view.since) };
    pen.text(&sub, PAD + 26.0 + title, top + 13.0, 11.5, Weight::Regular, theme::MUTED.alpha(1.0));
    let levels = [theme::INK.alpha(0.07), CODEX.alpha(0.26), CODEX.alpha(0.44), CODEX.alpha(0.62), CODEX.alpha(0.8), CODEX.alpha(1.0)];
    let mut lx = PAD + WIDTH - 18.0;
    lx -= text_width(words.more, 10.5, Weight::Regular);
    pen.text(words.more, lx, top + 13.0, 10.5, Weight::Regular, theme::MUTED.alpha(1.0));
    for level in levels.iter().rev() {
        lx -= 16.0;
        pen.round(rect(lx, top + 14.0, 12.0, 12.0), 3.0, Some(*level), None);
    }
    lx -= 6.0 + text_width(words.fewer, 10.5, Weight::Regular);
    pen.text(words.fewer, lx, top + 13.0, 10.5, Weight::Regular, theme::MUTED.alpha(1.0));
    let gx = PAD + 18.0 + 28.0;
    let months_y = top + 38.0;
    for (week, name) in &view.heatmap_months {
        pen.text(name, gx + *week as f64 * 20.0, months_y, 10.0, Weight::Regular, theme::MUTED.alpha(1.0));
    }
    let grid_y = months_y + 18.0;
    let weekday_names = if view.masthead.top == "tasks" { ["Mon", "Wed", "Fri"] } else { ["월", "수", "금"] };
    for (row, name) in [(1, weekday_names[0]), (3, weekday_names[1]), (5, weekday_names[2])] {
        pen.text(name, PAD + 18.0, grid_y + row as f64 * 20.0 + 1.0, 10.0, Weight::Regular, theme::MUTED.alpha(1.0));
    }
    for (index, level) in view.heatmap.iter().enumerate() {
        let Some(level) = level else { continue };
        let frame = rect(gx + (index / 7) as f64 * 20.0, grid_y + (index % 7) as f64 * 20.0, 16.0, 16.0);
        let fill = if *level < 0 { theme::INK.alpha(0.035) } else { levels[*level as usize] };
        pen.round(frame, 4.0, Some(fill), None);
    }
    if let Some(last) = view.heatmap.iter().rposition(Option::is_some) {
        pen.round(rect(gx + (last / 7) as f64 * 20.0, grid_y + (last % 7) as f64 * 20.0, 16.0, 16.0), 4.0, None, Some((theme::INK.alpha(1.0), 1.5)));
    }
    // Three charts under it.
    let y = top + height + 16.0;
    let chart_height = 330.0;
    let first_width = ((WIDTH - 28.0) * 1.35 / 3.35).floor();
    let other_width = ((WIDTH - 28.0 - first_width) / 2.0).floor();
    let frames = [rect(PAD, y, first_width, chart_height), rect(PAD + first_width + 14.0, y, other_width, chart_height), rect(PAD + first_width + other_width + 28.0, y, other_width, chart_height)];
    for frame in frames {
        pen.panel(frame, 16.0, theme::WHITE, EDGE.alpha(1.0), 1.0);
    }
    weekly_chart(pen, view, frames[0], words.weekly);
    project_chart(pen, view, frames[1], words.projects);
    hour_chart(pen, view, frames[2], words.hours, view.masthead.top == "tasks");
    y + chart_height + 24.0
}

fn weekly_chart(pen: &Pen, view: &ReportView, frame: NSRect, title: &str) {
    let (x, y, width) = (frame.origin.x + 16.0, frame.origin.y + 12.0, frame.size.width - 32.0);
    pen.text(title, x, y, 12.5, Weight::Semibold, theme::INK.alpha(1.0));
    legend(pen, x + width, y, true);
    let peak = view.weeks.iter().map(|week| week.codex + week.claude).max().unwrap_or(0).max(4);
    // Four steps of a round size, so every gridline is a whole number.
    let step = [1, 2, 5, 10, 20, 50, 100, 200].into_iter().find(|step| step * 4 >= peak).unwrap_or(peak.div_ceil(4));
    let top = step * 4;
    let (plot_x, plot_y, plot_width, plot_height) = (x + 24.0, y + 44.0, width - 24.0, 230.0);
    for index in 0..=4 {
        let value = step * index;
        let line_y = plot_y + plot_height - value as f64 / top as f64 * plot_height;
        pen.fill(rect(plot_x, line_y, plot_width, 1.0), if index == 0 { rgb(0.84, 0.84, 0.84).alpha(1.0) } else { rgb(0.937, 0.937, 0.937).alpha(1.0) });
        let label = value.to_string();
        let label_width = text_width(&label, 10.0, Weight::Regular);
        pen.text(&label, plot_x - 6.0 - label_width, line_y - 7.0, 10.0, Weight::Regular, theme::MUTED.alpha(1.0));
    }
    let slot = plot_width / view.weeks.len().max(1) as f64;
    let bar_width = (slot * 0.56).clamp(3.0, 18.0);
    let label_every = (view.weeks.len() / 6).max(1);
    for (index, week) in view.weeks.iter().enumerate() {
        let center = plot_x + slot * (index as f64 + 0.5);
        let codex = week.codex as f64 / top as f64 * plot_height;
        let claude = week.claude as f64 / top as f64 * plot_height;
        let base = plot_y + plot_height;
        if week.codex > 0 {
            let radius = if week.claude == 0 { 4.0f64.min(bar_width / 2.0) } else { 0.0 };
            pen.round(rect(center - bar_width / 2.0, base - codex, bar_width, codex), radius, Some(CODEX.alpha(1.0)), None);
        }
        if week.claude > 0 {
            let bottom = base - codex - if week.codex > 0 { 2.0 } else { 0.0 };
            pen.round(rect(center - bar_width / 2.0, bottom - claude, bar_width, claude), 4.0f64.min(bar_width / 2.0), Some(CLAUDE.alpha(1.0)), None);
        }
        if index == view.weeks.len() - 1 {
            let total = (week.codex + week.claude).to_string();
            let total_width = text_width(&total, 11.0, Weight::Semibold);
            pen.text(&total, center - total_width / 2.0, base - codex - claude - 20.0, 11.0, Weight::Semibold, theme::INK.alpha(1.0));
        }
        if index % label_every == 0 {
            let label_width = text_width(&week.label, 10.0, Weight::Regular);
            pen.text(&week.label, center - label_width / 2.0, base + 6.0, 10.0, Weight::Regular, theme::MUTED.alpha(1.0));
        }
    }
}

fn project_chart(pen: &Pen, view: &ReportView, frame: NSRect, title: &str) {
    let (x, y, width) = (frame.origin.x + 16.0, frame.origin.y + 12.0, frame.size.width - 32.0);
    pen.text(title, x, y, 12.5, Weight::Semibold, theme::INK.alpha(1.0));
    legend(pen, x + width, y, true);
    let most = view.projects.first().map(|project| project.count).unwrap_or(1).max(1);
    for (index, project) in view.projects.iter().enumerate() {
        let ry = y + 36.0 + index as f64 * 50.0;
        pen.icon("folder", rect(x, ry + 3.0, 13.0, 13.0), theme::MUTED);
        pen.text_in(&project.name, rect(x + 19.0, ry, width - 60.0, 18.0), 12.5, Weight::Semibold, theme::INK.alpha(1.0), NSTextAlignment::Left, false);
        let count = project.count.to_string();
        let count_width = text_width(&count, 11.5, Weight::Regular);
        pen.text(&count, x + width - count_width, ry + 1.0, 11.5, Weight::Regular, theme::MUTED.alpha(1.0));
        let full = width * project.count as f64 / most as f64;
        let codex = full * project.codex as f64 / project.count.max(1) as f64;
        if project.codex > 0 {
            pen.fill(rect(x, ry + 24.0, codex - if project.claude > 0 { 1.0 } else { 0.0 }, 8.0), CODEX.alpha(1.0));
        }
        if project.claude > 0 {
            let start = x + codex + if project.codex > 0 { 1.0 } else { 0.0 };
            pen.round(rect(start, ry + 24.0, x + full - start, 8.0), 4.0, Some(CLAUDE.alpha(1.0)), None);
            if project.codex > 0 {
                pen.fill(rect(start, ry + 24.0, 4.0, 8.0), CLAUDE.alpha(1.0));
            }
        }
    }
}

fn hour_chart(pen: &Pen, view: &ReportView, frame: NSRect, title: &str, english: bool) {
    let (x, y, width) = (frame.origin.x + 16.0, frame.origin.y + 12.0, frame.size.width - 32.0);
    pen.text(title, x, y, 12.5, Weight::Semibold, theme::INK.alpha(1.0));
    let peak_value = view.hours.iter().copied().max().unwrap_or(0);
    if let Some(peak) = view.hours.iter().position(|count| *count == peak_value && peak_value > 0) {
        let hour = view.first_hour as usize + peak;
        let note = if english { format!("Most at {hour}:00") } else { format!("가장 많은 때 {hour}시") };
        let note_width = text_width(&note, 11.0, Weight::Regular);
        pen.text(&note, x + width - note_width, y + 1.0, 11.0, Weight::Regular, theme::MUTED.alpha(1.0));
    }
    let (plot_y, plot_height) = (y + 44.0, 230.0);
    let slot = width / view.hours.len().max(1) as f64;
    let top = peak_value.max(1) as f64;
    pen.fill(rect(x, plot_y + plot_height, width, 1.0), rgb(0.84, 0.84, 0.84).alpha(1.0));
    for (index, count) in view.hours.iter().enumerate() {
        let height = *count as f64 / top * (plot_height - 24.0);
        let peak = *count == peak_value && peak_value > 0;
        let bar = rect(x + index as f64 * slot + 1.5, plot_y + plot_height - height, slot - 3.0, height);
        if *count > 0 {
            // Opaque, so the square foot drawn over the rounded bar does not darken where they overlap.
            let ink = if peak { theme::INK } else { rgb(0.66, 0.68, 0.69) };
            pen.round(bar, 4.0f64.min((slot - 3.0) / 2.0), Some(ink.alpha(1.0)), None);
            pen.fill(rect(bar.origin.x, bar.origin.y + bar.size.height - 4.0f64.min(bar.size.height), bar.size.width, 4.0f64.min(bar.size.height)), ink.alpha(1.0));
        }
        if peak {
            let label = count.to_string();
            let label_width = text_width(&label, 11.0, Weight::Semibold);
            pen.text(&label, bar.origin.x + bar.size.width / 2.0 - label_width / 2.0, bar.origin.y - 19.0, 11.0, Weight::Semibold, theme::INK.alpha(1.0));
        }
        let hour = view.first_hour as usize + index;
        if hour.is_multiple_of(3) {
            let label = format!("{hour:02}");
            let label_width = text_width(&label, 10.0, Weight::Regular);
            pen.text(&label, x + index as f64 * slot + slot / 2.0 - label_width / 2.0, plot_y + plot_height + 6.0, 10.0, Weight::Regular, theme::MUTED.alpha(1.0));
        }
    }
}

/// Lays out the body and returns how tall it is.
fn body(pen: &mut Pen, screen: &Screen) -> f64 {
    match &screen.body {
        Body::Day(view) => day(pen, view, screen),
        Body::Week(view) => week(pen, view, screen),
        Body::Month(view) => month(pen, view, screen),
        Body::Report(view) => report(pen, view, screen),
    }
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
    pen.fill(rect(0.0, TOP, SIZE.0, SIZE.1 - TOP), theme::INK.alpha(0.16));
    let panel = rect(SIZE.0 - 10.0 - 500.0, 60.0, 500.0, SIZE.1 - 70.0);
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
    let mut right = x + width;
    for pill in view.pills.iter().rev() {
        let pill_width = Pen::measuring().capsule(0.0, 0.0, 22.0, None, pill, 11.0, theme::MUTED.alpha(0.12), theme::MUTED, theme::MUTED);
        right -= pill_width;
        pen.capsule(right, y + 3.0, 22.0, None, pill, 11.0, theme::MUTED.alpha(0.12), theme::MUTED, theme::MUTED);
        right -= 6.0;
    }
    y += 42.0;
    pen.fill(rect(panel.origin.x, y, panel.size.width, 1.0), HAIRLINE.alpha(1.0));
    y += 16.0;
    // Each request and what came back, in order.
    pen.text(words.flow, x, y, 12.5, Weight::Semibold, theme::INK.alpha(1.0));
    y += 26.0;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layer {
    Top,
    Canvas,
    Overlay,
}

pub struct LayerIvars {
    layer: Layer,
    screen: RefCell<Option<Screen>>,
    hits: RefCell<Vec<(NSRect, Hit)>>,
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

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let ivars = self.ivars();
            let screen = ivars.screen.borrow();
            let Some(screen) = screen.as_ref() else { return };
            let mut pen = Pen { draw: true, hits: Vec::new() };
            if ivars.layer != Layer::Overlay {
                color(GROUND).setFill();
                NSBezierPath::fillRect(self.bounds());
            }
            match ivars.layer {
                Layer::Top => top_bar(&mut pen, screen),
                Layer::Canvas => {
                    body(&mut pen, screen);
                }
                Layer::Overlay => {
                    if let Some(view) = &screen.detail {
                        detail(&mut pen, view, screen);
                    }
                }
            }
            *ivars.hits.borrow_mut() = pen.hits;
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let point = self.convertPoint_fromView(event.locationInWindow(), None);
            let hit = self.ivars().hits.borrow().iter().rev().find(|(frame, _)| kit::contains(*frame, point)).map(|(_, hit)| hit.clone());
            if let Some(hit) = hit {
                // A press inside the panel that names the panel itself keeps it open.
                if matches!(&hit, Hit::Open(key) if self.ivars().layer == Layer::Overlay && self.ivars().screen.borrow().as_ref().and_then(|screen| screen.detail.as_ref()).is_some_and(|detail| &detail.key == key)) {
                    return;
                }
                self.ivars().poster.post(hit.message());
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
                Layer::Overlay => self.ivars().screen.borrow().as_ref().is_some_and(|screen| screen.detail.is_some()) && local.y >= TOP,
                Layer::Canvas => true,
            };
            if inside && owned { (self as *const JournalLayer).cast_mut().cast() } else { std::ptr::null_mut() }
        }
    }
);

impl JournalLayer {
    fn new(mtm: MainThreadMarker, frame: NSRect, layer: Layer, poster: Arc<dyn Poster>) -> Retained<JournalLayer> {
        let this = JournalLayer::alloc(mtm).set_ivars(LayerIvars { layer, screen: RefCell::new(None), hits: RefCell::new(Vec::new()), poster });
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }

    fn show(&self, screen: &Screen) {
        *self.ivars().screen.borrow_mut() = Some(screen.clone());
        self.setNeedsDisplay(true);
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

pub struct Journal {
    pub window: Retained<NSWindow>,
    top: Retained<JournalLayer>,
    canvas: Retained<JournalLayer>,
    overlay: Retained<JournalLayer>,
    scroll: Retained<NSScrollView>,
    _delegate: Retained<JournalDelegate>,
    shown: Option<Screen>,
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
        scroll.setDocumentView(Some(&canvas));
        content.addSubview(&scroll);
        let top = JournalLayer::new(mtm, rect(0.0, 0.0, width, TOP), Layer::Top, poster.clone());
        content.addSubview(&top);
        let overlay = JournalLayer::new(mtm, rect(0.0, 0.0, width, height), Layer::Overlay, poster.clone());
        overlay.setHidden(true);
        content.addSubview(&overlay);
        let delegate: Retained<JournalDelegate> = {
            let this = JournalDelegate::alloc(mtm).set_ivars(poster);
            unsafe { msg_send![super(this), init] }
        };
        window.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        window.center();
        Journal { window, top, canvas, overlay, scroll, _delegate: delegate, shown: None, tab: Cell::new(None) }
    }

    pub fn render(&mut self, screen: &Screen) {
        if self.shown.as_ref() == Some(screen) {
            return;
        }
        let mut measure = Pen::measuring();
        let height = body(&mut measure, screen).max(SIZE.1 - TOP);
        let frame = self.canvas.frame();
        if (frame.size.height - height).abs() > 0.5 {
            self.canvas.setFrameSize(NSSize::new(SIZE.0, height));
        }
        self.top.show(screen);
        self.canvas.show(screen);
        self.overlay.show(screen);
        self.overlay.setHidden(screen.detail.is_none());
        // A new view or period starts at its top.
        let place = (screen.place.tab, screen.place.back);
        if self.tab.get() != Some(place) {
            self.tab.set(Some(place));
            let clip = self.scroll.contentView();
            clip.scrollToPoint(NSPoint::new(0.0, 0.0));
            self.scroll.reflectScrolledClipView(&clip);
        }
        self.shown = Some(screen.clone());
    }

    pub fn show(&self, mtm: MainThreadMarker) {
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
        self.window.makeKeyAndOrderFront(None);
    }

    /// The window's content as drawn, for previews.
    pub fn content(&self) -> Option<Retained<NSView>> {
        self.window.contentView()
    }
}
