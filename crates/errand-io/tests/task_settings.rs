use errand_core::{Provider, tasks::AiSettings};
use errand_io::task_settings::{load, save, codex_models};
use std::fs;

#[test]
fn first_run_and_older_settings_do_not_enable_summaries() {
    let temp = tempfile::tempdir().unwrap();
    let settings = load(temp.path(), Provider::Claude).unwrap();
    assert!(!settings.summaries && !settings.onboarding_complete);
    let mut old = serde_json::json!({"version": 1, "provider": "claude", "models": {"codex": "", "claude": "sonnet"}});
    fs::write(temp.path().join("ai-settings.json"), old.to_string()).unwrap();
    let settings = load(temp.path(), Provider::Codex).unwrap();
    assert!(!settings.summaries && !settings.onboarding_complete);
    assert_eq!(settings.models.claude, "sonnet");

    // A previous explicit preference survives migration, while setup still needs to be completed.
    for enabled in [true, false] {
        old["summaries"] = serde_json::json!({"auto": enabled});
        fs::write(temp.path().join("ai-settings.json"), old.to_string()).unwrap();
        let mut settings = load(temp.path(), Provider::Codex).unwrap();
        assert_eq!(settings.summaries, enabled);
        assert!(!settings.onboarding_complete);
        settings.onboarding_complete = true;
        save(temp.path(), &settings).unwrap();
        assert_eq!(load(temp.path(), Provider::Codex).unwrap(), settings, "setup and the summary choice survive a restart");
    }
}

#[test]
fn remembers_each_providers_model_and_migrates_the_previous_provider() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("tasks");
    let mut settings = load(&root, Provider::Claude).unwrap();
    assert_eq!(settings.provider, Provider::Claude);
    assert_eq!(settings.model(), "");
    settings.models.claude = "sonnet[1m]".into();
    settings.models.codex = "codex-test-model".into();
    save(&root, &settings).unwrap();
    assert_eq!(load(&root, Provider::Codex).unwrap(), settings);
    settings.provider = Provider::Codex;
    save(&root, &settings).unwrap();
    assert_eq!(load(&root, Provider::Claude).unwrap(), settings);
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(root.join("ai-settings.json")).unwrap().permissions().mode() & 0o777, 0o600);
    }
}

#[test]
fn rejects_invalid_models_and_reports_storage_failure_without_overwriting() {
    let temp = tempfile::tempdir().unwrap();
    save(temp.path(), &AiSettings::default()).unwrap();
    for model in ["--help", "model with spaces", "$(command)", "a\nb"] {
        let mut settings = AiSettings::default(); settings.models.codex = model.into();
        assert!(save(temp.path(), &settings).is_err());
        assert_eq!(load(temp.path(), Provider::Codex).unwrap(), AiSettings::default());
    }
    fs::write(temp.path().join("ai-settings.json"), "broken").unwrap();
    assert!(load(temp.path(), Provider::Codex).is_err());
    let file = temp.path().join("not-a-directory"); fs::write(&file, "keep").unwrap();
    assert!(save(&file, &AiSettings::default()).is_err());
    assert_eq!(fs::read_to_string(file).unwrap(), "keep");
}

#[test]
fn model_picker_reads_visible_cached_models_only_and_handles_missing_cache() {
    let temp = tempfile::tempdir().unwrap();
    assert!(codex_models(temp.path()).is_empty());
    fs::write(temp.path().join("models_cache.json"), r#"{"models":[
        {"slug":"available-model","display_name":"Available model","visibility":"list"},
        {"slug":"hidden-model","visibility":"hide"},
        {"slug":"available-model","visibility":"list"},
        {"slug":"--bad","visibility":"list"},
        {"slug":"another-model","visibility":"list"}
    ]}"#).unwrap();
    let models = codex_models(temp.path());
    assert_eq!(models.iter().map(|model| (model.id.as_str(), model.label.as_str())).collect::<Vec<_>>(),
        [("available-model", "Available model"), ("another-model", "another-model")]);
}
