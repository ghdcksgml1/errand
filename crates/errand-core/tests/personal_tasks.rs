use errand_core::{
    Language, Provider, TimeZone, Timestamp,
    model::{self, AppState, WidgetMode, WidgetPreferences},
    reset_history::ResetHistory,
    sessions::SessionRecords,
    tasks::*,
    view::{self, ViewContext},
};
fn now() -> Timestamp {
    "2026-11-13T00:00:00Z".parse().unwrap()
}
fn file() -> Attachment {
    Attachment {
        id: "a.png".into(),
        name: "request.png".into(),
        bytes: 8,
        image: true,
    }
}
fn draft(state: &mut State) {
    update(state, Msg::Drop(vec![Input::File("a.png".into())]), now());
    let token = state.draft.as_ref().unwrap().token;
    update(
        state,
        Msg::Imported {
            token,
            result: Ok(vec![file()]),
        },
        now(),
    );
}
#[test]
fn draft_needs_confirmation_and_late_ai_preserves_edits() {
    let mut state = State::default();
    draft(&mut state);
    let d = state.draft.clone().unwrap();
    assert!(state.records.is_empty());
    update(&mut state, Msg::Title("직접 쓴 제목".into()), now());
    update(
        &mut state,
        Msg::Generated {
            token: d.token,
            revision: d.revision,
            result: Ok(Suggestion { title: "AI 제목".into(), memo: "확인할 내용".into() }),
        },
        now(),
    );
    assert_eq!(state.draft.as_ref().unwrap().title, "직접 쓴 제목");
    let effects = update(&mut state, Msg::Register, now());
    let Effect::Save { records, draft } = effects[0].clone() else {
        panic!()
    };
    update(
        &mut state,
        Msg::Saved {
            records: records.clone(),
            draft,
            result: Err("disk full".into()),
        },
        now(),
    );
    assert!(state.records.is_empty());
    assert!(state.draft.is_some());
    update(
        &mut state,
        Msg::Saved {
            records,
            draft,
            result: Ok(()),
        },
        now(),
    );
    assert_eq!(state.records.len(), 1);
    assert!(state.draft.is_none());
}
#[test]
fn cancellation_and_provider_switch_ignore_old_results() {
    let mut state = State::default();
    draft(&mut state);
    let old = state.draft.clone().unwrap();
    let settings = AiSettings { provider: Provider::Claude, ..AiSettings::default() };
    update(&mut state, Msg::SettingsSaved { settings, result: Ok(()) }, now());
    update(
        &mut state,
        Msg::Generated {
            token: old.token,
            revision: old.revision,
            result: Ok(Suggestion { title: "old".into(), memo: "stale memo".into() }),
        },
        now(),
    );
    assert_ne!(state.draft.as_ref().unwrap().title, "old");
    assert!(
        update(&mut state, Msg::Cancel, now())
            .iter()
            .any(|e| matches!(e, Effect::Cleanup(_)))
    );
    assert!(matches!(
        &update(
            &mut state,
            Msg::Imported {
                token: old.token,
                result: Ok(vec![file()])
            },
            now()
        )[..],
        [Effect::Cleanup(_)]
    ));
    draft(&mut state);
    update(
        &mut state,
        Msg::Generated {
            token: old.token,
            revision: old.revision,
            result: Ok(Suggestion { title: "old".into(), memo: "stale memo".into() }),
        },
        now(),
    );
    assert_ne!(state.draft.as_ref().unwrap().title, "old");
}
#[test]
fn all_widget_modes_show_personal_tasks_without_reporting_them_to_rooms() {
    let now = now();
    let mut state = AppState::new(
        ResetHistory::new(),
        SessionRecords::new(now),
        WidgetPreferences::default(),
        "Tester".into(),
    );
    state.tasks.records.push(Task {
        id: "manual:1".into(),
        title: "계약서 확인".into(),
        memo: "내일".into(),
        attachments: vec![file()],
        created: now,
        done: false,
    });
    let context = ViewContext {
        now,
        language: Language::Korean,
        zone: TimeZone::UTC,
        computer: "Mac",
    };
    assert!(view::dock(&state, &context).header.is_some());
    assert_eq!(view::tray(&state, &context, 500.0).cards[0].provider, None);
    assert_eq!(view::simple(&state, &context, false).lines.len(), 1);
    assert_eq!(view::badge(&state, &context).count.unwrap().0, 1);
    assert_eq!(state.sessions.counts().new, 0);
    for mode in WidgetMode::ALL {
        model::update(&mut state, model::Msg::SetWidgetMode(mode), now);
        model::update(
            &mut state,
            model::Msg::Tasks(Msg::Drop(vec![Input::File("file".into())])),
            now,
        );
        assert_eq!(state.widget.mode, mode);
        assert!(state.tasks.open());
        model::update(&mut state, model::Msg::Tasks(Msg::Cancel), now);
    }
}

#[test]
fn a_second_drop_waits_for_the_first_copy_and_cancel_clears_the_queue() {
    let mut state = State::default();
    update(&mut state, Msg::Drop(vec![Input::File("one".into())]), now());
    let token = state.draft.as_ref().unwrap().token;
    assert!(update(&mut state, Msg::Drop(vec![Input::File("two".into())]), now()).is_empty());
    let effects = update(&mut state, Msg::Imported { token, result: Ok(vec![file()]) }, now());
    assert!(effects.iter().any(|effect| matches!(effect, Effect::Import { inputs, remaining: 11, .. } if inputs == &vec![Input::File("two".into())])));
    assert!(!effects.iter().any(|effect| matches!(effect, Effect::Generate { .. })));
    update(&mut state, Msg::Cancel, now());
    assert!(matches!(&update(&mut state, Msg::Imported { token, result: Ok(vec![file()]) }, now())[..], [Effect::Cleanup(_)]));
}

#[test]
fn completing_another_card_keeps_the_current_drafts_title_job() {
    let mut state = State::default();
    draft(&mut state);
    let effects = update(&mut state, Msg::Saved { records: vec![], draft: None, result: Ok(()) }, now());
    assert!(state.draft.as_ref().unwrap().generating);
    assert!(!effects.contains(&Effect::CancelTitle));
}

#[test]
fn suggested_memo_is_saved_only_after_acceptance_and_never_replaces_typing() {
    let mut state = State::default();
    draft(&mut state);
    let d = state.draft.clone().unwrap();
    let generated = Msg::Generated { token: d.token, revision: d.revision,
        result: Ok(Suggestion { title: "문서 확인".into(), memo: "금요일 회의 전에 의견 보내기".into() }) };
    update(&mut state, generated.clone(), now());
    assert_eq!(state.draft.as_ref().unwrap().memo, "");
    assert!(matches!(&update(&mut state, Msg::Register, now())[..], [Effect::Save { records, .. }] if records[0].memo.is_empty()));
    update(&mut state, Msg::AcceptMemo, now());
    assert_eq!(state.draft.as_ref().unwrap().memo, "금요일 회의 전에 의견 보내기");
    assert!(matches!(&update(&mut state, Msg::Register, now())[..], [Effect::Save { records, .. }] if records[0].memo == "금요일 회의 전에 의견 보내기"));
    update(&mut state, Msg::Memo("내가 적은 메모".into()), now());
    update(&mut state, generated.clone(), now());
    update(&mut state, Msg::AcceptMemo, now());
    assert_eq!(state.draft.as_ref().unwrap().memo, "내가 적은 메모");
    // Clearing a note deliberately is also an edit; a late answer must not resurrect it.
    update(&mut state, Msg::Memo(String::new()), now());
    update(&mut state, generated, now());
    assert!(state.draft.as_ref().unwrap().memo_suggestion.is_none());
}

#[test]
fn settings_apply_after_persistence_and_restart_with_the_selected_model() {
    let mut state = State::default();
    draft(&mut state);
    update(&mut state, Msg::Title("내 제목".into()), now());
    update(&mut state, Msg::Memo("내 메모".into()), now());
    let old = state.draft.clone().unwrap();
    let mut settings = AiSettings { provider: Provider::Claude, ..AiSettings::default() };
    settings.models.claude = "sonnet".into();
    settings.models.codex = "codex-test-model".into();
    assert_eq!(update(&mut state, Msg::Configure(settings.clone()), now()), vec![Effect::SaveSettings(settings.clone())]);
    assert_eq!(state.settings, AiSettings::default());
    assert!(update(&mut state, Msg::SettingsSaved { settings: settings.clone(), result: Err("disk full".into()) }, now()).is_empty());
    assert_eq!(state.settings, AiSettings::default());
    assert!(!state.settings_saved);
    let effects = update(&mut state, Msg::SettingsSaved { settings: settings.clone(), result: Ok(()) }, now());
    assert!(matches!(&effects[..], [Effect::Generate { provider: Provider::Claude, model, .. }] if model == "sonnet"));
    assert_eq!(state.settings, settings);
    assert!(state.settings_error.is_none());
    update(&mut state, Msg::Generated { token: old.token, revision: old.revision, result: Ok(Suggestion { title: "stale".into(), memo: "stale".into() }) }, now());
    let current = state.draft.clone().unwrap();
    assert!(current.generating);
    update(&mut state, Msg::Generated { token: current.token, revision: current.revision, result: Ok(Suggestion { title: "new model".into(), memo: "new model".into() }) }, now());
    assert_eq!(state.draft.as_ref().unwrap().title, "내 제목");
    assert_eq!(state.draft.as_ref().unwrap().memo, "내 메모");
    let mut invalid = settings;
    invalid.models.claude = "--help".into();
    assert!(update(&mut state, Msg::Configure(invalid), now()).is_empty());
    assert!(state.settings_error.is_some());
}
