use bench::{
    CommonArgs, base_result, load_fixture, measure_repeats, print_fixture_load,
    print_result, run_warmup,
};
use serde_json::json;

fn main() -> anyhow::Result<()> {
    let args = CommonArgs::parse_with_defaults(CommonArgs {
        warmup_repeats: 2,
        repeats: 25,
        ..CommonArgs::default()
    });
    let fixture = load_fixture(&args)?;
    print_fixture_load("hover", &fixture.load_stats);
    run_warmup("hover", args.warmup_repeats, || {
        let _ = fixture.harness.hover(
            &fixture.targets.translation_path,
            fixture.targets.hover_position,
        );
        Ok(())
    })?;

    let (runs, last_summary) = measure_repeats("hover", args.repeats, || {
        let result = fixture.harness.hover(
            &fixture.targets.translation_path,
            fixture.targets.hover_position,
        )?;
        Ok(json!({
            "has_hover": result.is_some(),
        }))
    })?;

    print_result(&base_result(
        "hover",
        &args,
        &fixture.load_stats,
        last_summary,
        runs,
    ));
    Ok(())
}
