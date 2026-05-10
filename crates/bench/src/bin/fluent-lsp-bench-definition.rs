use bench::{
    CommonArgs, base_result, load_fixture, measure_repeats, print_fixture_load,
    print_result, run_warmup,
};
use serde_json::json;

fn main() -> anyhow::Result<()> {
    let args = CommonArgs::parse_with_defaults(CommonArgs {
        warmup_repeats: 3,
        repeats: 200,
        ..CommonArgs::default()
    });
    let fixture = load_fixture(&args)?;
    print_fixture_load("definition", &fixture.load_stats);
    run_warmup("definition", args.warmup_repeats, || {
        let _ = fixture.harness.definition(
            &fixture.targets.translation_path,
            fixture.targets.definition_position,
        );
        Ok(())
    })?;

    let (runs, last_summary) =
        measure_repeats("definition", args.repeats, || {
            let result = fixture.harness.definition(
                &fixture.targets.translation_path,
                fixture.targets.definition_position,
            );
            Ok(json!({
                "found": result.is_some(),
            }))
        })?;

    print_result(&base_result(
        "definition",
        &args,
        &fixture.load_stats,
        last_summary,
        runs,
    ));
    Ok(())
}
