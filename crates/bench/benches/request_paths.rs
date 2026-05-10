use bench::{
    CommonArgs, file_uri, load_disk_fixture, load_fixture,
    origin_and_translation_sources,
};
use criterion::Criterion;
use fluent_lsp::{
    WorkspaceConfig, benchmark_missing_entries_code_actions,
    build_workspace_index_snapshot,
};
use rustc_hash::FxHashMap;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tower_lsp::ls_types::Uri;

fn request_paths(c: &mut Criterion) {
    let mut args = CommonArgs::default();
    let shape = CommonArgs::criterion_shape();
    args.languages = shape.languages;
    args.files_per_language = shape.files_per_language;
    args.messages_per_file = shape.messages_per_file;
    let fixture = load_fixture(&args).expect("failed to build bench fixture");
    let origin_uri = file_uri(&fixture.targets.origin_path);
    let (origin_source, translation_source) =
        origin_and_translation_sources(&fixture)
            .expect("failed to load missing-entries bench sources");

    c.bench_function("definition", |b| {
        b.iter(|| {
            let _ = fixture.harness.definition(
                &fixture.targets.translation_path,
                fixture.targets.definition_position,
            );
        })
    });

    c.bench_function("references", |b| {
        b.iter(|| {
            let _ = fixture.harness.references(
                &fixture.targets.origin_path,
                &origin_uri,
                fixture.targets.references_position,
                true,
            );
        })
    });

    c.bench_function("hover", |b| {
        b.iter(|| {
            let _ = fixture
                .harness
                .hover(
                    &fixture.targets.translation_path,
                    fixture.targets.hover_position,
                )
                .expect("hover bench failed");
        })
    });

    c.bench_function("completion", |b| {
        b.iter(|| {
            let _ = fixture
                .harness
                .completion(
                    &fixture.targets.translation_path,
                    fixture.targets.completion_position,
                )
                .expect("completion bench failed");
        })
    });

    c.bench_function("code_action", |b| {
        b.iter(|| {
            let _ = fixture
                .harness
                .code_actions(
                    &fixture.targets.translation_path,
                    fixture.targets.code_action_position,
                )
                .expect("code action bench failed");
        })
    });

    c.bench_function("code_lens", |b| {
        b.iter(|| {
            let _ = fixture
                .harness
                .code_lenses(&fixture.targets.translation_path)
                .expect("code lens bench failed");
        })
    });

    c.bench_function("execute_command", |b| {
        b.iter(|| {
            let _ = fixture
                .harness
                .selector_combinations_document(
                    &fixture.targets.translation_path,
                    &fixture.targets.selector_key,
                )
                .expect("execute command bench failed");
        })
    });

    c.bench_function("missing_entries", |b| {
        b.iter(|| {
            let _ = benchmark_missing_entries_code_actions(
                &origin_source,
                &translation_source,
            );
        })
    });

    let index_fixture =
        load_disk_fixture(&args).expect("failed to build index bench fixture");
    let config = WorkspaceConfig::load(index_fixture.root.clone())
        .expect("failed to load workspace config");
    let overlays = FxHashMap::<Uri, String>::default();
    c.bench_function("index", |b| {
        b.iter(|| {
            let _ = build_workspace_index_snapshot(&config, &overlays);
        })
    });
}

fn criterion_output_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../benchmarks/criterion")
}

fn configured_criterion() -> Criterion {
    Criterion::default()
        .sample_size(30)
        .warm_up_time(Duration::from_secs(3))
        .measurement_time(Duration::from_secs(10))
        .output_directory(&criterion_output_directory())
        .configure_from_args()
        .output_directory(&criterion_output_directory())
}

fn main() {
    let mut criterion = configured_criterion();
    request_paths(&mut criterion);
    criterion.final_summary();
}
