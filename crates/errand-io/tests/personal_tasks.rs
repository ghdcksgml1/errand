use errand_core::{
    Provider,
    tasks::{Attachment, Input, Task},
};
use errand_io::tasks::*;
use std::fs;
#[test]
fn copies_files_and_restores_tasks_after_original_is_gone() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("store");
    let original = temp.path().join("검토 %20 ' $(literal).txt");
    fs::write(&original, "내용").unwrap();
    let files = import(&root, &[Input::File(original.clone())], 12).unwrap();
    fs::remove_file(original).unwrap();
    assert_eq!(
        fs::read_to_string(attachment_path(&root, &files[0]).unwrap()).unwrap(),
        "내용"
    );
    let records = vec![Task {
        id: "manual:123".into(),
        title: "문서 검토".into(),
        memo: "금요일까지".into(),
        attachments: files.clone(),
        created: "2026-11-13T00:00:00Z".parse().unwrap(),
        done: false,
    }];
    save(&root, &records, Provider::Claude).unwrap();
    assert_eq!(load(&root).unwrap(), (records, Provider::Claude));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(root.join("tasks.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(attachment_path(&root, &files[0]).unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
#[test]
fn failed_batch_rolls_back_and_cleanup_cannot_escape_store() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("store");
    let original = temp.path().join("original.txt");
    fs::write(&original, "keep").unwrap();
    assert!(
        import(
            &root,
            &[
                Input::File(original.clone()),
                Input::File(temp.path().join("absent"))
            ],
            12
        )
        .is_err()
    );
    assert_eq!(fs::read_dir(root.join("attachments")).unwrap().count(), 0);
    let escape = Attachment {
        id: "../../original.txt".into(),
        name: "x".into(),
        bytes: 4,
        image: false,
    };
    cleanup(&root, &[escape]);
    assert!(original.exists());
    assert!(import(&root, &[Input::File(temp.path().into())], 12).is_err());
    assert!(import(&root, &[Input::File(original)], 0).is_err());
}
#[test]
fn broken_manifest_is_an_error_instead_of_an_empty_task_list() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("tasks.json"), "broken").unwrap();
    assert!(load(temp.path()).is_err());
    assert_eq!(
        fs::read_to_string(temp.path().join("tasks.json")).unwrap(),
        "broken"
    );
}

#[test]
fn interrupted_drafts_are_removed_but_saved_attachments_are_kept() {
    let temp = tempfile::tempdir().unwrap();
    let saved = import(temp.path(), &[Input::Png(vec![1, 2, 3])], 12).unwrap();
    let draft = import(temp.path(), &[Input::Png(vec![4, 5, 6])], 12).unwrap();
    let task = Task { id: "manual:1".into(), title: "Saved".into(), memo: String::new(), attachments: saved.clone(), created: "2026-11-13T00:00:00Z".parse().unwrap(), done: true };
    discard_orphans(temp.path(), &[task]);
    assert!(attachment_path(temp.path(), &saved[0]).unwrap().exists());
    assert!(!attachment_path(temp.path(), &draft[0]).unwrap().exists());
}

#[cfg(unix)]
#[test]
fn both_cli_protocols_disable_tools_and_use_the_selected_model_for_title_and_memo() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let original = temp.path().join("Review $(literal) file.txt"); fs::write(&original, "Please review the onboarding document").unwrap();
    let root = temp.path().join("store"); let files = import(&root, &[Input::File(original)], 12).unwrap();
    let fake = temp.path().join("fake-cli");
    fs::write(&fake, r#"#!/usr/bin/python3
import json,sys,pathlib,os
args=sys.argv[1:]
request=sys.stdin.read()
assert 'Please review' in request
assert args[args.index('--model')+1]=='test-model'
if args[0]=='exec':
    assert '--ignore-user-config' in args and '--ephemeral' in args
    assert args[args.index('--sandbox')+1]=='read-only'
    assert 'shell_tool' in args and 'unified_exec' in args and 'multi_agent' in args
    schema=json.loads(pathlib.Path(args[args.index('--output-schema')+1]).read_text())
    assert schema['required']==['title','memo']
    pathlib.Path(args[args.index('--output-last-message')+1]).write_text(json.dumps({'title':'Review onboarding','memo':'Please review the onboarding document'}))
else:
    assert '--safe-mode' in args and '--strict-mcp-config' in args and '--no-session-persistence' in args
    assert args[args.index('--tools')+1]==''
    assert args[args.index('--setting-sources')+1]==''
    value=json.loads(request)
    assert value['message']['role']=='user'
    assert 'ANTHROPIC_API_KEY' not in os.environ
    print(json.dumps({'type':'result','is_error':False,'structured_output':{'title':'Review onboarding','memo':'Please review the onboarding document'}}))
"#).unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
    for provider in Provider::ALL {
        let title = errand_io::task_title::generate(&root, Some(&fake), provider, "test-model", &files, errand_core::Language::English, &std::sync::atomic::AtomicBool::new(false)).unwrap();
        assert_eq!(title.title, "Review onboarding");
        assert_eq!(title.memo, "Please review the onboarding document");
    }
}

#[cfg(target_os = "macos")]
#[test]
fn imports_preserve_macos_quarantine_before_the_copy_can_be_opened() {
    use std::os::unix::fs::PermissionsExt;
    use xattr::FileExt;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("store");
    let value = b"0083;6733abcd;ErrandSecurityTest;12345678-1234-1234-1234-123456789abc";
    for ext in ["dmg", "zip", "pkg", "png"] {
        let original = temp.path().join(format!("download.{ext}"));
        fs::write(&original, b"harmless fixture, not an executable").unwrap();
        let source = fs::File::open(&original).unwrap();
        source.set_xattr("com.apple.quarantine", value).unwrap();
        let actual_value = source.get_xattr("com.apple.quarantine").unwrap().unwrap();
        let files = import(&root, &[Input::File(original.clone())], 12).unwrap();
        let copy = fs::File::open(attachment_path(&root, &files[0]).unwrap()).unwrap();
        assert_eq!(copy.get_xattr("com.apple.quarantine").unwrap(), Some(actual_value));
        assert_eq!(fs::read(attachment_path(&root, &files[0]).unwrap()).unwrap(), fs::read(&original).unwrap());
        assert_eq!(copy.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    }
    let clean = temp.path().join("local.txt");
    fs::write(&clean, "local").unwrap();
    let files = import(&root, &[Input::File(clean), Input::Png(vec![1, 2, 3])], 12).unwrap();
    for file in &files {
        let copy = fs::File::open(attachment_path(&root, file).unwrap()).unwrap();
        assert!(copy.get_xattr("com.apple.quarantine").unwrap().is_none());
    }
}
