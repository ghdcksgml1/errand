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
            result: Ok("AI 제목".into()),
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
    update(&mut state, Msg::Provider(Provider::Claude), now());
    update(
        &mut state,
        Msg::Generated {
            token: old.token,
            revision: old.revision,
            result: Ok("old".into()),
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
            result: Ok("old".into()),
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
