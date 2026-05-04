use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use fluent_lsp::render_fluent_preview_text;
use fluent_syntax::parser;
use serde_json::Value;
use tempfile::tempdir;

fn run_scenario(name: &str) -> Value {
    let output_dir = tempdir().unwrap();
    let result_path = output_dir.path().join("result.json");
    let scenario_dir = scenario_dir(name);
    let workspace = scenario_dir.join("workspace");

    let status = Command::new("nvim")
        .arg("--headless")
        .arg("-u")
        .arg(scenario_dir.join("init.lua"))
        .arg("+lua require('smoke').run()")
        .env("FLUENT_LSP_BIN", env!("CARGO_BIN_EXE_fluent-lsp"))
        .env("FLUENT_LSP_WORKSPACE", &workspace)
        .env("FLUENT_LSP_RESULT", &result_path)
        .status()
        .expect("failed to run nvim");

    assert!(status.success(), "nvim exited with {status}");

    let raw = std::fs::read_to_string(&result_path).unwrap();
    serde_json::from_str(&raw).unwrap()
}

fn scenario_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("nvim")
        .join(name)
}

fn scenario_source(name: &str, relative_path: &str) -> String {
    let path = scenario_dir(name).join("workspace").join(relative_path);
    std::fs::read_to_string(path).unwrap()
}

fn hover_value<'a>(result: &'a Value, key: &str) -> &'a str {
    result["hovers"][key]
        .as_str()
        .unwrap_or_else(|| panic!("missing hover payload `{key}`"))
}

fn assert_fluent_parses(source: &str) {
    if let Err((_, errors)) = parser::parse(source) {
        panic!("failed to parse Fluent source with {errors:?}\n{source}");
    }
}

fn extract_ftl_blocks(markdown: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut rest = markdown;
    let opener = "```ftl\n";
    while let Some(start) = rest.find(opener) {
        let after_start = &rest[start + opener.len()..];
        let end = after_start
            .find("\n```")
            .expect("unterminated ftl markdown block");
        blocks.push(after_start[..end].to_string());
        rest = &after_start[end + "\n```".len()..];
    }
    blocks
}

fn assert_hover_block_matches(
    markdown: &str,
    block_index: usize,
    source: &str,
    key: &str,
    overrides: &[(&str, &str)],
) {
    assert_fluent_parses(source);
    let override_map = overrides
        .iter()
        .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
        .collect::<HashMap<_, _>>();
    let expected = render_fluent_preview_text(source, key, Some(&override_map))
        .unwrap_or_else(|| panic!("missing semantic preview for `{key}`"));
    let blocks = extract_ftl_blocks(markdown);
    let actual = blocks
        .get(block_index)
        .unwrap_or_else(|| panic!("missing hover block {block_index} in {markdown}"));
    assert_eq!(actual, &expected);
}

#[test]
fn nvim_smoke_minimal_server_attach() {
    let result = run_scenario("minimal_server_attach");
    assert_eq!(result["ok"], Value::Bool(true));
}

#[test]
fn nvim_smoke_definition_translation() {
    let result = run_scenario("definition_translation");
    assert_eq!(result["ok"], Value::Bool(true));
}

#[test]
fn nvim_smoke_references_origin() {
    let result = run_scenario("references_origin");
    assert_eq!(result["ok"], Value::Bool(true));
}

#[test]
fn nvim_smoke_hover_translation() {
    let result = run_scenario("hover_translation");
    assert_eq!(result["ok"], Value::Bool(true));

    let origin = scenario_source("hover_translation", "locales/en/app.ftl");
    let current = scenario_source("hover_translation", "locales/es/app.ftl");
    assert_hover_block_matches(
        hover_value(&result, "body"),
        0,
        &origin,
        "welcome-body",
        &[],
    );
    assert_hover_block_matches(
        hover_value(&result, "body"),
        1,
        &current,
        "welcome-body",
        &[],
    );
    assert_hover_block_matches(
        hover_value(&result, "selector"),
        0,
        &origin,
        "install-hint",
        &[("$gender", "female")],
    );
    assert_hover_block_matches(
        hover_value(&result, "selector"),
        1,
        &current,
        "install-hint",
        &[("$gender", "female")],
    );
    assert_hover_block_matches(
        hover_value(&result, "attribute"),
        0,
        &origin,
        "download-action.tooltip",
        &[],
    );
    assert_hover_block_matches(
        hover_value(&result, "attribute"),
        1,
        &current,
        "download-action.tooltip",
        &[],
    );
    assert_hover_block_matches(
        hover_value(&result, "post_selector"),
        0,
        &origin,
        "install-hint",
        &[("$gender", "other")],
    );
    assert_hover_block_matches(
        hover_value(&result, "post_selector"),
        1,
        &current,
        "install-hint",
        &[("$gender", "other")],
    );
    assert_hover_block_matches(
        hover_value(&result, "second_selector"),
        0,
        &origin,
        "install-hint",
        &[("$gender", "other"), ("$count", "one")],
    );
    assert_hover_block_matches(
        hover_value(&result, "second_selector"),
        1,
        &current,
        "install-hint",
        &[("$gender", "other"), ("$count", "one")],
    );
}

#[test]
fn nvim_smoke_hover_selector_mismatch() {
    let result = run_scenario("hover_selector_mismatch");
    assert_eq!(result["ok"], Value::Bool(true));

    let origin = scenario_source("hover_selector_mismatch", "locales/en/app.ftl");
    let current = scenario_source("hover_selector_mismatch", "locales/es/app.ftl");
    assert_hover_block_matches(
        hover_value(&result, "mismatch"),
        0,
        &origin,
        "mismatch-rollout",
        &[("$count", "other")],
    );
    assert_hover_block_matches(
        hover_value(&result, "mismatch"),
        1,
        &current,
        "mismatch-rollout",
        &[("$gender", "female")],
    );
    assert_hover_block_matches(
        hover_value(&result, "zero"),
        0,
        &origin,
        "mismatch-rollout",
        &[("$count", "0")],
    );
    assert_hover_block_matches(
        hover_value(&result, "zero"),
        1,
        &current,
        "mismatch-rollout",
        &[("$count", "0")],
    );
    assert_hover_block_matches(
        hover_value(&result, "one"),
        0,
        &origin,
        "mismatch-rollout",
        &[("$count", "1")],
    );
    assert_hover_block_matches(
        hover_value(&result, "one"),
        1,
        &current,
        "mismatch-rollout",
        &[("$count", "1")],
    );
}

#[test]
fn nvim_smoke_hover_selector_zero_lv() {
    let result = run_scenario("hover_selector_zero_lv");
    assert_eq!(result["ok"], Value::Bool(true));

    let origin = scenario_source("hover_selector_zero_lv", "locales/en/app.ftl");
    let current = scenario_source("hover_selector_zero_lv", "locales/lv/app.ftl");
    assert_hover_block_matches(
        hover_value(&result, "zero"),
        0,
        &origin,
        "zero-rollout",
        &[("$count", "zero")],
    );
    assert_hover_block_matches(
        hover_value(&result, "zero"),
        1,
        &current,
        "zero-rollout",
        &[("$count", "zero")],
    );
    assert_hover_block_matches(
        hover_value(&result, "one"),
        0,
        &origin,
        "zero-rollout",
        &[("$count", "one")],
    );
    assert_hover_block_matches(
        hover_value(&result, "one"),
        1,
        &current,
        "zero-rollout",
        &[("$count", "one")],
    );
}

#[test]
fn nvim_smoke_hover_comment_structure() {
    let result = run_scenario("hover_comment_structure");
    assert_eq!(result["ok"], Value::Bool(true));

    let source = scenario_source("hover_comment_structure", "locales/en/app.ftl");
    assert_hover_block_matches(
        hover_value(&result, "body"),
        0,
        &source,
        "commented-preview",
        &[],
    );
}

#[test]
fn nvim_smoke_workspace_locale_tree() {
    let result = run_scenario("workspace_locale_tree");
    assert_eq!(result["ok"], Value::Bool(true));
}

#[test]
fn nvim_smoke_hover_origin_language() {
    let result = run_scenario("hover_origin_language");
    assert_eq!(result["ok"], Value::Bool(true));

    let source = scenario_source("hover_origin_language", "locales/en/dialogs/menu.ftl");
    assert_hover_block_matches(
        hover_value(&result, "body"),
        0,
        &source,
        "commented-menu.tooltip",
        &[],
    );
}

#[test]
fn nvim_smoke_codelens_selector_combinations() {
    let result = run_scenario("codelens_selector_combinations");
    assert_eq!(result["ok"], Value::Bool(true));
}

#[test]
fn nvim_smoke_code_action_generate_selector() {
    let result = run_scenario("code_action_generate_selector");
    assert_eq!(result["ok"], Value::Bool(true));
}

#[test]
fn nvim_smoke_diagnostics_numeric_selectors() {
    let result = run_scenario("diagnostics_numeric_selectors");
    assert_eq!(result["ok"], Value::Bool(true));
}

#[test]
fn nvim_smoke_diagnostics_parse_errors() {
    let result = run_scenario("diagnostics_parse_errors");
    assert_eq!(result["ok"], Value::Bool(true));
    assert_fluent_parses(result["final_buffer"].as_str().unwrap());
}
