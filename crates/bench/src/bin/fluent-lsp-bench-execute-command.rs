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
    print_fixture_load("execute-command", &fixture.load_stats);
    run_warmup("execute-command", args.warmup_repeats, || {
        let _ = fixture.harness.selector_combinations_document(
            &fixture.targets.translation_path,
            &fixture.targets.selector_key,
        );
        Ok(())
    })?;

    let (runs, last_summary) =
        measure_repeats("execute-command", args.repeats, || {
            let document = fixture.harness.selector_combinations_document(
                &fixture.targets.translation_path,
                &fixture.targets.selector_key,
            )?;
            Ok(json!({
                "document_bytes": document.len(),
                "document_lines": document.lines().count(),
            }))
        })?;

    print_result(&base_result(
        "execute-command",
        &args,
        &fixture.load_stats,
        last_summary,
        runs,
    ));
    Ok(())
}
