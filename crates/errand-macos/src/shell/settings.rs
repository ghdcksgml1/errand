//! A regular settings window for the provider and per-provider model used to suggest task text.
use super::{details::button, kit::{self, Action, Weight, label, ns, rect}};
use errand_core::{Language, Provider, model::Msg, ports::Poster, tasks::{self, AiSettings, ModelOption, State, words}, theme};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained, runtime::Bool, sel};
use objc2_app_kit::*;
use objc2_foundation::NSRect;
use std::{cell::RefCell, rc::Rc, sync::Arc};
pub const SIZE: (f64, f64) = (460.0, 336.0);

define_class!(
    #[unsafe(super(NSView))] #[thread_kind = MainThreadOnly] #[name = "ErrandSettingsView"]
    pub struct SettingsView;
    impl SettingsView {
        #[unsafe(method(isFlipped))] fn is_flipped(&self) -> bool { true }
        #[unsafe(method(drawRect:))] fn draw(&self, dirty: NSRect) {
            kit::color(theme::Rgb { red: 0.975, green: 0.971, blue: 0.961 }).setFill();
            NSBezierPath::fillRect(dirty);
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

fn options(provider: Provider, cached: &[ModelOption], language: Language) -> Vec<ModelOption> {
    let mut options = vec![ModelOption { id: String::new(), label: words(language, "기본 모델 (자동)", "Default model (automatic)").into() }];
    match provider {
        Provider::Codex => options.extend(cached.iter().cloned()),
        Provider::Claude => options.extend([("sonnet", "Sonnet"), ("opus", "Opus"), ("haiku", "Haiku"), ("fable", "Fable")].map(|(id, label)| ModelOption { id: id.into(), label: label.into() })),
    }
    options
}
fn model_id(field: &NSComboBox, options: &[ModelOption]) -> String {
    let text = field.stringValue().to_string();
    options.iter().find(|option| option.label == text).map_or_else(|| text.trim().into(), |option| option.id.clone())
}
fn fill(field: &NSComboBox, options: &[ModelOption], selected: &str) {
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
        let window: Retained<SettingsWindow> = unsafe { msg_send![SettingsWindow::alloc(mtm), initWithContentRect: rect(0.0, 0.0, SIZE.0, SIZE.1), styleMask: NSWindowStyleMask::Titled | NSWindowStyleMask::Closable, backing: NSBackingStoreType::Buffered, defer: false] };
        let window = Retained::into_super(window);
        window.setTitle(&ns(words(language, "Errand 설정", "Errand Settings")));
        unsafe { window.setReleasedWhenClosed(false) };
        window.setAppearance(NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua }).as_deref());
        window.setBackgroundColor(Some(&NSColor::windowBackgroundColor()));
        let view: Retained<SettingsView> = unsafe { msg_send![SettingsView::alloc(mtm), initWithFrame: rect(0.0, 0.0, SIZE.0, SIZE.1)] };
        window.setContentView(Some(&view));
        let title = label(words(language, "AI 작업 도우미", "AI task assistant"), 21.0, Weight::Semibold, theme::INK, mtm);
        title.setFrame(rect(26.0, 23.0, 400.0, 28.0)); view.addSubview(&title);
        let subtitle = label(words(language, "첨부파일에서 제목과 메모를 추천할 AI를 골라 주세요.", "Choose the AI that suggests titles and notes from attachments."), 12.0, Weight::Regular, theme::MUTED, mtm);
        subtitle.setFrame(rect(27.0, 60.0, 408.0, 32.0)); subtitle.setMaximumNumberOfLines(2); subtitle.setLineBreakMode(NSLineBreakMode::ByWordWrapping); view.addSubview(&subtitle);
        for (caption, y) in [(words(language, "플랫폼", "Platform"), 108.0), (words(language, "모델", "Model"), 155.0)] {
            let text = label(caption, 12.0, Weight::Semibold, theme::INK, mtm); text.setFrame(rect(28.0, y + 5.0, 80.0, 20.0)); view.addSubview(&text);
        }
        let provider = NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), rect(112.0, 105.0, 318.0, 29.0), false);
        provider.addItemWithTitle(&ns("Codex")); provider.addItemWithTitle(&ns("Claude Code")); provider.setAccessibilityLabel(Some(&ns(words(language, "플랫폼", "Platform")))); view.addSubview(&provider);
        let model = NSComboBox::initWithFrame(NSComboBox::alloc(mtm), rect(114.0, 153.0, 314.0, 28.0));
        model.setFont(Some(&kit::font(13.0, Weight::Regular))); model.setCompletes(false); model.setNumberOfVisibleItems(8);
        model.setAccessibilityLabel(Some(&ns(words(language, "모델", "Model")))); view.addSubview(&model);
        let hint = label(words(language, "목록에서 선택하거나 모델 ID를 직접 입력할 수 있어요.", "Choose from the list or enter a model ID."), 11.0, Weight::Regular, theme::MUTED, mtm);
        hint.setFrame(rect(115.0, 190.0, 315.0, 32.0)); hint.setMaximumNumberOfLines(2); hint.setLineBreakMode(NSLineBreakMode::ByWordWrapping); view.addSubview(&hint);
        let privacy = label(words(language, "로컬 CLI의 로그인 계정을 사용합니다.\n첨부 내용은 선택한 모델 서비스로 전송됩니다.", "Uses your local CLI login.\nAttachment content is sent to the selected model service."), 11.0, Weight::Regular, theme::MUTED, mtm);
        privacy.setFrame(rect(28.0, 230.0, 402.0, 36.0)); privacy.setMaximumNumberOfLines(2); view.addSubview(&privacy);
        let status = label("", 11.0, Weight::Regular, theme::MUTED, mtm); status.setFrame(rect(28.0, 287.0, 292.0, 34.0)); status.setMaximumNumberOfLines(2); status.setLineBreakMode(NSLineBreakMode::ByWordWrapping); view.addSubview(&status);
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
        let (draft, model_field, current_choices) = (local.clone(), model.clone(), choices.clone());
        let (save, save_action) = button(words(language, "저장", "Save"), "", false, rect(338.0, 285.0, 91.0, 30.0), move || {
            let mut settings = draft.borrow().clone();
            let provider = settings.provider;
            settings.models[provider] = model_id(&model_field, &current_choices.borrow());
            poster.post(Msg::Tasks(tasks::Msg::Configure(settings)));
        }, mtm);
        save.setKeyEquivalent(&ns("\r")); view.addSubview(&save);
        Self { window, view, provider, model, status, local, cached, choices, _actions: vec![platform_action, save_action], applied: None, language }
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
            fill(&self.model, &self.choices.borrow(), &model);
            self.applied = Some(state.settings.clone());
        }
        let message = state.settings_error.as_deref().unwrap_or(if state.settings_saved { words(self.language, "저장했어요.", "Saved.") } else { "" });
        self.status.setStringValue(&ns(message)); self.status.setToolTip(Some(&ns(message)));
    }
    pub fn show(&mut self, mtm: MainThreadMarker) {
        self.applied = None;
        if !self.window.isVisible() { self.window.center(); }
        #[allow(deprecated)] NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
        self.window.makeKeyAndOrderFront(None);
    }
}
