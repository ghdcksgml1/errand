//! Offering a newer Errand: only a newer version is offered, and only a press installs it.

use errand_core::format::timestamp_from_seconds;
use errand_core::model::{AppState, Effect, Msg, WidgetPreferences, update};
use errand_core::reset_history::ResetHistory;
use errand_core::sessions::SessionRecords;
use errand_core::update::{RELEASES_PAGE, Release, UpdateState, VERSION, is_newer};
use errand_core::view::{self, MenuAction, MenuEntry, ViewContext};
use errand_core::{Language, TimeZone, Timestamp};

const NOW: f64 = 1_792_044_000.0;

fn now() -> Timestamp {
    timestamp_from_seconds(NOW).unwrap()
}

fn state() -> AppState {
    AppState::new(ResetHistory::new(), SessionRecords::new(now()), WidgetPreferences::default(), "Sam".to_owned())
}

fn context() -> ViewContext {
    ViewContext { now: now(), zone: TimeZone::UTC, computer: "Mac", language: Language::Korean }
}

fn send(state: &mut AppState, msg: Msg) -> Vec<Effect> {
    update(state, msg, now())
}

fn release(version: &str) -> Release {
    Release { version: version.to_owned(), download: Some(format!("https://github.com/ghdcksgml1/errand/releases/download/v{version}/Errand-{version}.zip")) }
}

#[test]
fn only_a_later_version_of_three_numbers_is_newer() {
    assert!(is_newer("1.8.0", "1.7.0"));
    assert!(is_newer("v1.7.1", "1.7.0"), "tags carry a v");
    assert!(is_newer("1.10.0", "1.9.0"), "by number, not by text");
    assert!(is_newer("2.0.0", "1.99.99"));
    assert!(!is_newer("1.7.0", "1.7.0"));
    assert!(!is_newer("1.6.9", "1.7.0"), "never back");
    for odd in ["1.8.0-beta", "1.8", "1.8.0.1", "latest", ""] {
        assert!(!is_newer(odd, "1.7.0"), "{odd}");
    }
}

#[test]
fn a_newer_release_is_offered_and_anything_else_is_not() {
    let mut state = state();
    assert_eq!(send(&mut state, Msg::CheckForUpdate), [Effect::FetchRelease]);
    assert_eq!(view::dock(&state, &context()).update, None);

    send(&mut state, Msg::ReleaseRead(Some(release(VERSION))));
    assert_eq!(state.update, UpdateState::Current, "this very version");
    send(&mut state, Msg::ReleaseRead(Some(release("0.0.1"))));
    assert_eq!(state.update, UpdateState::Current, "an older one");

    send(&mut state, Msg::ReleaseRead(Some(release("99.0.0"))));
    let offer = view::dock(&state, &context()).update.unwrap();
    assert_eq!((offer.title.as_str(), offer.enabled), ("새 버전 99.0.0", true));
    assert!(offer.tooltip.contains("99.0.0"));
    let menu = view::menu(&state, true, &context());
    assert!(matches!(&menu[0], MenuEntry::Item(item) if item.action == Some(MenuAction::InstallUpdate) && item.title == "Errand 99.0.0로 업데이트"), "first in the menu");

    send(&mut state, Msg::ReleaseRead(None));
    assert!(matches!(state.update, UpdateState::Available(_)), "being offline takes nothing back");
}

#[test]
fn nothing_installs_until_the_person_presses() {
    let mut state = state();
    assert!(send(&mut state, Msg::InstallUpdate).is_empty(), "nothing to install");
    let effects = send(&mut state, Msg::ReleaseRead(Some(release("99.0.0"))));
    assert!(effects.is_empty(), "finding a release installs nothing");
    assert_eq!(send(&mut state, Msg::InstallUpdate), [Effect::InstallUpdate(release("99.0.0"))]);
    let offer = view::dock(&state, &context()).update.unwrap();
    assert_eq!((offer.title.as_str(), offer.enabled), ("업데이트하는 중…", false));
    assert!(send(&mut state, Msg::InstallUpdate).is_empty(), "no second install while one runs");
    assert!(send(&mut state, Msg::CheckForUpdate).is_empty(), "and no check beside it");
    send(&mut state, Msg::ReleaseRead(Some(release("99.0.1"))));
    assert_eq!(state.update, UpdateState::Installing(release("99.0.0")), "a late answer changes nothing");
}

#[test]
fn a_failed_install_can_be_tried_again() {
    let mut state = state();
    send(&mut state, Msg::ReleaseRead(Some(release("99.0.0"))));
    send(&mut state, Msg::InstallUpdate);
    send(&mut state, Msg::UpdateFailed);
    let offer = view::dock(&state, &context()).update.unwrap();
    assert_eq!((offer.title.as_str(), offer.enabled), ("다시 시도", true));
    assert_eq!(send(&mut state, Msg::InstallUpdate), [Effect::InstallUpdate(release("99.0.0"))]);
}

#[test]
fn a_release_without_an_app_opens_its_page() {
    let mut state = state();
    send(&mut state, Msg::ReleaseRead(Some(Release { version: "99.0.0".to_owned(), download: None })));
    assert_eq!(send(&mut state, Msg::InstallUpdate), [Effect::OpenUrl(RELEASES_PAGE.to_owned())]);
    assert!(matches!(state.update, UpdateState::Available(_)), "still on offer");
}
