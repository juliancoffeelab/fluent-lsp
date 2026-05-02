use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use tempfile::tempdir;

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("workspace")
}

fn run_scenario(name: &str) -> Value {
    let output_dir = tempdir().unwrap();
    let result_path = output_dir.path().join("result.json");
    let fixture = fixture_root();
    let scenario_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("nvim")
        .join(name);

    let status = Command::new("nvim")
        .arg("--headless")
        .arg("-u")
        .arg(scenario_dir.join("init.lua"))
        .arg("+lua require('smoke').run()")
        .env("FLUENT_LSP_BIN", env!("CARGO_BIN_EXE_fluent-lsp"))
        .env("FLUENT_LSP_WORKSPACE", &fixture)
        .env("FLUENT_LSP_RESULT", &result_path)
        .status()
        .expect("failed to run nvim");

    assert!(status.success(), "nvim exited with {status}");

    let raw = std::fs::read_to_string(&result_path).unwrap();
    serde_json::from_str(&raw).unwrap()
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
}

#[test]
fn nvim_smoke_hover_selector_combinations() {
    let result = run_scenario("hover_selector_combinations");
    assert_eq!(result["ok"], Value::Bool(true));
}

#[test]
fn nvim_smoke_hover_comment_structure() {
    let result = run_scenario("hover_comment_structure");
    assert_eq!(result["ok"], Value::Bool(true));
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
}

#[test]
fn nvim_smoke_codelens_selector_combinations() {
    let result = run_scenario("codelens_selector_combinations");
    assert_eq!(result["ok"], Value::Bool(true));
}
