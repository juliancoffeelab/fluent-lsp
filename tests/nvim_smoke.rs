use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use fluent_lsp::render_fluent_preview_text;
use fluent_syntax::parser;
use serde_json::{Value, json};
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

struct RawLspProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl RawLspProcess {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_fluent-lsp"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("failed to start fluent-lsp");

        Self {
            stdin: child.stdin.take().expect("missing stdin"),
            stdout: BufReader::new(child.stdout.take().expect("missing stdout")),
            child,
        }
    }

    fn send(&mut self, message: &Value) {
        let body = serde_json::to_vec(message).expect("serialize LSP message");
        write!(self.stdin, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
        self.stdin.write_all(&body).unwrap();
        self.stdin.flush().unwrap();
    }

    fn recv(&mut self) -> Value {
        let mut content_length = None;
        loop {
            let mut line = String::new();
            let bytes_read = self.stdout.read_line(&mut line).unwrap();
            if bytes_read == 0 {
                let status = self.child.try_wait().unwrap();
                panic!("unexpected EOF from fluent-lsp; child status: {status:?}");
            }
            if line == "\r\n" {
                break;
            }
            if let Some(length) = line.strip_prefix("Content-Length: ") {
                content_length = Some(length.trim().parse::<usize>().unwrap());
            }
        }

        let mut body = vec![0; content_length.expect("missing content length")];
        self.stdout.read_exact(&mut body).unwrap();
        serde_json::from_slice(&body).expect("parse LSP response")
    }
}

impl Drop for RawLspProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
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

fn completion_labels_for_source(
    scenario: &str,
    relative_path: &str,
    source_text: &str,
    position: (u32, u32),
) -> Vec<String> {
    let workspace = scenario_dir(scenario).join("workspace");
    let path = workspace.join(relative_path);
    let mut lsp = RawLspProcess::start();

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": format!("file://{}", workspace.display()),
            "capabilities": {}
        }
    }));
    let initialize = lsp.recv();
    assert_eq!(initialize["id"], 1);

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }));
    let _ = recv_notification(&mut lsp, "window/logMessage");

    send_open_document(&mut lsp, &path, source_text);

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": format!("file://{}", path.display()) },
            "position": { "line": position.0, "character": position.1 }
        }
    }));
    let response = recv_response(&mut lsp, 2);
    let items = response["result"].as_array().cloned().unwrap_or_default();
    items
        .into_iter()
        .filter_map(|item| item["label"].as_str().map(ToString::to_string))
        .collect()
}

fn send_open_document(lsp: &mut RawLspProcess, path: &Path, text: &str) {
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": format!("file://{}", path.display()),
                "languageId": "fluent",
                "version": 1,
                "text": text
            }
        }
    }));
}

fn recv_notification(lsp: &mut RawLspProcess, method: &str) -> Value {
    loop {
        let message = lsp.recv();
        if message["method"] == Value::String(method.to_string()) {
            return message;
        }
    }
}

fn recv_response(lsp: &mut RawLspProcess, request_id: i64) -> Value {
    loop {
        let message = lsp.recv();
        if message["id"] == Value::from(request_id) {
            return message;
        }
    }
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
fn nvim_smoke_completion_origin_keys() {
    let result = run_scenario("completion_origin_keys");
    assert_eq!(result["ok"], Value::Bool(true));

    let app_origin = scenario_source("completion_origin_keys", "locales/en/app.ftl");
    let menu_origin = scenario_source("completion_origin_keys", "locales/en/dialogs/menu.ftl");
    let top_level_source = result["top_level_source"].as_str().unwrap();
    let bare_dot_source = result["bare_dot_source"].as_str().unwrap();
    let attribute_prefix_source = result["attribute_prefix_source"].as_str().unwrap();
    let empty = HashMap::new();

    let top_level_labels = completion_labels_for_source(
        "completion_origin_keys",
        "locales/es/app.ftl",
        top_level_source,
        position_after(top_level_source, "down"),
    );
    assert_eq!(
        top_level_labels,
        vec!["download-action".to_string(), "download-count".to_string()]
    );
    for label in ["download-action", "download-count"] {
        assert!(
            render_fluent_preview_text(&app_origin, label, Some(&empty)).is_some(),
            "missing origin entry for {label}"
        );
    }

    let bare_dot_labels = completion_labels_for_source(
        "completion_origin_keys",
        "locales/es/dialogs/menu.ftl",
        bare_dot_source,
        position_after(bare_dot_source, "."),
    );
    assert_eq!(
        bare_dot_labels,
        vec![".label".to_string(), ".tooltip".to_string()]
    );
    let attribute_prefix_labels = completion_labels_for_source(
        "completion_origin_keys",
        "locales/es/dialogs/menu.ftl",
        attribute_prefix_source,
        position_after(attribute_prefix_source, ".l"),
    );
    assert_eq!(attribute_prefix_labels, vec![".label".to_string()]);
    for attribute in ["menu-save.label", "menu-save.tooltip"] {
        assert!(
            render_fluent_preview_text(&menu_origin, attribute, Some(&empty)).is_some(),
            "missing origin attribute for {attribute}"
        );
    }
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
fn nvim_smoke_code_action_fill_missing_keys() {
    let result = run_scenario("code_action_fill_missing_keys");
    assert_eq!(result["ok"], Value::Bool(true));

    let final_buffer = result["final_buffer"].as_str().unwrap();
    assert_fluent_parses(final_buffer);
    assert_eq!(
        final_buffer,
        "hello = Hola Mundo\nmenu-save =\n    .label = Guardar\n    .tooltip = { \"\" }\n\nsync-status = { \"\" }\n\n"
    );
    assert_eq!(
        render_fluent_preview_text(final_buffer, "hello", None).as_deref(),
        Some("Hola Mundo")
    );
}

#[test]
fn nvim_smoke_code_action_copy_missing_keys() {
    let result = run_scenario("code_action_copy_missing_keys");
    assert_eq!(result["ok"], Value::Bool(true));

    let final_buffer = result["final_buffer"].as_str().unwrap();
    let origin = scenario_source("code_action_copy_missing_keys", "locales/en/app.ftl");
    assert_fluent_parses(final_buffer);
    assert_eq!(
        final_buffer,
        "hello = Hola Mundo\nmenu-save =\n    .label = Guardar\n    # [LSP-COPY]\n    .tooltip = Save this file\n\n# [LSP-COPY]\nsync-status = Sync ready\n\n"
    );
    assert_eq!(final_buffer.matches("# [LSP-COPY]").count(), 2);
    assert_eq!(
        render_fluent_preview_text(final_buffer, "hello", None).as_deref(),
        Some("Hola Mundo")
    );
    assert_eq!(
        render_fluent_preview_text(final_buffer, "menu-save.tooltip", None).as_deref(),
        render_fluent_preview_text(&origin, "menu-save.tooltip", None).as_deref()
    );
    assert_eq!(
        render_fluent_preview_text(final_buffer, "sync-status", None).as_deref(),
        render_fluent_preview_text(&origin, "sync-status", None).as_deref()
    );
}

#[test]
fn nvim_smoke_diagnostics_numeric_selectors() {
    let result = run_scenario("diagnostics_numeric_selectors");
    assert_eq!(result["ok"], Value::Bool(true));
}

#[test]
fn nvim_smoke_diagnostics_lsp_copy_markers() {
    let result = run_scenario("diagnostics_lsp_copy_markers");
    assert_eq!(result["ok"], Value::Bool(true));

    let initial_messages = result["initial_messages"].as_array().unwrap();
    assert_eq!(initial_messages.len(), 2);
    assert!(initial_messages.iter().all(|message| {
        message == "Entry still contains an `# [LSP-COPY]` marker"
    }));

    let final_buffer = result["final_buffer"].as_str().unwrap();
    assert_fluent_parses(final_buffer);
    assert_eq!(final_buffer.matches("# [LSP-COPY]").count(), 0);
    assert_eq!(
        render_fluent_preview_text(final_buffer, "download-action.tooltip", None).as_deref(),
        Some("Download this build")
    );
}

#[test]
fn nvim_smoke_diagnostics_parse_errors() {
    let result = run_scenario("diagnostics_parse_errors");
    assert_eq!(result["ok"], Value::Bool(true));
    assert_fluent_parses(result["final_buffer"].as_str().unwrap());
}

fn position_of(source: &str, needle: &str) -> (u32, u32) {
    let offset = source.find(needle).expect("needle not found");
    let prefix = &source[..offset];
    let line = u32::try_from(prefix.bytes().filter(|byte| *byte == b'\n').count()).unwrap();
    let line_start = prefix.rfind('\n').map(|idx| idx + 1).unwrap_or(0);
    let character = u32::try_from(source[line_start..offset].chars().count()).unwrap();
    (line, character)
}

fn position_after(source: &str, needle: &str) -> (u32, u32) {
    let (line, character) = position_of(source, needle);
    (
        line,
        character + u32::try_from(needle.chars().count()).unwrap(),
    )
}
