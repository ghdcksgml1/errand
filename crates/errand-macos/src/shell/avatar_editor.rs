//! The character editor beside the dock: the person's character, arrows either side of each part, the
//! clothes' colours, and a shuffle for a whole new one. Every pick is kept as soon as it is made.

use super::details::{PopoverView, button};
use super::dock::AvatarView;
use super::kit::{Action, FlippedView, Weight, color, icon_image, label, ns, rect};
use block2::RcBlock;
use errand_core::Language;
use errand_core::avatar::{Avatar, CLOTHES, PARTS};
use errand_core::layout::{AVATAR_EDITOR_HEIGHT, AVATAR_EDITOR_WIDTH};
use errand_core::model::Msg;
use errand_core::ports::Poster;
use errand_core::theme::{self, Rgb};
use errand_core::view::AvatarEditorVm;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSAccessibility, NSBezierPath, NSButton, NSCellImagePosition, NSImage, NSTextAlignment, NSTextField};
use objc2_foundation::{NSRect, NSSize};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

/// The parts' rows, beside the character.
const ROWS_TOP: f64 = 48.0;
const ROW: f64 = 28.0;
const SWATCH: f64 = 22.0;

pub struct AvatarEditor {
    pub view: Retained<PopoverView>,
    body: Retained<FlippedView>,
    preview: Retained<AvatarView>,
    names: Vec<Retained<NSTextField>>,
    choices: Vec<Retained<NSTextField>>,
    arrows: Vec<[Retained<NSButton>; 2]>,
    swatches: Vec<Retained<NSButton>>,
    /// The editor as last drawn: what each button picks.
    shown: Rc<RefCell<Option<AvatarEditorVm>>>,
    _actions: Vec<Retained<Action>>,
}

impl AvatarEditor {
    pub fn new(mtm: MainThreadMarker, poster: Arc<dyn Poster>, language: Language) -> AvatarEditor {
        let size = NSSize::new(AVATAR_EDITOR_WIDTH, AVATAR_EDITOR_HEIGHT);
        let view = PopoverView::new(mtm, size);
        let body = FlippedView::new(mtm, rect(0.0, 0.0, size.width, size.height));
        view.addSubview(&body);
        let heading = label(language.avatar_editor_title(), 20.0, Weight::Semibold, theme::INK, mtm);
        heading.setFrame(rect(20.0, 15.0, size.width - 40.0, 30.0));
        body.addSubview(&heading);
        let rows_height = PARTS.len() as f64 * ROW;
        let preview = AvatarView::new(mtm, rect(16.0, ROWS_TOP + (rows_height - 88.0) / 2.0, 88.0, 88.0), None);
        body.addSubview(&preview);
        let shown: Rc<RefCell<Option<AvatarEditorVm>>> = Rc::new(RefCell::new(None));
        // A button picks from the editor as last drawn; the borrow ends before the message goes out.
        let pick = |choose: fn(&AvatarEditorVm, usize) -> Avatar, index: usize| {
            let (shown, poster) = (shown.clone(), poster.clone());
            move || {
                let avatar = shown.borrow().as_ref().map(|vm| choose(vm, index));
                if let Some(avatar) = avatar {
                    poster.post(Msg::PickAvatar(avatar));
                }
            }
        };
        let mut actions = Vec::new();
        let (mut names, mut choices, mut arrows) = (Vec::new(), Vec::new(), Vec::new());
        for index in 0..PARTS.len() {
            let y = ROWS_TOP + index as f64 * ROW;
            let name = label("", 12.0, Weight::Regular, theme::MUTED, mtm);
            name.setFrame(rect(118.0, y + 5.0, 52.0, 18.0));
            let choice = label("", 12.5, Weight::Semibold, theme::INK, mtm);
            choice.setAlignment(NSTextAlignment::Center);
            choice.setFrame(rect(194.0, y + 5.0, 68.0, 18.0));
            let (previous, previous_action) = arrow("chevron-left", rect(170.0, y + 3.0, 22.0, 22.0), pick(|vm, index| vm.parts[index].previous, index), mtm);
            let (next, next_action) = arrow("chevron-right", rect(264.0, y + 3.0, 22.0, 22.0), pick(|vm, index| vm.parts[index].next, index), mtm);
            for view in [&*name, &*choice] {
                body.addSubview(view);
            }
            for button in [&previous, &next] {
                body.addSubview(button);
            }
            actions.extend([previous_action, next_action]);
            names.push(name);
            choices.push(choice);
            arrows.push([previous, next]);
        }
        let swatches_left = (size.width - CLOTHES.len() as f64 * SWATCH) / 2.0;
        let swatches = (0..CLOTHES.len())
            .map(|index| {
                let swatch = NSButton::new(mtm);
                swatch.setBordered(false);
                swatch.setImagePosition(NSCellImagePosition::ImageOnly);
                swatch.setFrame(rect(swatches_left + index as f64 * SWATCH, ROWS_TOP + rows_height + 12.0, SWATCH, SWATCH));
                let action = Action::new(mtm, pick(|vm, index| vm.clothes[index].avatar, index));
                action.attach(&swatch);
                actions.push(action);
                body.addSubview(&swatch);
                swatch
            })
            .collect();
        let shuffle_poster = poster.clone();
        let (shuffle, shuffle_action) = button(language.shuffle_avatar(), "shuffle", false, rect(14.0, size.height - 40.0, 96.0, 28.0), move || shuffle_poster.post(Msg::ShuffleAvatar), mtm);
        body.addSubview(&shuffle);
        let (close, close_action) = button(language.close(), "x", false, rect(size.width - 95.0, size.height - 40.0, 79.0, 28.0), move || poster.post(Msg::CloseAvatarEditor), mtm);
        body.addSubview(&close);
        actions.extend([shuffle_action, close_action]);
        AvatarEditor { view, body, preview, names, choices, arrows, swatches, shown, _actions: actions }
    }

    pub fn render(&mut self, vm: &AvatarEditorVm) {
        if self.shown.borrow().as_ref() == Some(vm) {
            return;
        }
        self.preview.show(vm.avatar);
        for (index, part) in vm.parts.iter().enumerate() {
            self.names[index].setStringValue(&ns(part.label));
            self.choices[index].setStringValue(&ns(part.choice));
            let [previous, next] = &self.arrows[index];
            previous.setAccessibilityLabel(Some(&ns(&part.previous_label)));
            next.setAccessibilityLabel(Some(&ns(&part.next_label)));
        }
        for (swatch, clothes) in self.swatches.iter().zip(&vm.clothes) {
            swatch.setImage(Some(&dot(clothes.colour, clothes.chosen)));
            swatch.setAccessibilityLabel(Some(&ns(clothes.label)));
            swatch.setToolTip(Some(&ns(clothes.label)));
        }
        *self.shown.borrow_mut() = Some(vm.clone());
    }

    /// Turns the tail toward the dock, level with the person's character.
    pub fn point(&self, tail_on_right: bool, tail_y: f64) {
        self.view.point(&self.body, tail_on_right, tail_y);
    }
}

/// A borderless chevron that steps a part along.
fn arrow(icon: &str, frame: NSRect, run: impl Fn() + 'static, mtm: MainThreadMarker) -> (Retained<NSButton>, Retained<Action>) {
    let button = NSButton::new(mtm);
    button.setBordered(false);
    button.setImage(icon_image(icon, 14.0, None, 0.0, 0.0).as_deref());
    button.setImagePosition(NSCellImagePosition::ImageOnly);
    button.setContentTintColor(Some(&color(theme::INK)));
    button.setFrame(frame);
    let action = Action::new(mtm, run);
    action.attach(&button);
    (button, action)
}

/// A dot of a clothes colour, ringed in ink when it is the one worn.
fn dot(colour: Rgb, chosen: bool) -> Retained<NSImage> {
    let handler = RcBlock::new(move |_: NSRect| -> Bool {
        color(colour).setFill();
        NSBezierPath::bezierPathWithOvalInRect(rect(4.0, 4.0, SWATCH - 8.0, SWATCH - 8.0)).fill();
        if chosen {
            let ring = NSBezierPath::bezierPathWithOvalInRect(rect(1.0, 1.0, SWATCH - 2.0, SWATCH - 2.0));
            ring.setLineWidth(2.0);
            color(theme::INK).setStroke();
            ring.stroke();
        }
        Bool::YES
    });
    NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(SWATCH, SWATCH), true, &handler)
}
