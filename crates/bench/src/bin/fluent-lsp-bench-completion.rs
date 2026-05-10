use bench::{
    CommonArgs, base_result, load_fixture, measure_repeats, print_fixture_load,
    print_result, run_warmup,
};
use serde_json::json;
use tower_lsp::ls_types::CompletionResponse;

fn main() -> anyhow::Result<()> {
    let args = CommonArgs::parse_with_defaults(CommonArgs {
        warmup_repeats: 3,
        repeats: 200,
        ..CommonArgs::default()
    });
    let fixture = load_fixture(&args)?;
    print_fixture_load("completion", &fixture.load_stats);
    run_warmup("completion", args.warmup_repeats, || {
        let _ = fixture.harness.completion(
            &fixture.targets.translation_path,
            fixture.targets.completion_position,
        );
        Ok(())
    })?;

    let (runs, last_summary) =
        measure_repeats("completion", args.repeats, || {
            let result = fixture.harness.completion(
                &fixture.targets.translation_path,
                fixture.targets.completion_position,
            )?;
            let item_count = match result {
                Some(CompletionResponse::Array(items)) => items.len(),
                Some(CompletionResponse::List(list)) => list.items.len(),
                None => 0,
            };
            Ok(json!({
                "item_count": item_count,
            }))
        })?;

    print_result(&base_result(
        "completion",
        &args,
        &fixture.load_stats,
        last_summary,
        runs,
    ));
    Ok(())
}
