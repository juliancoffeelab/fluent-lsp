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

#[test]
fn nvim_definition_from_translation_jumps_into_origin_file() {
    let output_dir = tempdir().unwrap();
    let result_path = output_dir.path().join("result.json");
    let fixture = fixture_root();

    let status = Command::new("nvim")
        .arg("--headless")
        .arg("-u")
        .arg("tests/nvim/init.lua")
        .arg("+lua require('smoke').run()")
        .env("FLUENT_LSP_BIN", env!("CARGO_BIN_EXE_fluent-lsp"))
        .env("FLUENT_LSP_WORKSPACE", &fixture)
        .env("FLUENT_LSP_RESULT", &result_path)
        .status()
        .expect("failed to run nvim");

    assert!(status.success(), "nvim exited with {status}");

    let raw = std::fs::read_to_string(&result_path).unwrap();
    let result: Value = serde_json::from_str(&raw).unwrap();
    let file = result["file"].as_str().unwrap();
    assert!(
        file.ends_with("locales/en/app.ftl"),
        "unexpected file: {file}"
    );
    assert_eq!(result["line"], 2);
}
