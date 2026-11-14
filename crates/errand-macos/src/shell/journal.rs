//! The desktop window: the person's work by day, week and month, a report of the last weeks, and one task's
//! whole record, drawn in the widget's own cards, chips and colours. Everything it shows comes from
//! `errand_core::journal::view`; this file only lays it out and draws it.

use super::kit::{self, Weight, color, ns, paint, rect};
use super::marks::draw_app_mark;
use errand_core::avatar::Avatar;
use errand_core::journal::metro::{COLOURED_LINES, MetroLine, MetroView};
use errand_core::journal::summary::Open;
use errand_core::journal::view::{Body, Callout, CalendarCell, DayView, DetailView, Draft, HourAxis, JournalTab, Masthead, MonthView, RecapCard, RecapView, ReportSpan, ReportView, Screen, Stat, Translate, WeekView, Words, translate_label};
use jiff::civil::Date;
use errand_core::model::Msg;
use errand_core::ports::Poster;
use errand_core::theme::{self, Paint, Rgb};
use errand_core::{Language, Provider};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAffineTransformNSAppKitAdditions, NSBitmapImageFileType, NSBitmapImageRep, NSDeviceRGBColorSpace, NSModalResponseOK, NSSavePanel,
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSApplication, NSBackingStoreType, NSBezierPath, NSColor, NSCompositingOperation, NSEvent,
    NSGraphicsContext, NSLineBreakMode, NSLineCapStyle, NSLineJoinStyle, NSMutableParagraphStyle, NSScrollView, NSShadow, NSStringDrawing, NSStringDrawingOptions, NSStringNSExtendedStringDrawing, NSTextAlignment, NSView, NSWindow, NSWindowDelegate,
    NSWindowStyleMask, NSWindowTitleVisibility,
};
use objc2_foundation::{NSAffineTransform, NSAttributedStringKey, NSDictionary, NSNotification, NSNumber, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize};
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
    Span(ReportSpan),
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
}

impl Hit {
    fn message(self) -> Msg {
        match self {
            Hit::Tab(tab) => Msg::JournalTab(tab),
            Hit::Step(step) => Msg::JournalStep(step),
            Hit::Span(span) => Msg::JournalSpan(span),
            Hit::Open(key) => Msg::JournalOpen(Some(key)),
            Hit::Close => Msg::JournalOpen(None),
            Hit::Copy(text) => Msg::JournalCopy(text),
            Hit::OpenTask(url) => Msg::JournalOpenTask(url),
            Hit::Translate(key, texts, language) => Msg::JournalTranslate { key, texts, language },
            Hit::Original(original) => Msg::JournalOriginal(original),
            Hit::Summarize(date) => Msg::JournalSummarize(date),
            Hit::EditSummary(date) => Msg::JournalEditSummary(date),
            Hit::Recap(card) => Msg::JournalRecap(card),
            Hit::SaveRecap(_) => Msg::Redraw,
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
    if !masthead.stats.is_empty() {
        stats(pen, &masthead.stats, PAD + WIDTH, 100.0);
        return;
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

/// Counts at the end of the date line, right-aligned, their figures' baseline at `baseline`.
fn stats(pen: &Pen, stats: &[Stat], right: f64, baseline: f64) {
    let mut x = right;
    for stat in stats.iter().rev() {
        let figure: f64 = stat.parts.iter().map(|part| text_width(&part.text, if part.small { 15.0 } else { 26.0 }, Weight::Bold)).sum();
        let label = text_width(&stat.label, 11.5, Weight::Semibold);
        let width = figure.max(label);
        x -= width;
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

fn day(pen: &mut Pen, view: &DayView, screen: &Screen) -> f64 {
    masthead(pen, &view.masthead, screen, false);
    let words = &screen.words;
    // The day, hour by hour: the record itself, large. Two lanes or more when work overlaps.
    let top = 120.0;
    let lane = 50.0;
    let plot = top + 14.0 + 18.0 + 10.0;
    let lanes_top = plot + 22.0;
    let height = 14.0 + 18.0 + 10.0 + 22.0 + view.lanes as f64 * lane + 26.0 + 14.0;
    pen.panel(rect(PAD, top, WIDTH, height), 18.0, theme::WHITE, EDGE.alpha(1.0), 1.0);
    pen.text(words.by_hour, PAD + 18.0, top + 14.0, 13.0, Weight::Bold, theme::INK.alpha(1.0));
    let mut right = PAD + WIDTH - 18.0;
    if view.bars.iter().any(|bar| bar.waiting.is_some()) {
        let words_width = text_width(words.waiting, 11.5, Weight::Regular);
        right -= words_width;
        pen.text(words.waiting, right, top + 15.0, 11.5, Weight::Regular, theme::MUTED.alpha(1.0));
        right -= 20.0;
        dashed(pen, right, right + 14.0, top + 23.0, theme::ASKING.alpha(0.75));
        right -= 16.0;
    }
    legend(pen, right, top + 14.0, false);
    let (x, width) = (PAD + 18.0, WIDTH - 36.0);
    hour_labels(pen, &view.axis, x, width, plot);
    let span = (view.axis.last - view.axis.first) as f64;
    let grid_height = view.lanes as f64 * lane + 4.0;
    for hour in view.axis.first..=view.axis.last {
        pen.fill(rect(x + (hour - view.axis.first) as f64 / span * width, plot + 18.0, 1.0, grid_height), rgb(0.941, 0.945, 0.949).alpha(1.0));
    }
    // The present goes under the work, so names that cross it stay readable.
    let now_place = view.axis.now.map(|now| x + now * width);
    if let Some(place) = now_place {
        pen.fill(rect(place - 0.75, plot + 16.0, 1.5, grid_height + 6.0), theme::INK.alpha(1.0));
    }
    for item in &view.bars {
        let left = x + item.start * width;
        let frame = rect(left, lanes_top + item.lane as f64 * lane, ((item.end - item.start) * width).max(14.0), 30.0);
        let running = item.callout == Some(Callout::Running);
        let ink = match item.callout {
            Some(Callout::Running) => theme::RUNNING,
            Some(Callout::Asking) => theme::ASKING,
            _ => theme::INK,
        };
        if let Some(until) = item.waiting {
            dashed(pen, frame.origin.x + frame.size.width, x + until * width, frame.origin.y + 15.0, theme::ASKING.alpha(0.75));
        }
        match item.callout {
            Some(Callout::Running) => pen.round(frame, 8.0, Some(theme::WHITE.alpha(1.0)), Some((theme::RUNNING_BRIGHT.alpha(0.7), 1.25))),
            Some(Callout::Asking) => pen.round(frame, 8.0, Some(ASKING_FILL.alpha(1.0)), Some((theme::ASKING.alpha(0.55), 1.5))),
            _ => pen.round(frame, 8.0, Some(service(item.provider).alpha(0.18)), Some((service(item.provider).alpha(0.55), 1.0))),
        }
        if running {
            pen.icon("loader-circle", rect(frame.origin.x + 7.0, frame.origin.y + 9.0, 12.0, 12.0), theme::RUNNING_BRIGHT);
        } else if frame.size.width >= 24.0 {
            pen.app(item.provider, rect(frame.origin.x + 6.0, frame.origin.y + 7.0, 16.0, 16.0));
        }
        pen.hit(frame, Hit::Open(item.key.clone()));
        let label_width = text_width(&item.label, 12.0, Weight::Bold);
        let next = view.bars.iter().filter(|other| other.lane == item.lane && other.start > item.start).map(|other| x + other.start * width).fold(x + width, f64::min);
        if label_width + 34.0 <= frame.size.width {
            pen.text_in(&item.label, rect(frame.origin.x + 28.0, frame.origin.y + 7.0, frame.size.width - 34.0, 17.0), 12.0, Weight::Bold, ink.alpha(1.0), NSTextAlignment::Left, false);
        } else {
            // Below the bar, as far as the next bar in its lane allows; the present line gives way.
            let room = (next - frame.origin.x - 8.0).min(180.0);
            if room >= 28.0 {
                let label = rect(frame.origin.x, frame.origin.y + 33.0, room.min(label_width + 2.0), 16.0);
                if now_place.is_some_and(|place| place > label.origin.x - 2.0 && place < label.origin.x + label.size.width + 2.0) {
                    pen.fill(rect(label.origin.x - 3.0, label.origin.y, label.size.width + 6.0, label.size.height), theme::WHITE.alpha(1.0));
                }
                pen.text_in(&item.label, label, 11.5, Weight::Semibold, ink.alpha(1.0), NSTextAlignment::Left, false);
            }
        }
    }
    if let Some(place) = now_place {
        let words_width = text_width(&view.axis.now_text, 11.0, Weight::Bold);
        let capsule = rect((place - words_width / 2.0 - 8.0).clamp(x, x + width - words_width - 16.0), lanes_top + view.lanes as f64 * lane + 2.0, words_width + 16.0, 20.0);
        pen.round(capsule, 10.0, Some(theme::INK.alpha(1.0)), None);
        pen.text(&view.axis.now_text, capsule.origin.x + 8.0, capsule.origin.y + 3.0, 11.0, Weight::Bold, theme::WHITE.alpha(1.0));
    }

    // Below: the day's summary as the character hands it over, beside the day's tasks.
    let y = top + height + 16.0;
    let side = 392.0;
    let draft_width = WIDTH - side - 16.0;
    let draft_height = draft(pen, view, screen, rect(PAD, y, draft_width, 0.0));
    let side_height = day_side(pen, view, screen, rect(PAD + draft_width + 16.0, y, side, 0.0));
    y + draft_height.max(side_height) + 24.0
}

/// A dashed line across, as the wait for the person's answer is drawn.
fn dashed(pen: &Pen, from: f64, to: f64, y: f64, ink: Paint) {
    if !pen.draw || to <= from {
        return;
    }
    let path = NSBezierPath::bezierPath();
    path.moveToPoint(NSPoint::new(from, y));
    path.lineToPoint(NSPoint::new(to, y));
    path.setLineWidth(1.5);
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

/// A white card with a tail pointing left at the character, as a speech bubble. Draws nothing when measuring.
fn bubble(pen: &Pen, frame: NSRect) {
    pen.panel(frame, 20.0, theme::WHITE, EDGE.alpha(1.0), 1.0);
    if !pen.draw {
        return;
    }
    let (x, y) = (frame.origin.x, frame.origin.y + 24.0);
    let tail = NSBezierPath::bezierPath();
    tail.moveToPoint(NSPoint::new(x + 1.0, y - 8.0));
    tail.lineToPoint(NSPoint::new(x - 8.0, y));
    tail.lineToPoint(NSPoint::new(x + 1.0, y + 8.0));
    tail.closePath();
    color(theme::WHITE).setFill();
    tail.fill();
    let edge = NSBezierPath::bezierPath();
    edge.moveToPoint(NSPoint::new(x + 0.5, y - 8.0));
    edge.lineToPoint(NSPoint::new(x - 8.0, y));
    edge.lineToPoint(NSPoint::new(x + 0.5, y + 8.0));
    edge.setLineWidth(1.0);
    paint(EDGE.alpha(1.0)).setStroke();
    edge.stroke();
}

/// A rounded button with an optional icon; returns its width. `dark` is the main action.
#[allow(clippy::too_many_arguments)]
fn button(pen: &mut Pen, right: f64, y: f64, icon: Option<&str>, words: &str, dark: bool, hit: Hit) -> f64 {
    let words_width = text_width(words, 12.5, Weight::Bold);
    let width = words_width + if icon.is_some() { 47.0 } else { 30.0 };
    let frame = rect(right - width, y, width, 32.0);
    let (fill, ink) = if dark { (theme::INK.alpha(1.0), theme::WHITE) } else { (rgb(0.933, 0.941, 0.949).alpha(1.0), theme::INK) };
    pen.round(frame, 16.0, Some(fill), None);
    let mut cursor = frame.origin.x + 15.0;
    if let Some(icon) = icon {
        pen.icon(icon, rect(cursor, y + 9.5, 13.0, 13.0), ink);
        cursor += 19.0;
    }
    pen.text(words, cursor, y + 7.5, 12.5, Weight::Bold, ink.alpha(1.0));
    pen.hit(frame, hit);
    width
}

/// The day's summary card, or what stands in for it; returns its height.
fn draft(pen: &mut Pen, view: &DayView, screen: &Screen, area: NSRect) -> f64 {
    let words = &screen.words;
    let (x, y) = (area.origin.x, area.origin.y);
    face(pen, screen, rect(x, y, 58.0, 58.0));
    let card_x = x + 70.0;
    let card_width = area.size.width - 70.0;
    let (inner_x, inner_width) = (card_x + 22.0, card_width - 44.0);
    let card = match &view.draft {
        Draft::Ready(card) => card,
        other => {
            // A short card: one line of what is happening and, when it helps, one button.
            let (note, ink, action): (&str, Rgb, Option<&String>) = match other {
                Draft::Nothing(note) => (note, theme::MUTED, None),
                Draft::Offer(note, button) => (note, theme::INK, Some(button)),
                Draft::Making(note) => (note, theme::RUNNING, None),
                Draft::Failed(why, button) => (why, theme::STALLED, button.as_ref()),
                Draft::Ready(_) => unreachable!(),
            };
            let height = 96.0;
            bubble(pen, rect(card_x, y, card_width, height));
            pen.text(&view_title(view, screen), inner_x, y + 18.0, 17.0, Weight::Bold, theme::INK.alpha(1.0));
            let mut note_x = inner_x;
            if matches!(other, Draft::Making(_)) {
                pen.icon("loader-circle", rect(inner_x, y + 54.0, 13.0, 13.0), theme::RUNNING_BRIGHT);
                note_x += 19.0;
            }
            pen.text_in(note, rect(note_x, y + 52.0, inner_width - 180.0, 36.0), 13.0, Weight::Medium, ink.alpha(1.0), NSTextAlignment::Left, true);
            if let Some(action) = action {
                button(pen, card_x + card_width - 22.0, y + 46.0, Some("refresh-cw"), action, true, Hit::Summarize(view.date));
            }
            return height;
        }
    };
    // Measure first: the lines decide the card's height.
    let row = 28.0;
    let left_height = if card.left.is_empty() { 0.0 } else { 13.0 + 20.0 + card.left.len() as f64 * row };
    let stale_height = if card.stale.is_some() { 36.0 } else { 0.0 };
    let height = 18.0 + 26.0 + 20.0 + 12.0 + card.done.len() as f64 * row + left_height + 14.0 + stale_height + 32.0 + 18.0;
    bubble(pen, rect(card_x, y, card_width, height));
    let mut cy = y + 18.0;
    let chip_width = Pen::measuring().capsule(0.0, 0.0, 22.0, None, &card.chip, 11.5, rgb(0.953, 0.957, 0.965).alpha(1.0), theme::MUTED, theme::MUTED);
    pen.text_in(&card.title, rect(inner_x, cy, inner_width - chip_width - 10.0, 26.0), 19.0, Weight::Bold, theme::INK.alpha(1.0), NSTextAlignment::Left, false);
    pen.capsule(inner_x + inner_width - chip_width, cy + 2.0, 22.0, None, &card.chip, 11.5, rgb(0.953, 0.957, 0.965).alpha(1.0), rgb(0.243, 0.282, 0.302), theme::MUTED);
    cy += 26.0;
    pen.text_in(&card.note, rect(inner_x, cy, inner_width, 18.0), 12.5, Weight::Regular, theme::MUTED.alpha(1.0), NSTextAlignment::Left, false);
    cy += 20.0 + 12.0;
    for line in &card.done {
        let frame = rect(inner_x, cy, inner_width, row);
        pen.icon("circle-check", rect(inner_x, cy + 6.0, 16.0, 16.0), rgb(0.243, 0.282, 0.302));
        pen.text_in(&line.project, rect(inner_x + 26.0, cy + 6.0, 92.0, 17.0), 12.0, Weight::Semibold, theme::MUTED.alpha(1.0), NSTextAlignment::Left, false);
        let mut text_right = inner_x + inner_width;
        if !line.mark.is_empty() {
            let mark_width = Pen::measuring().capsule(0.0, 0.0, 20.0, None, &line.mark, 11.0, theme::MUTED.alpha(0.12), theme::MUTED, theme::MUTED);
            pen.capsule(text_right - mark_width, cy + 4.0, 20.0, None, &line.mark, 11.0, theme::MUTED.alpha(0.12), rgb(0.243, 0.282, 0.302), theme::MUTED);
            text_right -= mark_width + 10.0;
        }
        pen.text_in(&line.text, rect(inner_x + 126.0, cy + 5.0, text_right - inner_x - 126.0, 18.0), 13.5, Weight::Regular, theme::INK.alpha(1.0), NSTextAlignment::Left, false);
        if let Some(key) = &line.key {
            pen.hit(frame, Hit::Open(key.clone()));
        }
        cy += row;
    }
    if !card.left.is_empty() {
        cy += 6.0;
        dashed(pen, inner_x, inner_x + inner_width, cy, rgb(0.867, 0.882, 0.894).alpha(1.0));
        cy += 7.0;
        pen.text(words.left, inner_x, cy, 12.0, Weight::Bold, theme::MUTED.alpha(1.0));
        cy += 20.0;
        for line in &card.left {
            let frame = rect(inner_x, cy, inner_width, row);
            let ink = match line.open {
                Open::Asking => {
                    pen.icon("message-circle-question-mark", rect(inner_x, cy + 6.0, 16.0, 16.0), theme::ASKING);
                    theme::ASKING
                }
                Open::Running => {
                    pen.icon("loader-circle", rect(inner_x, cy + 6.0, 16.0, 16.0), theme::RUNNING_BRIGHT);
                    theme::RUNNING
                }
                Open::Todo => {
                    pen.round(rect(inner_x + 2.0, cy + 8.0, 12.0, 12.0), 4.0, None, Some((theme::MUTED.alpha(1.0), 1.75)));
                    theme::INK
                }
            };
            let title_width = pen.text(&line.title, inner_x + 26.0, cy + 5.0, 13.5, Weight::Bold, ink.alpha(1.0)).min(inner_width * 0.5);
            pen.text_in(&format!(" · {}", line.text), rect(inner_x + 26.0 + title_width, cy + 5.0, inner_width - 26.0 - title_width, 18.0), 13.5, Weight::Regular, theme::INK.alpha(1.0), NSTextAlignment::Left, false);
            if let Some(key) = &line.key {
                pen.hit(frame, Hit::Open(key.clone()));
            }
            cy += row;
        }
    }
    cy += 14.0;
    if let Some((note, can_redo)) = &card.stale {
        let frame = rect(inner_x, cy, inner_width, 28.0);
        pen.round(frame, 10.0, Some(rgb(0.953, 0.957, 0.965).alpha(1.0)), None);
        pen.icon(if *can_redo { "refresh-cw" } else { "loader-circle" }, rect(inner_x + 10.0, cy + 7.5, 13.0, 13.0), theme::MUTED);
        pen.text(note, inner_x + 30.0, cy + 6.0, 12.5, Weight::Medium, rgb(0.243, 0.282, 0.302).alpha(1.0));
        if *can_redo {
            let redo_width = text_width(words.redo, 12.5, Weight::Bold);
            pen.text(words.redo, inner_x + inner_width - redo_width - 12.0, cy + 6.0, 12.5, Weight::Bold, theme::INK.alpha(1.0));
            pen.hit(frame, Hit::Summarize(card.date));
        }
        cy += 36.0;
    }
    let mut right = inner_x + inner_width;
    right -= button(pen, right, cy, Some("copy"), words.copy_report, true, Hit::Copy(card.report.clone())) + 8.0;
    right -= button(pen, right, cy, Some("pencil"), words.edit, false, Hit::EditSummary(card.date));
    pen.text_in(&card.footer, rect(inner_x, cy + 8.0, right - inner_x - 12.0, 17.0), 11.5, Weight::Regular, theme::MUTED.alpha(1.0), NSTextAlignment::Left, false);
    height
}

/// The draft card's heading when there is no summary yet.
fn view_title(view: &DayView, screen: &Screen) -> String {
    let past = view.masthead.can_go_forward;
    match (screen.language, past) {
        (Language::Korean, false) => "오늘 정리".to_owned(),
        (Language::Korean, true) => "이날 정리".to_owned(),
        (Language::English, false) => "Today's summary".to_owned(),
        (Language::English, true) => "That day's summary".to_owned(),
    }
}

/// The day's tasks as a short list, and the same day a month before; returns the height.
fn day_side(pen: &mut Pen, view: &DayView, screen: &Screen, area: NSRect) -> f64 {
    let words = &screen.words;
    let (x, y, width) = (area.origin.x, area.origin.y, area.size.width);
    let row = 46.0;
    let list_height = 6.0 + 34.0 + view.tasks.len().max(1) as f64 * row + 6.0;
    pen.panel(rect(x, y, width, list_height), 18.0, theme::WHITE, EDGE.alpha(1.0), 1.0);
    let heading = if view.masthead.can_go_forward { words.tasks_that_day } else { words.tasks_today };
    let heading_width = pen.text(heading, x + 16.0, y + 15.0, 13.0, Weight::Bold, theme::INK.alpha(1.0));
    pen.text(&view.tasks.len().to_string(), x + 16.0 + heading_width + 6.0, y + 15.0, 12.0, Weight::Semibold, theme::MUTED.alpha(1.0));
    let mut ry = y + 40.0;
    if view.tasks.is_empty() {
        let note = if screen.loading { words.loading } else { words.empty };
        pen.text_in(note, rect(x + 16.0, ry + 14.0, width - 32.0, 18.0), 12.5, Weight::Regular, theme::MUTED.alpha(1.0), NSTextAlignment::Left, false);
    }
    for task in &view.tasks {
        pen.fill(rect(x + 16.0, ry, width - 32.0, 1.0), HAIRLINE.alpha(1.0));
        pen.app(task.provider, rect(x + 16.0, ry + 14.0, 18.0, 18.0));
        let mut title_right = x + width - 16.0;
        if let Some(callout) = task.callout {
            let (fill, ink, icon_ink) = chip_colors(callout);
            let word = callout.word(screen.language);
            let chip = Pen::measuring().capsule(0.0, 0.0, 20.0, None, word, 11.0, fill, ink, icon_ink);
            pen.capsule(title_right - chip, ry + 13.0, 20.0, None, word, 11.0, fill, ink, icon_ink);
            title_right -= chip + 8.0;
        }
        pen.text_in(&task.title, rect(x + 44.0, ry + 6.0, title_right - x - 44.0, 18.0), 13.0, Weight::Bold, theme::INK.alpha(1.0), NSTextAlignment::Left, false);
        pen.text_in(&task.meta, rect(x + 44.0, ry + 24.0, title_right - x - 44.0, 16.0), 11.0, Weight::Regular, theme::MUTED.alpha(1.0), NSTextAlignment::Left, false);
        pen.hit(rect(x, ry, width, row), Hit::Open(task.key.clone()));
        ry += row;
    }
    let mut height = list_height;
    if let Some(back) = &view.back_then {
        let by = y + list_height + 12.0;
        let text_height = pen.text_height(&back.text, width - 32.0 - 44.0, 13.0, Weight::Semibold, 2);
        let card_height = 12.0 + 17.0 + 2.0 + text_height + 14.0;
        let frame = rect(x, by, width, card_height);
        pen.panel(frame, 18.0, theme::WHITE, EDGE.alpha(1.0), 1.0);
        let violet = rgb(0.357, 0.294, 0.769);
        pen.round(rect(x + 16.0, by + 12.0, 32.0, 32.0), 10.0, Some(violet.alpha(0.12)), None);
        pen.icon("rotate-ccw-clock", rect(x + 24.0, by + 20.0, 16.0, 16.0), violet);
        pen.text(&back.label, x + 60.0, by + 12.0, 11.5, Weight::Bold, violet.alpha(1.0));
        pen.text_in(&back.text, rect(x + 60.0, by + 31.0, width - 76.0, text_height), 13.0, Weight::Semibold, theme::INK.alpha(1.0), NSTextAlignment::Left, true);
        pen.hit(frame, Hit::Step(-back.days));
        height += 12.0 + card_height;
    }
    height
}

fn week(pen: &mut Pen, view: &WeekView, screen: &Screen) -> f64 {
    masthead(pen, &view.masthead, screen, false);
    let words = &screen.words;
    let mut top = 120.0;
    if !view.metro.is_empty() {
        top += metro(pen, &view.metro, PAD, top, WIDTH) + 16.0;
    }
    let side = 368.0;
    let sheet_width = WIDTH - side - 16.0;
    // The week as one page: each day's summary, in order.
    let row_line = 22.0;
    let day_height = |day: &errand_core::journal::view::WeekDay| 20.0 + (day.lines.len() as f64 * row_line + if day.note.is_some() || day.more > 0 { 24.0 } else { 0.0 }).max(40.0);
    let rows_height: f64 = view.days.iter().map(day_height).sum();
    let sheet_height = 18.0 + 46.0 + 12.0 + rows_height.max(60.0) + 10.0;
    let (x, y) = (PAD, top);
    pen.panel(rect(x, y, sheet_width, sheet_height), 20.0, theme::WHITE, EDGE.alpha(1.0), 1.0);
    let (inner_x, inner_width) = (x + 22.0, sheet_width - 44.0);
    pen.text(&view.title, inner_x, y + 18.0, 19.0, Weight::Bold, theme::INK.alpha(1.0));
    let mut right = inner_x + inner_width;
    if !view.report.is_empty() {
        right -= button(pen, right, y + 22.0, Some("copy"), words.copy_week, true, Hit::Copy(view.report.clone())) + 12.0;
    }
    pen.text_in(words.week_note, rect(inner_x, y + 44.0, right - inner_x, 18.0), 12.5, Weight::Regular, theme::MUTED.alpha(1.0), NSTextAlignment::Left, false);
    let mut ry = y + 18.0 + 46.0 + 12.0;
    if view.days.is_empty() {
        let note = if screen.loading { words.loading } else { words.empty };
        pen.text_in(note, rect(inner_x, ry + 20.0, inner_width, 18.0), 13.0, Weight::Regular, theme::MUTED.alpha(1.0), NSTextAlignment::Center, false);
    }
    let strip_width = 132.0;
    let lines_x = inner_x + 72.0;
    let lines_width = inner_width - 72.0 - strip_width - 14.0;
    for day in &view.days {
        pen.fill(rect(inner_x, ry, inner_width, 1.0), HAIRLINE.alpha(1.0));
        let top_y = ry + 10.0;
        pen.text(&day.day, inner_x, top_y - 2.0, 17.0, Weight::Bold, theme::INK.alpha(1.0));
        pen.text(&day.label, inner_x, top_y + 20.0, 12.0, Weight::Regular, theme::MUTED.alpha(1.0));
        let mut ly = top_y;
        for line in &day.lines {
            let mut text_right = lines_x + lines_width;
            if !line.mark.is_empty() {
                let mark_width = text_width(&line.mark, 11.0, Weight::Semibold);
                pen.text(&line.mark, text_right - mark_width, ly + 3.0, 11.0, Weight::Semibold, theme::MUTED.alpha(1.0));
                text_right -= mark_width + 10.0;
            }
            pen.text_in(&line.project, rect(lines_x, ly + 2.0, 92.0, 17.0), 11.5, Weight::Semibold, theme::MUTED.alpha(1.0), NSTextAlignment::Left, false);
            let faded = day.note.as_deref() == Some(words.no_summary);
            pen.text_in(&line.text, rect(lines_x + 100.0, ly + 1.0, text_right - lines_x - 100.0, 18.0), 13.0, Weight::Regular, if faded { rgb(0.243, 0.282, 0.302).alpha(1.0) } else { theme::INK.alpha(1.0) }, NSTextAlignment::Left, false);
            if let Some(key) = &line.key {
                pen.hit(rect(lines_x, ly, lines_width, row_line), Hit::Open(key.clone()));
            }
            ly += row_line;
        }
        if day.note.is_some() || day.more > 0 {
            let mut chip = day.note.clone().unwrap_or_default();
            if day.more > 0 {
                let more = words.more_lines.replace("{}", &day.more.to_string());
                chip = if chip.is_empty() { more } else { format!("{chip} · {more}") };
            }
            pen.capsule(lines_x, ly + 3.0, 20.0, None, &chip, 11.0, rgb(0.953, 0.957, 0.965).alpha(1.0), rgb(0.243, 0.282, 0.302), theme::MUTED);
        }
        // The day's work across 08–19, so the page still shows when it happened.
        let strip = rect(inner_x + inner_width - strip_width, top_y + 6.0, strip_width, 10.0);
        pen.round(strip, 5.0, Some(rgb(0.953, 0.957, 0.965).alpha(1.0)), None);
        for (start, end, provider) in &day.strip {
            pen.round(rect(strip.origin.x + start * strip_width, strip.origin.y, ((end - start) * strip_width).max(2.0), 10.0), 3.0, Some(service(*provider).alpha(0.55)), None);
        }
        ry += day_height(day);
    }

    // Beside it: the recap, what is left, and the kinds of work.
    let sx = PAD + sheet_width + 16.0;
    let mut sy = top;
    if let Some(recap) = &view.recap {
        let frame = rect(sx, sy, side, 232.0);
        if pen.draw {
            NSGraphicsContext::saveGraphicsState_class();
            let shadow = NSShadow::new();
            shadow.setShadowColor(Some(&paint(INDIGO.alpha(0.25))));
            shadow.setShadowBlurRadius(28.0);
            shadow.setShadowOffset(NSSize::new(0.0, -12.0));
            shadow.set();
            color(INDIGO).setFill();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(frame, 22.0, 22.0).fill();
            NSGraphicsContext::restoreGraphicsState_class();
            NSGraphicsContext::saveGraphicsState_class();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(frame, 22.0, 22.0).addClip();
            kit::draw_character(&Avatar::named(&screen.avatar), rect(sx + side - 150.0, sy + 92.0, 168.0, 168.0));
            NSGraphicsContext::restoreGraphicsState_class();
        }
        pen.text(&recap.note, sx + 22.0, sy + 20.0, 12.0, Weight::Bold, LAVENDER.alpha(1.0));
        pen.text_in(&recap.title, rect(sx + 22.0, sy + 40.0, side - 150.0, 60.0), 22.0, Weight::Bold, theme::WHITE.alpha(1.0), NSTextAlignment::Left, true);
        // A small stack of cards, the front one with the week's count.
        for (index, shade) in [rgb(0.290, 0.239, 0.549), rgb(0.416, 0.357, 0.753)].into_iter().enumerate() {
            pen.round(rect(sx + 22.0 + index as f64 * 30.0, sy + 112.0 - index as f64 * 3.0, 44.0, 60.0), 8.0, Some(shade.alpha(1.0)), None);
        }
        pen.round(rect(sx + 82.0, sy + 106.0, 44.0, 60.0), 8.0, Some(LAVENDER.alpha(1.0)), None);
        pen.text(&recap.count.to_string(), sx + 89.0, sy + 140.0, 18.0, Weight::Bold, INDIGO.alpha(1.0));
        let button_width = text_width(&recap.button, 13.0, Weight::Bold) + 46.0;
        let open = rect(sx + 22.0, sy + 232.0 - 22.0 - 34.0, button_width, 34.0);
        pen.round(open, 17.0, Some(theme::WHITE.alpha(1.0)), None);
        pen.text(&recap.button, open.origin.x + 16.0, open.origin.y + 8.0, 13.0, Weight::Bold, INDIGO.alpha(1.0));
        pen.icon("chevron-right", rect(open.origin.x + button_width - 26.0, open.origin.y + 10.0, 14.0, 14.0), INDIGO);
        pen.hit(frame, Hit::Recap(Some(0)));
        sy += 232.0 + 12.0;
    }
    if !view.carried.is_empty() {
        let height = 12.0 + 20.0 + 4.0 + view.carried.len() as f64 * 28.0 + 10.0;
        pen.panel(rect(sx, sy, side, height), 18.0, theme::WHITE, EDGE.alpha(1.0), 1.0);
        let heading = pen.text(&view.carried_title, sx + 16.0, sy + 13.0, 13.0, Weight::Bold, theme::INK.alpha(1.0));
        pen.text(&view.carried.len().to_string(), sx + 16.0 + heading + 6.0, sy + 13.0, 12.0, Weight::Semibold, theme::MUTED.alpha(1.0));
        let mut cy = sy + 36.0;
        for line in &view.carried {
            left_line(pen, line, sx + 16.0, cy, side - 32.0, 13.0);
            cy += 28.0;
        }
        sy += height + 12.0;
    }
    if !view.kinds.is_empty() {
        let height = 12.0 + 20.0 + 6.0 + view.kinds.len() as f64 * 22.0 + 10.0;
        pen.panel(rect(sx, sy, side, height), 18.0, theme::WHITE, EDGE.alpha(1.0), 1.0);
        pen.text(words.kinds, sx + 16.0, sy + 13.0, 13.0, Weight::Bold, theme::INK.alpha(1.0));
        let most = view.kinds.iter().map(|(_, count)| *count).max().unwrap_or(1).max(1) as f64;
        let mut ky = sy + 40.0;
        for (name, count) in &view.kinds {
            pen.text_in(name, rect(sx + 16.0, ky + 2.0, 70.0, 17.0), 12.0, Weight::Regular, theme::INK.alpha(1.0), NSTextAlignment::Left, false);
            let track = side - 32.0 - 80.0 - 26.0;
            pen.round(rect(sx + 96.0, ky + 6.0, (track * *count as f64 / most).max(4.0), 9.0), 4.0, Some(rgb(0.243, 0.282, 0.302).alpha(1.0)), None);
            let number = count.to_string();
            pen.text(&number, sx + side - 16.0 - text_width(&number, 12.0, Weight::Bold), ky + 2.0, 12.0, Weight::Bold, theme::INK.alpha(1.0));
            ky += 22.0;
        }
        sy += height + 12.0;
    }
    (y + sheet_height).max(sy) + 24.0
}

/// The metro map's line colours: violet, aqua, orange and berry, which stay apart for colour-blind eyes too; a
/// fifth project, or all the rest, share grey.
const LINE_COLOURS: [Rgb; COLOURED_LINES] = [rgb(0.384, 0.314, 0.839), rgb(0.106, 0.686, 0.478), rgb(0.922, 0.408, 0.204), rgb(0.753, 0.224, 0.541)];
const OTHER_LINE: Rgb = rgb(0.553, 0.592, 0.612);
const LANE_GAP: f64 = 74.0;
const DAY_BAND: Rgb = rgb(0.965, 0.969, 0.973);

fn line_colour(line: &MetroLine) -> Rgb {
    line.colour.map_or(OTHER_LINE, |slot| LINE_COLOURS[slot])
}

fn stroke(path: &NSBezierPath, width: f64, ink: Rgb, dash: Option<[f64; 2]>) {
    path.setLineWidth(width);
    path.setLineCapStyle(NSLineCapStyle::Round);
    path.setLineJoinStyle(NSLineJoinStyle::Round);
    if let Some(pattern) = dash {
        unsafe { path.setLineDash_count_phase(pattern.as_ptr() as *mut f64, 2, 0.0) };
    }
    color(ink).setStroke();
    path.stroke();
}

fn segment(from: (f64, f64), to: (f64, f64)) -> Retained<NSBezierPath> {
    let path = NSBezierPath::bezierPath();
    path.moveToPoint(NSPoint::new(from.0, from.1));
    path.lineToPoint(NSPoint::new(to.0, to.1));
    path
}

/// A stop: white inside, ringed in its line's colour; an interchange for a pull request; dashed while waiting.
fn station(pen: &Pen, x: f64, y: f64, radius: f64, ring: Rgb, pull: bool, asking: bool) {
    if !pen.draw {
        return;
    }
    let radius = if pull { radius + 2.0 } else { radius };
    let oval = NSBezierPath::bezierPathWithOvalInRect(rect(x - radius, y - radius, radius * 2.0, radius * 2.0));
    color(theme::WHITE).setFill();
    oval.fill();
    let ring = if pull { theme::INK } else if asking { theme::ASKING } else { ring };
    stroke(&oval, 3.5, ring, asking.then_some([4.0, 3.0]));
    if pull {
        let dot = (radius - 4.5).max(2.5);
        color(theme::INK).setFill();
        NSBezierPath::bezierPathWithOvalInRect(rect(x - dot, y - dot, dot * 2.0, dot * 2.0)).fill();
    }
}

/// The week as a metro map in a panel across the page; returns the panel's height.
fn metro(pen: &mut Pen, view: &MetroView, x: f64, y: f64, width: f64) -> f64 {
    let (inner_x, inner_width) = (x + 22.0, width - 44.0);
    let map_top = y + 80.0;
    let lane_y = |lane: usize| map_top + 80.0 + lane as f64 * LANE_GAP;
    let last_lane = lane_y(view.lines.len().max(1) - 1);

    // The legend flows in rows under the map: the lines, then what the marks mean.
    let mut legend: Vec<(Option<usize>, Option<usize>, &str, f64)> = Vec::new();
    for (index, line) in view.lines.iter().enumerate() {
        legend.push((Some(index), None, &line.legend, text_width(&line.legend, 12.0, Weight::Regular)));
    }
    for (index, words) in view.keys.iter().enumerate() {
        legend.push((None, Some(index), words, text_width(words, 12.0, Weight::Regular)));
    }
    let legend_top = last_lane + 96.0;
    let mut placed = Vec::new();
    let (mut lx, mut ly) = (inner_x, legend_top);
    for (line, key, words, words_width) in legend {
        let item = 22.0 + 6.0 + words_width;
        if lx > inner_x && lx + item > inner_x + inner_width {
            lx = inner_x;
            ly += 24.0;
        }
        placed.push((line, key, words, lx, ly));
        lx += item + 20.0;
    }
    let height = ly + 22.0 + 18.0 - y;

    pen.panel(rect(x, y, width, height), 20.0, theme::WHITE, EDGE.alpha(1.0), 1.0);
    pen.text(&view.title, inner_x, y + 18.0, 19.0, Weight::Bold, theme::INK.alpha(1.0));
    pen.text_in(&view.note, rect(inner_x, y + 44.0, inner_width, 18.0), 12.5, Weight::Regular, theme::MUTED.alpha(1.0), NSTextAlignment::Left, false);

    let carried: Vec<&MetroLine> = view.lines.iter().filter(|line| line.carried > 0).collect();
    let badge = 118.0;
    let start = inner_x + badge + 4.0;
    let end_room = if carried.is_empty() { 28.0 } else { 108.0 };
    let days = view.days.len().max(1);
    // A full week keeps its days wide enough for names by closing the gaps between them.
    let gap = (64.0 - (days as f64 - 4.0).max(0.0) * 10.0).max(34.0);
    let zone_width = ((inner_x + inner_width - end_room) - (start + 22.0) - gap * (days as f64 - 1.0)) / days as f64;
    let zone_x = |day: usize| start + 22.0 + day as f64 * (zone_width + gap);
    let last = days - 1;
    let zones_end = zone_x(last) + zone_width;

    for (index, day) in view.days.iter().enumerate() {
        let zx = zone_x(index);
        pen.round(rect(zx - 12.0, map_top + 26.0, zone_width + 24.0, last_lane + 58.0 - map_top - 26.0), 14.0, Some(DAY_BAND.alpha(1.0)), None);
        pen.text_in(&day.label, rect(zx - 12.0, map_top, zone_width + 24.0, 18.0), 13.0, Weight::Bold, theme::INK.alpha(1.0), NSTextAlignment::Center, false);
        pen.text_in(&day.total, rect(zx - 12.0, last_lane + 64.0, zone_width + 24.0, 16.0), 11.5, Weight::Semibold, theme::MUTED.alpha(1.0), NSTextAlignment::Center, false);
    }

    // The lines: a white casing under each, so a crossing reads as one line passing over another.
    if pen.draw {
        for line in &view.lines {
            let path = NSBezierPath::bezierPath();
            path.moveToPoint(NSPoint::new(start, lane_y(line.lanes[0])));
            for day in 0..view.days.len() {
                let here = lane_y(line.lanes[day]);
                path.lineToPoint(NSPoint::new(zone_x(day) + zone_width, here));
                if day < last {
                    let (from, to, next) = (zone_x(day) + zone_width, zone_x(day + 1), lane_y(line.lanes[day + 1]));
                    let middle = (from + to) / 2.0;
                    path.curveToPoint_controlPoint1_controlPoint2(NSPoint::new(to, next), NSPoint::new(middle, here), NSPoint::new(middle, next));
                }
            }
            stroke(&path, 13.0, theme::WHITE, None);
            stroke(&path, 7.0, line_colour(line), None);
        }
    }
    // Ends: a bar where a line stops, a dashed run to the terminal where work is still open.
    let terminal_x = inner_x + inner_width - 78.0;
    for line in &view.lines {
        let here = lane_y(line.lanes[last]);
        if !pen.draw {
            break;
        }
        if line.carried > 0 {
            stroke(&segment((zones_end + 8.0, here), (terminal_x - 6.0, here)), 5.0, line_colour(line), Some([7.0, 7.0]));
        } else {
            stroke(&segment((zones_end + 9.0, here - 11.0), (zones_end + 9.0, here + 11.0)), 7.0, line_colour(line), None);
        }
    }
    if !carried.is_empty() {
        let ys: Vec<f64> = carried.iter().map(|line| lane_y(line.lanes[last])).collect();
        let (low, high) = (ys.iter().copied().fold(f64::MAX, f64::min), ys.iter().copied().fold(f64::MIN, f64::max));
        let frame = rect(terminal_x, low - 26.0, 78.0, high - low + 52.0);
        pen.round(frame, 13.0, Some(theme::WHITE.alpha(1.0)), Some((theme::INK.alpha(1.0), 2.5)));
        let middle = frame.origin.y + frame.size.height / 2.0;
        pen.text_in(&view.terminal, rect(terminal_x, middle - 17.0, 78.0, 17.0), 12.5, Weight::Bold, theme::INK.alpha(1.0), NSTextAlignment::Center, false);
        pen.text_in(&view.terminal_count, rect(terminal_x, middle + 1.0, 78.0, 16.0), 11.0, Weight::Semibold, theme::MUTED.alpha(1.0), NSTextAlignment::Center, false);
    }
    // Each line's name at its start, in its colour.
    for line in &view.lines {
        let frame = rect(inner_x, lane_y(line.lanes[0]) - 13.0, badge, 26.0);
        pen.round(frame, 13.0, Some(line_colour(line).alpha(1.0)), None);
        pen.text_in(&line.name, kit::inset(frame, 10.0, 5.0), 12.0, Weight::Bold, theme::WHITE.alpha(1.0), NSTextAlignment::Center, false);
    }

    // Stops that began close together on one line move apart just enough not to cover each other.
    let mut spots: Vec<(f64, f64)> = view
        .stops
        .iter()
        .map(|stop| (zone_x(stop.day) + 12.0 + stop.at * (zone_width - 24.0), (5.5 + (stop.minutes.max(0) as f64).sqrt() * 0.55).min(16.0)))
        .collect();
    for line in 0..view.lines.len() {
        for day in 0..view.days.len() {
            let mut group: Vec<usize> = (0..view.stops.len()).filter(|&index| view.stops[index].line == line && view.stops[index].day == day).collect();
            group.sort_by(|&a, &b| spots[a].0.total_cmp(&spots[b].0));
            for pair in group.windows(2) {
                let least = spots[pair[0]].0 + spots[pair[0]].1 + spots[pair[1]].1 + 3.0;
                spots[pair[1]].0 = spots[pair[1]].0.max(least);
            }
            if let Some(&last) = group.last() {
                let over = spots[last].0 + spots[last].1 - (zone_x(day) + zone_width);
                if over > 0.0 {
                    group.iter().for_each(|&index| spots[index].0 -= over);
                }
            }
        }
    }

    // Stops, then their names on top, each kept inside its day.
    for (stop, &(sx, radius)) in view.stops.iter().zip(&spots) {
        let line = &view.lines[stop.line];
        let (zx, here) = (zone_x(stop.day), lane_y(line.lanes[stop.day]));
        station(pen, sx, here, radius, line_colour(line), stop.pull.is_some(), stop.asking);
        pen.hit(rect(sx - radius - 6.0, here - radius - 6.0, radius * 2.0 + 12.0, radius * 2.0 + 12.0), Hit::Open(stop.key.clone()));
        let Some(above) = stop.name_above else { continue };
        let weight = if stop.minutes >= 45 { Weight::Bold } else { Weight::Semibold };
        let words = text_width(&stop.title, 12.0, weight).min(zone_width + 12.0);
        let left = (sx - words / 2.0).clamp(zx - 6.0, zx + zone_width + 6.0 - words);
        let top = if above { here - radius - 8.0 - 17.0 } else { here + radius + 7.0 };
        pen.round(rect(left - 4.0, top, words + 8.0, 17.0), 5.0, Some(theme::WHITE.alpha(0.9)), None);
        pen.text_in(&stop.title, rect(left, top, words, 17.0), 12.0, weight, theme::INK.alpha(1.0), NSTextAlignment::Center, false);
        pen.hit(rect(left, top, words, 17.0), Hit::Open(stop.key.clone()));
        let tag = stop.pull.as_deref().or(stop.asking.then_some(view.waiting.as_str()));
        if let Some(tag) = tag {
            let tag_width = text_width(tag, 10.5, Weight::Bold) + 14.0;
            let tag_left = (sx - tag_width / 2.0).clamp(zx - 6.0, zx + zone_width + 6.0 - tag_width);
            let tag_top = if above { top - 4.0 - 16.0 } else { top + 17.0 + 4.0 };
            let frame = rect(tag_left, tag_top, tag_width, 16.0);
            if stop.pull.is_some() {
                pen.round(frame, 8.0, Some(theme::INK.alpha(1.0)), None);
                pen.text_in(tag, rect(tag_left, tag_top + 1.5, tag_width, 14.0), 10.5, Weight::Bold, theme::WHITE.alpha(1.0), NSTextAlignment::Center, false);
            } else {
                pen.round(frame, 8.0, Some(theme::WHITE.alpha(1.0)), Some((theme::ASKING.alpha(1.0), 1.0)));
                pen.text_in(tag, rect(tag_left, tag_top + 1.5, tag_width, 14.0), 10.5, Weight::Bold, theme::ASKING.alpha(1.0), NSTextAlignment::Center, false);
            }
        }
    }

    // The legend.
    for (line, key, words, lx, ly) in placed {
        let middle = ly + 11.0;
        match (line, key) {
            (Some(index), _) => pen.round(rect(lx + 2.0, middle - 3.0, 18.0, 6.0), 3.0, Some(line_colour(&view.lines[index]).alpha(1.0)), None),
            (_, Some(0)) => station(pen, lx + 11.0, middle, 5.5, theme::MUTED, false, false),
            (_, Some(1)) => station(pen, lx + 11.0, middle, 5.5, theme::MUTED, true, false),
            (_, Some(2)) => station(pen, lx + 11.0, middle, 5.5, theme::MUTED, false, true),
            (_, Some(3)) => {
                if pen.draw {
                    stroke(&segment((lx + 11.0, middle - 8.0), (lx + 11.0, middle + 8.0)), 5.0, theme::MUTED, None);
                }
            }
            _ => {
                if pen.draw {
                    stroke(&segment((lx + 2.0, middle), (lx + 20.0, middle)), 4.0, theme::MUTED, Some([5.0, 5.0]));
                }
            }
        }
        pen.text(words, lx + 28.0, ly + 3.0, 12.0, Weight::Regular, rgb(0.243, 0.282, 0.302).alpha(1.0));
    }
    height
}

/// A left-over item: its state's mark, its title in that state's colour, then why.
fn left_line(pen: &mut Pen, line: &errand_core::journal::view::LeftLine, x: f64, y: f64, width: f64, size: f64) {
    let ink = match line.open {
        Open::Asking => {
            pen.icon("message-circle-question-mark", rect(x, y + 6.0, 15.0, 15.0), theme::ASKING);
            theme::ASKING
        }
        Open::Running => {
            pen.icon("loader-circle", rect(x, y + 6.0, 15.0, 15.0), theme::RUNNING_BRIGHT);
            theme::RUNNING
        }
        Open::Todo => {
            pen.round(rect(x + 2.0, y + 8.0, 12.0, 12.0), 4.0, None, Some((theme::MUTED.alpha(1.0), 1.75)));
            theme::INK
        }
    };
    let title_width = pen.text(&line.title, x + 24.0, y + 5.0, size, Weight::Bold, ink.alpha(1.0)).min(width * 0.55);
    pen.text_in(&format!(" · {}", line.text), rect(x + 24.0 + title_width, y + 5.0, width - 24.0 - title_width, 18.0), size, Weight::Regular, theme::INK.alpha(1.0), NSTextAlignment::Left, false);
    if let Some(key) = &line.key {
        pen.hit(rect(x, y, width, 28.0), Hit::Open(key.clone()));
    }
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
    pen.fill(rect(0.0, TOP, SIZE.0, SIZE.1 - TOP), rgb(0.094, 0.078, 0.188).alpha(0.82));
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
    let pen = Pen { draw: true, hits: Vec::new() };
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
    // A month without pull requests has no card for them.
    if view.pull_requests.is_empty() {
        return (top + height).max(top + projects_height) + 24.0;
    }
    let pulls_top = top + projects_height + 14.0;
    let pulls_height = 40.0 + view.pull_requests.len() as f64 * 30.0 + 6.0;
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
    Canvas,
    Overlay,
    /// The scrolling body inside the task panel.
    DetailBody,
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
            match ivars.layer {
                Layer::Top | Layer::Canvas => color(GROUND).setFill(),
                Layer::DetailBody => color(theme::WHITE).setFill(),
                Layer::Overlay => NSColor::clearColor().setFill(),
            }
            if ivars.layer != Layer::Overlay {
                NSBezierPath::fillRect(self.bounds());
            }
            match ivars.layer {
                Layer::Top => top_bar(&mut pen, screen),
                Layer::Canvas => {
                    body(&mut pen, screen);
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
            *ivars.hits.borrow_mut() = pen.hits;
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let point = self.convertPoint_fromView(event.locationInWindow(), None);
            let hit = self.ivars().hits.borrow().iter().rev().find(|(frame, _)| kit::contains(*frame, point)).map(|(_, hit)| hit.clone());
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
            };
            // The panel's scrolling body is a subview of the overlay, so it is asked first.
            if inside && owned { unsafe { msg_send![super(self), hitTest: point] } } else { std::ptr::null_mut() }
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
    detail_scroll: Retained<NSScrollView>,
    detail_body: Retained<JournalLayer>,
    detail_key: Option<String>,
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
        self.overlay.setHidden(screen.detail.is_none() && screen.recap.is_none());
        self.detail_scroll.setHidden(screen.detail.is_none() || screen.recap.is_some());
        if let Some(detail) = &screen.detail {
            let visible = self.detail_scroll.frame().size;
            let height = detail_body(&mut Pen::measuring(), detail, &screen.words, screen.language).max(visible.height);
            self.detail_body.setFrameSize(NSSize::new(visible.width, height));
            self.detail_body.show(screen);
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
