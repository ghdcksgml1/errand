//! Manual smoke check: cargo run -p errand-io --example task-title -- codex /path/to/codex /path/to/sample.png
use errand_core::{Language, Provider, tasks::Input};
fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(args.len(), 4, "provider executable sample-file");
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
        &files,
        Language::Korean,
        &std::sync::atomic::AtomicBool::new(false),
    ) {
        Ok(title) => println!("{title}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
