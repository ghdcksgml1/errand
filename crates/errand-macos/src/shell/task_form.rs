//! Draft fields live across renders; a scan or an AI answer never recreates the editor under the cursor.
use super::{
    details::{PopoverView, button},
    kit::{self, Action, FlippedView, Weight, label, ns, rect},
};
use errand_core::{
    Language, Provider,
    model::Msg,
    ports::Poster,
    tasks::{self, State, words},
    theme,
};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_app_kit::*;
use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol, NSSize};
use std::{cell::RefCell, rc::Rc, sync::Arc};
pub const WIDTH: f64 = 380.0;
pub const HEIGHT: f64 = 550.0;
define_class!(
    #[unsafe(super(NSObject))] #[thread_kind = MainThreadOnly] #[name = "ErrandTaskFieldWatcher"] #[ivars = Box<dyn Fn()>]
    struct Watcher;
    unsafe impl NSObjectProtocol for Watcher {}
    unsafe impl NSControlTextEditingDelegate for Watcher {
        #[unsafe(method(controlTextDidChange:))] fn changed(&self, _notification: &NSNotification) { (self.ivars())(); }
    }
    unsafe impl NSTextFieldDelegate for Watcher {}
);
fn field(
    mtm: MainThreadMarker,
    caption: &str,
    y: f64,
    height: f64,
    body: &NSView,
) -> Retained<NSTextField> {
    let text = label(caption, 11.0, Weight::Semibold, theme::MUTED, mtm);
    text.setFrame(rect(20.0, y, 340.0, 16.0));
    body.addSubview(&text);
    let field =
        NSTextField::initWithFrame(NSTextField::alloc(mtm), rect(18.0, y + 21.0, 344.0, height));
    field.setBezeled(true);
    field.setBezelStyle(NSTextFieldBezelStyle::RoundedBezel);
    field.setFont(Some(&kit::font(13.0, Weight::Regular)));
    field.setAccessibilityLabel(Some(&ns(caption)));
    body.addSubview(&field);
    field
}
pub struct TaskForm {
    pub view: Retained<PopoverView>,
    body: Retained<FlippedView>,
    title: Retained<NSTextField>,
    memo: Retained<NSTextField>,
    status: Retained<NSTextField>,
    heading: Retained<NSTextField>,
    preview: Retained<NSImageView>,
    placeholder: Retained<NSTextField>,
    files: Retained<FlippedView>,
    go: Retained<NSButton>,
    retry: Retained<NSButton>,
    add: Retained<NSButton>,
    codex: Retained<NSButton>,
    claude: Retained<NSButton>,
    _actions: Vec<Retained<Action>>,
    file_actions: Vec<Retained<Action>>,
    _watchers: Vec<Retained<Watcher>>,
    selected: Rc<RefCell<Option<String>>>,
    key: Option<String>,
    attachments: Vec<tasks::Attachment>,
    poster: Arc<dyn Poster>,
    language: Language,
}
impl TaskForm {
    pub fn new(mtm: MainThreadMarker, poster: Arc<dyn Poster>, language: Language) -> Self {
        let view = PopoverView::new(mtm, NSSize::new(WIDTH, HEIGHT));
        let body = FlippedView::new(mtm, rect(0.0, 0.0, WIDTH, HEIGHT));
        view.addSubview(&body);
        let heading = label(
            words(language, "작업 등록", "Add a task"),
            18.0,
            Weight::Semibold,
            theme::INK,
            mtm,
        );
        heading.setFrame(rect(20.0, 19.0, 285.0, 26.0));
        body.addSubview(&heading);
        let post = poster.clone();
        let (close, close_action) = button(
            "",
            "x",
            false,
            rect(337.0, 16.0, 27.0, 27.0),
            move || post.post(Msg::Tasks(tasks::Msg::Cancel)),
            mtm,
        );
        close.setAccessibilityLabel(Some(&ns(words(language, "닫기", "Close"))));
        close.setKeyEquivalent(&ns("\u{1b}"));
        body.addSubview(&close);
        let preview =
            NSImageView::initWithFrame(NSImageView::alloc(mtm), rect(20.0, 55.0, 340.0, 118.0));
        preview.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
        body.addSubview(&preview);
        let placeholder = label(
            words(
                language,
                "첨부파일을 확인해 주세요",
                "Review your attachments",
            ),
            13.0,
            Weight::Regular,
            theme::MUTED,
            mtm,
        );
        placeholder.setFrame(rect(34.0, 105.0, 315.0, 22.0));
        body.addSubview(&placeholder);
        let files = FlippedView::new(mtm, rect(0.0, 0.0, 340.0, 70.0));
        let scroll =
            NSScrollView::initWithFrame(NSScrollView::alloc(mtm), rect(20.0, 183.0, 340.0, 70.0));
        scroll.setDrawsBackground(false);
        scroll.setHasVerticalScroller(true);
        scroll.setDocumentView(Some(&files));
        body.addSubview(&scroll);
        let title = field(mtm, words(language, "제목", "Title"), 265.0, 28.0, &body);
        let memo = field(
            mtm,
            words(language, "메모 · 선택", "Note · optional"),
            325.0,
            48.0,
            &body,
        );
        memo.setPlaceholderString(Some(&ns(words(
            language,
            "기억할 내용을 적어 두세요",
            "Anything to remember",
        ))));
        let status = label("", 11.0, Weight::Regular, theme::MUTED, mtm);
        status.setFrame(rect(20.0, 401.0, 340.0, 42.0));
        status.setMaximumNumberOfLines(3);
        status.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
        body.addSubview(&status);
        let post = poster.clone();
        let (retry, retry_action) = button(
            words(language, "제목 다시 만들기", "Suggest title"),
            "refresh-cw",
            false,
            rect(218.0, 443.0, 143.0, 25.0),
            move || post.post(Msg::Tasks(tasks::Msg::Retry)),
            mtm,
        );
        body.addSubview(&retry);
        let post = poster.clone();
        let (codex, codex_action) = button(
            "Codex",
            "",
            false,
            rect(18.0, 443.0, 72.0, 25.0),
            move || post.post(Msg::Tasks(tasks::Msg::Provider(Provider::Codex))),
            mtm,
        );
        codex.setButtonType(NSButtonType::PushOnPushOff);
        body.addSubview(&codex);
        let post = poster.clone();
        let (claude, claude_action) = button(
            "Claude Code",
            "",
            false,
            rect(94.0, 443.0, 115.0, 25.0),
            move || post.post(Msg::Tasks(tasks::Msg::Provider(Provider::Claude))),
            mtm,
        );
        claude.setButtonType(NSButtonType::PushOnPushOff);
        body.addSubview(&claude);
        let note = label(
            words(
                language,
                "로컬 CLI 사용 · 내용은 선택한 모델 서비스로 전송됩니다",
                "Local CLI · content is sent to the selected model service",
            ),
            9.5,
            Weight::Regular,
            theme::MUTED,
            mtm,
        );
        note.setFrame(rect(20.0, 473.0, 342.0, 16.0));
        body.addSubview(&note);
        let post = poster.clone();
        let (add, add_action) = button(
            words(language, "파일 추가", "Add files"),
            "paperclip",
            false,
            rect(18.0, 503.0, 100.0, 29.0),
            move || post.post(Msg::Tasks(tasks::Msg::PickFiles)),
            mtm,
        );
        body.addSubview(&add);
        let post = poster.clone();
        let (cancel, cancel_action) = button(
            words(language, "취소", "Cancel"),
            "",
            false,
            rect(178.0, 503.0, 70.0, 29.0),
            move || post.post(Msg::Tasks(tasks::Msg::Cancel)),
            mtm,
        );
        body.addSubview(&cancel);
        let selected: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let (post, chosen, title_field, memo_field) = (
            poster.clone(),
            selected.clone(),
            title.clone(),
            memo.clone(),
        );
        let (go, go_action) = button(
            words(language, "작업 등록", "Add task"),
            "check",
            false,
            rect(254.0, 503.0, 109.0, 29.0),
            move || {
                if let Some(id) = chosen.borrow().clone() {
                    post.post(Msg::Tasks(tasks::Msg::Complete(id)));
                } else {
                    post.post(Msg::Tasks(tasks::Msg::Title(
                        title_field.stringValue().to_string(),
                    )));
                    post.post(Msg::Tasks(tasks::Msg::Memo(
                        memo_field.stringValue().to_string(),
                    )));
                    post.post(Msg::Tasks(tasks::Msg::Register));
                }
            },
            mtm,
        );
        go.setKeyEquivalent(&ns("\r"));
        body.addSubview(&go);
        let mut watchers = vec![];
        for (field, title) in [(&title, true), (&memo, false)] {
            let (field_copy, post) = (field.clone(), poster.clone());
            let changed = Box::new(move || {
                post.post(Msg::Tasks(if title {
                    tasks::Msg::Title(field_copy.stringValue().to_string())
                } else {
                    tasks::Msg::Memo(field_copy.stringValue().to_string())
                }))
            }) as Box<dyn Fn()>;
            let watcher = Watcher::alloc(mtm).set_ivars(changed);
            let watcher: Retained<Watcher> = unsafe { msg_send![super(watcher), init] };
            unsafe { field.setDelegate(Some(ProtocolObject::from_ref(&*watcher))) };
            watchers.push(watcher);
        }
        Self {
            view,
            body,
            heading,
            title,
            memo,
            status,
            preview,
            placeholder,
            files,
            go,
            retry,
            add,
            codex,
            claude,
            selected,
            key: None,
            attachments: vec![],
            _actions: vec![
                close_action,
                retry_action,
                codex_action,
                claude_action,
                add_action,
                cancel_action,
                go_action,
            ],
            file_actions: vec![],
            _watchers: watchers,
            poster,
            language,
        }
    }
    pub fn point(&self, right: bool, y: f64) {
        self.view.point(&self.body, right, y);
    }
    pub fn render(&mut self, state: &State, mtm: MainThreadMarker) -> bool {
        let selected = state
            .selected
            .as_ref()
            .and_then(|id| state.records.iter().find(|t| &t.id == id));
        let (key, title, memo, files, draft) = if let Some(draft) = &state.draft {
            (
                format!("draft:{}", draft.token),
                draft.title.as_str(),
                draft.memo.as_str(),
                &draft.attachments,
                Some(draft),
            )
        } else if let Some(task) = selected {
            (
                task.id.clone(),
                task.title.as_str(),
                task.memo.as_str(),
                &task.attachments,
                None,
            )
        } else {
            self.key = None;
            return false;
        };
        let changed = self.key.as_ref() != Some(&key);
        self.key = Some(key);
        *self.selected.borrow_mut() = selected.map(|t| t.id.clone());
        self.heading.setStringValue(&ns(words(
            self.language,
            if draft.is_some() {
                "작업 등록"
            } else {
                "첨부 작업"
            },
            if draft.is_some() {
                "Add a task"
            } else {
                "Attached task"
            },
        )));
        // setStringValue also replaces the editor; leave it alone when the user's value already matches.
        if self.title.stringValue().to_string() != title {
            self.title.setStringValue(&ns(title));
        }
        if self.memo.stringValue().to_string() != memo {
            self.memo.setStringValue(&ns(memo));
        }
        self.title.setEditable(draft.is_some());
        self.memo.setEditable(draft.is_some());
        let error = draft
            .and_then(|d| d.error.as_deref())
            .or(state.error.as_deref());
        let message = error.unwrap_or_else(|| {
            if draft.is_some_and(|d| d.importing) {
                words(
                    self.language,
                    "첨부파일을 보관하는 중...",
                    "Copying attachments...",
                )
            } else if draft.is_some_and(|d| d.generating) {
                words(
                    self.language,
                    "AI가 제목을 만드는 중... 직접 입력해도 됩니다.",
                    "Suggesting a title... You can also type your own.",
                )
            } else {
                words(
                    self.language,
                    "제목을 확인하고 등록해 주세요.",
                    "Review the title, then add your task.",
                )
            }
        });
        self.status.setStringValue(&ns(message));
        self.status.setToolTip(Some(&ns(message)));
        self.go.setTitle(&ns(words(
            self.language,
            if draft.is_some() {
                "작업 등록"
            } else {
                "완료"
            },
            if draft.is_some() {
                "Add task"
            } else {
                "Complete"
            },
        )));
        self.go.setEnabled(
            state.readable
                && draft.is_none_or(|d| {
                    !d.importing && !d.title.trim().is_empty() && !d.attachments.is_empty()
                }),
        );
        self.retry.setHidden(draft.is_none());
        self.retry.setEnabled(
            draft.is_some_and(|d| !d.importing && !d.generating && !d.attachments.is_empty()),
        );
        self.add.setHidden(draft.is_none());
        self.add.setEnabled(
            draft.is_some_and(|d| !d.importing && d.attachments.len() < tasks::MAX_FILES),
        );
        self.codex.setHidden(draft.is_none());
        self.claude.setHidden(draft.is_none());
        self.codex.setState(if state.provider == Provider::Codex {
            1
        } else {
            0
        });
        self.claude.setState(if state.provider == Provider::Claude {
            1
        } else {
            0
        });
        if changed || &self.attachments != files {
            self.attachments = files.clone();
            kit::clear(&self.files);
            self.file_actions.clear();
            self.files
                .setFrameSize(NSSize::new(340.0, (files.len() as f64 * 28.0).max(70.0)));
            let root = crate::paths::Paths::current()
                .app_support()
                .join("tasks/attachments");
            let image = files.iter().find(|file| file.image).and_then(|file| {
                NSImage::initWithContentsOfFile(
                    NSImage::alloc(),
                    &ns(&root.join(&file.id).to_string_lossy()),
                )
            });
            self.preview.setImage(image.as_deref());
            self.placeholder.setHidden(image.is_some());
            for (index, file) in files.iter().enumerate() {
                let (post, id) = (self.poster.clone(), file.id.clone());
                let name = format!("{} · {} KB", file.name, file.bytes.div_ceil(1024));
                let (open, action) = button(
                    &name,
                    "paperclip",
                    false,
                    rect(0.0, index as f64 * 28.0, 302.0, 25.0),
                    move || post.post(Msg::Tasks(tasks::Msg::OpenAttachment(id.clone()))),
                    mtm,
                );
                open.setToolTip(Some(&ns(&name)));
                self.files.addSubview(&open);
                self.file_actions.push(action);
                if draft.is_some() {
                    let (post, id) = (self.poster.clone(), file.id.clone());
                    let (remove, action) = button(
                        "",
                        "x",
                        false,
                        rect(305.0, index as f64 * 28.0, 24.0, 25.0),
                        move || post.post(Msg::Tasks(tasks::Msg::Remove(id.clone()))),
                        mtm,
                    );
                    remove.setAccessibilityLabel(Some(&ns(words(
                        self.language,
                        "첨부 삭제",
                        "Remove attachment",
                    ))));
                    self.files.addSubview(&remove);
                    self.file_actions.push(action);
                }
            }
        }
        changed && draft.is_some()
    }
    pub fn focus(&self) -> &NSView {
        &self.title
    }
}
