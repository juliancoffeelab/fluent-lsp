use bench::{
    CommonArgs, base_result, file_uri, load_fixture, measure_repeats,
    print_fixture_load, print_result, run_warmup,
};
use serde_json::json;

fn main() -> anyhow::Result<()> {
    let args = CommonArgs::parse_with_defaults(CommonArgs {
        warmup_repeats: 3,
        repeats: 200,
        ..CommonArgs::default()
    });
    let fixture = load_fixture(&args)?;
    let uri = file_uri(&fixture.targets.origin_path);
    print_fixture_load("references", &fixture.load_stats);
    run_warmup("references", args.warmup_repeats, || {
        let _ = fixture.harness.references(
            &fixture.targets.origin_path,
            &uri,
            fixture.targets.references_position,
            true,
        );
        Ok(())
    })?;

    let (runs, last_summary) = measure_repeats(
        "references",
        args.repeats,
        || {
            let result = fixture.harness.references(
                &fixture.targets.origin_path,
                &uri,
                fixture.targets.references_position,
                true,
            );
            Ok(json!({
                "reference_count": result.as_ref().map(|items| items.len()).unwrap_or(0),
            }))
        },
    )?;

    print_result(&base_result(
        "references",
        &args,
        &fixture.load_stats,
        last_summary,
        runs,
    ));
    Ok(())
}
