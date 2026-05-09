use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

use fluent_lsp::{IndexBuildSummary, WorkspaceConfig, build_workspace_index_snapshot};
use rustc_hash::FxHashMap;
use serde_json::json;
use tempfile::TempDir;
use tower_lsp::ls_types::Uri;

#[derive(Clone, Copy)]
struct Shape {
    languages: usize,
    files_per_language: usize,
    messages_per_file: usize,
}

fn main() {
    let args = Args::parse();
    let shape = Shape {
        languages: args.languages,
        files_per_language: args.files_per_language,
        messages_per_file: args.messages_per_file,
    };
    let generated_workspace = args
        .workspace_root
        .is_none()
        .then(|| generate_workspace(shape));
    let workspace_root = args
        .workspace_root
        .clone()
        .unwrap_or_else(|| generated_workspace.as_ref().unwrap().path().to_path_buf());
    let config = WorkspaceConfig::load(workspace_root).unwrap();
    let overlays = FxHashMap::<Uri, String>::default();

    let mut runs = Vec::with_capacity(args.repeats);
    let mut last_summary = None;
    for iteration in 0..args.repeats {
        let started = Instant::now();
        let (snapshot, summary) = build_workspace_index_snapshot(&config, &overlays);
        let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
        last_summary = Some(summary);
        black_box((snapshot, summary));
        runs.push(json!({
            "iteration": iteration,
            "elapsed_ms": elapsed_ms,
        }));
    }

    let result = json!({
        "shape": {
            "languages": shape.languages,
            "files_per_language": shape.files_per_language,
            "messages_per_file": shape.messages_per_file,
        },
        "repeats": args.repeats,
        "summary": summary_to_json(last_summary.expect("bench should run at least once")),
        "runs": runs,
    });
    println!("{}", serde_json::to_string_pretty(&result).unwrap());
}

fn summary_to_json(summary: IndexBuildSummary) -> serde_json::Value {
    json!({
        "discovered_files": summary.discovered_files,
        "snapshotted_files": summary.snapshotted_files,
        "indexed_files": summary.indexed_files,
    })
}

struct Args {
    languages: usize,
    files_per_language: usize,
    messages_per_file: usize,
    repeats: usize,
    workspace_root: Option<PathBuf>,
}

impl Args {
    fn parse() -> Self {
        let mut args = std::env::args().skip(1);
        let mut parsed = Self {
            languages: 31,
            files_per_language: 50,
            messages_per_file: 200,
            repeats: 10,
            workspace_root: None,
        };

        while let Some(flag) = args.next() {
            match flag.as_str() {
                "--languages" => {
                    let value = args
                        .next()
                        .unwrap_or_else(|| panic!("missing value for {flag}"));
                    parsed.languages = value.parse().expect("invalid --languages");
                }
                "--files-per-language" => {
                    let value = args
                        .next()
                        .unwrap_or_else(|| panic!("missing value for {flag}"));
                    parsed.files_per_language =
                        value.parse().expect("invalid --files-per-language");
                }
                "--messages-per-file" => {
                    let value = args
                        .next()
                        .unwrap_or_else(|| panic!("missing value for {flag}"));
                    parsed.messages_per_file = value.parse().expect("invalid --messages-per-file");
                }
                "--repeats" => {
                    let value = args
                        .next()
                        .unwrap_or_else(|| panic!("missing value for {flag}"));
                    parsed.repeats = value.parse().expect("invalid --repeats");
                }
                "--workspace-root" => {
                    let value = args
                        .next()
                        .unwrap_or_else(|| panic!("missing value for {flag}"));
                    parsed.workspace_root = Some(PathBuf::from(value));
                }
                _ => panic!("unknown flag {flag}"),
            }
        }

        parsed
    }
}

fn generate_workspace(shape: Shape) -> TempDir {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    std::fs::write(
        root.join("fluent-lsp.toml"),
        "origin_language = \"lang00\"\nfile_masks = [\"locales/{lang}/{filepath}.ftl\"]\n",
    )
    .unwrap();

    for language in 0..shape.languages {
        for file in 0..shape.files_per_language {
            let relative = format!("locales/lang{language:02}/file{file:03}.ftl");
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(
                &path,
                build_file_contents(language, file, shape.messages_per_file),
            )
            .unwrap();
        }
    }

    temp
}

fn build_file_contents(language: usize, file: usize, messages: usize) -> String {
    let mut source = String::new();
    for message in 0..messages {
        source.push_str(&format!(
            "message-{message:04} = lang{language:02} file {file} value {message}\n"
        ));
    }
    source
}
