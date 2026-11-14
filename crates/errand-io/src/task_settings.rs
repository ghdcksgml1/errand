//! Errand's AI preferences, kept separately from task records and the CLIs' own settings.
use errand_core::{PerProvider, Provider, tasks::{AiSettings, ModelOption, valid_model}};
use serde_json::{Value, json};
use std::{fs, io::Write, path::Path};
const READ_ERROR: &str = "AI 설정을 읽지 못했습니다. 플랫폼과 모델을 다시 저장해 주세요. / Could not read AI settings; save your platform and model again.";
const ERROR: &str = "AI 설정을 저장하지 못했습니다. 폴더 권한을 확인해 주세요. / Could not save AI settings; check folder permissions.";

pub fn load(root: &Path, previous_provider: Provider) -> Result<AiSettings, String> {
    let bytes = match fs::read(root.join("ai-settings.json")) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(AiSettings { provider: previous_provider, ..AiSettings::default() }),
        Err(_) => return Err(READ_ERROR.into()),
    };
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| READ_ERROR)?;
    if value["version"] != 1 { return Err(READ_ERROR.into()) }
    let provider = match value["provider"].as_str() { Some("codex") => Provider::Codex, Some("claude") => Provider::Claude, _ => return Err(READ_ERROR.into()) };
    let model = |key: &str| value["models"][key].as_str().filter(|id| valid_model(id)).map(str::to_owned).ok_or_else(|| READ_ERROR.to_owned());
    // Settings saved before summaries existed leave them on.
    let summaries = value["summaries"]["auto"].as_bool().unwrap_or(true);
    Ok(AiSettings { provider, models: PerProvider { codex: model("codex")?, claude: model("claude")? }, summaries })
}
pub fn save(root: &Path, settings: &AiSettings) -> Result<(), String> {
    if !settings.valid() { return Err(ERROR.into()) }
    fs::create_dir_all(root).map_err(|_| ERROR)?;
    #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; fs::set_permissions(root, fs::Permissions::from_mode(0o700)).map_err(|_| ERROR)?; }
    let value = json!({"version": 1, "provider": if settings.provider == Provider::Codex { "codex" } else { "claude" },
        "models": { "codex": settings.models.codex, "claude": settings.models.claude }, "summaries": { "auto": settings.summaries }});
    let mut temp = tempfile::NamedTempFile::new_in(root).map_err(|_| ERROR)?;
    temp.write_all(&serde_json::to_vec(&value).map_err(|_| ERROR)?).map_err(|_| ERROR)?;
    temp.as_file().sync_all().map_err(|_| ERROR)?;
    temp.persist(root.join("ai-settings.json")).map_err(|_| ERROR)?;
    Ok(())
}
/// Use the account's cached Codex picker, without making a model API request or copying credentials.
pub fn codex_models(home: &Path) -> Vec<ModelOption> {
    let Some(value) = fs::read(home.join("models_cache.json")).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok()) else { return vec![] };
    let mut models = vec![];
    for model in value["models"].as_array().into_iter().flatten() {
        if model["visibility"] != "list" { continue }
        let Some(id) = model["slug"].as_str().filter(|id| !id.is_empty() && valid_model(id)) else { continue };
        if models.iter().any(|entry: &ModelOption| entry.id == id) { continue }
        models.push(ModelOption { id: id.into(), label: model["display_name"].as_str().unwrap_or(id).into() });
    }
    models
}
