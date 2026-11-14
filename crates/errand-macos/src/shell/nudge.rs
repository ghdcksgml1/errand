//! The pet's morning bubble beside the widget: what the last day's work came to, with the way into it, to the
//! task waiting on the person's answer, or away until tomorrow.

use super::details::{PopoverView, button, primary_button};
use super::kit::{Action, FlippedView, Weight, label, ns, rect};
use errand_core::layout::{NUDGE_HEIGHT, NUDGE_WIDTH};
use errand_core::model::Msg;
use errand_core::ports::Poster;
use errand_core::theme;
use errand_core::view::NudgeVm;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSButton, NSLineBreakMode, NSTextField};
use objc2_foundation::NSSize;
use std::sync::Arc;

pub struct Nudge {
    pub view: Retained<PopoverView>,
    body: Retained<FlippedView>,
    title: Retained<NSTextField>,
    note: Retained<NSTextField>,
    see: Retained<NSButton>,
    answer: Retained<NSButton>,
    later: Retained<NSButton>,
    _actions: Vec<Retained<Action>>,
    shown: Option<NudgeVm>,
}

impl Nudge {
    pub fn new(mtm: MainThreadMarker, poster: Arc<dyn Poster>) -> Nudge {
        let view = PopoverView::new(mtm, NSSize::new(NUDGE_WIDTH, NUDGE_HEIGHT));
        let body = FlippedView::new(mtm, rect(0.0, 0.0, NUDGE_WIDTH, NUDGE_HEIGHT));
        view.addSubview(&body);
        let title = label("", 15.0, Weight::Semibold, theme::INK, mtm);
        title.setFrame(rect(18.0, 18.0, NUDGE_WIDTH - 36.0, 22.0));
        title.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        body.addSubview(&title);
        let note = label("", 12.0, Weight::Regular, theme::MUTED, mtm);
        note.setFrame(rect(18.0, 44.0, NUDGE_WIDTH - 36.0, 18.0));
        note.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        body.addSubview(&note);
        let post = |msg: fn() -> Msg| {
            let poster = poster.clone();
            move || poster.post(msg())
        };
        let (see, see_action) = primary_button("", "", false, rect(14.0, 76.0, 84.0, 30.0), post(|| Msg::NudgeSee), mtm);
        see.setKeyEquivalent(&ns("\r"));
        let (answer, answer_action) = button("", "", false, rect(100.0, 76.0, 104.0, 30.0), post(|| Msg::NudgeAnswer), mtm);
        let (later, later_action) = button("", "", false, rect(NUDGE_WIDTH - 14.0 - 76.0, 76.0, 76.0, 30.0), post(|| Msg::NudgeLater), mtm);
        for control in [&see, &answer, &later] {
            body.addSubview(control);
        }
        Nudge { view, body, title, note, see, answer, later, _actions: vec![see_action, answer_action, later_action], shown: None }
    }

    pub fn render(&mut self, vm: &NudgeVm) {
        if self.shown.as_ref() == Some(vm) {
            return;
        }
        self.title.setStringValue(&ns(&vm.title));
        self.note.setStringValue(&ns(&vm.note));
        self.note.setToolTip(Some(&ns(&vm.note)));
        self.see.setTitle(&ns(vm.see));
        self.later.setTitle(&ns(vm.later));
        match vm.answer {
            Some(words) => {
                self.answer.setTitle(&ns(words));
                self.answer.setHidden(false);
            }
            None => self.answer.setHidden(true),
        }
        self.shown = Some(vm.clone());
    }

    /// Turns the tail toward the widget, `tail_y` down the bubble.
    pub fn point(&self, tail_on_right: bool, tail_y: f64) {
        self.view.point(&self.body, tail_on_right, tail_y);
    }
}
