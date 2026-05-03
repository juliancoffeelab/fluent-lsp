use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};
use std::convert::TryFrom;
use tempfile::tempdir;

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("workspace")
}

struct LspProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl LspProcess {
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
            self.stdout.read_line(&mut line).unwrap();
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

impl Drop for LspProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn goto_definition_from_translation_resolves_to_origin_fluent_file() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_uri = format!("file://{}", source_path.display());
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = LspProcess::start();

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": format!("file://{}", root.display()),
            "capabilities": {}
        }
    }));

    let initialize = lsp.recv();
    assert_eq!(initialize["id"], 1);
    assert_eq!(
        initialize["result"]["capabilities"]["definitionProvider"],
        Value::Bool(true)
    );
    assert_eq!(
        initialize["result"]["capabilities"]["hoverProvider"],
        Value::Bool(true)
    );
    assert!(initialize["result"]["capabilities"]
        .get("inlayHintProvider")
        .is_none());
    assert_eq!(
        initialize["result"]["capabilities"]["codeLensProvider"]["resolveProvider"],
        Value::Bool(false)
    );
    assert_eq!(
        initialize["result"]["capabilities"]["executeCommandProvider"]["commands"][0],
        Value::String("fluent-lsp.showSelectorCombinations".to_string())
    );
    assert_eq!(
        initialize["result"]["capabilities"]["referencesProvider"],
        Value::Bool(true)
    );

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }));

    let initialized_log = lsp.recv();
    assert_eq!(initialized_log["method"], "window/logMessage");

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": source_uri,
                "languageId": "fluent",
                "version": 1,
                "text": source_text
            }
        }
    }));

    assert_definition(
        &mut lsp,
        2,
        &source_path,
        position_of(&source_text, "welcome-title"),
        &root.join("locales/en/app.ftl"),
        1,
        0,
    );

    assert_definition(
        &mut lsp,
        3,
        &source_path,
        position_of(&source_text, "brand-name ="),
        &root.join("locales/en/app.ftl"),
        8,
        1,
    );

    assert_definition(
        &mut lsp,
        4,
        &source_path,
        position_of(&source_text, "label = Lanzar"),
        &root.join("locales/en/app.ftl"),
        13,
        5,
    );

    let nested_path = root.join("locales/es/dialogs/menu.ftl");
    let nested_text = std::fs::read_to_string(&nested_path).unwrap();
    let nested_uri = format!("file://{}", nested_path.display());

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": nested_uri,
                "languageId": "fluent",
                "version": 1,
                "text": nested_text
            }
        }
    }));

    assert_definition(
        &mut lsp,
        5,
        &nested_path,
        position_of(&nested_text, "label = Guardar"),
        &root.join("locales/en/dialogs/menu.ftl"),
        4,
        5,
    );
}

fn assert_definition(
    lsp: &mut LspProcess,
    request_id: i64,
    source_path: &Path,
    position: (u32, u32),
    expected_target: &Path,
    expected_line: u32,
    expected_character: u32,
) {
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": position.0, "character": position.1 }
        }
    }));

    let definition = lsp.recv();
    assert_eq!(definition["id"], request_id);
    assert_eq!(
        definition["result"]["uri"],
        Value::String(format!("file://{}", expected_target.display()))
    );
    assert_eq!(
        definition["result"]["range"]["start"]["line"],
        Value::from(expected_line),
    );
    assert_eq!(
        definition["result"]["range"]["start"]["character"],
        Value::from(expected_character),
    );
}

#[test]
fn references_from_origin_resolve_to_translated_fluent_files() {
    let root = fixture_root();
    let source_path = root.join("locales/en/app.ftl");
    let source_uri = format!("file://{}", source_path.display());
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = LspProcess::start();

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 10,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": format!("file://{}", root.display()),
            "capabilities": {}
        }
    }));

    let initialize = lsp.recv();
    assert_eq!(initialize["id"], 10);
    assert_eq!(
        initialize["result"]["capabilities"]["referencesProvider"],
        Value::Bool(true)
    );

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }));
    let initialized_log = lsp.recv();
    assert_eq!(initialized_log["method"], "window/logMessage");

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": source_uri,
                "languageId": "fluent",
                "version": 1,
                "text": source_text
            }
        }
    }));

    assert_references(
        &mut lsp,
        11,
        &source_path,
        position_of(&source_text, "welcome-title"),
        &[
            ReferenceExpectation::new("locales/es/app.ftl", 1, 0),
            ReferenceExpectation::new("locales/fr/app.ftl", 0, 0),
        ],
    );

    assert_references(
        &mut lsp,
        12,
        &source_path,
        position_of(&source_text, "brand-name ="),
        &[
            ReferenceExpectation::new("locales/es/app.ftl", 3, 1),
            ReferenceExpectation::new("locales/fr/app.ftl", 2, 1),
        ],
    );

    assert_references(
        &mut lsp,
        13,
        &source_path,
        position_of(&source_text, "label = Launch"),
        &[
            ReferenceExpectation::new("locales/es/app.ftl", 5, 5),
            ReferenceExpectation::new("locales/fr/app.ftl", 4, 5),
        ],
    );

    let nested_path = root.join("locales/en/dialogs/menu.ftl");
    let nested_text = std::fs::read_to_string(&nested_path).unwrap();
    let nested_uri = format!("file://{}", nested_path.display());

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": nested_uri,
                "languageId": "fluent",
                "version": 1,
                "text": nested_text
            }
        }
    }));

    assert_references(
        &mut lsp,
        14,
        &nested_path,
        position_of(&nested_text, "label = Save"),
        &[
            ReferenceExpectation::new("locales/es/dialogs/menu.ftl", 1, 5),
            ReferenceExpectation::new("locales/fr/dialogs/menu.ftl", 1, 5),
            ReferenceExpectation::new("locales/lv/dialogs/menu.ftl", 1, 5),
        ],
    );
}

#[test]
fn hover_from_translation_shows_local_formatted_messages() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_uri = format!("file://{}", source_path.display());
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = LspProcess::start();

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 20,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": format!("file://{}", root.display()),
            "capabilities": {}
        }
    }));

    let initialize = lsp.recv();
    assert_eq!(initialize["id"], 20);
    assert_eq!(
        initialize["result"]["capabilities"]["hoverProvider"],
        Value::Bool(true)
    );

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }));
    let initialized_log = lsp.recv();
    assert_eq!(initialized_log["method"], "window/logMessage");

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": source_uri,
                "languageId": "fluent",
                "version": 1,
                "text": source_text
            }
        }
    }));

    assert_hover(
        &mut lsp,
        21,
        &source_path,
        position_of(&source_text, "welcome-title"),
        "```ftl\nBienvenido\n```",
        1,
        0,
    );

    assert_hover(
        &mut lsp,
        22,
        &source_path,
        position_of(&source_text, "Abre la build mas reciente de"),
        "```ftl\nOpen the latest { -brand-name } build and pick up where you left off.\n```\n\n---\n\n```ftl\nAbre la build mas reciente de { -brand-name } y sigue donde lo dejaste.\n```",
        2,
        0,
    );

    assert_hover(
        &mut lsp,
        23,
        &source_path,
        position_of(&source_text, "[female] ella"),
        "`$gender=female`, `$count=*`\n\n```ftl\nCopy the download link for her account on { $count } devices now.\n```\n\n---\n\n`$gender=female`, `$count=*`\n\n```ftl\nCopia el enlace de descarga para la cuenta de ella en { $count } dispositivos ahora.\n```",
        8,
        0,
    );

    assert_hover(
        &mut lsp,
        24,
        &source_path,
        position_of(&source_text, "Instala la build recomendada para la cuenta de"),
        "`$gender=*`, `$count=*`\n\n```ftl\nInstall the recommended build for their account on { $count } devices now.\n```\n\n---\n\n`$gender=*`, `$count=*`\n\n```ftl\nInstala la build recomendada para la cuenta de elle en { $count } dispositivos ahora.\n```",
        21,
        5,
    );

    assert_hover(
        &mut lsp,
        25,
        &source_path,
        position_of(&source_text, "en { $count } { $count ->"),
        "`$gender=other`, `$count=*`\n\n```ftl\nCopy the download link for their account on { $count } devices now.\n```\n\n---\n\n`$gender=other`, `$count=*`\n\n```ftl\nCopia el enlace de descarga para la cuenta de elle en { $count } dispositivos ahora.\n```",
        8,
        0,
    );

    assert_hover(
        &mut lsp,
        26,
        &source_path,
        position_of(&source_text, "[one] dispositivo"),
        "`$gender=other`, `$count=one`\n\n```ftl\nCopy the download link for their account on { $count } device now.\n```\n\n---\n\n`$gender=other`, `$count=one`\n\n```ftl\nCopia el enlace de descarga para la cuenta de elle en { $count } dispositivo ahora.\n```",
        8,
        0,
    );
}

#[test]
fn hover_from_translation_matches_available_selector_variables_across_source_and_local() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_uri = format!("file://{}", source_path.display());
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = LspProcess::start();

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 27,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": format!("file://{}", root.display()),
            "capabilities": {}
        }
    }));

    let initialize = lsp.recv();
    assert_eq!(initialize["id"], 27);
    assert_eq!(
        initialize["result"]["capabilities"]["hoverProvider"],
        Value::Bool(true)
    );

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }));
    let initialized_log = lsp.recv();
    assert_eq!(initialized_log["method"], "window/logMessage");

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": source_uri,
                "languageId": "fluent",
                "version": 1,
                "text": source_text
            }
        }
    }));

    assert_hover(
        &mut lsp,
        28,
        &source_path,
        position_of(&source_text, "[female] ella misma"),
        "`$platform=*`, `$count=*`\n\n```ftl\nSummary for mobile users with { $count } packages ready.\n```\n\n---\n\n`$gender=female`, `$count=*`\n\n```ftl\nResumen para ella misma con { $count } paquetes listo.\n```",
        50,
        0,
    );

    assert_hover(
        &mut lsp,
        29,
        &source_path,
        position_of(&source_text, "[0] ningun paquete"),
        "`$platform=*`, `$count=0`\n\n```ftl\nSummary for mobile users with no packages ready.\n```\n\n---\n\n`$gender=other`, `$count=0`\n\n```ftl\nResumen para elle misme con ningun paquete listo.\n```",
        50,
        0,
    );

    assert_hover(
        &mut lsp,
        30,
        &source_path,
        position_of(&source_text, "[1] un paquete"),
        "`$platform=*`, `$count=1`\n\n```ftl\nSummary for mobile users with one package ready.\n```\n\n---\n\n`$gender=other`, `$count=1`\n\n```ftl\nResumen para elle misme con un paquete listo.\n```",
        50,
        0,
    );
}

#[test]
fn hover_from_latvian_translation_preserves_zero_category_selectors() {
    let root = fixture_root();
    let source_path = root.join("locales/lv/app.ftl");
    let source_uri = format!("file://{}", source_path.display());
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = LspProcess::start();

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 31,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": format!("file://{}", root.display()),
            "capabilities": {}
        }
    }));

    let initialize = lsp.recv();
    assert_eq!(initialize["id"], 31);
    assert_eq!(
        initialize["result"]["capabilities"]["hoverProvider"],
        Value::Bool(true)
    );

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }));
    let initialized_log = lsp.recv();
    assert_eq!(initialized_log["method"], "window/logMessage");

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": source_uri,
                "languageId": "fluent",
                "version": 1,
                "text": source_text
            }
        }
    }));

    assert_hover(
        &mut lsp,
        32,
        &source_path,
        position_of(&source_text, "[zero] neviena pakotne nav gatava"),
        "`$count=zero`\n\n```ftl\nZero summary: no packages ready.\n```\n\n---\n\n`$count=zero`\n\n```ftl\nKopsavilkums ar neviena pakotne nav gatava.\n```",
        0,
        0,
    );

    assert_hover(
        &mut lsp,
        33,
        &source_path,
        position_of(&source_text, "[one] viena pakotne ir gatava"),
        "`$count=one`\n\n```ftl\nZero summary: one package ready.\n```\n\n---\n\n`$count=one`\n\n```ftl\nKopsavilkums ar viena pakotne ir gatava.\n```",
        0,
        0,
    );
}

#[test]
fn hover_from_origin_file_shows_formatted_attribute_text() {
    let root = fixture_root();
    let source_path = root.join("locales/en/dialogs/menu.ftl");
    let source_uri = format!("file://{}", source_path.display());
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = LspProcess::start();

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 30,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": format!("file://{}", root.display()),
            "capabilities": {}
        }
    }));

    let initialize = lsp.recv();
    assert_eq!(initialize["id"], 30);

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }));
    let initialized_log = lsp.recv();
    assert_eq!(initialized_log["method"], "window/logMessage");

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": source_uri,
                "languageId": "fluent",
                "version": 1,
                "text": source_text
            }
        }
    }));

    assert_hover(
        &mut lsp,
        31,
        &source_path,
        position_of(&source_text, "label = Save"),
        "```ftl\nSave\n```",
        4,
        5,
    );
}

#[test]
fn code_lens_opens_full_selector_combinations_document() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_uri = format!("file://{}", source_path.display());
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = LspProcess::start();

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 40,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": format!("file://{}", root.display()),
            "capabilities": {
                "window": {
                    "showDocument": {
                        "support": true
                    }
                }
            }
        }
    }));

    let initialize = lsp.recv();
    assert_eq!(initialize["id"], 40);
    assert_eq!(
        initialize["result"]["capabilities"]["codeLensProvider"]["resolveProvider"],
        Value::Bool(false)
    );
    assert_eq!(
        initialize["result"]["capabilities"]["executeCommandProvider"]["commands"][0],
        Value::String("fluent-lsp.showSelectorCombinations".to_string())
    );

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }));
    let initialized_log = lsp.recv();
    assert_eq!(initialized_log["method"], "window/logMessage");

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": source_uri,
                "languageId": "fluent",
                "version": 1,
                "text": source_text
            }
        }
    }));

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 41,
        "method": "textDocument/codeLens",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) }
        }
    }));

    let lenses = lsp.recv();
    assert_eq!(lenses["id"], 41);
    let items = lenses["result"]
        .as_array()
        .expect("expected code lens array");
    assert!(items.len() >= 3);
    let install_hint_lens = items
        .iter()
        .find(|item| item["range"]["start"]["line"].as_u64() == Some(8))
        .expect("missing install-hint codelens");
    assert_eq!(
        install_hint_lens["command"]["title"],
        Value::String("Show all 6 selector combinations".to_string())
    );
    assert_eq!(
        install_hint_lens["command"]["command"],
        Value::String("fluent-lsp.showSelectorCombinations".to_string())
    );
    assert_eq!(
        install_hint_lens["range"]["start"]["character"],
        Value::from(0)
    );

    let command = install_hint_lens["command"].clone();
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 42,
        "method": "workspace/executeCommand",
        "params": {
            "command": command["command"],
            "arguments": command["arguments"]
        }
    }));

    let request = lsp.recv();
    assert_eq!(request["method"], "window/showDocument");
    assert_eq!(request["params"]["external"], Value::Bool(false));
    assert_eq!(request["params"]["takeFocus"], Value::Bool(true));
    assert_eq!(
        request["params"]["selection"]["start"]["line"],
        Value::from(0)
    );
    assert_eq!(
        request["params"]["selection"]["start"]["character"],
        Value::from(0)
    );

    let document_uri = request["params"]["uri"]
        .as_str()
        .expect("showDocument uri must be a string");
    assert!(
        document_uri.contains("fluent-lsp-selector-combinations-")
            && document_uri.ends_with("-install-hint.md"),
        "unexpected temp document uri: {document_uri}"
    );
    let document_path = document_uri
        .strip_prefix("file://")
        .expect("expected file uri for temp document");
    let document_text = std::fs::read_to_string(document_path).expect("read temp document");
    assert!(document_text.contains("Current language: `es`"));
    assert!(document_text.contains("Source language: `en`"));
    assert!(document_text.contains("Source text:"));
    assert!(document_text.contains("Current text:"));
    assert!(document_text.contains("Source language combinations:"));
    assert!(document_text.contains("Current language combinations:"));
    assert!(document_text.contains("Copy the download link for their account"));
    assert!(document_text.contains("Copia el enlace de descarga para la cuenta de elle"));
    assert!(document_text.contains("`$gender=other`, `$count=other`\n```ftl\nCopy the download link for their account on { $count } devices now.\n```"));
    assert!(document_text.contains("`$gender=other`, `$count=other`\n```ftl\nCopia el enlace de descarga para la cuenta de elle en { $count } dispositivos ahora.\n```"));

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": request["id"],
        "result": {
            "success": true
        }
    }));

    let response = lsp.recv();
    assert_eq!(response["id"], 42);
    assert_eq!(response["result"], Value::Null);
}

#[test]
fn code_lens_opens_full_selector_combinations_document_for_attribute() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_uri = format!("file://{}", source_path.display());
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = LspProcess::start();

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 50,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": format!("file://{}", root.display()),
            "capabilities": {
                "window": {
                    "showDocument": {
                        "support": true
                    }
                }
            }
        }
    }));

    let initialize = lsp.recv();
    assert_eq!(initialize["id"], 50);

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }));
    let initialized_log = lsp.recv();
    assert_eq!(initialized_log["method"], "window/logMessage");

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": source_uri,
                "languageId": "fluent",
                "version": 1,
                "text": source_text
            }
        }
    }));

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 51,
        "method": "textDocument/codeLens",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) }
        }
    }));

    let lenses = lsp.recv();
    assert_eq!(lenses["id"], 51);
    let items = lenses["result"]
        .as_array()
        .expect("expected code lens array");
    let attribute_lens = items
        .iter()
        .find(|item| item["range"]["start"]["line"].as_u64() == Some(21))
        .expect("missing download-action.tooltip codelens");

    let command = attribute_lens["command"].clone();
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 52,
        "method": "workspace/executeCommand",
        "params": {
            "command": command["command"],
            "arguments": command["arguments"]
        }
    }));

    let request = lsp.recv();
    assert_eq!(request["method"], "window/showDocument");
    let document_uri = request["params"]["uri"]
        .as_str()
        .expect("showDocument uri must be a string");
    assert!(
        document_uri.ends_with("-download-action_tooltip.md"),
        "unexpected temp document uri: {document_uri}"
    );
    let document_path = document_uri
        .strip_prefix("file://")
        .expect("expected file uri for temp document");
    let document_text = std::fs::read_to_string(document_path).expect("read temp document");
    assert!(document_text.contains("Current language: `es`"));
    assert!(document_text.contains("Source language: `en`"));
    assert!(document_text.contains("Source language combinations:"));
    assert!(document_text.contains("Current language combinations:"));
    assert!(document_text.contains("Install the recommended build for their account"));
    assert!(document_text.contains("Instala la build recomendada para la cuenta de elle"));
    assert!(document_text.contains("`$gender=other`, `$count=other`\n```ftl\nInstall the recommended build for their account on { $count } devices now.\n```"));
    assert!(document_text.contains("`$gender=other`, `$count=other`\n```ftl\nInstala la build recomendada para la cuenta de elle en { $count } dispositivos ahora.\n```"));

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": request["id"],
        "result": {
            "success": true
        }
    }));

    let response = lsp.recv();
    assert_eq!(response["id"], 52);
    assert_eq!(response["result"], Value::Null);
}

#[test]
fn code_lens_origin_document_omits_duplicate_source_sections() {
    let root = fixture_root();
    let source_path = root.join("locales/en/app.ftl");
    let source_uri = format!("file://{}", source_path.display());
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = LspProcess::start();

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 55,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": format!("file://{}", root.display()),
            "capabilities": {
                "window": {
                    "showDocument": {
                        "support": true
                    }
                }
            }
        }
    }));

    let initialize = lsp.recv();
    assert_eq!(initialize["id"], 55);

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }));
    let initialized_log = lsp.recv();
    assert_eq!(initialized_log["method"], "window/logMessage");

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": source_uri,
                "languageId": "fluent",
                "version": 1,
                "text": source_text
            }
        }
    }));

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 56,
        "method": "textDocument/codeLens",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) }
        }
    }));

    let lenses = lsp.recv();
    assert_eq!(lenses["id"], 56);
    let items = lenses["result"]
        .as_array()
        .expect("expected code lens array");
    let install_hint_lens = items
        .iter()
        .find(|item| item["command"]["arguments"][1] == Value::String("install-hint".to_string()))
        .expect("missing install-hint codelens");

    let command = install_hint_lens["command"].clone();
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 57,
        "method": "workspace/executeCommand",
        "params": {
            "command": command["command"],
            "arguments": command["arguments"]
        }
    }));

    let request = lsp.recv();
    assert_eq!(request["method"], "window/showDocument");
    let document_uri = request["params"]["uri"]
        .as_str()
        .expect("showDocument uri must be a string");
    let document_path = document_uri
        .strip_prefix("file://")
        .expect("expected file uri for temp document");
    let document_text = std::fs::read_to_string(document_path).expect("read temp document");
    assert!(document_text.contains("Current language: `en`"));
    assert!(!document_text.contains("Source language:"));
    assert!(!document_text.contains("Source text:"));
    assert!(!document_text.contains("Source language combinations:"));
    assert_eq!(document_text.matches("Current language combinations:").count(), 1);

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": request["id"],
        "result": {
            "success": true
        }
    }));

    let response = lsp.recv();
    assert_eq!(response["id"], 57);
    assert_eq!(response["result"], Value::Null);
}

#[test]
fn code_lens_falls_back_to_show_message_with_fixed_selector_limit() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_uri = format!("file://{}", source_path.display());
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = LspProcess::start();

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 60,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": format!("file://{}", root.display()),
            "capabilities": {}
        }
    }));

    let initialize = lsp.recv();
    assert_eq!(initialize["id"], 60);

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }));
    let initialized_log = lsp.recv();
    assert_eq!(initialized_log["method"], "window/logMessage");

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": source_uri,
                "languageId": "fluent",
                "version": 1,
                "text": source_text
            }
        }
    }));

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 61,
        "method": "textDocument/codeLens",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) }
        }
    }));

    let lenses = lsp.recv();
    assert_eq!(lenses["id"], 61);
    let items = lenses["result"]
        .as_array()
        .expect("expected code lens array");
    let rollout_lens = items
        .iter()
        .find(|item| {
            item["command"]["arguments"][1] == Value::String("audience-rollout".to_string())
        })
        .expect("missing audience-rollout codelens");
    assert_eq!(
        rollout_lens["command"]["title"],
        Value::String("Show all 12 selector combinations".to_string())
    );

    let command = rollout_lens["command"].clone();
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 62,
        "method": "workspace/executeCommand",
        "params": {
            "command": command["command"],
            "arguments": command["arguments"]
        }
    }));

    let first = lsp.recv();
    let second = lsp.recv();
    let (response, notification) = if first["id"] == Value::from(62) {
        (first, second)
    } else {
        (second, first)
    };

    assert_eq!(response["id"], 62);
    assert_eq!(response["result"], Value::Null);

    assert_eq!(notification["method"], "window/showMessage");
    let message = notification["params"]["message"]
        .as_str()
        .expect("showMessage payload must be a string");
    assert!(message.contains("Selector combinations for audience-rollout"));
    assert!(message.contains("Current language combinations:"));
    assert_eq!(message.matches("\n```ftl\n").count(), 10);
    assert!(message.contains("`...`\n2 more"));
    assert!(!message.contains("```ftl\nResumen para otras personas en movil con { $count } elementos.\n```"));
}

#[test]
fn code_action_generates_prefix_selector_by_default() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 70);
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions(
        &mut lsp,
        71,
        &source_path,
        position_of(&source_text, "coins-line"),
    );
    let action = &actions[0];
    assert_eq!(
        action["title"],
        Value::String("Generate number selector (prefix)".to_string())
    );
    assert_eq!(
        action["kind"],
        Value::String("refactor.rewrite".to_string())
    );
    assert_eq!(
        action["edit"]["changes"][format!("file://{}", source_path.display())][0]["newText"],
        Value::String(
            "Tienes { $coins } { $coins ->\n    [one] monedas.\n    *[other] monedas.\n}"
                .to_string()
        )
    );
}

#[test]
fn code_action_uses_client_selector_style_setting() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 72);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "workspace/didChangeConfiguration",
        "params": {
            "settings": {
                "fluent-lsp": {
                    "selector_style": "whole"
                }
            }
        }
    }));
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions(
        &mut lsp,
        73,
        &source_path,
        position_of(&source_text, "{ $coins }"),
    );
    let action = &actions[0];
    assert_eq!(
        action["title"],
        Value::String("Generate number selector from $coins (whole)".to_string())
    );
    assert_eq!(
        action["edit"]["changes"][format!("file://{}", source_path.display())][0]["newText"],
        Value::String(
            "{ $coins ->\n    [one] Tienes { $coins } monedas.\n    *[other] Tienes { $coins } monedas.\n}"
                .to_string()
        )
    );
}

#[test]
fn file_config_selector_style_overrides_client_setting() {
    let fixture = fixture_root();
    let temp = tempdir().unwrap();
    copy_dir(&fixture, temp.path());
    std::fs::write(
        temp.path().join("fluent-lsp.toml"),
        "origin_language = \"en\"\nfile_masks = [\"locales/{lang}/{filepath}.ftl\"]\nselector_style = \"whole\"\n",
    )
    .unwrap();

    let source_path = temp.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(temp.path(), 74);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "workspace/didChangeConfiguration",
        "params": {
            "settings": {
                "fluent-lsp": {
                    "selector_style": "prefix"
                }
            }
        }
    }));
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions(
        &mut lsp,
        75,
        &source_path,
        position_of(&source_text, "coins-line"),
    );
    let action = &actions[0];
    assert_eq!(
        action["title"],
        Value::String("Generate number selector (whole)".to_string())
    );
}

#[test]
fn code_action_uses_snippet_text_edit_when_supported() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp_with_capabilities(
        &root,
        76,
        json!({
            "workspace": {
                "workspaceEdit": {
                    "documentChanges": true,
                    "snippetEditSupport": true
                }
            }
        }),
    );
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions(
        &mut lsp,
        77,
        &source_path,
        position_of(&source_text, "coins-line"),
    );
    let action = &actions[0];
    assert_eq!(
        action["edit"]["documentChanges"][0]["edits"][0]["snippet"],
        Value::String(
            "Tienes { $coins } { $coins ->\n    [one] monedas.\n    *[other] monedas.\n}"
                .to_string()
        )
    );
    assert!(action["edit"]["changes"].is_null());
}

struct ReferenceExpectation<'a> {
    relative_path: &'a str,
    line: u32,
    character: u32,
}

impl<'a> ReferenceExpectation<'a> {
    fn new(relative_path: &'a str, line: u32, character: u32) -> Self {
        Self {
            relative_path,
            line,
            character,
        }
    }
}

fn initialized_lsp(root: &Path, request_id: i64) -> LspProcess {
    initialized_lsp_with_capabilities(root, request_id, json!({}))
}

fn initialized_lsp_with_capabilities(
    root: &Path,
    request_id: i64,
    capabilities: Value,
) -> LspProcess {
    let mut lsp = LspProcess::start();

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": format!("file://{}", root.display()),
            "capabilities": capabilities
        }
    }));

    let initialize = lsp.recv();
    assert_eq!(initialize["id"], request_id);
    assert_eq!(
        initialize["result"]["capabilities"]["codeActionProvider"]["codeActionKinds"][0],
        Value::String("refactor.rewrite".to_string())
    );

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }));
    let initialized_log = lsp.recv();
    assert_eq!(initialized_log["method"], "window/logMessage");

    lsp
}

fn open_document(lsp: &mut LspProcess, path: &Path, text: &str) {
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

fn request_code_actions(
    lsp: &mut LspProcess,
    request_id: i64,
    source_path: &Path,
    position: (u32, u32),
) -> Vec<Value> {
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "textDocument/codeAction",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "range": {
                "start": { "line": position.0, "character": position.1 },
                "end": { "line": position.0, "character": position.1 }
            },
            "context": {
                "diagnostics": []
            }
        }
    }));

    let response = lsp.recv();
    assert_eq!(response["id"], request_id);
    response["result"]
        .as_array()
        .expect("expected code action array")
        .clone()
}

fn copy_dir(source: &Path, dest: &Path) {
    std::fs::create_dir_all(dest).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let source_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if source_path.is_dir() {
            copy_dir(&source_path, &dest_path);
        } else {
            std::fs::copy(&source_path, &dest_path).unwrap();
        }
    }
}

fn assert_references(
    lsp: &mut LspProcess,
    request_id: i64,
    source_path: &Path,
    position: (u32, u32),
    expected: &[ReferenceExpectation<'_>],
) {
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": position.0, "character": position.1 },
            "context": { "includeDeclaration": false }
        }
    }));

    let references = lsp.recv();
    assert_eq!(references["id"], request_id);
    let items = references["result"]
        .as_array()
        .expect("expected references array");
    assert_eq!(items.len(), expected.len());

    for (item, expected) in items.iter().zip(expected.iter()) {
        let uri = item["uri"].as_str().unwrap();
        assert!(
            uri.ends_with(expected.relative_path),
            "unexpected reference uri: {uri}"
        );
        assert_eq!(item["range"]["start"]["line"], Value::from(expected.line));
        assert_eq!(
            item["range"]["start"]["character"],
            Value::from(expected.character)
        );
    }
}

fn assert_hover(
    lsp: &mut LspProcess,
    request_id: i64,
    source_path: &Path,
    position: (u32, u32),
    expected_value: &str,
    expected_line: u32,
    expected_character: u32,
) {
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": position.0, "character": position.1 }
        }
    }));

    let hover = lsp.recv();
    assert_eq!(hover["id"], request_id);
    assert_eq!(
        hover["result"]["contents"]["kind"],
        Value::String("markdown".to_string())
    );
    assert_eq!(
        hover["result"]["contents"]["value"],
        Value::String(expected_value.to_string())
    );
    assert_eq!(
        hover["result"]["range"]["start"]["line"],
        Value::from(expected_line),
    );
    assert_eq!(
        hover["result"]["range"]["start"]["character"],
        Value::from(expected_character),
    );
}

fn position_of(source: &str, needle: &str) -> (u32, u32) {
    let offset = source.find(needle).expect("needle not found in source");
    let prefix = &source[..offset];
    let line = u32::try_from(prefix.bytes().filter(|byte| *byte == b'\n').count()).unwrap();
    let line_start = prefix.rfind('\n').map(|idx| idx + 1).unwrap_or(0);
    let character = u32::try_from(source[line_start..offset].chars().count()).unwrap();
    (line, character)
}
