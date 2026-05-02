use std::collections::HashMap;
use std::ops::Range as ByteRange;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use fluent_syntax::ast::{Entry, Resource};
use fluent_syntax::parser;
use fluent_syntax::serializer;
use regex::Regex;
use serde::Deserialize;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::{
    DidChangeTextDocumentParams, DidOpenTextDocumentParams, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverContents, HoverParams, InitializeParams, InitializeResult,
    Location, MarkupContent, MarkupKind, MessageType, OneOf, Position, Range, ReferenceParams,
    ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind, Url,
};
use tower_lsp::{Client, LanguageServer};

const CONFIG_FILE_NAMES: [&str; 2] = ["fluent-lsp.toml", ".fluent-lsp.toml"];
const DEFAULT_HOVER_SELECTOR_COMBINATIONS_LIMIT: usize = 32;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub origin_language: String,
    #[serde(default)]
    pub file_masks: Vec<String>,
    #[serde(default)]
    pub hover_selector_combinations: bool,
    #[serde(default = "default_hover_selector_combinations_limit")]
    pub hover_selector_combinations_limit: usize,
}

#[derive(Debug, Clone)]
pub struct WorkspaceConfig {
    root_dir: PathBuf,
    origin_language: String,
    file_masks: Vec<FileMask>,
    hover_selector_combinations: bool,
    hover_selector_combinations_limit: usize,
}

#[derive(Debug, Clone)]
struct FileMask {
    raw: String,
    matcher: Regex,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileMatch {
    mask_index: usize,
    language: String,
    filepath: String,
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
        let origin_language = config.origin_language.trim().to_string();
        if origin_language.is_empty() {
            anyhow::bail!("origin_language must not be empty");
        }

        let file_masks = compile_file_masks(&config.file_masks)?;
        if file_masks.is_empty() {
            anyhow::bail!(
                "file_masks must include at least one template containing {{lang}} and {{filepath}}"
            );
        }

        Ok(Self {
            root_dir,
            origin_language,
            file_masks,
            hover_selector_combinations: config.hover_selector_combinations,
            hover_selector_combinations_limit: config.hover_selector_combinations_limit.max(1),
        })
    }

    fn origin_language(&self) -> &str {
        &self.origin_language
    }

    fn file_match(&self, path: &Path) -> Option<FileMatch> {
        if !is_fluent_file(path) {
            return None;
        }

        let relative = self.relative_path(path)?;
        for (mask_index, mask) in self.file_masks.iter().enumerate() {
            let captures = match mask.matcher.captures(&relative) {
                Some(captures) => captures,
                None => continue,
            };
            let language = captures.name("lang")?.as_str().to_string();
            let filepath = captures.name("filepath")?.as_str().to_string();
            return Some(FileMatch {
                mask_index,
                language,
                filepath,
            });
        }

        None
    }

    fn is_origin_file(&self, path: &Path) -> bool {
        self.file_match(path)
            .map(|file_match| file_match.language == self.origin_language)
            .unwrap_or(false)
    }

    fn matches_translation_file(&self, path: &Path) -> bool {
        self.file_match(path)
            .map(|file_match| file_match.language != self.origin_language)
            .unwrap_or(false)
    }

    fn origin_file_for(&self, path: &Path) -> Option<PathBuf> {
        let file_match = self.file_match(path)?;
        Some(self.render_path(
            file_match.mask_index,
            &self.origin_language,
            &file_match.filepath,
        ))
    }

    fn matches_origin_counterpart(&self, path: &Path, origin_match: &FileMatch) -> bool {
        self.file_match(path)
            .map(|candidate| {
                candidate.mask_index == origin_match.mask_index
                    && candidate.filepath == origin_match.filepath
                    && candidate.language != self.origin_language
            })
            .unwrap_or(false)
    }

    fn describe_masks(&self) -> String {
        if self.file_masks.is_empty() {
            "<none>".to_string()
        } else {
            self.file_masks
                .iter()
                .map(|mask| mask.raw.clone())
                .collect::<Vec<_>>()
                .join(", ")
        }
    }

    fn hover_selector_combinations(&self) -> bool {
        self.hover_selector_combinations
    }

    fn hover_selector_combinations_limit(&self) -> usize {
        self.hover_selector_combinations_limit
    }

    fn render_path(&self, mask_index: usize, language: &str, filepath: &str) -> PathBuf {
        let relative = self.file_masks[mask_index]
            .raw
            .replace("{lang}", language)
            .replace("{filepath}", filepath);
        self.root_dir.join(relative)
    }

    fn relative_path(&self, path: &Path) -> Option<String> {
        let relative = path.strip_prefix(&self.root_dir).ok()?;
        Some(relative.to_string_lossy().replace('\\', "/"))
    }
}

fn default_hover_selector_combinations_limit() -> usize {
    DEFAULT_HOVER_SELECTOR_COMBINATIONS_LIMIT
}

fn compile_file_masks(masks: &[String]) -> Result<Vec<FileMask>> {
    let mut compiled = Vec::with_capacity(masks.len());

    for mask in masks {
        if mask.matches("{lang}").count() != 1 || mask.matches("{filepath}").count() != 1 {
            anyhow::bail!(
                "invalid file mask `{mask}`: expected exactly one {{lang}} and one {{filepath}} placeholder"
            );
        }

        let lang_offset = mask
            .find("{lang}")
            .with_context(|| format!("invalid file mask `{mask}`: missing {{lang}}"))?;
        let filepath_offset = mask
            .find("{filepath}")
            .with_context(|| format!("invalid file mask `{mask}`: missing {{filepath}}"))?;
        let (first_offset, first_placeholder, second_offset, second_placeholder) =
            if lang_offset < filepath_offset {
                (lang_offset, "{lang}", filepath_offset, "{filepath}")
            } else {
                (filepath_offset, "{filepath}", lang_offset, "{lang}")
            };

        let mut pattern = String::from("^");
        pattern.push_str(&regex::escape(&mask[..first_offset]));
        pattern.push_str(match first_placeholder {
            "{lang}" => "(?P<lang>[^/]+)",
            "{filepath}" => "(?P<filepath>.+)",
            _ => unreachable!(),
        });
        pattern.push_str(&regex::escape(
            &mask[first_offset + first_placeholder.len()..second_offset],
        ));
        pattern.push_str(match second_placeholder {
            "{lang}" => "(?P<lang>[^/]+)",
            "{filepath}" => "(?P<filepath>.+)",
            _ => unreachable!(),
        });
        pattern.push_str(&regex::escape(
            &mask[second_offset + second_placeholder.len()..],
        ));
        pattern.push('$');

        compiled.push(FileMask {
            raw: mask.clone(),
            matcher: Regex::new(&pattern)
                .with_context(|| format!("invalid file mask regex generated from `{mask}`"))?,
        });
    }

    Ok(compiled)
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct HoverEntryRender {
    comments: Option<String>,
    entry: String,
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

        let key = extract_definition_key(
            &source,
            &path,
            params.text_document_position_params.position,
        )?;
        let origin_path = workspace.origin_file_for(&path)?;
        let origin_uri = Url::from_file_path(&origin_path).ok()?;
        let origin_source = self.read_document_text(&origin_uri).await?;

        let definition = find_fluent_definition(&origin_source, &key)?;
        Some(Location {
            uri: origin_uri,
            range: definition,
        })
    }

    async fn references_for(&self, params: ReferenceParams) -> Option<Vec<Location>> {
        let state = self.state.read().await;
        let workspace = state.workspace.clone()?;
        let uri = params.text_document_position.text_document.uri;
        let path = uri.to_file_path().ok()?;
        if !workspace.is_origin_file(&path) {
            return None;
        }
        let origin_match = workspace.file_match(&path)?;

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
            if !workspace.matches_origin_counterpart(&translation_path, &origin_match) {
                continue;
            }
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

    async fn hover_for(&self, params: HoverParams) -> Option<Hover> {
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

        let key = extract_definition_key(
            &source,
            &path,
            params.text_document_position_params.position,
        )?;
        let origin_path = workspace.origin_file_for(&path)?;
        let origin_uri = Url::from_file_path(&origin_path).ok()?;
        let origin_source = self.read_document_text(&origin_uri).await?;
        let origin_entry = render_fluent_hover_entry(&origin_source, &key)?;
        let hover_range = find_fluent_definition(&source, &key)?;
        let hover_suffix = if workspace.hover_selector_combinations() {
            render_selector_combinations_section(
                &origin_source,
                &key,
                workspace.hover_selector_combinations_limit(),
            )
        } else {
            None
        };
        let hover_value = render_hover_markdown(&origin_entry, hover_suffix.as_deref());

        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: hover_value,
            }),
            range: Some(hover_range),
        })
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
                hover_provider: Some(tower_lsp::lsp_types::HoverProviderCapability::Simple(true)),
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
                            "loaded origin language {} with masks [{}]",
                            workspace.origin_language(),
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

    async fn hover(&self, params: HoverParams) -> LspResult<Option<Hover>> {
        Ok(self.hover_for(params).await)
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

pub fn render_fluent_entry(source: &str, key: &str) -> Option<String> {
    let resource = parse_fluent_resource(source);
    let entry = find_fluent_entry(&resource, key)?;
    let rendered = serializer::serialize(&Resource {
        body: vec![entry.clone()],
    });
    Some(rendered)
}

fn render_fluent_hover_entry(source: &str, key: &str) -> Option<HoverEntryRender> {
    let resource = parse_fluent_resource(source);
    let entry_index = find_fluent_entry_index(&resource, key)?;
    let mut comment_entries = Vec::new();

    let mut comment_start = entry_index;
    while comment_start > 0 && is_free_comment_entry(&resource.body[comment_start - 1]) {
        comment_start -= 1;
    }
    for comment_entry in &resource.body[comment_start..entry_index] {
        comment_entries.push(comment_entry.clone());
    }

    let entry = resource.body[entry_index].clone();
    let entry = strip_inline_comment(entry, &mut comment_entries);
    let comments = if comment_entries.is_empty() {
        None
    } else {
        Some(render_hover_comments(&comment_entries))
    };
    let entry = serializer::serialize(&Resource { body: vec![entry] });

    Some(HoverEntryRender { comments, entry })
}

fn render_hover_markdown(entry: &HoverEntryRender, hover_suffix: Option<&str>) -> String {
    let mut sections = Vec::new();
    if let Some(comments) = &entry.comments {
        sections.push(format!("Comments:\n```ftl\n{comments}```"));
    }
    sections.push(format!("Entry:\n```ftl\n{}```", entry.entry));
    if let Some(hover_suffix) = hover_suffix {
        sections.push(hover_suffix.to_string());
    }
    sections.join("\n\n")
}

fn strip_inline_comment<'a>(
    entry: Entry<&'a str>,
    comment_entries: &mut Vec<Entry<&'a str>>,
) -> Entry<&'a str> {
    match entry {
        Entry::Message(mut message) => {
            if let Some(comment) = message.comment.take() {
                comment_entries.push(Entry::Comment(comment));
            }
            Entry::Message(message)
        }
        Entry::Term(mut term) => {
            if let Some(comment) = term.comment.take() {
                comment_entries.push(Entry::Comment(comment));
            }
            Entry::Term(term)
        }
        other => other,
    }
}

fn is_free_comment_entry(entry: &Entry<&str>) -> bool {
    matches!(
        entry,
        Entry::Comment(_) | Entry::GroupComment(_) | Entry::ResourceComment(_)
    )
}

fn render_hover_comments(entries: &[Entry<&str>]) -> String {
    let mut rendered = String::new();
    for entry in entries {
        match entry {
            Entry::Comment(comment) => append_comment_with_prefix(&mut rendered, comment, "#"),
            Entry::GroupComment(comment) => {
                append_comment_with_prefix(&mut rendered, comment, "##")
            }
            Entry::ResourceComment(comment) => {
                append_comment_with_prefix(&mut rendered, comment, "###")
            }
            _ => {}
        }
    }
    rendered
}

fn append_comment_with_prefix(
    buffer: &mut String,
    comment: &fluent_syntax::ast::Comment<&str>,
    prefix: &str,
) {
    for line in &comment.content {
        buffer.push_str(prefix);
        if !line.trim().is_empty() {
            buffer.push(' ');
            buffer.push_str(line);
        }
        buffer.push('\n');
    }
}

fn render_selector_combinations_section(
    source: &str,
    key: &str,
    max_items: usize,
) -> Option<String> {
    let resource = parse_fluent_resource(source);
    let pattern = find_fluent_pattern(&resource, key)?;
    let expansion = expand_pattern(pattern, max_items);
    if expansion.items.is_empty() || expansion.items.iter().all(|item| item.selectors.is_empty()) {
        return None;
    }

    let rendered_count = expansion.items.len();
    let mut lines = vec!["Static combinations:".to_string()];
    for item in expansion.items {
        let selectors = item
            .selectors
            .iter()
            .map(|(selector, variant)| format!("`{selector}={variant}`"))
            .collect::<Vec<_>>()
            .join(", ");
        let text = item.text.replace('\n', "\\n");
        lines.push(format!("- {selectors}: `{text}`"));
    }
    let omitted = expansion.total_count.saturating_sub(rendered_count);
    if omitted > 0 {
        lines.push(format!("- `...`: {omitted} more"));
    }

    Some(lines.join("\n"))
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

fn find_fluent_pattern<'a>(
    resource: &'a Resource<&'a str>,
    key: &str,
) -> Option<&'a fluent_syntax::ast::Pattern<&'a str>> {
    let entry = find_fluent_entry(resource, key)?;
    let (entry_key, attribute_key) = split_fluent_key(key);

    match entry {
        Entry::Message(message) if entry_key == message.id.name => {
            if let Some(attribute_key) = attribute_key {
                message
                    .attributes
                    .iter()
                    .find(|attribute| attribute.id.name == attribute_key)
                    .map(|attribute| &attribute.value)
            } else {
                message.value.as_ref()
            }
        }
        Entry::Term(term) if entry_key == format!("-{}", term.id.name) => {
            if let Some(attribute_key) = attribute_key {
                term.attributes
                    .iter()
                    .find(|attribute| attribute.id.name == attribute_key)
                    .map(|attribute| &attribute.value)
            } else {
                Some(&term.value)
            }
        }
        _ => None,
    }
}

fn find_fluent_definition_span(resource: &Resource<&str>, key: &str) -> Option<ByteRange<usize>> {
    let entry = find_fluent_entry(resource, key)?;
    let (entry_key, attribute_key) = split_fluent_key(key);

    match entry {
        Entry::Message(message) if entry_key == message.id.name => {
            if let Some(attribute_key) = attribute_key {
                message
                    .attributes
                    .iter()
                    .find(|attribute| attribute.id.name == attribute_key)
                    .map(|attribute| attribute.id.span.clone())
            } else {
                Some(message.id.span.clone())
            }
        }
        Entry::Term(term) if entry_key == format!("-{}", term.id.name) => {
            if let Some(attribute_key) = attribute_key {
                term.attributes
                    .iter()
                    .find(|attribute| attribute.id.name == attribute_key)
                    .map(|attribute| attribute.id.span.clone())
            } else {
                Some(term.id.span.clone())
            }
        }
        _ => None,
    }
}

fn find_fluent_entry<'a>(resource: &'a Resource<&'a str>, key: &str) -> Option<&'a Entry<&'a str>> {
    let (entry_key, attribute_key) = split_fluent_key(key);

    for entry in &resource.body {
        if entry_matches_key(entry, entry_key, attribute_key) {
            return Some(entry);
        }
    }

    None
}

fn find_fluent_entry_index(resource: &Resource<&str>, key: &str) -> Option<usize> {
    let (entry_key, attribute_key) = split_fluent_key(key);
    resource
        .body
        .iter()
        .position(|entry| entry_matches_key(entry, entry_key, attribute_key))
}

fn entry_matches_key(entry: &Entry<&str>, entry_key: &str, attribute_key: Option<&str>) -> bool {
    match entry {
        Entry::Message(message) if entry_key == message.id.name => {
            attribute_key.is_none()
                || message
                    .attributes
                    .iter()
                    .any(|attribute| Some(attribute.id.name) == attribute_key)
        }
        Entry::Term(term) if entry_key == format!("-{}", term.id.name) => {
            attribute_key.is_none()
                || term
                    .attributes
                    .iter()
                    .any(|attribute| Some(attribute.id.name) == attribute_key)
        }
        _ => false,
    }
}

fn split_fluent_key(key: &str) -> (&str, Option<&str>) {
    match key.rsplit_once('.') {
        Some((entry, attribute)) if !attribute.is_empty() => (entry, Some(attribute)),
        _ => (key, None),
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SelectorExpansion {
    items: Vec<SelectorExpansionItem>,
    total_count: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SelectorExpansionItem {
    selectors: Vec<(String, String)>,
    text: String,
}

fn expand_pattern(
    pattern: &fluent_syntax::ast::Pattern<&str>,
    max_items: usize,
) -> SelectorExpansion {
    let mut items = vec![SelectorExpansionItem::default()];
    let mut total_count = 1usize;

    for element in &pattern.elements {
        let element_items = expand_pattern_element(element, max_items);
        if element_items.items.is_empty() {
            continue;
        }

        total_count = total_count.saturating_mul(element_items.total_count);

        let mut combined = Vec::new();
        for left in &items {
            for right in &element_items.items {
                if combined.len() >= max_items {
                    break;
                }
                let mut selectors = left.selectors.clone();
                selectors.extend(right.selectors.clone());
                combined.push(SelectorExpansionItem {
                    selectors,
                    text: format!("{}{}", left.text, right.text),
                });
            }
            if combined.len() >= max_items {
                break;
            }
        }

        items = combined;
        if items.is_empty() {
            break;
        }
    }

    SelectorExpansion { items, total_count }
}

fn expand_pattern_element(
    element: &fluent_syntax::ast::PatternElement<&str>,
    max_items: usize,
) -> SelectorExpansion {
    match element {
        fluent_syntax::ast::PatternElement::TextElement { value } => SelectorExpansion {
            items: vec![SelectorExpansionItem {
                selectors: Vec::new(),
                text: (*value).to_string(),
            }],
            total_count: 1,
        },
        fluent_syntax::ast::PatternElement::Placeable { expression } => {
            expand_expression(expression, max_items)
        }
    }
}

fn expand_expression(
    expression: &fluent_syntax::ast::Expression<&str>,
    max_items: usize,
) -> SelectorExpansion {
    match expression {
        fluent_syntax::ast::Expression::Inline(inline) => SelectorExpansion {
            items: vec![SelectorExpansionItem {
                selectors: Vec::new(),
                text: render_inline_expression(inline),
            }],
            total_count: 1,
        },
        fluent_syntax::ast::Expression::Select { selector, variants } => {
            let selector_name = render_inline_expression(selector);
            let mut items = Vec::new();
            let mut total_count = 0usize;

            for variant in variants {
                let nested = expand_pattern(&variant.value, max_items);
                total_count = total_count.saturating_add(nested.total_count);
                let variant_name = render_variant_key(&variant.key);
                for item in nested.items {
                    if items.len() >= max_items {
                        break;
                    }
                    let mut selectors = vec![(selector_name.clone(), variant_name.clone())];
                    selectors.extend(item.selectors);
                    items.push(SelectorExpansionItem {
                        selectors,
                        text: item.text,
                    });
                }
                if items.len() >= max_items {
                    break;
                }
            }

            SelectorExpansion { items, total_count }
        }
    }
}

fn render_variant_key(key: &fluent_syntax::ast::VariantKey<&str>) -> String {
    match key {
        fluent_syntax::ast::VariantKey::Identifier { name } => (*name).to_string(),
        fluent_syntax::ast::VariantKey::NumberLiteral { value } => (*value).to_string(),
    }
}

fn render_inline_expression(expression: &fluent_syntax::ast::InlineExpression<&str>) -> String {
    match expression {
        fluent_syntax::ast::InlineExpression::StringLiteral { value } => format!("\"{value}\""),
        fluent_syntax::ast::InlineExpression::NumberLiteral { value } => (*value).to_string(),
        fluent_syntax::ast::InlineExpression::FunctionReference { id, .. } => {
            format!("{}()", id.name)
        }
        fluent_syntax::ast::InlineExpression::MessageReference { id, attribute } => match attribute
        {
            Some(attribute) => format!("{}.{}", id.name, attribute.name),
            None => id.name.to_string(),
        },
        fluent_syntax::ast::InlineExpression::TermReference {
            id,
            attribute,
            arguments,
        } => {
            let mut rendered = format!("-{}", id.name);
            if let Some(attribute) = attribute {
                rendered.push('.');
                rendered.push_str(attribute.name);
            }
            if arguments.is_some() {
                rendered.push_str("()");
            }
            rendered
        }
        fluent_syntax::ast::InlineExpression::VariableReference { id } => format!("${}", id.name),
        fluent_syntax::ast::InlineExpression::Placeable { expression } => {
            format!("{{ {} }}", render_expression_summary(expression))
        }
    }
}

fn render_expression_summary(expression: &fluent_syntax::ast::Expression<&str>) -> String {
    match expression {
        fluent_syntax::ast::Expression::Inline(inline) => render_inline_expression(inline),
        fluent_syntax::ast::Expression::Select { selector, .. } => {
            format!("{} -> …", render_inline_expression(selector))
        }
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
        let source =
            "welcome-title = Salut\n-brand-name = Nocturne\nbutton-copy =\n    .label = Lancer\n";

        let message =
            extract_definition_key(source, Path::new("locales/fr/app.ftl"), Position::new(0, 4))
                .unwrap();
        assert_eq!(message, "welcome-title");

        let term =
            extract_definition_key(source, Path::new("locales/fr/app.ftl"), Position::new(1, 2))
                .unwrap();
        assert_eq!(term, "-brand-name");

        let attribute =
            extract_definition_key(source, Path::new("locales/fr/app.ftl"), Position::new(3, 6))
                .unwrap();
        assert_eq!(attribute, "button-copy.label");
    }

    #[test]
    fn collects_translation_files_without_origin_language_files() {
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
                "locales/es/dialogs/menu.ftl".to_string(),
                "locales/fr/app.ftl".to_string(),
                "locales/fr/dialogs/menu.ftl".to_string(),
            ]
        );
    }

    #[test]
    fn resolves_origin_file_from_nested_translation_path() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("workspace");
        let workspace = WorkspaceConfig::load(root.clone()).unwrap();
        let translation = root.join("locales/es/dialogs/menu.ftl");

        let file_match = workspace.file_match(&translation).unwrap();
        assert_eq!(file_match.language, "es");
        assert_eq!(file_match.filepath, "dialogs/menu");
        assert_eq!(
            workspace.origin_file_for(&translation).unwrap(),
            root.join("locales/en/dialogs/menu.ftl")
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
    fn renders_fluent_entry_with_comments() {
        let source = "# Shown on first launch\nwelcome-title = Welcome\n# Product naming\n# Keep in title case\n-brand-name = Nightly\n";

        let message = render_fluent_entry(source, "welcome-title").unwrap();
        assert_eq!(
            message,
            "# Shown on first launch\nwelcome-title = Welcome\n"
        );

        let term = render_fluent_entry(source, "-brand-name").unwrap();
        assert_eq!(
            term,
            "# Product naming\n# Keep in title case\n-brand-name = Nightly\n"
        );
    }

    #[test]
    fn renders_selector_combinations_section() {
        let source = "install-hint =\n    { $platform ->\n        [macos] Press Command\n       *[other] Press Ctrl\n    } + { $action ->\n        [copy] C\n       *[paste] V\n    }\n";

        let rendered = render_selector_combinations_section(source, "install-hint", 3).unwrap();
        assert_eq!(
            rendered,
            "Static combinations:\n- `$platform=macos`, `$action=copy`: `Press Command + C`\n- `$platform=macos`, `$action=paste`: `Press Command + V`\n- `$platform=other`, `$action=copy`: `Press Ctrl + C`\n- `...`: 1 more"
        );
    }

    #[test]
    fn renders_hover_markdown_with_actual_comment_markers() {
        let entry = HoverEntryRender {
            comments: Some("### Shared menu copy\n## File menu\n# Primary action\n".to_string()),
            entry: "menu-save = Save\n".to_string(),
        };
        let rendered = render_hover_markdown(
            &entry,
            Some("Static combinations:\n- `$kind=default`: `Save`"),
        );
        assert_eq!(
            rendered,
            "Comments:\n```ftl\n### Shared menu copy\n## File menu\n# Primary action\n```\n\nEntry:\n```ftl\nmenu-save = Save\n```\n\nStatic combinations:\n- `$kind=default`: `Save`"
        );
    }

    #[test]
    fn renders_hover_entry_with_free_and_inline_comments() {
        let source = "### Shared menu copy\n## File menu\n# Primary action\nmenu-save = Save\n";
        let rendered = render_fluent_hover_entry(source, "menu-save").unwrap();
        assert_eq!(
            rendered,
            HoverEntryRender {
                comments: Some(
                    "### Shared menu copy\n## File menu\n# Primary action\n".to_string()
                ),
                entry: "menu-save = Save\n".to_string(),
            }
        );
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
