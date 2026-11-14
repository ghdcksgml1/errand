//! A regular settings window for the provider and per-provider model used to suggest task text and to sum up
//! the desktop window's days, with the switch for those summaries.
use super::{details::{button, primary_button}, kit::{self, Action, Weight, label, ns, rect}};
use errand_core::{Language, Provider, model::Msg, ports::Poster, tasks::{self, AiSettings, ModelOption, State, words}, theme};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained, runtime::Bool, sel};
use objc2_app_kit::*;
use objc2_foundation::NSRect;
use std::{cell::RefCell, rc::Rc, sync::Arc};
pub const SIZE: (f64, f64) = (520.0, 620.0);

define_class!(
    #[unsafe(super(NSView))] #[thread_kind = MainThreadOnly] #[name = "ErrandSettingsView"]
    pub struct SettingsView;
    impl SettingsView {
        #[unsafe(method(isFlipped))] fn is_flipped(&self) -> bool { true }
        #[unsafe(method(drawRect:))] fn draw(&self, _dirty: NSRect) {
            // The desktop window's ground, and a white card under each of the two things to set.
            kit::color(theme::GROUND).setFill();
            NSBezierPath::fillRect(self.bounds());
            // 16 + 488 + 16 = 520: both groups share the same outer edges.
            for card in [rect(16.0, 116.0, SIZE.0 - 32.0, 164.0), rect(16.0, 292.0, SIZE.0 - 32.0, 158.0)] {
                let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(kit::inset(card, 0.5, 0.5), 14.0, 14.0);
                kit::color(theme::WHITE).setFill();
                path.fill();
                kit::color(theme::EDGE).setStroke();
                path.setLineWidth(1.0);
                path.stroke();
            }
        }
    }
);
define_class!(
    #[unsafe(super(NSWindow))] #[thread_kind = MainThreadOnly] #[name = "ErrandSettingsWindow"]
    struct SettingsWindow;
    impl SettingsWindow {
        // Errand is a menu-bar app without an Edit menu; model IDs still need standard editing shortcuts.
        #[unsafe(method(performKeyEquivalent:))]
        fn key_equivalent(&self, event: &NSEvent) -> Bool {
            let key = event.charactersIgnoringModifiers().map(|s| s.to_string().to_lowercase()).unwrap_or_default();
            let action = match key.as_str() {
                "x" => Some(sel!(cut:)), "c" => Some(sel!(copy:)), "v" => Some(sel!(paste:)), "a" => Some(sel!(selectAll:)),
                "z" if event.modifierFlags().contains(NSEventModifierFlags::Shift) => Some(sel!(redo:)),
                "z" => Some(sel!(undo:)), _ => None,
            };
            if let Some(action) = action.filter(|_| event.modifierFlags().contains(NSEventModifierFlags::Command)) {
                if self.firstResponder().is_some_and(|responder| unsafe { responder.tryToPerform_with(action, Some(self)) }) { return Bool::YES; }
            }
            unsafe { msg_send![super(self), performKeyEquivalent: event] }
        }
    }
);

pub(super) fn window(mtm: MainThreadMarker, size: (f64, f64), title: &str) -> Retained<NSWindow> {
    let window: Retained<SettingsWindow> = unsafe { msg_send![SettingsWindow::alloc(mtm), initWithContentRect: rect(0.0, 0.0, size.0, size.1), styleMask: NSWindowStyleMask::Titled | NSWindowStyleMask::Closable, backing: NSBackingStoreType::Buffered, defer: false] };
    let window = Retained::into_super(window);
    window.setTitle(&ns(title));
    unsafe { window.setReleasedWhenClosed(false) };
    window.setAppearance(NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua }).as_deref());
    window.setBackgroundColor(Some(&kit::color(theme::GROUND)));
    window
}

pub(super) fn options(provider: Provider, cached: &[ModelOption], language: Language) -> Vec<ModelOption> {
    let mut options = vec![ModelOption { id: String::new(), label: words(language, "기본 모델 (자동)", "Default model (automatic)").into() }];
    match provider {
        Provider::Codex => options.extend(cached.iter().cloned()),
        Provider::Claude => options.extend([("sonnet", "Sonnet"), ("opus", "Opus"), ("haiku", "Haiku"), ("fable", "Fable")].map(|(id, label)| ModelOption { id: id.into(), label: label.into() })),
    }
    options
}
pub(super) fn model_id(field: &NSComboBox, options: &[ModelOption]) -> String {
    let text = field.stringValue().to_string();
    options.iter().find(|option| option.label == text).map_or_else(|| text.trim().into(), |option| option.id.clone())
}
pub(super) fn fill(field: &NSComboBox, options: &[ModelOption], selected: &str) {
    unsafe {
        field.removeAllItems();
        for option in options { field.addItemWithObjectValue(&ns(&option.label)); }
    }
    field.setStringValue(&ns(options.iter().find(|option| option.id == selected).map_or(selected, |option| option.label.as_str())));
}
pub struct Settings {
    pub window: Retained<NSWindow>,
    pub view: Retained<SettingsView>,
    provider: Retained<NSPopUpButton>,
    model: Retained<NSComboBox>,
    auto: Retained<NSButton>,
    status: Retained<NSTextField>,
    local: Rc<RefCell<AiSettings>>,
    cached: Rc<RefCell<Vec<ModelOption>>>,
    choices: Rc<RefCell<Vec<ModelOption>>>,
    _actions: Vec<Retained<Action>>,
    applied: Option<AiSettings>,
    language: Language,
}
impl Settings {
    pub fn new(mtm: MainThreadMarker, poster: Arc<dyn Poster>, language: Language) -> Self {
        let window = window(mtm, SIZE, words(language, "Errand 설정", "Errand Settings"));
        let view: Retained<SettingsView> = unsafe { msg_send![SettingsView::alloc(mtm), initWithFrame: rect(0.0, 0.0, SIZE.0, SIZE.1)] };
        window.setContentView(Some(&view));
        let title = label(words(language, "AI 작업 도우미", "AI task assistant"), 24.0, Weight::Semibold, theme::INK, mtm);
        title.setFrame(rect(28.0, 22.0, 464.0, 32.0)); view.addSubview(&title);
        let subtitle = label(words(language, "제목·메모 추천, 작업 기록 요약과 번역에 사용할 AI를 선택해 주세요.", "Choose the AI for task titles, notes, work summaries and translations."), 13.0, Weight::Regular, theme::MUTED, mtm);
        subtitle.setFrame(rect(28.0, 64.0, 464.0, 42.0)); subtitle.setMaximumNumberOfLines(2); subtitle.setLineBreakMode(NSLineBreakMode::ByWordWrapping); view.addSubview(&subtitle);
        for (caption, y) in [(words(language, "AI 서비스", "AI service"), 139.0), (words(language, "모델", "Model"), 189.0)] {
            let text = label(caption, 13.0, Weight::Semibold, theme::INK, mtm); text.setFrame(rect(28.0, y + 5.0, 88.0, 20.0)); view.addSubview(&text);
        }
        let provider = NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), rect(126.0, 138.0, 366.0, 29.0), false);
        provider.addItemWithTitle(&ns("Codex")); provider.addItemWithTitle(&ns("Claude Code")); provider.setAccessibilityLabel(Some(&ns(words(language, "AI 서비스", "AI service")))); view.addSubview(&provider);
        let model = NSComboBox::initWithFrame(NSComboBox::alloc(mtm), rect(128.0, 188.0, 362.0, 28.0));
        model.setFont(Some(&kit::font(13.0, Weight::Regular))); model.setCompletes(false); model.setNumberOfVisibleItems(8);
        model.setAccessibilityLabel(Some(&ns(words(language, "모델", "Model")))); view.addSubview(&model);
        let hint = label(words(language, "목록에서 선택하거나 모델 ID를 직접 입력할 수 있어요.\n기본 모델을 그대로 사용해도 돼요.", "Choose from the list or enter a model ID.\nYou can keep the default model."), 12.0, Weight::Regular, theme::MUTED, mtm);
        hint.setFrame(rect(128.0, 226.0, 362.0, 40.0)); hint.setMaximumNumberOfLines(2); hint.setLineBreakMode(NSLineBreakMode::ByWordWrapping); view.addSubview(&hint);
        let heading = label(words(language, "작업 기록 요약", "Work summaries"), 15.0, Weight::Semibold, theme::INK, mtm);
        heading.setFrame(rect(28.0, 305.0, 300.0, 20.0)); view.addSubview(&heading);
        let auto = unsafe { NSButton::checkboxWithTitle_target_action(&ns(words(language, "하루 작업 기록 자동 요약", "Summarize each day's work automatically")), None, None, mtm) };
        auto.setFrame(rect(26.0, 332.0, 464.0, 24.0)); view.addSubview(&auto);
        let auto_hint = label(words(language, "켜면 지난 작업은 자동으로, 오늘 작업은 오후 6시부터 요약해요.\n창을 열지 않아도 아침에 지난 작업을 요약할 수 있어요.\n요청·답변 일부와 커밋 메시지가 선택한 AI로 전송되고,\n완성된 요약은 이 Mac에 저장돼요.", "When enabled, past work and today's work after 6 pm are summarized. Morning summaries can run with the window closed. Parts of requests, answers and commit messages go to the selected AI. Finished summaries are saved on this Mac."), 12.0, Weight::Regular, theme::MUTED, mtm);
        auto_hint.setFrame(rect(46.0, 363.0, 444.0, 76.0)); auto_hint.setMaximumNumberOfLines(5); auto_hint.setLineBreakMode(NSLineBreakMode::ByWordWrapping); view.addSubview(&auto_hint);
        let privacy = label(words(language, "이 Mac에 로그인된 Codex 또는 Claude Code 계정을 사용해요.\n제목·메모 추천에는 첨부 내용, 번역에는 답변 일부를 전송해요.\nAI 기능을 사용하면 해당 계정의 사용량에 반영돼요.", "Uses your Codex or Claude Code login on this Mac. Title and note suggestions and translations also send their content to the selected AI and may count toward your account's usage."), 12.0, Weight::Regular, theme::MUTED, mtm);
        privacy.setFrame(rect(28.0, 464.0, 464.0, 54.0)); privacy.setMaximumNumberOfLines(3); privacy.setLineBreakMode(NSLineBreakMode::ByWordWrapping); view.addSubview(&privacy);
        let status = label("", 12.0, Weight::Regular, theme::MUTED, mtm); status.setFrame(rect(28.0, 530.0, 464.0, 36.0)); status.setMaximumNumberOfLines(2); status.setLineBreakMode(NSLineBreakMode::ByWordWrapping); view.addSubview(&status);
        let local = Rc::new(RefCell::new(AiSettings::default()));
        let cached = Rc::new(RefCell::new(vec![]));
        let choices = Rc::new(RefCell::new(options(Provider::Codex, &[], language)));
        fill(&model, &choices.borrow(), "");
        let (draft, picker, model_field, cache, current_choices) = (local.clone(), provider.clone(), model.clone(), cached.clone(), choices.clone());
        let platform_action = Action::new(mtm, move || {
            let mut draft = draft.borrow_mut();
            let old_provider = draft.provider;
            draft.models[old_provider] = model_id(&model_field, &current_choices.borrow());
            draft.provider = if picker.indexOfSelectedItem() == 1 { Provider::Claude } else { Provider::Codex };
            *current_choices.borrow_mut() = options(draft.provider, &cache.borrow(), language);
            fill(&model_field, &current_choices.borrow(), draft.model());
        });
        platform_action.attach(&provider);
        let (draft, checkbox) = (local.clone(), auto.clone());
        let auto_action = Action::new(mtm, move || {
            draft.borrow_mut().summaries = checkbox.state() == NSControlStateValueOn;
        });
        auto_action.attach(&auto);
        let delete_poster = poster.clone();
        let (delete, delete_action) = button(words(language, "요약 삭제", "Delete summaries"), "", false, rect(348.0, 300.0, 144.0, 28.0), move || {
            let Some(mtm) = MainThreadMarker::new() else { return };
            let alert = NSAlert::new(mtm);
            alert.setMessageText(&ns(words(language, "저장된 작업 요약을 모두 지울까요?", "Delete every saved work summary?")));
            alert.setInformativeText(&ns(words(language, "삭제한 요약은 되돌릴 수 없어요. 자동 요약이 켜져 있으면 지난 7일의 작업이 다시 요약될 수 있어요.", "This cannot be undone. With automatic summaries on, work from the last 7 days may be summarized again.")));
            let go = alert.addButtonWithTitle(&ns(words(language, "지우기", "Delete")));
            go.setHasDestructiveAction(true);
            alert.addButtonWithTitle(&ns(words(language, "취소", "Cancel")));
            if alert.runModal() == NSAlertFirstButtonReturn {
                delete_poster.post(Msg::JournalDeleteSummaries);
            }
        }, mtm);
        view.addSubview(&delete);
        let (draft, model_field, current_choices) = (local.clone(), model.clone(), choices.clone());
        let (save, save_action) = primary_button(words(language, "저장", "Save"), "", false, rect(380.0, 576.0, 112.0, 32.0), move || {
            let mut settings = draft.borrow().clone();
            let provider = settings.provider;
            settings.models[provider] = model_id(&model_field, &current_choices.borrow());
            poster.post(Msg::Tasks(tasks::Msg::Configure(settings)));
        }, mtm);
        save.setKeyEquivalent(&ns("\r")); view.addSubview(&save);
        Self { window, view, provider, model, auto, status, local, cached, choices, _actions: vec![platform_action, auto_action, delete_action, save_action], applied: None, language }
    }
    pub fn render(&mut self, state: &State) {
        let new_settings = self.applied.as_ref() != Some(&state.settings);
        let new_models = *self.cached.borrow() != state.model_options;
        if new_settings || new_models {
            let mut local = self.local.borrow_mut();
            let model = if new_settings { state.settings.model().to_owned() } else { model_id(&self.model, &self.choices.borrow()) };
            if new_settings { *local = state.settings.clone(); }
            *self.cached.borrow_mut() = state.model_options.clone();
            *self.choices.borrow_mut() = options(local.provider, &self.cached.borrow(), self.language);
            self.provider.selectItemAtIndex(if local.provider == Provider::Claude { 1 } else { 0 });
            self.auto.setState(if local.summaries { NSControlStateValueOn } else { NSControlStateValueOff });
            fill(&self.model, &self.choices.borrow(), &model);
            self.applied = Some(state.settings.clone());
        }
        let message = state.settings_error.as_deref().map(|error| self.language.error_text(error)).unwrap_or(
            if state.settings_saved { words(self.language, "저장했어요.", "Saved.") }
            else { "" });
        self.status.setStringValue(&ns(message)); self.status.setToolTip(Some(&ns(message)));
    }
    pub fn show(&mut self, mtm: MainThreadMarker) {
        self.applied = None;
        if !self.window.isVisible() { self.window.center(); }
        #[allow(deprecated)] NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
        self.window.makeKeyAndOrderFront(None);
    }
}
