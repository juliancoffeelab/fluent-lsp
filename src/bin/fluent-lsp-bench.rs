use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tempfile::TempDir;

struct LspProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl LspProcess {
    fn start(server: &Path) -> Self {
        let mut child = Command::new(server)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|error| panic!("failed to start {}: {error}", server.display()));
        Self {
            stdin: child.stdin.take().unwrap(),
            stdout: BufReader::new(child.stdout.take().unwrap()),
            child,
        }
    }

    fn send(&mut self, message: &Value) {
        let body = serde_json::to_vec(message).unwrap();
        write!(self.stdin, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
        self.stdin.write_all(&body).unwrap();
        self.stdin.flush().unwrap();
    }

    fn recv(&mut self) -> Value {
        let mut content_length = None;
        loop {
            let mut line = String::new();
            let read = self.stdout.read_line(&mut line).unwrap();
            if read == 0 {
                panic!("unexpected EOF from fluent-lsp");
            }
            if line == "\r\n" {
                break;
            }
            if let Some(length) = line.strip_prefix("Content-Length: ") {
                content_length = Some(length.trim().parse::<usize>().unwrap());
            }
        }
        let mut body = vec![0; content_length.unwrap()];
        self.stdout.read_exact(&mut body).unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    fn recv_response(&mut self, id: i64) -> Value {
        loop {
            let message = self.recv();
            if message["id"] == Value::from(id) {
                return message;
            }
        }
    }

    fn recv_until_index_ready(&mut self) {
        loop {
            let message = self.recv();
            if message["method"] == "$/progress" && message["params"]["value"]["kind"] == "end" {
                return;
            }
        }
    }

    fn rss_kb(&self) -> Option<u64> {
        let status = std::fs::read_to_string(format!("/proc/{}/status", self.child.id())).ok()?;
        status.lines().find_map(|line| {
            let value = line.strip_prefix("VmRSS:")?.trim();
            value.split_whitespace().next()?.parse().ok()
        })
    }
}

impl Drop for LspProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Clone, Copy)]
struct Shape {
    languages: usize,
    files_per_language: usize,
    messages_per_file: usize,
}

fn main() {
    let mut args = std::env::args().skip(1);
    let output = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("benchmarks/results/latest.json"));
    let server = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(default_server_path);

    let small = Shape {
        languages: 4,
        files_per_language: 3,
        messages_per_file: 40,
    };
    let synthetic = Shape {
        languages: 31,
        files_per_language: 50,
        messages_per_file: std::env::var("FLUENT_LSP_BENCH_MESSAGES")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(2000),
    };

    let started = Instant::now();
    let results = json!({
        "schema": 1,
        "server": server,
        "generated_at_unix_ms": unix_ms(),
        "flavors": [
            run_flavor("small-realistic", small, &server),
            run_flavor("synthetic-large", synthetic, &server),
        ],
        "total_wall_ms": started.elapsed().as_millis(),
    });
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&output, serde_json::to_string_pretty(&results).unwrap()).unwrap();
    println!("{}", serde_json::to_string_pretty(&results).unwrap());
}

fn default_server_path() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    exe.parent().unwrap().join("fluent-lsp")
}

fn run_flavor(name: &str, shape: Shape, server: &Path) -> Value {
    let workspace = generate_workspace(shape);
    let file_count = shape.languages * shape.files_per_language;
    let message_count = shape.languages * shape.files_per_language * shape.messages_per_file;
    let mut lsp = LspProcess::start(server);
    let startup_start = Instant::now();
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": format!("file://{}", workspace.path().display()),
            "capabilities": {"window": {"workDoneProgress": true}}
        }
    }));
    lsp.recv_response(1);
    lsp.send(&json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}));
    lsp.recv_until_index_ready();
    let startup_ms = startup_start.elapsed().as_secs_f64() * 1000.0;
    let steady_rss_kb = lsp.rss_kb();

    let origin = workspace.path().join("locales/lang00/file000.ftl");
    let translation = workspace.path().join("locales/lang01/file000.ftl");
    let mut groups: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
    for iteration in 0..20 {
        measure(&mut groups, "definition", || {
            request_definition(&mut lsp, &translation, iteration);
        });
        measure(&mut groups, "references", || {
            request_references(&mut lsp, &origin, iteration);
        });
        measure(&mut groups, "hover", || {
            request_hover(&mut lsp, &translation, iteration);
        });
        measure(&mut groups, "completion", || {
            request_completion(&mut lsp, &translation, iteration);
        });
        measure(&mut groups, "codeAction.allAtCursor", || {
            request_code_action(&mut lsp, &translation, iteration);
        });
    }

    json!({
        "name": name,
        "shape": {
            "languages": shape.languages,
            "files_per_language": shape.files_per_language,
            "messages_per_file": shape.messages_per_file,
            "files_scanned_at_startup": file_count,
            "messages_generated": message_count
        },
        "indexed_startup_ms": startup_ms,
        "steady_rss_kb": steady_rss_kb,
        "warm_requests": summarize(groups),
        "notes": "Request file-read count is expected to be zero for indexed warm requests except disk add/delete refresh scans.",
    })
}

fn measure<F>(groups: &mut BTreeMap<&'static str, Vec<f64>>, name: &'static str, mut f: F)
where
    F: FnMut(),
{
    let start = Instant::now();
    f();
    groups
        .entry(name)
        .or_default()
        .push(start.elapsed().as_secs_f64() * 1000.0);
}

fn summarize(groups: BTreeMap<&str, Vec<f64>>) -> Value {
    let mut result = serde_json::Map::new();
    for (name, mut values) in groups {
        values.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p50 = percentile(&values, 0.50);
        let p95 = percentile(&values, 0.95);
        let max = values.last().copied().unwrap_or_default();
        result.insert(
            name.to_string(),
            json!({"p50_ms": p50, "p95_ms": p95, "max_ms": max}),
        );
    }
    Value::Object(result)
}

fn percentile(values: &[f64], q: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let index = ((values.len() - 1) as f64 * q).ceil() as usize;
    values[index]
}

fn request_definition(lsp: &mut LspProcess, path: &Path, id_offset: usize) {
    lsp.send(&request(
        10_000 + id_offset as i64,
        "textDocument/definition",
        path,
        0,
        0,
        json!({}),
    ));
    lsp.recv_response(10_000 + id_offset as i64);
}

fn request_references(lsp: &mut LspProcess, path: &Path, id_offset: usize) {
    lsp.send(&request(
        20_000 + id_offset as i64,
        "textDocument/references",
        path,
        0,
        0,
        json!({"context": {"includeDeclaration": false}}),
    ));
    lsp.recv_response(20_000 + id_offset as i64);
}

fn request_hover(lsp: &mut LspProcess, path: &Path, id_offset: usize) {
    lsp.send(&request(
        30_000 + id_offset as i64,
        "textDocument/hover",
        path,
        1,
        12,
        json!({}),
    ));
    lsp.recv_response(30_000 + id_offset as i64);
}

fn request_completion(lsp: &mut LspProcess, path: &Path, id_offset: usize) {
    let id = 40_000 + id_offset as i64;
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "textDocument/completion",
        "params": {
            "textDocument": {"uri": format!("file://{}", path.display())},
            "position": {"line": 2, "character": 5}
        }
    }));
    lsp.recv_response(id);
}

fn request_code_action(lsp: &mut LspProcess, path: &Path, id_offset: usize) {
    let id = 50_000 + id_offset as i64;
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "textDocument/codeAction",
        "params": {
            "textDocument": {"uri": format!("file://{}", path.display())},
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}},
            "context": {"diagnostics": []}
        }
    }));
    lsp.recv_response(id);
}

fn request(id: i64, method: &str, path: &Path, line: u32, character: u32, extra: Value) -> Value {
    let mut params = serde_json::Map::new();
    params.insert(
        "textDocument".to_string(),
        json!({"uri": format!("file://{}", path.display())}),
    );
    params.insert(
        "position".to_string(),
        json!({"line": line, "character": character}),
    );
    if let Some(object) = extra.as_object() {
        for (key, value) in object {
            params.insert(key.clone(), value.clone());
        }
    }
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

fn generate_workspace(shape: Shape) -> TempDir {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("fluent-lsp.toml"),
        "origin_language = \"lang00\"\nfile_masks = [\"locales/{lang}/{filepath}.ftl\"]\n",
    )
    .unwrap();
    for lang in 0..shape.languages {
        let language = format!("lang{lang:02}");
        let dir = temp.path().join("locales").join(&language);
        std::fs::create_dir_all(&dir).unwrap();
        for file in 0..shape.files_per_language {
            let mut source = String::new();
            for message in 0..shape.messages_per_file {
                let key = format!("msg-{file:03}-{message:04}");
                if message % 17 == 0 {
                    source.push_str(&format!("# Generated docs for {key}\n"));
                }
                if lang != 0 && message % 23 == 0 {
                    continue;
                }
                if message % 11 == 0 {
                    source.push_str(&format!(
                        "{key} = {{ $count ->\n    [one] {language} one item\n   *[other] {language} many items\n}}\n"
                    ));
                } else if message % 7 == 0 {
                    source.push_str(&format!("{key} =\n    .label = {language} label {message}\n    .tooltip = {language} tooltip {message}\n"));
                } else if message % 5 == 0 {
                    source.push_str(&format!("-{key} = {language} term {message}\n"));
                } else {
                    source.push_str(&format!("{key} = {language} value {message}\n"));
                }
                source.push('\n');
            }
            if lang == 1 && file == shape.files_per_language - 1 {
                source.push_str("local-only-extra = Local only file marker\n");
            }
            std::fs::write(dir.join(format!("file{file:03}.ftl")), source).unwrap();
        }
    }
    temp
}

fn unix_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis()
}
