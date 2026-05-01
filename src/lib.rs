use std::collections::HashMap;
use std::ops::Range as ByteRange;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use fluent_syntax::ast::{Entry, Resource};
use fluent_syntax::parser;
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Deserialize;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::{
    DidChangeTextDocumentParams, DidOpenTextDocumentParams, GotoDefinitionParams,
    GotoDefinitionResponse, InitializeParams, InitializeResult, Location, MessageType, OneOf,
    Position, Range, ReferenceParams, ServerCapabilities, TextDocumentSyncCapability,
    TextDocumentSyncKind, Url,
};
use tower_lsp::{Client, LanguageServer};

const CONFIG_FILE_NAMES: [&str; 2] = ["fluent-lsp.toml", ".fluent-lsp.toml"];

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub english_file: PathBuf,
    #[serde(default)]
    pub file_masks: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceConfig {
    root_dir: PathBuf,
    english_file: PathBuf,
    file_masks: Vec<String>,
    matcher: Option<GlobSet>,
}

impl WorkspaceConfig {
    pub fn load(root_dir: PathBuf) -> Result<Self> {
        let config_path = CONFIG_FILE_NAMES
            .iter()
            .map(|name| root_dir.join(name))
            .find(|path| path.is_file())
            .with_context(|| {
                format!(
                    "missing config file; tried {}",
                    CONFIG_FILE_NAMES
                        .iter()
                        .map(|name| root_dir.join(name).display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;

        let raw = std::fs::read_to_string(&config_path)
            .with_context(|| format!("failed to read {}", config_path.display()))?;
        let config: Config = toml::from_str(&raw)
            .with_context(|| format!("failed to parse {}", config_path.display()))?;
        let english_file = root_dir.join(&config.english_file);

        let matcher = compile_globs(&config.file_masks)?;

        Ok(Self {
            root_dir,
            english_file,
            file_masks: config.file_masks,
            matcher,
        })
    }

    pub fn english_file(&self) -> &Path {
        &self.english_file
    }

    pub fn is_english_file(&self, path: &Path) -> bool {
        path == self.english_file
    }

    pub fn matches_translation_file(&self, path: &Path) -> bool {
        if path == self.english_file || !is_fluent_file(path) {
            return false;
        }

        match &self.matcher {
            Some(matcher) => {
                let relative = path.strip_prefix(&self.root_dir).unwrap_or(path);
                matcher.is_match(relative)
            }
            None => true,
        }
    }

    pub fn describe_masks(&self) -> String {
        if self.file_masks.is_empty() {
            "*".to_string()
        } else {
            self.file_masks.join(", ")
        }
    }
}

fn compile_globs(globs: &[String]) -> Result<Option<GlobSet>> {
    if globs.is_empty() {
        return Ok(None);
    }

    let mut builder = GlobSetBuilder::new();
    for glob in globs {
        builder.add(Glob::new(glob).with_context(|| format!("invalid glob: {glob}"))?);
    }

    Ok(Some(builder.build()?))
}

fn collect_translation_files(workspace: &WorkspaceConfig) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_files_under(&workspace.root_dir, &mut files);
    files.retain(|path| workspace.matches_translation_file(path));
    files.sort();
    files
}

fn collect_files_under(root: &Path, files: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files_under(&path, files);
        } else if path.is_file() {
            files.push(path);
        }
    }
}

#[derive(Default)]
struct ServerState {
    root_dir: Option<PathBuf>,
    workspace: Option<WorkspaceConfig>,
    open_documents: HashMap<Url, String>,
}

pub struct Backend {
    client: Client,
    state: Arc<RwLock<ServerState>>,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            state: Arc::new(RwLock::new(ServerState::default())),
        }
    }

    async fn read_document_text(&self, uri: &Url) -> Option<String> {
        if let Some(text) = self.state.read().await.open_documents.get(uri).cloned() {
            return Some(text);
        }

        let path = uri.to_file_path().ok()?;
        tokio::fs::read_to_string(path).await.ok()
    }

    async fn definition_for(&self, params: GotoDefinitionParams) -> Option<Location> {
        let state = self.state.read().await;
        let workspace = state.workspace.clone()?;
        let uri = params.text_document_position_params.text_document.uri;
        let path = uri.to_file_path().ok()?;
        if !workspace.matches_translation_file(&path) {
            return None;
        }

        let source = state
            .open_documents
            .get(&uri)
            .cloned()
            .or_else(|| std::fs::read_to_string(&path).ok())?;
        drop(state);

        let key = extract_definition_key(&source, &path, params.text_document_position_params.position)?;
        let english_uri = Url::from_file_path(workspace.english_file()).ok()?;
        let english_source = self.read_document_text(&english_uri).await?;

        let definition = find_fluent_definition(&english_source, &key)?;
        Some(Location {
            uri: english_uri,
            range: definition,
        })
    }

    async fn references_for(&self, params: ReferenceParams) -> Option<Vec<Location>> {
        let state = self.state.read().await;
        let workspace = state.workspace.clone()?;
        let uri = params.text_document_position.text_document.uri;
        let path = uri.to_file_path().ok()?;
        if !workspace.is_english_file(&path) {
            return None;
        }

        let source = state
            .open_documents
            .get(&uri)
            .cloned()
            .or_else(|| std::fs::read_to_string(&path).ok())?;
        drop(state);

        let key = extract_definition_key(&source, &path, params.text_document_position.position)?;
        let mut references = Vec::new();

        if params.context.include_declaration {
            let range = find_fluent_definition(&source, &key)?;
            references.push(Location {
                uri: uri.clone(),
                range,
            });
        }

        for translation_path in collect_translation_files(&workspace) {
            let translation_uri = match Url::from_file_path(&translation_path) {
                Ok(uri) => uri,
                Err(()) => continue,
            };
            let translation_source = match self.read_document_text(&translation_uri).await {
                Some(source) => source,
                None => continue,
            };
            let Some(range) = find_fluent_definition(&translation_source, &key) else {
                continue;
            };
            references.push(Location {
                uri: translation_uri,
                range,
            });
        }

        Some(references)
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> LspResult<InitializeResult> {
        let root_dir = params
            .root_uri
            .and_then(|uri| uri.to_file_path().ok())
            .or_else(|| {
                params.workspace_folders.as_ref().and_then(|folders| {
                    folders
                        .first()
                        .and_then(|folder| folder.uri.to_file_path().ok())
                })
            });

        let workspace = root_dir
            .as_ref()
            .and_then(|root| WorkspaceConfig::load(root.clone()).ok());

        {
            let mut state = self.state.write().await;
            state.root_dir = root_dir;
            state.workspace = workspace;
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                ..ServerCapabilities::default()
            },
            ..InitializeResult::default()
        })
    }

    async fn initialized(&self, _: tower_lsp::lsp_types::InitializedParams) {
        let state = self.state.read().await;
        match &state.workspace {
            Some(workspace) => {
                self.client
                    .log_message(
                        MessageType::INFO,
                        format!(
                            "loaded {} with masks [{}]",
                            workspace.english_file().display(),
                            workspace.describe_masks()
                        ),
                    )
                    .await;
            }
            None => {
                let root = state
                    .root_dir
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "<unknown root>".to_string());
                self.client
                    .log_message(
                        MessageType::WARNING,
                        format!("no fluent-lsp config found under {root}"),
                    )
                    .await;
            }
        }
    }

    async fn shutdown(&self) -> LspResult<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.state
            .write()
            .await
            .open_documents
            .insert(params.text_document.uri, params.text_document.text);
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.into_iter().last() {
            self.state
                .write()
                .await
                .open_documents
                .insert(params.text_document.uri, change.text);
        }
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> LspResult<Option<GotoDefinitionResponse>> {
        Ok(self
            .definition_for(params)
            .await
            .map(GotoDefinitionResponse::Scalar))
    }

    async fn references(&self, params: ReferenceParams) -> LspResult<Option<Vec<Location>>> {
        Ok(self.references_for(params).await)
    }
}

pub fn extract_key_at_position(source: &str, position: Position) -> Option<String> {
    let (line, line_start) = line_at(source, position.line as usize)?;
    let line_offset = utf16_position_to_byte_index(line, position.character as usize)?;

    let mut probe = line_offset;
    if !is_key_byte(line.as_bytes().get(probe).copied()) && probe > 0 {
        probe -= 1;
    }
    if !is_key_byte(line.as_bytes().get(probe).copied()) {
        return None;
    }

    let bytes = line.as_bytes();
    let mut start = probe;
    while start > 0 && is_key_byte(bytes.get(start - 1).copied()) {
        start -= 1;
    }

    let mut end = probe;
    while end + 1 < bytes.len() && is_key_byte(bytes.get(end + 1).copied()) {
        end += 1;
    }

    let token = &line[start..=end];
    if token.bytes().any(|byte| byte.is_ascii_alphabetic()) {
        Some(source[line_start + start..line_start + end + 1].to_string())
    } else {
        None
    }
}

pub fn extract_definition_key(source: &str, path: &Path, position: Position) -> Option<String> {
    if is_fluent_file(path) {
        extract_fluent_key_at_position(source, position)
    } else {
        extract_key_at_position(source, position)
    }
}

pub fn find_fluent_definition(source: &str, key: &str) -> Option<Range> {
    let resource = parse_fluent_resource(source);
    let span = find_fluent_definition_span(&resource, key)?;
    byte_range_to_lsp_range(source, span)
}

fn parse_fluent_resource(source: &str) -> Resource<&str> {
    match parser::parse(source) {
        Ok(resource) => resource,
        Err((resource, _errors)) => resource,
    }
}

fn extract_fluent_key_at_position(source: &str, position: Position) -> Option<String> {
    let resource = parse_fluent_resource(source);
    let target = position_to_byte_index(source, position)?;

    for entry in &resource.body {
        match entry {
            Entry::Message(message) => {
                if byte_range_contains(&message.id.span, target) {
                    return Some(message.id.name.to_string());
                }

                for attribute in &message.attributes {
                    if byte_range_contains(&attribute.id.span, target) {
                        return Some(format!("{}.{}", message.id.name, attribute.id.name));
                    }
                }
            }
            Entry::Term(term) => {
                if byte_range_contains(&term.id.span, target) {
                    return Some(format!("-{}", term.id.name));
                }

                for attribute in &term.attributes {
                    if byte_range_contains(&attribute.id.span, target) {
                        return Some(format!("-{}.{}", term.id.name, attribute.id.name));
                    }
                }
            }
            _ => {}
        }
    }

    None
}

fn find_fluent_definition_span(resource: &Resource<&str>, key: &str) -> Option<ByteRange<usize>> {
    let (entry_key, attribute_key) = split_fluent_key(key);

    for entry in &resource.body {
        match entry {
            Entry::Message(message) if entry_key == message.id.name => {
                if let Some(attribute_key) = attribute_key {
                    if let Some(attribute) = message
                        .attributes
                        .iter()
                        .find(|attribute| attribute.id.name == attribute_key)
                    {
                        return Some(attribute.id.span.clone());
                    }
                } else {
                    return Some(message.id.span.clone());
                }
            }
            Entry::Term(term) if entry_key == format!("-{}", term.id.name) => {
                if let Some(attribute_key) = attribute_key {
                    if let Some(attribute) = term
                        .attributes
                        .iter()
                        .find(|attribute| attribute.id.name == attribute_key)
                    {
                        return Some(attribute.id.span.clone());
                    }
                } else {
                    return Some(term.id.span.clone());
                }
            }
            _ => {}
        }
    }

    None
}

fn split_fluent_key(key: &str) -> (&str, Option<&str>) {
    match key.rsplit_once('.') {
        Some((entry, attribute)) if !attribute.is_empty() => (entry, Some(attribute)),
        _ => (key, None),
    }
}

fn position_to_byte_index(source: &str, position: Position) -> Option<usize> {
    let (line, line_start) = line_at(source, position.line as usize)?;
    let line_offset = utf16_position_to_byte_index(line, position.character as usize)?;
    Some(line_start + line_offset)
}

fn byte_range_to_lsp_range(source: &str, span: ByteRange<usize>) -> Option<Range> {
    Some(Range {
        start: byte_index_to_position(source, span.start)?,
        end: byte_index_to_position(source, span.end)?,
    })
}

fn byte_index_to_position(source: &str, target: usize) -> Option<Position> {
    if target > source.len() || !source.is_char_boundary(target) {
        return None;
    }

    let mut line = 0u32;
    let mut character = 0u32;
    for (byte_idx, ch) in source.char_indices() {
        if byte_idx == target {
            return Some(Position::new(line, character));
        }

        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += ch.len_utf16() as u32;
        }
    }

    if target == source.len() {
        Some(Position::new(line, character))
    } else {
        None
    }
}

fn line_at(source: &str, line_index: usize) -> Option<(&str, usize)> {
    let mut start = 0;
    for (idx, line) in source.split('\n').enumerate() {
        if idx == line_index {
            return Some((line, start));
        }
        start += line.len() + 1;
    }

    None
}

fn utf16_position_to_byte_index(line: &str, character: usize) -> Option<usize> {
    let mut utf16_seen = 0;
    for (byte_idx, ch) in line.char_indices() {
        if utf16_seen == character {
            return Some(byte_idx);
        }
        utf16_seen += ch.len_utf16();
        if utf16_seen > character {
            return Some(byte_idx);
        }
    }

    if utf16_seen == character {
        Some(line.len())
    } else {
        None
    }
}

fn is_key_byte(byte: Option<u8>) -> bool {
    matches!(
        byte,
        Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'-' | b'.')
    )
}

fn is_fluent_file(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("ftl")
}

fn byte_range_contains(span: &ByteRange<usize>, target: usize) -> bool {
    span.start <= target && target < span.end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_key_inside_quotes() {
        let source = "const title = t(\"welcome-title\");\n";
        let key = extract_key_at_position(source, Position::new(0, 19)).unwrap();
        assert_eq!(key, "welcome-title");
    }

    #[test]
    fn extracts_key_from_fluent_message_term_and_attribute() {
        let source = "welcome-title = Salut\n-brand-name = Nocturne\nbutton-copy =\n    .label = Lancer\n";

        let message = extract_definition_key(source, Path::new("locales/fr/app.ftl"), Position::new(0, 4)).unwrap();
        assert_eq!(message, "welcome-title");

        let term = extract_definition_key(source, Path::new("locales/fr/app.ftl"), Position::new(1, 2)).unwrap();
        assert_eq!(term, "-brand-name");

        let attribute = extract_definition_key(source, Path::new("locales/fr/app.ftl"), Position::new(3, 6)).unwrap();
        assert_eq!(attribute, "button-copy.label");
    }

    #[test]
    fn collects_translation_files_without_english() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("workspace");
        let workspace = WorkspaceConfig::load(root).unwrap();

        let files = collect_translation_files(&workspace);
        let relative: Vec<_> = files
            .iter()
            .map(|path| {
                path.strip_prefix(&workspace.root_dir)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();

        assert_eq!(
            relative,
            vec![
                "locales/es/app.ftl".to_string(),
                "locales/fr/app.ftl".to_string(),
            ]
        );
    }

    #[test]
    fn finds_message_line() {
        let source = "# Comment\nwelcome-title = Hello\nother = Value\n";
        let range = find_fluent_definition(source, "welcome-title").unwrap();
        assert_eq!(range.start, Position::new(1, 0));
        assert_eq!(range.end, Position::new(1, 13));
    }

    #[test]
    fn skips_non_message_lines_without_equals() {
        let source = "term\nwelcome-title = Hello\n";
        let range = find_fluent_definition(source, "welcome-title").unwrap();
        assert_eq!(range.start, Position::new(1, 0));
    }

    #[test]
    fn finds_term_and_attribute_ranges() {
        let source = "-brand-name = Nightly\nbutton-copy =\n    .label = Launch\n";

        let term = find_fluent_definition(source, "-brand-name").unwrap();
        assert_eq!(term.start, Position::new(0, 1));
        assert_eq!(term.end, Position::new(0, 11));

        let attribute = find_fluent_definition(source, "button-copy.label").unwrap();
        assert_eq!(attribute.start, Position::new(2, 5));
        assert_eq!(attribute.end, Position::new(2, 10));
    }

    #[test]
    fn local_fluent_syntax_fork_exposes_identifier_spans() {
        let resource = parser::parse("welcome-title = Welcome\n").unwrap();
        match &resource.body[0] {
            Entry::Message(message) => assert_eq!(message.id.span, 0..13),
            _ => panic!("expected message entry"),
        }
    }
}
