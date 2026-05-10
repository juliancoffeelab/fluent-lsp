use bench::{
    CommonArgs, base_result, load_disk_fixture, measure_repeats,
    print_fixture_load, print_result, run_warmup,
};
use fluent_lsp::{WorkspaceConfig, build_workspace_index_snapshot};
use rustc_hash::FxHashMap;
use serde_json::json;
use tower_lsp::ls_types::Uri;

fn main() -> anyhow::Result<()> {
    let args = CommonArgs::parse_with_defaults(CommonArgs {
        warmup_repeats: 1,
        repeats: 3,
        ..CommonArgs::default()
    });
    let fixture = load_disk_fixture(&args)?;
    let config = WorkspaceConfig::load(fixture.root.clone())?;
    let overlays = FxHashMap::<Uri, String>::default();
    print_fixture_load("index", &fixture.load_stats);
    run_warmup("index", args.warmup_repeats, || {
        let _ = build_workspace_index_snapshot(&config, &overlays);
        Ok(())
    })?;

    let (runs, last_summary) = measure_repeats("index", args.repeats, || {
        let (snapshot, summary) =
            build_workspace_index_snapshot(&config, &overlays);
        Ok(json!({
            "discovered_files": summary.discovered_files,
            "snapshotted_files": summary.snapshotted_files,
            "indexed_files": summary.indexed_files,
            "snapshot_file_count": snapshot.len(),
        }))
    })?;

    print_result(&base_result(
        "index",
        &args,
        &fixture.load_stats,
        last_summary,
        runs,
    ));
    Ok(())
}
