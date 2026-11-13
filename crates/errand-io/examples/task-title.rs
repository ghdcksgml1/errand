//! Manual smoke check: cargo run -p errand-io --example task-title -- codex /path/to/codex /path/to/sample.png
use errand_core::{Language, Provider, tasks::Input};
fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert!((4..=5).contains(&args.len()), "provider executable sample-file [model]");
    let provider = if args[1] == "claude" {
        Provider::Claude
    } else {
        Provider::Codex
    };
    let root = tempfile::tempdir().unwrap();
    let files =
        errand_io::tasks::import(root.path(), &[Input::File((&args[3]).into())], 12).unwrap();
    match errand_io::task_title::generate(
        root.path(),
        Some(std::path::Path::new(&args[2])),
        provider,
        args.get(4).map_or("", String::as_str),
        &files,
        Language::Korean,
        &std::sync::atomic::AtomicBool::new(false),
    ) {
        Ok(suggestion) => println!("Title: {}\nMemo: {}", suggestion.title, suggestion.memo),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
