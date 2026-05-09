use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

use fluent_lsp::benchmark_missing_entries_code_actions;
use serde_json::json;
use tempfile::TempDir;

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
    let workspace_root = args.workspace_root.clone().unwrap_or_else(|| {
        generated_workspace.as_ref().unwrap().path().to_path_buf()
    });

    let origin = workspace_root.join("locales/lang00/file000.ftl");
    let translation = workspace_root.join("locales/lang01/file000.ftl");
    let origin_source =
        std::fs::read_to_string(&origin).expect("failed to read origin source");
    let translation_source = std::fs::read_to_string(&translation)
        .expect("failed to read translation source");

    let mut runs = Vec::with_capacity(args.repeats);
    let mut last_summary = None;
    for iteration in 0..args.repeats {
        let started = Instant::now();
        let summary = benchmark_missing_entries_code_actions(
            &origin_source,
            &translation_source,
        );
        let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
        last_summary = Some(summary);
        black_box(summary);
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
        "summary": {
            "missing_message_count": last_summary.expect("bench should run").missing_message_count,
            "missing_attribute_group_count": last_summary.expect("bench should run").missing_attribute_group_count,
            "empty_stub_edit_count": last_summary.expect("bench should run").empty_stub_edit_count,
            "copy_source_edit_count": last_summary.expect("bench should run").copy_source_edit_count,
            "single_message_copy_action_count": last_summary.expect("bench should run").single_message_copy_action_count,
        },
        "runs": runs,
    });
    println!("{}", serde_json::to_string_pretty(&result).unwrap());
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
            messages_per_file: 2000,
            repeats: 10,
            workspace_root: None,
        };

        while let Some(flag) = args.next() {
            match flag.as_str() {
                "--languages" => {
                    parsed.languages = args
                        .next()
                        .unwrap_or_else(|| panic!("missing value for {flag}"))
                        .parse()
                        .expect("invalid --languages");
                }
                "--files-per-language" => {
                    parsed.files_per_language = args
                        .next()
                        .unwrap_or_else(|| panic!("missing value for {flag}"))
                        .parse()
                        .expect("invalid --files-per-language");
                }
                "--messages-per-file" => {
                    parsed.messages_per_file = args
                        .next()
                        .unwrap_or_else(|| panic!("missing value for {flag}"))
                        .parse()
                        .expect("invalid --messages-per-file");
                }
                "--repeats" => {
                    parsed.repeats = args
                        .next()
                        .unwrap_or_else(|| panic!("missing value for {flag}"))
                        .parse()
                        .expect("invalid --repeats");
                }
                "--workspace-root" => {
                    parsed.workspace_root =
                        Some(PathBuf::from(args.next().unwrap_or_else(|| {
                            panic!("missing value for {flag}")
                        })));
                }
                _ => panic!("unknown flag {flag}"),
            }
        }

        parsed
    }
}

fn generate_workspace(shape: Shape) -> TempDir {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("fluent-lsp.toml"),
        "origin_language = \"lang00\"\nfile_masks = [\"locales/{lang}/{filepath}.ftl\"]\n",
    )
    .unwrap();
    for lang in 0..shape.languages {
        let language = format!("lang{lang:02}");
        let dir = temp.path().join("locales").join(&language);
        std::fs::create_dir_all(&dir).unwrap();
        for file in 0..shape.files_per_language {
            let mut source = String::new();
            for message in 0..shape.messages_per_file {
                let key = format!("msg-{file:03}-{message:04}");
                if message % 17 == 0 {
                    source.push_str(&format!("# Generated docs for {key}\n"));
                }
                if lang != 0 && message % 23 == 0 {
                    continue;
                }
                if message % 11 == 0 {
                    source.push_str(&format!(
                        "{key} = {{ $count ->\n    [one] {language} one item\n   *[other] {language} many items\n}}\n"
                    ));
                } else if message % 7 == 0 {
                    source.push_str(&format!(
                        "{key} =\n    .label = {language} label {message}\n    .tooltip = {language} tooltip {message}\n"
                    ));
                } else if message % 5 == 0 {
                    source.push_str(&format!(
                        "-{key} = {language} term {message}\n"
                    ));
                } else {
                    source.push_str(&format!(
                        "{key} = {language} value {message}\n"
                    ));
                }
                source.push('\n');
            }
            if lang == 1 && file == shape.files_per_language - 1 {
                source.push_str("local-only-extra = Local only file marker\n");
            }
            std::fs::write(dir.join(format!("file{file:03}.ftl")), source)
                .unwrap();
        }
    }
    temp
}
