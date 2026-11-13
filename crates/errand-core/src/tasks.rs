//! Personal tasks are owned by Errand, independent of provider sessions and room reports.
use crate::{Language, PerProvider, Provider, Timestamp};
use std::path::PathBuf;

pub const MAX_FILES: usize = 12;
pub const MAX_FILE_BYTES: u64 = 50 * 1024 * 1024;

/// Model overrides belong to Errand, never to either CLI's global settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AiSettings {
    pub provider: Provider,
    pub models: PerProvider<String>,
}
impl Default for AiSettings {
    fn default() -> Self { Self { provider: Provider::Codex, models: PerProvider::default() } }
}
impl AiSettings {
    pub fn model(&self) -> &str { &self.models[self.provider] }
    pub fn valid(&self) -> bool { self.models.iter().all(|(_, id)| valid_model(id)) }
}
pub fn valid_model(id: &str) -> bool {
    id.len() <= 160 && !id.starts_with('-') && id.chars().all(|c| c.is_ascii_alphanumeric() || "-_.:/@[]".contains(c))
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelOption { pub id: String, pub label: String }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion { pub title: String, pub memo: String }

#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    File(PathBuf),
    Png(Vec<u8>),
}
#[derive(Clone, Debug, PartialEq)]
pub struct Attachment {
    /// An internally generated relative path, resolved only by the attachment store.
    pub id: String,
    pub name: String,
    pub bytes: u64,
    pub image: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub memo: String,
    pub attachments: Vec<Attachment>,
    pub created: Timestamp,
    pub done: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    pub token: u64,
    pub revision: u64,
    pub attachments: Vec<Attachment>,
    pub title: String,
    pub memo: String,
    pub memo_suggestion: Option<String>,
    pub memo_edited: bool,
    pub edited: bool,
    pub importing: bool,
    pub generating: bool,
    pub error: Option<String>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub records: Vec<Task>,
    pub draft: Option<Draft>,
    pub selected: Option<String>,
    pub settings: AiSettings,
    pub model_options: Vec<ModelOption>,
    pub settings_error: Option<String>,
    pub settings_saved: bool,
    pub dragging: bool,
    pub error: Option<String>,
    /// A broken manifest must never be overwritten by a fresh empty list.
    pub readable: bool,
    serial: u64,
    queued: Vec<Input>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            records: vec![],
            draft: None,
            selected: None,
            settings: AiSettings::default(),
            model_options: vec![],
            settings_error: None,
            settings_saved: false,
            dragging: false,
            error: None,
            readable: true,
            serial: 0,
            queued: vec![],
        }
    }
}
impl State {
    pub fn active(&self) -> impl Iterator<Item = &Task> {
        self.records.iter().filter(|task| !task.done)
    }
    pub fn open(&self) -> bool {
        self.draft.is_some() || self.selected.is_some()
    }
}
#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Dragging(bool),
    Drop(Vec<Input>),
    PickFiles,
    Cancel,
    Title(String),
    Memo(String),
    AcceptMemo,
    ShowSettings,
    Configure(AiSettings),
    SettingsSaved { settings: AiSettings, result: Result<(), String> },
    ModelsRead(Vec<ModelOption>),
    Retry,
    Imported {
        token: u64,
        result: Result<Vec<Attachment>, String>,
    },
    Generated {
        token: u64,
        revision: u64,
        result: Result<Suggestion, String>,
    },
    Remove(String),
    Register,
    Open(String),
    OpenAttachment(String),
    Complete(String),
    Saved {
        records: Vec<Task>,
        draft: Option<u64>,
        result: Result<(), String>,
    },
}
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Import {
        token: u64,
        inputs: Vec<Input>,
        remaining: usize,
    },
    Generate {
        token: u64,
        revision: u64,
        provider: Provider,
        model: String,
        attachments: Vec<Attachment>,
    },
    SaveSettings(AiSettings),
    ShowSettings,
    ReadModels,
    CancelTitle,
    Cleanup(Vec<Attachment>),
    Save {
        records: Vec<Task>,
        draft: Option<u64>,
    },
    Open(Attachment),
    PickFiles,
}
pub fn words(language: Language, korean: &'static str, english: &'static str) -> &'static str {
    if language == Language::Korean {
        korean
    } else {
        english
    }
}
fn generate(state: &mut State) -> Vec<Effect> {
    let Some(draft) = &mut state.draft else {
        return vec![];
    };
    draft.revision += 1;
    if draft.attachments.is_empty() || draft.importing {
        draft.generating = false;
        return vec![Effect::CancelTitle];
    }
    draft.memo_suggestion = None;
    draft.generating = true;
    draft.error = None;
    vec![Effect::Generate {
        token: draft.token,
        revision: draft.revision,
        provider: state.settings.provider,
        model: state.settings.model().to_owned(),
        attachments: draft.attachments.clone(),
    }]
}
pub fn update(state: &mut State, msg: Msg, now: Timestamp) -> Vec<Effect> {
    match msg {
        Msg::Dragging(on) => {
            state.dragging = on;
            vec![]
        }
        Msg::PickFiles => vec![Effect::PickFiles],
        Msg::Drop(inputs) => {
            state.dragging = false;
            if inputs.is_empty() {
                return vec![];
            }
            state.selected = None;
            if state.readable { state.error = None; }
            if state.draft.is_none() {
                state.serial += 1;
                state.draft = Some(Draft {
                    token: state.serial,
                    revision: 0,
                    attachments: vec![],
                    title: String::new(),
                    memo: String::new(),
                    memo_suggestion: None,
                    memo_edited: false,
                    edited: false,
                    importing: false,
                    generating: false,
                    error: None,
                });
            }
            let draft = state.draft.as_mut().unwrap();
            if draft.importing {
                state.queued.extend(inputs);
                return vec![];
            }
            draft.importing = true;
            draft.generating = false;
            draft.revision += 1;
            vec![
                Effect::CancelTitle,
                Effect::Import {
                    token: draft.token,
                    inputs,
                    remaining: MAX_FILES.saturating_sub(draft.attachments.len()),
                },
            ]
        }
        Msg::Imported { token, result } => {
            let Some(draft) = state.draft.as_mut().filter(|d| d.token == token) else {
                return result
                    .map(|files| vec![Effect::Cleanup(files)])
                    .unwrap_or_default();
            };
            draft.importing = false;
            let mut effects = match result {
                Ok(files) => {
                    if draft.title.is_empty() {
                        draft.title = files.first().map(|f| f.name.clone()).unwrap_or_default();
                    }
                    draft.attachments.extend(files);
                    generate(state)
                }
                Err(error) => {
                    draft.error = Some(error);
                    vec![]
                }
            };
            if !state.queued.is_empty() {
                let queued = std::mem::take(&mut state.queued);
                effects.retain(|effect| !matches!(effect, Effect::Generate { .. }));
                effects.extend(update(state, Msg::Drop(queued), now));
            }
            effects
        }
        Msg::Title(title) => {
            if let Some(draft) = &mut state.draft {
                draft.title = title;
                draft.edited = true;
            }
            vec![]
        }
        Msg::Memo(memo) => {
            if let Some(draft) = &mut state.draft {
                draft.memo = memo;
                draft.memo_edited = true;
                draft.memo_suggestion = None;
            }
            vec![]
        }
        Msg::AcceptMemo => {
            if let Some(draft) = &mut state.draft && draft.memo.is_empty() && !draft.memo_edited
                && let Some(memo) = draft.memo_suggestion.take() {
                draft.memo = memo;
                draft.memo_edited = true;
            }
            vec![]
        }
        Msg::ShowSettings => {
            state.settings_saved = false;
            vec![Effect::ReadModels, Effect::ShowSettings]
        }
        Msg::ModelsRead(models) => { state.model_options = models; vec![] }
        Msg::Configure(settings) => {
            state.settings_saved = false;
            if !settings.valid() {
                state.settings_error = Some("모델 이름을 확인해 주세요. / Enter a valid model ID.".into());
                return vec![];
            }
            vec![Effect::SaveSettings(settings)]
        }
        Msg::SettingsSaved { settings, result } => match result {
            Ok(()) => {
                let changed = state.settings != settings;
                state.settings = settings;
                state.settings_error = None;
                state.settings_saved = true;
                if changed { generate(state) } else { vec![] }
            }
            Err(error) => { state.settings_error = Some(error); vec![] }
        },
        Msg::Retry => {
            if let Some(draft) = &mut state.draft {
                draft.edited = false;
                if draft.memo.is_empty() { draft.memo_edited = false; }
            }
            generate(state)
        }
        Msg::Generated {
            token,
            revision,
            result,
        } => {
            if let Some(draft) = state
                .draft
                .as_mut()
                .filter(|d| d.token == token && d.revision == revision)
            {
                draft.generating = false;
                match result {
                    Ok(suggestion) => {
                        if !draft.edited { draft.title = suggestion.title; }
                        if draft.memo.is_empty() && !draft.memo_edited {
                            draft.memo_suggestion = (!suggestion.memo.is_empty()).then_some(suggestion.memo);
                        }
                    }
                    Err(error) => draft.error = Some(error),
                }
            }
            vec![]
        }
        Msg::Remove(id) => {
            let Some(draft) = &mut state.draft else {
                return vec![];
            };
            let removed = draft
                .attachments
                .iter()
                .filter(|f| f.id == id)
                .cloned()
                .collect();
            draft.attachments.retain(|f| f.id != id);
            draft.generating = false;
            draft.memo_suggestion = None;
            draft.revision += 1;
            vec![Effect::CancelTitle, Effect::Cleanup(removed)]
        }
        Msg::Cancel => {
            state.selected = None;
            state.queued.clear();
            if state.readable { state.error = None; }
            let mut effects = vec![Effect::CancelTitle];
            if let Some(draft) = state.draft.take() {
                effects.push(Effect::Cleanup(draft.attachments));
            }
            effects
        }
        Msg::Register => {
            let Some(draft) = &state.draft else {
                return vec![];
            };
            if draft.importing || draft.title.trim().is_empty() || draft.attachments.is_empty() {
                return vec![];
            }
            let mut records = state.records.clone();
            records.insert(
                0,
                Task {
                    id: format!("manual:{}", now.as_nanosecond()),
                    title: draft.title.trim().chars().take(200).collect(),
                    memo: draft.memo.trim().chars().take(10_000).collect(),
                    attachments: draft.attachments.clone(),
                    created: now,
                    done: false,
                },
            );
            vec![Effect::Save {
                records,
                draft: Some(draft.token),
            }]
        }
        Msg::Saved {
            records,
            draft,
            result,
        } => match result {
            Ok(()) => {
                state.records = records;
                state.error = None;
                let closed_draft = draft.is_some() && state.draft.as_ref().map(|d| d.token) == draft;
                if closed_draft { state.draft = None; }
                state.selected = None;
                // Checking a different card must not cancel the title of the draft being edited.
                if closed_draft { vec![Effect::CancelTitle] } else { vec![] }
            }
            Err(error) => {
                state.error = Some(error.clone());
                if let Some(d) = &mut state.draft {
                    d.error = Some(error);
                }
                vec![]
            }
        },
        Msg::Open(id) => {
            if state.draft.is_none() {
                state.selected = Some(id);
            }
            vec![]
        }
        Msg::OpenAttachment(id) => state
            .draft
            .as_ref()
            .into_iter()
            .flat_map(|d| &d.attachments)
            .chain(state.records.iter().flat_map(|t| &t.attachments))
            .find(|f| f.id == id)
            .cloned()
            .map(|f| vec![Effect::Open(f)])
            .unwrap_or_default(),
        Msg::Complete(id) => {
            let mut records = state.records.clone();
            let Some(task) = records.iter_mut().find(|t| t.id == id) else {
                return vec![];
            };
            task.done = !task.done;
            vec![Effect::Save {
                records,
                draft: None,
            }]
        }
    }
}
