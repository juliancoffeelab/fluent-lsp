use bench::{
    CommonArgs, base_result, load_fixture, measure_repeats,
    origin_and_translation_sources, print_fixture_load, print_result,
    run_warmup,
};
use fluent_lsp::benchmark_missing_entries_code_actions;
use serde_json::json;

fn main() -> anyhow::Result<()> {
    let args = CommonArgs::parse_with_defaults(CommonArgs {
        warmup_repeats: 2,
        repeats: 25,
        ..CommonArgs::default()
    });
    let fixture = load_fixture(&args)?;
    let (origin_source, translation_source) =
        origin_and_translation_sources(&fixture)?;
    print_fixture_load("missing-entries", &fixture.load_stats);
    run_warmup("missing-entries", args.warmup_repeats, || {
        let _ = benchmark_missing_entries_code_actions(
            &origin_source,
            &translation_source,
        );
        Ok(())
    })?;

    let (runs, last_summary) = measure_repeats(
        "missing-entries",
        args.repeats,
        || {
            let summary = benchmark_missing_entries_code_actions(
                &origin_source,
                &translation_source,
            );
            Ok(json!({
                "missing_message_count": summary.missing_message_count,
                "missing_attribute_group_count": summary.missing_attribute_group_count,
                "empty_stub_edit_count": summary.empty_stub_edit_count,
                "copy_source_edit_count": summary.copy_source_edit_count,
                "single_message_copy_action_count": summary.single_message_copy_action_count,
            }))
        },
    )?;

    print_result(&base_result(
        "missing-entries",
        &args,
        &fixture.load_stats,
        last_summary,
        runs,
    ));
    Ok(())
}
