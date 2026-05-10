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
    print_fixture_load("code-action", &fixture.load_stats);
    run_warmup("code-action", args.warmup_repeats, || {
        let _ = fixture.harness.code_actions(
            &fixture.targets.translation_path,
            fixture.targets.code_action_position,
        );
        Ok(())
    })?;

    let (runs, last_summary) = measure_repeats(
        "code-action",
        args.repeats,
        || {
            let result = fixture.harness.code_actions(
                &fixture.targets.translation_path,
                fixture.targets.code_action_position,
            )?;
            Ok(json!({
                "action_count": result.as_ref().map(|items| items.len()).unwrap_or(0),
            }))
        },
    )?;

    print_result(&base_result(
        "code-action",
        &args,
        &fixture.load_stats,
        last_summary,
        runs,
    ));
    Ok(())
}
