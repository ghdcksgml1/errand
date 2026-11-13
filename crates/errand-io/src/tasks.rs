//! Private attachment copies and an atomically replaced personal-task manifest.
use errand_core::{
    Provider,
    tasks::{Attachment, Input, MAX_FILE_BYTES, Task},
};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
pub const STORAGE_ERROR: &str = "첨부 작업을 저장하지 못했습니다. 저장 공간과 폴더 권한을 확인해 주세요. / Could not save. Check free space and folder permissions.";
fn private_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|_| STORAGE_ERROR.to_owned())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|_| STORAGE_ERROR.to_owned())?;
    }
    Ok(())
}
pub fn attachment_path(root: &Path, attachment: &Attachment) -> Option<PathBuf> {
    let mut parts = Path::new(&attachment.id).components();
    if !matches!(parts.next(), Some(Component::Normal(_))) || parts.next().is_some() {
        return None;
    }
    let path = root.join("attachments").join(&attachment.id);
    if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return None;
    }
    Some(path)
}
pub fn cleanup(root: &Path, files: &[Attachment]) {
    for file in files {
        if let Some(path) = attachment_path(root, file) {
            let _ = fs::remove_file(path);
        }
    }
}
pub fn import(root: &Path, inputs: &[Input], remaining: usize) -> Result<Vec<Attachment>, String> {
    if inputs.is_empty() || inputs.len() > remaining {
        return Err(
            "한 작업에는 파일을 최대 12개까지 넣을 수 있습니다. / Up to 12 files per task.".into(),
        );
    }
    private_dir(root)?;
    let directory = root.join("attachments");
    private_dir(&directory)?;
    let mut imported = Vec::new();
    for input in inputs {
        let result = (|| {
            let (name, mut reader): (String, Box<dyn Read>) = match input {
                Input::File(path) => {
                    let metadata = fs::metadata(path)
                        .map_err(|_| "파일을 읽을 수 없습니다. / Could not read the file.")?;
                    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
                        return Err("폴더는 지원하지 않습니다. 파일은 각각 50MB 이하여야 합니다. / Files only, up to 50 MB each.".into());
                    }
                    (
                        path.file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                        Box::new(
                            fs::File::open(path)
                                .map_err(|_| "파일을 열 수 없습니다. / Could not open the file.")?,
                        ),
                    )
                }
                Input::Png(bytes) => (
                    "Dropped image.png".into(),
                    Box::new(std::io::Cursor::new(bytes.clone())),
                ),
            };
            let ext = Path::new(&name)
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let ext = if ext.len() <= 12 && ext.chars().all(|c| c.is_ascii_alphanumeric()) {
                ext
            } else {
                String::new()
            };
            let id = format!(
                "{}-{}.{}",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed),
                ext
            );
            let mut temp =
                tempfile::NamedTempFile::new_in(&directory).map_err(|_| STORAGE_ERROR)?;
            let bytes = std::io::copy(&mut reader.by_ref().take(MAX_FILE_BYTES + 1), &mut temp)
                .map_err(|_| STORAGE_ERROR)?;
            if bytes > MAX_FILE_BYTES {
                return Err(
                    "파일은 각각 50MB 이하여야 합니다. / Each file must be under 50 MB.".into(),
                );
            }
            temp.as_file().sync_all().map_err(|_| STORAGE_ERROR)?;
            temp.persist(directory.join(&id))
                .map_err(|_| STORAGE_ERROR)?;
            Ok(Attachment {
                id,
                name,
                bytes,
                image: matches!(
                    ext.as_str(),
                    "png"
                        | "jpg"
                        | "jpeg"
                        | "gif"
                        | "webp"
                        | "heic"
                        | "heif"
                        | "tif"
                        | "tiff"
                        | "bmp"
                ),
            })
        })();
        match result {
            Ok(file) => imported.push(file),
            Err(error) => {
                cleanup(root, &imported);
                return Err(error);
            }
        }
    }
    Ok(imported)
}
pub fn save(root: &Path, records: &[Task], provider: Provider) -> Result<(), String> {
    private_dir(root)?;
    let records: Vec<_> = records.iter().map(|task| json!({
        "id": task.id, "title": task.title, "memo": task.memo, "created": task.created.to_string(), "done": task.done,
        "attachments": task.attachments.iter().map(|f| json!({"id": f.id, "name": f.name, "bytes": f.bytes, "image": f.image})).collect::<Vec<_>>()
    })).collect();
    let bytes = serde_json::to_vec(&json!({"version": 1, "provider": if provider == Provider::Claude { "claude" } else { "codex" }, "tasks": records})).map_err(|_| STORAGE_ERROR)?;
    let mut temp = tempfile::NamedTempFile::new_in(root).map_err(|_| STORAGE_ERROR)?;
    temp.write_all(&bytes).map_err(|_| STORAGE_ERROR)?;
    temp.as_file().sync_all().map_err(|_| STORAGE_ERROR)?;
    temp.persist(root.join("tasks.json"))
        .map_err(|_| STORAGE_ERROR)?;
    Ok(())
}
pub fn load(root: &Path) -> Result<(Vec<Task>, Provider), String> {
    let path = root.join("tasks.json");
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((vec![], Provider::Codex));
        }
        Err(_) => return Err(STORAGE_ERROR.into()),
    };
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| STORAGE_ERROR)?;
    if value["version"] != 1 {
        return Err(STORAGE_ERROR.into());
    }
    let string = |v: &Value, key: &str| {
        v[key]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| STORAGE_ERROR.to_owned())
    };
    let mut records = vec![];
    for value in value["tasks"].as_array().ok_or(STORAGE_ERROR)? {
        let mut attachments = vec![];
        for f in value["attachments"].as_array().ok_or(STORAGE_ERROR)? {
            let file = Attachment {
                id: string(f, "id")?,
                name: string(f, "name")?,
                bytes: f["bytes"].as_u64().ok_or(STORAGE_ERROR)?,
                image: f["image"].as_bool().ok_or(STORAGE_ERROR)?,
            };
            attachment_path(root, &file).ok_or(STORAGE_ERROR)?;
            attachments.push(file);
        }
        let id = string(value, "id")?;
        if !id.starts_with("manual:") {
            return Err(STORAGE_ERROR.into());
        }
        records.push(Task {
            id,
            title: string(value, "title")?,
            memo: string(value, "memo")?,
            created: string(value, "created")?
                .parse()
                .map_err(|_| STORAGE_ERROR)?,
            done: value["done"].as_bool().ok_or(STORAGE_ERROR)?,
            attachments,
        });
    }
    Ok((
        records,
        if value["provider"] == "claude" {
            Provider::Claude
        } else {
            Provider::Codex
        },
    ))
}

/// Called only after a manifest was successfully read, before import workers start.
/// Cancelling normally removes drafts immediately; this also removes drafts interrupted by a crash.
pub fn discard_orphans(root: &Path, records: &[Task]) {
    let kept: std::collections::HashSet<&str> = records.iter().flat_map(|task| &task.attachments).map(|file| file.id.as_str()).collect();
    let Ok(entries) = fs::read_dir(root.join("attachments")) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !kept.contains(name.as_str()) && name.split_once('-').is_some_and(|(time, _)| !time.is_empty() && time.bytes().all(|c| c.is_ascii_digit())) {
            let file = Attachment { id: name, name: String::new(), bytes: 0, image: false };
            cleanup(root, &[file]);
        }
    }
}
