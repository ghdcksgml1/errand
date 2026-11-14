//! First-run guidance, separate from the everyday settings window. Steps only edit a local draft;
//! finishing saves it before opening work records. Account status comes from the normal app reads.
use super::{compact::{CompactEvents, Simple}, details::{button, primary_button}, dock::AvatarView,
    kit::{self, Action, FlippedView, Weight, label, ns, rect}, marks::AppMarkView, settings};
use errand_core::{Language, Provider, avatar::Avatar, model::{AppState, Msg}, ports::Poster,
    tasks::{self, AiSettings, ModelOption, words}, theme, view};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained};
use objc2_app_kit::*;
use objc2_foundation::{NSPoint, NSRect};
use std::{cell::RefCell, rc::Rc, sync::Arc};

pub const SIZE: (f64, f64) = (720.0, 640.0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step { Welcome, Accounts, Ai, Ready }
impl Step {
    const ALL: [Self; 4] = [Self::Welcome, Self::Accounts, Self::Ai, Self::Ready];
    fn index(self) -> usize { self as usize }
}

define_class!(
    #[unsafe(super(NSView))] #[thread_kind = MainThreadOnly] #[name = "ErrandOnboardingView"]
    pub struct OnboardingView;
    impl OnboardingView {
        #[unsafe(method(isFlipped))] fn is_flipped(&self) -> bool { true }
        #[unsafe(method(drawRect:))] fn draw(&self, _dirty: NSRect) {
            kit::color(theme::GROUND).setFill();
            NSBezierPath::fillRect(self.bounds());
        }
    }
);
define_class!(
    /// The example is made with the real widget, but none of its sample tasks can be opened.
    #[unsafe(super(NSView))] #[thread_kind = MainThreadOnly] #[name = "ErrandOnboardingExample"]
    struct Example;
    impl Example {
        #[unsafe(method(isFlipped))] fn is_flipped(&self) -> bool { true }
        #[unsafe(method(hitTest:))] fn hit_test(&self, _point: NSPoint) -> *mut NSView { std::ptr::null_mut() }
    }
);

fn text(parent: &NSView, value: &str, frame: NSRect, size: f64, weight: Weight, muted: bool, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let field = label(value, size, weight, if muted { theme::MUTED.blend(0.15, theme::INK) } else { theme::INK }, mtm);
    field.setFrame(frame);
    field.setMaximumNumberOfLines(0);
    field.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
    parent.addSubview(&field);
    field
}

fn skip_example_controls(view: &NSView) {
    for child in view.subviews() {
        skip_example_controls(&child);
        if let Ok(control) = child.downcast::<NSControl>() { control.setRefusesFirstResponder(true); }
    }
}

struct Flow {
    step: Step,
    active: bool,
    submitting: bool,
    open_records: bool,
    draft: AiSettings,
    cached: Vec<ModelOption>,
    choices: Vec<ModelOption>,
    language: Language,
    pages: Vec<Retained<FlippedView>>,
    navigation: Vec<Retained<NSTextField>>,
    heading: Retained<NSTextField>,
    subtitle: Retained<NSTextField>,
    back: Retained<NSButton>,
    skip: Retained<NSButton>,
    next: Retained<NSButton>,
    provider: Retained<NSPopUpButton>,
    model: Retained<NSComboBox>,
    auto: Retained<NSButton>,
    status: Retained<NSTextField>,
    local_error: Option<String>,
    account_status: Vec<Retained<NSTextField>>,
    account_detail: Vec<Retained<NSTextField>>,
    open_apps: Vec<Retained<NSButton>>,
    review_ai: Retained<NSTextField>,
    review_auto: Retained<NSTextField>,
}
impl Flow {
    fn read_model(&mut self) {
        self.draft.models[self.draft.provider] = settings::model_id(&self.model, &self.choices);
    }

    fn message(&self, error: Option<&str>) {
        let value = self.local_error.as_deref().or(error).map(|value| self.language.error_text(value)).unwrap_or("");
        self.status.setStringValue(&ns(value));
        self.status.setToolTip(Some(&ns(value)));
        self.status.setTextColor(Some(&kit::color(theme::STALLED.blend(0.1, theme::INK))));
    }

    fn show_step(&mut self, step: Step) {
        self.step = step;
        let say = |ko, en| words(self.language, ko, en);
        let (heading, subtitle, next) = match step {
            Step::Welcome => (say("AI에 맡긴 일, 한눈에 확인하세요", "Your AI work, at a glance"),
                say("Errand는 Codex와 Claude의 작업 상태와 남은 사용량을\n바탕화면 한쪽에서 보여주는 앱이에요.", "Errand keeps your Codex and Claude tasks and remaining usage\nin a small widget on your desktop."), say("둘러보기", "Take a look")),
            Step::Accounts => (say("쓰고 있는 계정을 확인해요", "Check the accounts you use"),
                say("이 Mac의 Codex와 Claude에 로그인된 계정을 사용해요.\n아직 로그인하지 않았다면 해당 앱을 열어 주세요.", "Errand uses the Codex and Claude accounts on this Mac.\nOpen either app if you still need to sign in."), say("다음", "Continue")),
            Step::Ai => (say("AI가 도와줄 일을 골라 주세요", "Choose how AI helps you"),
                say("제목·메모 추천, 번역과 요약에 사용할 AI를 정해요.\n자동 요약은 기본으로 꺼져 있어요. 나중에 바꿀 수 있어요.", "Choose the AI for titles, notes, translations and summaries.\nAutomatic summaries are off by default. You can change this later."), say("설정 확인", "Review choices")),
            Step::Ready => (say("이제 첫 작업을 확인해 보세요", "You're ready for your first task"),
                say("선택한 설정을 저장하고 작업 기록으로 이동할게요.\n아직 작업이 없다면 Codex나 Claude에서 하나를 시작해 보세요.", "Save your choices and open your work records.\nNo tasks yet? Start one in Codex or Claude when you're ready."), say("작업 기록 열기", "Open work records")),
        };
        self.heading.setStringValue(&ns(heading));
        self.subtitle.setStringValue(&ns(subtitle));
        self.next.setTitle(&ns(next));
        self.back.setHidden(step == Step::Welcome);
        self.skip.setFrameOrigin(NSPoint::new(if step == Step::Welcome { 48.0 } else { 140.0 }, 586.0));
        for (index, page) in self.pages.iter().enumerate() { page.setHidden(index != step.index()); }
        for (index, label) in self.navigation.iter().enumerate() {
            let current = index == step.index();
            label.setTextColor(Some(&kit::color(if current { theme::INK } else { theme::MUTED.blend(0.15, theme::INK) })));
            label.setFont(Some(&kit::font(12.0, if current { Weight::Semibold } else { Weight::Regular })));
            label.setAccessibilityValueDescription(Some(&ns(if current { say("현재 단계", "Current step") } else { "" })));
        }
        if step == Step::Ready {
            let provider = if self.draft.provider == Provider::Codex { "Codex" } else { "Claude Code" };
            let model = if self.draft.model().is_empty() { say("기본 모델", "Default model") } else { self.draft.model() };
            self.review_ai.setStringValue(&ns(&format!("{provider} · {model}")));
            self.review_ai.setToolTip(Some(&ns(&format!("{provider} · {model}"))));
            self.review_auto.setStringValue(&ns(if self.draft.summaries {
                say("켜짐 · 선택한 AI로 작업 내용을 보내 요약해요.", "On · Sends work to your selected AI to summarize.")
            } else { say("꺼짐 · 직접 요청할 때만 요약해요.", "Off · Summaries run only when you ask.") }));
        }
    }

    fn busy(&mut self, busy: bool) {
        self.submitting = busy;
        for button in [&self.next, &self.back, &self.skip] { button.setEnabled(!busy); }
    }
}

pub struct Onboarding {
    pub window: Retained<NSWindow>,
    pub view: Retained<OnboardingView>,
    flow: Rc<RefCell<Flow>>,
    poster: Arc<dyn Poster>,
    _actions: Vec<Retained<Action>>,
    _example: Simple,
}
impl Onboarding {
    pub fn new(mtm: MainThreadMarker, poster: Arc<dyn Poster>, language: Language) -> Self {
        let say = |ko, en| words(language, ko, en);
        let window = settings::window(mtm, SIZE, say("Errand 시작하기", "Welcome to Errand"));
        let view: Retained<OnboardingView> = unsafe { msg_send![OnboardingView::alloc(mtm), initWithFrame: rect(0.0, 0.0, SIZE.0, SIZE.1)] };
        window.setContentView(Some(&view));
        text(&view, "Errand", rect(48.0, 25.0, 140.0, 26.0), 18.0, Weight::Semibold, false, mtm);
        let navigation = [say("앱 소개", "Welcome"), say("사용 환경", "Accounts"), say("AI 설정", "AI choices"), say("준비 완료", "Ready")].iter().enumerate()
            .map(|(index, title)| text(&view, title, rect(248.0 + index as f64 * 110.0, 30.0, 104.0, 20.0), 12.0, Weight::Regular, true, mtm)).collect();
        let heading = text(&view, "", rect(48.0, 88.0, 624.0, 40.0), 28.0, Weight::Semibold, false, mtm);
        let subtitle = text(&view, "", rect(48.0, 139.0, 624.0, 46.0), 14.0, Weight::Regular, true, mtm);
        let pages: Vec<_> = Step::ALL.iter().map(|_| {
            let page = FlippedView::new(mtm, rect(48.0, 206.0, 624.0, 308.0));
            view.addSubview(&page);
            page
        }).collect();
        let intro = &pages[0];
        text(intro, say("진행 상황은 위젯에서", "Follow work in the widget"), rect(0.0, 28.0, 280.0, 24.0), 16.0, Weight::Semibold, false, mtm);
        text(intro, say("새 답변과 진행 중인 작업을 확인하고,\n카드를 눌러 원래 대화로 돌아가세요.", "See new answers and running tasks.\nClick a card to return to its conversation."), rect(0.0, 62.0, 278.0, 66.0), 14.0, Weight::Regular, false, mtm);
        text(intro, say("지난 일은 작업 기록에서", "Look back in work records"), rect(0.0, 158.0, 280.0, 24.0), 16.0, Weight::Semibold, false, mtm);
        text(intro, say("하루·주·월 단위로 작업을 모아 봐요.\n자동 요약을 켜지 않아도 볼 수 있어요.", "Browse work by day, week or month.\nAutomatic summaries can stay off."), rect(0.0, 192.0, 278.0, 66.0), 14.0, Weight::Regular, false, mtm);
        let example: Retained<Example> = unsafe { msg_send![Example::alloc(mtm), initWithFrame: rect(320.0, 0.0, 304.0, 292.0)] };
        intro.addSubview(&example);
        let avatar = AvatarView::new(mtm, rect(112.0, 0.0, 80.0, 80.0), None);
        avatar.show(Avatar::named("Errand")); example.addSubview(&avatar);
        let demo = example_widget(mtm, language);
        demo.view.setFrameOrigin(NSPoint::new(0.0, 100.0)); example.addSubview(&demo.view);
        text(&example, say("위젯 사용 예시", "Example widget"), rect(0.0, 252.0, 304.0, 20.0), 12.0, Weight::Regular, true, mtm).setAlignment(NSTextAlignment::Center);
        skip_example_controls(&example);
        example.setAccessibilityElement(true);
        example.setAccessibilityRole(Some(unsafe { NSAccessibilityImageRole }));
        example.setAccessibilityLabel(Some(&ns(say("작업 카드와 남은 사용량을 보여주는 위젯 예시", "An example widget showing task cards and remaining usage"))));
        unsafe { example.setAccessibilityChildren(Some(&objc2_foundation::NSArray::new())) };

        let mut actions = vec![];
        let (mut account_status, mut account_detail, mut open_apps) = (vec![], vec![], vec![]);
        for (index, provider) in Provider::ALL.into_iter().enumerate() {
            let y = index as f64 * 126.0;
            pages[1].addSubview(&AppMarkView::new(mtm, rect(0.0, y + 4.0, 30.0, 30.0), provider));
            text(&pages[1], provider.title(), rect(44.0, y, 260.0, 24.0), 18.0, Weight::Semibold, false, mtm);
            account_status.push(text(&pages[1], "", rect(44.0, y + 32.0, 424.0, 22.0), 14.0, Weight::Medium, false, mtm));
            account_detail.push(text(&pages[1], "", rect(44.0, y + 59.0, 424.0, 54.0), 12.0, Weight::Regular, true, mtm));
            let post = poster.clone();
            let title = if language == Language::Korean { format!("{} 열기", provider.title()) } else { format!("Open {}", provider.title()) };
            let (open, action) = button(&title, "", false, rect(488.0, y + 18.0, 136.0, 34.0), move || post.post(Msg::OpenApp(provider)), mtm);
            pages[1].addSubview(&open); open_apps.push(open); actions.push(action);
        }
        text(&pages[1], say("로그인은 나중에 해도 돼요. 계정 상태는 자동으로 갱신돼요.", "You can sign in later. Account status updates automatically."), rect(0.0, 270.0, 624.0, 34.0), 13.0, Weight::Regular, true, mtm);

        text(&pages[2], say("AI 서비스", "AI service"), rect(0.0, 0.0, 240.0, 22.0), 14.0, Weight::Medium, false, mtm);
        text(&pages[2], say("모델", "Model"), rect(280.0, 0.0, 344.0, 22.0), 14.0, Weight::Medium, false, mtm);
        let provider = NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), rect(-2.0, 30.0, 248.0, 30.0), false);
        provider.addItemWithTitle(&ns("Codex")); provider.addItemWithTitle(&ns("Claude Code"));
        provider.setAccessibilityLabel(Some(&ns(say("AI 서비스", "AI service")))); pages[2].addSubview(&provider);
        let model = NSComboBox::initWithFrame(NSComboBox::alloc(mtm), rect(280.0, 30.0, 344.0, 28.0));
        model.setFont(Some(&kit::font(13.0, Weight::Regular))); model.setCompletes(false); model.setNumberOfVisibleItems(8);
        model.setAccessibilityLabel(Some(&ns(say("모델", "Model")))); pages[2].addSubview(&model);
        text(&pages[2], say("기본 모델을 그대로 쓰거나, 목록에서 고르거나 모델 ID를 입력할 수 있어요.", "Keep the default model, pick from the list, or enter a model ID."), rect(0.0, 74.0, 624.0, 36.0), 12.0, Weight::Regular, true, mtm);
        let auto = unsafe { NSButton::checkboxWithTitle_target_action(&ns(say("하루 작업 기록 자동 요약", "Summarize each day's work automatically")), None, None, mtm) };
        auto.setFont(Some(&kit::font(15.0, Weight::Medium))); auto.setFrame(rect(-2.0, 122.0, 624.0, 28.0)); pages[2].addSubview(&auto);
        text(&pages[2], say("켜면 지난 작업은 자동으로, 오늘 작업은 오후 6시부터 요약해요.\n창을 닫아도 아침에 지난 작업을 요약할 수 있어요.\n요청·답변 일부와 커밋 메시지가 선택한 AI로 전송돼요.", "When on, summaries of today's work start at 6 pm. Past work can also\nbe summarized in the morning, even with the window closed. Parts of\nrequests, answers and commit messages are sent to the selected AI."), rect(22.0, 160.0, 602.0, 68.0), 13.0, Weight::Regular, false, mtm);
        text(&pages[2], say("선택한 Codex 또는 Claude Code에 로그인되어 있어야 해요. 제목·메모 추천에는\n첨부 내용, 번역에는 답변 일부를 보내며 해당 계정의 사용량에 반영돼요.", "Sign in to your selected Codex or Claude Code CLI to use AI features.\nTitle and note suggestions send attachments; translations send answer\nexcerpts. These requests count toward that account's usage."), rect(0.0, 252.0, 624.0, 56.0), 12.0, Weight::Regular, true, mtm);

        text(&pages[3], say("선택한 AI", "Your AI"), rect(0.0, 0.0, 140.0, 22.0), 14.0, Weight::Medium, true, mtm);
        let review_ai = text(&pages[3], "", rect(152.0, 0.0, 472.0, 46.0), 15.0, Weight::Medium, false, mtm);
        review_ai.setMaximumNumberOfLines(2); review_ai.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        text(&pages[3], say("자동 요약", "Automatic summaries"), rect(0.0, 62.0, 146.0, 40.0), 14.0, Weight::Medium, true, mtm);
        let review_auto = text(&pages[3], "", rect(152.0, 62.0, 472.0, 46.0), 15.0, Weight::Medium, false, mtm);
        text(&pages[3], say("처음에는 이렇게 써 보세요", "Try this first"), rect(0.0, 151.0, 624.0, 26.0), 18.0, Weight::Semibold, false, mtm);
        text(&pages[3], say("Codex나 Claude에 작업을 맡기면 위젯에 카드가 나타나요.\n새 답변이 오면 카드를 눌러 확인하고, 확인한 작업은 체크해 주세요.\n지난 작업은 메뉴 막대의 ‘작업 기록 열기’에서 다시 볼 수 있어요.", "Start a task in Codex or Claude and watch its card appear in the widget.\nWhen an answer arrives, click its card to read it, then check it off.\nOpen Work Records from the menu bar to revisit past work."), rect(0.0, 191.0, 624.0, 78.0), 14.0, Weight::Regular, false, mtm);
        let status = text(&view, "", rect(48.0, 530.0, 624.0, 40.0), 12.0, Weight::Regular, false, mtm);
        let (back, _) = button(say("이전", "Back"), "", false, rect(48.0, 586.0, 80.0, 36.0), || {}, mtm);
        let (skip, _) = button(say("나중에 설정", "Set up later"), "", false, rect(140.0, 586.0, 132.0, 36.0), || {}, mtm);
        skip.setToolTip(Some(&ns(say("자동 요약을 끈 채 위젯으로 시작해요.", "Start with the widget and automatic summaries off."))));
        let (next, _) = primary_button("", "", false, rect(498.0, 586.0, 174.0, 36.0), || {}, mtm);
        next.setKeyEquivalent(&ns("\r"));
        for button in [&back, &skip, &next] { view.addSubview(button); }
        let choices = settings::options(Provider::Codex, &[], language);
        settings::fill(&model, &choices, "");
        let flow = Rc::new(RefCell::new(Flow { step: Step::Welcome, active: false, submitting: false, open_records: false,
            draft: AiSettings::default(), cached: vec![], choices, language, pages, navigation, heading, subtitle,
            back, skip, next, provider, model, auto, status, local_error: None, account_status, account_detail, open_apps, review_ai, review_auto }));
        let weak = Rc::downgrade(&flow);
        let provider_action = Action::new(mtm, move || {
            let Some(flow) = weak.upgrade() else { return };
            let mut flow = flow.borrow_mut();
            flow.read_model();
            flow.draft.provider = if flow.provider.indexOfSelectedItem() == 1 { Provider::Claude } else { Provider::Codex };
            flow.choices = settings::options(flow.draft.provider, &flow.cached, flow.language);
            settings::fill(&flow.model, &flow.choices, flow.draft.model());
        });
        provider_action.attach(&flow.borrow().provider); actions.push(provider_action);
        let weak = Rc::downgrade(&flow);
        let auto_action = Action::new(mtm, move || {
            if let Some(flow) = weak.upgrade() { let mut flow = flow.borrow_mut(); flow.draft.summaries = flow.auto.state() == NSControlStateValueOn; }
        });
        auto_action.attach(&flow.borrow().auto); actions.push(auto_action);
        let weak = Rc::downgrade(&flow);
        let back_action = Action::new(mtm, move || {
            if let Some(flow) = weak.upgrade() {
                let mut flow = flow.borrow_mut();
                if flow.step == Step::Ai { flow.read_model(); }
                let previous = Step::ALL[flow.step.index().saturating_sub(1)];
                flow.show_step(previous);
            }
        });
        back_action.attach(&flow.borrow().back); actions.push(back_action);
        let (weak, post) = (Rc::downgrade(&flow), poster.clone());
        let skip_action = Action::new(mtm, move || {
            if let Some(flow) = weak.upgrade() {
                let mut flow = flow.borrow_mut(); flow.open_records = false; flow.local_error = None; flow.busy(true);
                post.post(Msg::Tasks(tasks::Msg::SkipOnboarding));
            }
        });
        skip_action.attach(&flow.borrow().skip); actions.push(skip_action);
        let (weak, post, editing_window) = (Rc::downgrade(&flow), poster.clone(), window.clone());
        let next_action = Action::new(mtm, move || {
            let Some(flow) = weak.upgrade() else { return };
            editing_window.makeFirstResponder(None);
            let mut flow = flow.borrow_mut();
            flow.local_error = None;
            if flow.step == Step::Ai {
                flow.read_model();
                if !flow.draft.valid() {
                    flow.local_error = Some(words(flow.language, "올바른 모델 ID를 입력해 주세요.", "Enter a valid model ID.").into());
                    flow.message(None); return;
                }
            }
            if flow.step == Step::Ready {
                flow.open_records = true; flow.busy(true);
                post.post(Msg::Tasks(tasks::Msg::Configure(flow.draft.clone())));
            } else {
                let next = Step::ALL[flow.step.index() + 1];
                flow.show_step(next); flow.message(None);
            }
        });
        next_action.attach(&flow.borrow().next); actions.push(next_action);
        flow.borrow_mut().show_step(Step::Welcome);
        Self { window, view, flow, poster, _actions: actions, _example: demo }
    }

    pub fn render(&mut self, state: &AppState) {
        let mut flow = self.flow.borrow_mut();
        if !state.tasks.onboarding {
            if flow.active && flow.submitting && state.tasks.settings_saved && state.tasks.settings.onboarding_complete {
                self.window.orderOut(None);
                flow.busy(false);
                if flow.open_records { self.poster.post(Msg::ShowDashboard); }
            }
            flow.active = false;
            return;
        }
        if !flow.active {
            flow.active = true; flow.local_error = None; flow.busy(false);
            flow.draft = state.tasks.settings.clone();
            flow.cached = state.tasks.model_options.clone();
            flow.choices = settings::options(flow.draft.provider, &flow.cached, flow.language);
            flow.provider.selectItemAtIndex(if flow.draft.provider == Provider::Claude { 1 } else { 0 });
            flow.auto.setState(if flow.draft.summaries { NSControlStateValueOn } else { NSControlStateValueOff });
            settings::fill(&flow.model, &flow.choices, flow.draft.model());
            flow.show_step(Step::Welcome);
        } else if flow.cached != state.tasks.model_options {
            flow.read_model();
            flow.cached = state.tasks.model_options.clone();
            flow.choices = settings::options(flow.draft.provider, &flow.cached, flow.language);
            settings::fill(&flow.model, &flow.choices, flow.draft.model());
        }
        let say = |ko, en| words(flow.language, ko, en);
        for (index, provider) in Provider::ALL.into_iter().enumerate() {
            let account = &state.accounts[provider];
            let usage = &state.usage[provider];
            let (status, detail) = if state.fetching.contains(&provider) && !usage.has_response {
                (say("사용량을 확인하고 있어요", "Checking usage"), say("확인이 끝나기 전에 다음 단계로 넘어가도 돼요.", "You can continue while this check finishes.").to_owned())
            } else if usage.failure.is_some() {
                (say("사용량 확인이 필요해요", "Usage needs attention"), say("앱을 열어 로그인 상태를 확인해 주세요. 나중에 위젯에서 다시 확인할 수 있어요.", "Open the app to check your sign-in. You can check usage again from the widget later.").to_owned())
            } else if usage.has_response && !usage.windows.is_empty() {
                (say("사용량을 가져왔어요", "Usage is available"), account.email.clone().unwrap_or_else(|| say("현재 로그인된 계정을 사용해요.", "Using the account signed in on this Mac.").into()))
            } else if let Some(email) = &account.email {
                (say("로그인된 계정을 찾았어요", "Found a signed-in account"), email.clone())
            } else if account.running {
                (say("앱이 실행 중이에요", "The app is running"), say("계정 정보가 아직 없어요. 앱에서 로그인 상태를 확인해 주세요.", "Account details are not available yet. Check your sign-in in the app.").into())
            } else {
                (say("앱에서 로그인해 주세요", "Sign in in the app"), say("아직 계정 정보가 없어요. 사용 중인 앱을 열어 주세요.", "No account details yet. Open the app you use to get started.").into())
            };
            flow.account_status[index].setStringValue(&ns(status));
            flow.account_detail[index].setStringValue(&ns(&detail));
            flow.account_detail[index].setToolTip(Some(&ns(&detail)));
            flow.open_apps[index].setEnabled(!state.launching);
        }
        if state.tasks.settings_error.is_some() { flow.busy(false); }
        flow.message(state.tasks.settings_error.as_deref());
    }

    pub fn show(&self, mtm: MainThreadMarker) {
        if !self.window.isVisible() { self.window.center(); }
        #[allow(deprecated)] NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
        self.window.makeKeyAndOrderFront(None);
    }

    /// Used only by the offline preview renderer; real navigation runs through the native buttons.
    pub fn preview_step(&self, step: Step) { self.flow.borrow_mut().show_step(step); }
}

/// A labelled teaching example rendered by the same component as the live widget. No personal records
/// or credentials are read, and clicks cannot leave this example.
fn example_widget(mtm: MainThreadMarker, language: Language) -> Simple {
    use errand_core::{model::{WidgetPreferences, UsageSlot}, reset_history::ResetHistory, sessions::{SessionRecords, SessionRow, VisibleSession}, usage::{Span, UsageWindow, WindowName}};
    use jiff::{SignedDuration, Timestamp};
    struct NoActions;
    impl Poster for NoActions { fn post(&self, _msg: Msg) {} }
    let now = Timestamp::now();
    let mut state = AppState::new(ResetHistory::new(), SessionRecords::new(now), WidgetPreferences::default(), "Errand".into());
    for (provider, remaining) in [(Provider::Codex, 72), (Provider::Claude, 58)] {
        state.usage[provider] = UsageSlot { has_response: true, windows: vec![UsageWindow {
            identifier: "onboarding-example".into(), name: WindowName::span(Span::Minutes(10080)), remaining_percent: Some(remaining), resets_at: None,
        }], ..Default::default() };
    }
    state.sessions.shown = [(Provider::Codex, false, words(language, "로그인 화면 수정", "Update the sign-in page")),
        (Provider::Claude, true, words(language, "변경 사항 검토", "Review the changes"))].into_iter().map(|(provider, running, title)| VisibleSession {
        seen: false, row: SessionRow { key: format!("onboarding:{provider:?}"), provider, title: title.into(), prompt: title.into(),
            prompt_at: now - SignedDuration::from_mins(5), running, aborted: false,
            finished_at: (!running).then_some(now - SignedDuration::from_mins(1)), result: None, action: None, folder: String::new(),
            url: String::new(), seen_in_app: false, stalled: false, parent: None, on_screen: false, question: None },
    }).collect();
    let context = view::ViewContext { now, zone: errand_core::format::local_zone(), computer: "Mac", language };
    let mut demo = Simple::new(mtm, Arc::new(NoActions), language, CompactEvents::none());
    demo.render(&view::simple(&state, &context, true), mtm);
    demo
}
