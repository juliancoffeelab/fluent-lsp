use bench::{
    CommonArgs, base_result, load_fixture, measure_repeats, print_fixture_load,
    print_result, run_warmup,
};
use serde_json::json;

fn main() -> anyhow::Result<()> {
    let args = CommonArgs::parse_with_defaults(CommonArgs {
        warmup_repeats: 1,
        repeats: 5,
        ..CommonArgs::default()
    });
    let fixture = load_fixture(&args)?;
    print_fixture_load("code-lens", &fixture.load_stats);
    run_warmup("code-lens", args.warmup_repeats, || {
        let _ = fixture
            .harness
            .code_lenses(&fixture.targets.translation_path);
        Ok(())
    })?;

    let (runs, last_summary) = measure_repeats(
        "code-lens",
        args.repeats,
        || {
            let result = fixture
                .harness
                .code_lenses(&fixture.targets.translation_path)?;
            Ok(json!({
                "lens_count": result.as_ref().map(|items| items.len()).unwrap_or(0),
            }))
        },
    )?;

    print_result(&base_result(
        "code-lens",
        &args,
        &fixture.load_stats,
        last_summary,
        runs,
    ));
    Ok(())
}
