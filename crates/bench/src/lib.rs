use std::cell::RefCell;
use std::hint::black_box;
use std::ops::Range as ByteRange;
use std::path::{Path, PathBuf};
use std::time::Instant;
use std::time::SystemTime;

use anyhow::{Result, ensure};
use fluent_lsp::{
    FastMap, FastSet, FileMatch, IndexedFile, OriginAttributeTemplate,
    OriginMessageTemplate, SelectorStyle, WorkspaceConfig, WorkspaceIndex,
    build_workspace_index, code_actions_for_document, code_lenses_for_document,
    completion_for_position, definition_for_position, hover_for_position,
    references_for_position, selector_combinations_document_for_key,
};
use rkyv::{AlignedVec, Archive, Deserialize, Serialize};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower_lsp::jsonrpc::{Error as LspError, Result as LspResult};
use tower_lsp::ls_types::{
    CodeActionResponse, CodeLens, CompletionResponse, Hover, Location,
    Position, Range, Uri,
};

const PREBUILT_ARCHIVE_BYTES: &[u8] =
    include_bytes!("../data/prebuilt-bench-snapshot.rkyv");

#[derive(Debug, Clone, Copy)]
pub struct Shape {
    pub languages: usize,
    pub files_per_language: usize,
    pub messages_per_file: usize,
}

#[derive(Debug, Clone)]
pub struct CommonArgs {
    pub languages: usize,
    pub files_per_language: usize,
    pub messages_per_file: usize,
    pub warmup_repeats: usize,
    pub repeats: usize,
    pub workspace_root: Option<PathBuf>,
}

impl Default for CommonArgs {
    fn default() -> Self {
        Self {
            languages: 8,
            files_per_language: 10,
            messages_per_file: 200,
            warmup_repeats: 1,
            repeats: 10,
            workspace_root: None,
        }
    }
}

impl CommonArgs {
    pub fn parse() -> Self {
        Self::parse_with_defaults(Self::default())
    }

    pub fn parse_with_defaults(defaults: Self) -> Self {
        let mut args = std::env::args().skip(1);
        let mut parsed = defaults;

        while let Some(flag) = args.next() {
            match flag.as_str() {
                "--languages" => {
                    parsed.languages = parse_usize_flag(&mut args, &flag);
                }
                "--files-per-language" => {
                    parsed.files_per_language =
                        parse_usize_flag(&mut args, &flag);
                }
                "--messages-per-file" => {
                    parsed.messages_per_file =
                        parse_usize_flag(&mut args, &flag);
                }
                "--repeats" => {
                    parsed.repeats = parse_usize_flag(&mut args, &flag);
                }
                "--warmup-repeats" => {
                    parsed.warmup_repeats = parse_usize_flag(&mut args, &flag);
                }
                "--workspace-root" => {
                    let value = args
                        .next()
                        .unwrap_or_else(|| panic!("missing value for {flag}"));
                    parsed.workspace_root = Some(PathBuf::from(value));
                }
                _ => panic!("unknown flag {flag}"),
            }
        }

        parsed
    }

    pub fn shape(&self) -> Shape {
        Shape {
            languages: self.languages,
            files_per_language: self.files_per_language,
            messages_per_file: self.messages_per_file,
        }
    }

    pub fn criterion_shape() -> Shape {
        Shape {
            languages: env_usize("FLUENT_LSP_BENCH_LANGUAGES").unwrap_or(8),
            files_per_language: env_usize(
                "FLUENT_LSP_BENCH_FILES_PER_LANGUAGE",
            )
            .unwrap_or(10),
            messages_per_file: env_usize("FLUENT_LSP_BENCH_MESSAGES_PER_FILE")
                .unwrap_or(200),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DirectBenchHarness {
    pub workspace: WorkspaceConfig,
    pub index: WorkspaceIndex,
    pub snapshot: FastMap<PathBuf, SystemTime>,
}

#[derive(Debug, Clone, Default)]
pub struct FixtureLoadStats {
    pub total_ms: f64,
    pub index_load_ms: f64,
    pub deserialize_ms: Option<f64>,
    pub materialize_ms: Option<f64>,
    pub workspace_generate_ms: Option<f64>,
    pub target_scan_ms: f64,
}

impl DirectBenchHarness {
    pub fn load(root_dir: PathBuf) -> Result<Self> {
        let workspace = WorkspaceConfig::load(root_dir)?;
        let overlays = FastMap::<Uri, String>::default();
        let (index, snapshot, _) = build_workspace_index(&workspace, &overlays);
        Ok(Self {
            workspace,
            index,
            snapshot,
        })
    }

    pub fn workspace_root(&self) -> &Path {
        self.workspace.root_dir()
    }

    pub fn snapshot_file_count(&self) -> usize {
        self.snapshot.len()
    }

    pub fn definition(
        &self,
        path: &Path,
        position: Position,
    ) -> Option<Location> {
        definition_for_position(&self.workspace, &self.index, path, position)
    }

    pub fn references(
        &self,
        path: &Path,
        uri: &Uri,
        position: Position,
        include_declaration: bool,
    ) -> Option<Vec<Location>> {
        references_for_position(
            &self.workspace,
            &self.index,
            path,
            uri,
            position,
            include_declaration,
        )
    }

    pub fn hover(
        &self,
        path: &Path,
        position: Position,
    ) -> LspResult<Option<Hover>> {
        hover_for_position(&self.workspace, &self.index, path, position)
    }

    pub fn completion(
        &self,
        path: &Path,
        position: Position,
    ) -> LspResult<Option<CompletionResponse>> {
        completion_for_position(&self.workspace, &self.index, path, position)
    }

    pub fn code_actions(
        &self,
        path: &Path,
        position: Position,
    ) -> LspResult<Option<CodeActionResponse>> {
        let Some(uri) = Uri::from_file_path(path) else {
            return Err(LspError::invalid_params(format!(
                "expected a file path: {}",
                path.display()
            )));
        };
        let source = std::fs::read_to_string(path).map_err(|error| {
            LspError::invalid_params(format!(
                "failed to read {}: {error}",
                path.display()
            ))
        })?;
        let indexed_file = self.index.file(path);
        let origin_file = indexed_file
            .and_then(|file| self.index.origin_for(&self.workspace, file));
        code_actions_for_document(
            &self.workspace,
            path,
            &uri,
            &source,
            position,
            self.workspace
                .selector_style()
                .unwrap_or(SelectorStyle::default()),
            false,
            origin_file.map(|file| file.origin_message_templates.as_slice()),
            origin_file.map(|file| file.source.as_str()),
            indexed_file
                .and_then(|file| file.selected_key_at_position(position)),
        )
    }

    pub fn code_lenses(&self, path: &Path) -> LspResult<Option<Vec<CodeLens>>> {
        let Some(uri) = Uri::from_file_path(path) else {
            return Err(LspError::invalid_params(format!(
                "expected a file path: {}",
                path.display()
            )));
        };
        let source = std::fs::read_to_string(path).map_err(|error| {
            LspError::invalid_params(format!(
                "failed to read {}: {error}",
                path.display()
            ))
        })?;
        Ok(code_lenses_for_document(
            &self.workspace,
            path,
            &uri,
            &source,
        ))
    }

    pub fn selector_combinations_document(
        &self,
        path: &Path,
        key: &str,
    ) -> LspResult<String> {
        let source = std::fs::read_to_string(path).map_err(|error| {
            LspError::invalid_params(format!(
                "failed to read {}: {error}",
                path.display()
            ))
        })?;
        let origin_source = if self.workspace.is_origin_file(path) {
            Some(source.as_str())
        } else {
            let Some(file) = self.index.file(path) else {
                return Err(LspError::invalid_params(format!(
                    "failed to resolve indexed metadata for {}",
                    path.display()
                )));
            };
            self.index
                .origin_for(&self.workspace, file)
                .map(|origin_file| origin_file.source.as_str())
        };
        selector_combinations_document_for_key(
            &self.workspace,
            path,
            key,
            &source,
            origin_source,
        )
    }
}

pub struct BenchFixture {
    _temp: Option<TempDir>,
    pub root: PathBuf,
    pub harness: DirectBenchHarness,
    pub targets: BenchTargets,
    pub load_stats: FixtureLoadStats,
}

pub struct BenchTargets {
    pub origin_path: PathBuf,
    pub translation_path: PathBuf,
    pub definition_position: Position,
    pub references_position: Position,
    pub hover_position: Position,
    pub completion_position: Position,
    pub code_action_position: Position,
    pub selector_key: String,
}

#[derive(Archive, Deserialize, Serialize, Debug, Clone)]
struct BenchArchive {
    config_toml: String,
    files: Vec<BenchArchiveFile>,
    targets: BenchTargetsArchive,
}

#[derive(Archive, Deserialize, Serialize, Debug, Clone)]
struct BenchArchiveFile {
    relative_path: String,
    mask_index: usize,
    language: String,
    filepath: String,
    source: String,
    definitions: Vec<KeyRangeArchive>,
    selection_ranges: Vec<KeyRangeArchive>,
    keys: Vec<String>,
    block_line_ranges: Vec<KeyBlockArchive>,
    ordered_message_keys: Vec<String>,
    message_attributes: Vec<MessageAttributesArchive>,
    origin_message_templates: Vec<OriginMessageTemplateArchive>,
}

#[derive(Archive, Deserialize, Serialize, Debug, Clone)]
struct BenchTargetsArchive {
    origin_relative_path: String,
    translation_relative_path: String,
    definition_position: PositionArchive,
    references_position: PositionArchive,
    hover_position: PositionArchive,
    completion_position: PositionArchive,
    code_action_position: PositionArchive,
    selector_key: String,
}

#[derive(Archive, Deserialize, Serialize, Debug, Clone)]
struct PositionArchive {
    line: u32,
    character: u32,
}

#[derive(Archive, Deserialize, Serialize, Debug, Clone)]
struct RangeArchive {
    start: PositionArchive,
    end: PositionArchive,
}

#[derive(Archive, Deserialize, Serialize, Debug, Clone)]
struct ByteRangeArchive {
    start: usize,
    end: usize,
}

#[derive(Archive, Deserialize, Serialize, Debug, Clone)]
struct KeyRangeArchive {
    key: String,
    range: RangeArchive,
}

#[derive(Archive, Deserialize, Serialize, Debug, Clone)]
struct KeyBlockArchive {
    key: String,
    start_line: usize,
    end_line: usize,
}

#[derive(Archive, Deserialize, Serialize, Debug, Clone)]
struct MessageAttributesArchive {
    message_key: String,
    attributes: Vec<String>,
}

#[derive(Archive, Deserialize, Serialize, Debug, Clone)]
struct OriginMessageTemplateArchive {
    key: String,
    has_value: bool,
    attributes: Vec<OriginAttributeTemplateArchive>,
    source_span: ByteRangeArchive,
}

#[derive(Archive, Deserialize, Serialize, Debug, Clone)]
struct OriginAttributeTemplateArchive {
    key: String,
    source_span: ByteRangeArchive,
}

pub fn load_fixture(args: &CommonArgs) -> Result<BenchFixture> {
    if args.workspace_root.is_none() {
        return load_embedded_fixture();
    }
    load_fixture_from_disk(args)
}

pub fn build_archive_bytes(args: &CommonArgs) -> Result<Vec<u8>> {
    let fixture = load_fixture_from_disk(args)?;
    let archive = bench_archive_from_fixture(&fixture)?;
    Ok(rkyv::to_bytes::<_, 1_048_576>(&archive)?.as_ref().to_vec())
}

pub fn load_disk_fixture(args: &CommonArgs) -> Result<BenchFixture> {
    load_fixture_from_disk(args)
}

fn load_embedded_fixture() -> Result<BenchFixture> {
    fixture_from_archive_bytes(PREBUILT_ARCHIVE_BYTES)
}

fn load_fixture_from_disk(args: &CommonArgs) -> Result<BenchFixture> {
    let load_started = Instant::now();
    let shape = args.shape();
    ensure!(
        shape.languages >= 2,
        "benchmark fixtures require at least 2 languages"
    );
    ensure!(
        shape.files_per_language >= 1,
        "benchmark fixtures require at least 1 file per language"
    );
    let generated_workspace = if args.workspace_root.is_none() {
        let started = Instant::now();
        let temp = generate_workspace(shape);
        Some((temp, started.elapsed().as_secs_f64() * 1000.0))
    } else {
        None
    };
    let root = args.workspace_root.clone().unwrap_or_else(|| {
        generated_workspace
            .as_ref()
            .expect("generated workspace should exist")
            .0
            .path()
            .to_path_buf()
    });
    let index_started = Instant::now();
    let harness = DirectBenchHarness::load(root.clone())?;
    let index_load_ms = index_started.elapsed().as_secs_f64() * 1000.0;
    let origin_path = root.join("locales/lang00/file000.ftl");
    let translation_path = root.join("locales/lang01/file000.ftl");
    let target_started = Instant::now();
    let translation_source = std::fs::read_to_string(&translation_path)?;
    let origin_source = std::fs::read_to_string(&origin_path)?;
    let targets = BenchTargets {
        origin_path,
        translation_path,
        definition_position: position_inside(
            &translation_source,
            "bench-definition",
        ),
        references_position: position_inside(
            &origin_source,
            "bench-definition",
        ),
        hover_position: position_of_prefix(
            &translation_source,
            "Local hover value",
            "Local hover",
        ),
        completion_position: position_of_prefix(
            &translation_source,
            "bench-completion-sentinel = Local sentinel",
            "bench-completion",
        ),
        code_action_position: position_of_prefix(
            &translation_source,
            "{ $count ->",
            "{ $count",
        ),
        selector_key: "bench-select".to_string(),
    };
    let target_scan_ms = target_started.elapsed().as_secs_f64() * 1000.0;
    let workspace_generate_ms = generated_workspace.as_ref().map(|(_, ms)| *ms);
    let generated_temp = generated_workspace.map(|(temp, _)| temp);
    Ok(BenchFixture {
        _temp: generated_temp,
        root,
        harness,
        targets,
        load_stats: FixtureLoadStats {
            total_ms: load_started.elapsed().as_secs_f64() * 1000.0,
            index_load_ms,
            deserialize_ms: None,
            materialize_ms: None,
            workspace_generate_ms,
            target_scan_ms,
        },
    })
}

fn fixture_from_archive_bytes(bytes: &[u8]) -> Result<BenchFixture> {
    let load_started = Instant::now();
    let deserialize_started = Instant::now();
    let mut aligned = AlignedVec::with_capacity(bytes.len());
    aligned.extend_from_slice(bytes);
    let archive =
        unsafe { rkyv::from_bytes_unchecked::<BenchArchive>(&aligned)? };
    let deserialize_ms = deserialize_started.elapsed().as_secs_f64() * 1000.0;
    fixture_from_archive(archive, load_started, deserialize_ms)
}

fn fixture_from_archive(
    archive: BenchArchive,
    load_started: Instant,
    deserialize_ms: f64,
) -> Result<BenchFixture> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().to_path_buf();
    let materialize_started = Instant::now();
    materialize_archive(&root, &archive)?;
    let materialize_ms = materialize_started.elapsed().as_secs_f64() * 1000.0;
    let index_started = Instant::now();
    let workspace = WorkspaceConfig::load(root.clone())?;

    let mut files = FastMap::default();
    let mut by_identity = FastMap::default();
    for archived in archive.files {
        let indexed = indexed_file_from_archive(&root, archived);
        by_identity.insert(
            (
                indexed.file_match.mask_index,
                indexed.file_match.language.clone(),
                indexed.file_match.filepath.clone(),
            ),
            indexed.path.clone(),
        );
        files.insert(indexed.path.clone(), indexed);
    }
    let index_load_ms = index_started.elapsed().as_secs_f64() * 1000.0;

    let harness = DirectBenchHarness {
        workspace,
        index: WorkspaceIndex { files, by_identity },
        snapshot: FastMap::default(),
    };
    let target_started = Instant::now();
    let targets = BenchTargets {
        origin_path: root.join(archive.targets.origin_relative_path),
        translation_path: root.join(archive.targets.translation_relative_path),
        definition_position: archive.targets.definition_position.into_runtime(),
        references_position: archive.targets.references_position.into_runtime(),
        hover_position: archive.targets.hover_position.into_runtime(),
        completion_position: archive.targets.completion_position.into_runtime(),
        code_action_position: archive
            .targets
            .code_action_position
            .into_runtime(),
        selector_key: archive.targets.selector_key,
    };
    let target_scan_ms = target_started.elapsed().as_secs_f64() * 1000.0;

    Ok(BenchFixture {
        _temp: Some(temp),
        root: root.clone(),
        harness,
        targets,
        load_stats: FixtureLoadStats {
            total_ms: load_started.elapsed().as_secs_f64() * 1000.0,
            index_load_ms,
            deserialize_ms: Some(deserialize_ms),
            materialize_ms: Some(materialize_ms),
            workspace_generate_ms: None,
            target_scan_ms,
        },
    })
}

fn bench_archive_from_fixture(fixture: &BenchFixture) -> Result<BenchArchive> {
    let root = &fixture.root;
    let mut files = fixture
        .harness
        .index
        .files
        .values()
        .map(|file| archive_file_from_indexed(root, file))
        .collect::<Result<Vec<_>>>()?;
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));

    Ok(BenchArchive {
        config_toml: read_workspace_config_toml(root)?,
        files,
        targets: BenchTargetsArchive {
            origin_relative_path: relative_string(
                root,
                &fixture.targets.origin_path,
            )?,
            translation_relative_path: relative_string(
                root,
                &fixture.targets.translation_path,
            )?,
            definition_position: PositionArchive::from_runtime(
                fixture.targets.definition_position,
            ),
            references_position: PositionArchive::from_runtime(
                fixture.targets.references_position,
            ),
            hover_position: PositionArchive::from_runtime(
                fixture.targets.hover_position,
            ),
            completion_position: PositionArchive::from_runtime(
                fixture.targets.completion_position,
            ),
            code_action_position: PositionArchive::from_runtime(
                fixture.targets.code_action_position,
            ),
            selector_key: fixture.targets.selector_key.clone(),
        },
    })
}

fn read_workspace_config_toml(root: &Path) -> Result<String> {
    let config_path = ["fluent-lsp.toml", ".fluent-lsp.toml"]
        .into_iter()
        .map(|name| root.join(name))
        .find(|path| path.is_file())
        .ok_or_else(|| anyhow::anyhow!("missing workspace config file"))?;
    Ok(std::fs::read_to_string(config_path)?)
}

fn materialize_archive(root: &Path, archive: &BenchArchive) -> Result<()> {
    std::fs::write(root.join("fluent-lsp.toml"), &archive.config_toml)?;
    for file in &archive.files {
        let path = root.join(&file.relative_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, &file.source)?;
    }
    Ok(())
}

fn archive_file_from_indexed(
    root: &Path,
    file: &IndexedFile,
) -> Result<BenchArchiveFile> {
    Ok(BenchArchiveFile {
        relative_path: relative_string(root, &file.path)?,
        mask_index: file.file_match.mask_index,
        language: file.file_match.language.clone(),
        filepath: file.file_match.filepath.clone(),
        source: file.source.clone(),
        definitions: key_ranges_from_map(&file.definitions),
        selection_ranges: key_ranges_from_map(&file.selection_ranges),
        keys: sorted_keys(&file.keys),
        block_line_ranges: sorted_block_ranges(&file.block_line_ranges),
        ordered_message_keys: file.ordered_message_keys.clone(),
        message_attributes: sorted_message_attributes(&file.message_attributes),
        origin_message_templates: file
            .origin_message_templates
            .iter()
            .map(OriginMessageTemplateArchive::from_runtime)
            .collect(),
    })
}

fn indexed_file_from_archive(
    root: &Path,
    archived: BenchArchiveFile,
) -> IndexedFile {
    IndexedFile {
        path: root.join(&archived.relative_path),
        file_match: FileMatch {
            mask_index: archived.mask_index,
            language: archived.language,
            filepath: archived.filepath,
        },
        source: archived.source,
        definitions: archived
            .definitions
            .into_iter()
            .map(|entry| (entry.key, entry.range.into_runtime()))
            .collect(),
        selection_ranges: archived
            .selection_ranges
            .into_iter()
            .map(|entry| (entry.key, entry.range.into_runtime()))
            .collect(),
        keys: archived.keys.into_iter().collect::<FastSet<_>>(),
        block_line_ranges: archived
            .block_line_ranges
            .into_iter()
            .map(|entry| (entry.key, (entry.start_line, entry.end_line)))
            .collect(),
        source_renders: RefCell::new(FastMap::default()),
        ordered_message_keys: archived.ordered_message_keys,
        message_attributes: archived
            .message_attributes
            .into_iter()
            .map(|entry| (entry.message_key, entry.attributes))
            .collect(),
        origin_message_templates: archived
            .origin_message_templates
            .into_iter()
            .map(OriginMessageTemplateArchive::into_runtime)
            .collect(),
    }
}

fn relative_string(root: &Path, path: &Path) -> Result<String> {
    Ok(path
        .strip_prefix(root)?
        .to_string_lossy()
        .replace('\\', "/"))
}

fn key_ranges_from_map(map: &FastMap<String, Range>) -> Vec<KeyRangeArchive> {
    let mut entries = map
        .iter()
        .map(|(key, range)| KeyRangeArchive {
            key: key.clone(),
            range: RangeArchive::from_runtime(range),
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.key.cmp(&right.key));
    entries
}

fn sorted_keys(keys: &FastSet<String>) -> Vec<String> {
    let mut sorted = keys.iter().cloned().collect::<Vec<_>>();
    sorted.sort();
    sorted
}

fn sorted_block_ranges(
    ranges: &FastMap<String, (usize, usize)>,
) -> Vec<KeyBlockArchive> {
    let mut entries = ranges
        .iter()
        .map(|(key, (start_line, end_line))| KeyBlockArchive {
            key: key.clone(),
            start_line: *start_line,
            end_line: *end_line,
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.key.cmp(&right.key));
    entries
}

fn sorted_message_attributes(
    attributes: &FastMap<String, Vec<String>>,
) -> Vec<MessageAttributesArchive> {
    let mut entries = attributes
        .iter()
        .map(|(message_key, values)| MessageAttributesArchive {
            message_key: message_key.clone(),
            attributes: values.clone(),
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.message_key.cmp(&right.message_key));
    entries
}

pub fn base_result(
    name: &str,
    args: &CommonArgs,
    setup: &FixtureLoadStats,
    summary: Value,
    runs: Vec<Value>,
) -> Value {
    json!({
        "name": name,
        "shape": {
            "languages": args.languages,
            "files_per_language": args.files_per_language,
            "messages_per_file": args.messages_per_file,
        },
        "warmup_repeats": args.warmup_repeats,
        "repeats": args.repeats,
        "setup": {
            "total_ms": setup.total_ms,
            "index_load_ms": setup.index_load_ms,
            "deserialize_ms": setup.deserialize_ms,
            "materialize_ms": setup.materialize_ms,
            "workspace_generate_ms": setup.workspace_generate_ms,
            "target_scan_ms": setup.target_scan_ms,
        },
        "summary": summary,
        "runs": runs,
    })
}

pub fn print_result(result: &Value) {
    println!("{}", serde_json::to_string_pretty(result).unwrap());
}

pub fn measure_repeats(
    name: &str,
    repeats: usize,
    mut bench: impl FnMut() -> Result<Value>,
) -> Result<(Vec<Value>, Value)> {
    let mut runs = Vec::with_capacity(repeats);
    let mut last_summary = Value::Null;
    let started = Instant::now();
    let mut next_percent = 10usize;
    for iteration in 0..repeats {
        let run_started = Instant::now();
        let summary = bench()?;
        let elapsed_ms = run_started.elapsed().as_secs_f64() * 1000.0;
        black_box(&summary);
        last_summary = summary;
        let completed = iteration + 1;
        runs.push(json!({
            "iteration": iteration,
            "elapsed_ms": elapsed_ms,
        }));
        if should_report_progress(completed, repeats, next_percent) {
            eprintln!(
                "{name}: measured {completed}/{repeats} ({:>3}%) total_elapsed={:.3} ms last_run={:.6} ms",
                percent_complete(completed, repeats),
                started.elapsed().as_secs_f64() * 1000.0,
                elapsed_ms,
            );
            advance_percent(&mut next_percent);
        }
    }
    Ok((runs, last_summary))
}

pub fn print_fixture_load(name: &str, setup: &FixtureLoadStats) {
    eprintln!(
        "{name}: setup total={:.3} ms index_load={:.3} ms deserialize={} materialize={} workspace_generate={} target_scan={:.3} ms",
        setup.total_ms,
        setup.index_load_ms,
        format_optional_ms(setup.deserialize_ms),
        format_optional_ms(setup.materialize_ms),
        format_optional_ms(setup.workspace_generate_ms),
        setup.target_scan_ms,
    );
}

pub fn run_warmup(
    name: &str,
    warmup_repeats: usize,
    mut bench: impl FnMut() -> Result<()>,
) -> Result<()> {
    if warmup_repeats == 0 {
        return Ok(());
    }
    let started = Instant::now();
    let mut next_percent = 10usize;
    for iteration in 0..warmup_repeats {
        bench()?;
        let completed = iteration + 1;
        if should_report_progress(completed, warmup_repeats, next_percent) {
            eprintln!(
                "{name}: warmup {completed}/{warmup_repeats} ({:>3}%) elapsed={:.3} ms",
                percent_complete(completed, warmup_repeats),
                started.elapsed().as_secs_f64() * 1000.0,
            );
            advance_percent(&mut next_percent);
        }
    }
    Ok(())
}

pub fn origin_and_translation_sources(
    fixture: &BenchFixture,
) -> Result<(String, String)> {
    Ok((
        std::fs::read_to_string(&fixture.targets.origin_path)?,
        std::fs::read_to_string(&fixture.targets.translation_path)?,
    ))
}

fn parse_usize_flag(
    args: &mut impl Iterator<Item = String>,
    flag: &str,
) -> usize {
    args.next()
        .unwrap_or_else(|| panic!("missing value for {flag}"))
        .parse()
        .unwrap_or_else(|_| panic!("invalid {flag}"))
}

fn env_usize(name: &str) -> Option<usize> {
    std::env::var(name).ok()?.parse().ok()
}

fn should_report_progress(
    completed: usize,
    total: usize,
    next_percent: usize,
) -> bool {
    completed == total || percent_complete(completed, total) >= next_percent
}

fn percent_complete(completed: usize, total: usize) -> usize {
    if total == 0 {
        100
    } else {
        completed.saturating_mul(100) / total
    }
}

fn advance_percent(next_percent: &mut usize) {
    *next_percent += 10;
}

fn format_optional_ms(value: Option<f64>) -> String {
    value
        .map(|ms| format!("{ms:.3} ms"))
        .unwrap_or_else(|| "n/a".to_string())
}

fn generate_workspace(shape: Shape) -> TempDir {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("fluent-lsp.toml"),
        r#"origin_language = "lang00"
file_masks = ["locales/{lang}/{filepath}.ftl"]
"#,
    )
    .unwrap();

    for language in 0..shape.languages {
        for file in 0..shape.files_per_language {
            let relative =
                format!("locales/lang{language:02}/file{file:03}.ftl");
            let path = temp.path().join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let source = if file == 0 {
                build_primary_bench_file(language, shape.messages_per_file)
            } else {
                build_generated_file(language, file, shape.messages_per_file)
            };
            std::fs::write(path, source).unwrap();
        }
    }

    temp
}

fn build_primary_bench_file(
    language: usize,
    messages_per_file: usize,
) -> String {
    let language_tag = format!("lang{language:02}");
    let is_origin = language == 0;
    let mut source = if is_origin {
        format!(
            r#"### Bench file
## Bench group
bench-definition = Origin definition value

bench-hover = Origin hover value

bench-select = {{ $count ->
    [one] One origin item
   *[other] {{ $count }} origin items
}}

bench-completion-alpha = Origin alpha
bench-completion-beta = Origin beta
bench-completion-gamma = Origin gamma

bench-copy = Copy from origin

bench-missing-attr =
    .title = Origin title
    .body = Origin body

bench-number = Download on {{ $count }} origin devices

"#
        )
    } else {
        format!(
            r#"### Bench file
## Bench group
bench-definition = {language_tag} definition value

bench-hover = Local hover value

bench-select = {{ $count ->
    [one] One {language_tag} item
   *[other] {{ $count }} {language_tag} items
}}

bench-completion-sentinel = Local sentinel

bench-copy = # [LSP-COPY]

bench-missing-attr =
    .title = Local title

bench-number = Download on {{ $count }} local devices

"#
        )
    };

    for message in 0..messages_per_file {
        let key = format!("msg-000-{message:04}");
        if message % 17 == 0 {
            source.push_str(&format!("# Generated docs for {key}\n"));
        }
        if !is_origin && message % 23 == 0 {
            continue;
        }
        if message % 11 == 0 {
            source.push_str(&format!(
                "{key} = {{ $count ->\n    [one] {language_tag} one item\n   *[other] {language_tag} many items\n}}\n"
            ));
        } else if message % 7 == 0 {
            source.push_str(&format!(
                "{key} =\n    .label = {language_tag} label {message}\n    .tooltip = {language_tag} tooltip {message}\n"
            ));
        } else if message % 5 == 0 {
            source
                .push_str(&format!("-{key} = {language_tag} term {message}\n"));
        } else {
            source
                .push_str(&format!("{key} = {language_tag} value {message}\n"));
        }
        source.push('\n');
    }

    source
}

fn build_generated_file(
    language: usize,
    file: usize,
    messages_per_file: usize,
) -> String {
    let language_tag = format!("lang{language:02}");
    let is_origin = language == 0;
    let mut source = String::new();
    for message in 0..messages_per_file {
        let key = format!("msg-{file:03}-{message:04}");
        if message % 17 == 0 {
            source.push_str(&format!("# Generated docs for {key}\n"));
        }
        if !is_origin && message % 23 == 0 {
            continue;
        }
        if message % 11 == 0 {
            source.push_str(&format!(
                "{key} = {{ $count ->\n    [one] {language_tag} one item\n   *[other] {language_tag} many items\n}}\n"
            ));
        } else if message % 7 == 0 {
            source.push_str(&format!(
                "{key} =\n    .label = {language_tag} label {message}\n    .tooltip = {language_tag} tooltip {message}\n"
            ));
        } else if message % 5 == 0 {
            source
                .push_str(&format!("-{key} = {language_tag} term {message}\n"));
        } else {
            source
                .push_str(&format!("{key} = {language_tag} value {message}\n"));
        }
        source.push('\n');
    }
    source
}

fn position_of_prefix(source: &str, needle: &str, prefix: &str) -> Position {
    let byte_index = source
        .find(needle)
        .unwrap_or_else(|| panic!("missing benchmark needle `{needle}`"));
    let prefix_end = byte_index + prefix.len();
    byte_index_to_position(source, prefix_end)
}

fn position_inside(source: &str, needle: &str) -> Position {
    let byte_index = source
        .find(needle)
        .unwrap_or_else(|| panic!("missing benchmark needle `{needle}`"));
    let offset = byte_index + needle.len() / 2;
    byte_index_to_position(source, offset)
}

fn byte_index_to_position(source: &str, byte_index: usize) -> Position {
    let mut line = 0u32;
    let mut character = 0u32;
    for ch in source[..byte_index].chars() {
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += 1;
        }
    }
    Position::new(line, character)
}

pub fn file_uri(path: &Path) -> tower_lsp::ls_types::Uri {
    tower_lsp::ls_types::Uri::from_file_path(path)
        .unwrap_or_else(|| panic!("invalid file path {}", path.display()))
}

impl PositionArchive {
    fn from_runtime(position: Position) -> Self {
        Self {
            line: position.line,
            character: position.character,
        }
    }

    fn into_runtime(self) -> Position {
        Position::new(self.line, self.character)
    }
}

impl RangeArchive {
    fn from_runtime(range: &Range) -> Self {
        Self {
            start: PositionArchive::from_runtime(range.start),
            end: PositionArchive::from_runtime(range.end),
        }
    }

    fn into_runtime(self) -> Range {
        Range::new(self.start.into_runtime(), self.end.into_runtime())
    }
}

impl ByteRangeArchive {
    fn from_runtime(range: &ByteRange<usize>) -> Self {
        Self {
            start: range.start,
            end: range.end,
        }
    }

    fn into_runtime(self) -> ByteRange<usize> {
        self.start..self.end
    }
}

impl OriginMessageTemplateArchive {
    fn from_runtime(template: &OriginMessageTemplate) -> Self {
        Self {
            key: template.key.clone(),
            has_value: template.has_value,
            attributes: template
                .attributes
                .iter()
                .map(OriginAttributeTemplateArchive::from_runtime)
                .collect(),
            source_span: ByteRangeArchive::from_runtime(&template.source_span),
        }
    }

    fn into_runtime(self) -> OriginMessageTemplate {
        OriginMessageTemplate {
            key: self.key,
            has_value: self.has_value,
            attributes: self
                .attributes
                .into_iter()
                .map(OriginAttributeTemplateArchive::into_runtime)
                .collect(),
            source_span: self.source_span.into_runtime(),
        }
    }
}

impl OriginAttributeTemplateArchive {
    fn from_runtime(template: &OriginAttributeTemplate) -> Self {
        Self {
            key: template.key.clone(),
            source_span: ByteRangeArchive::from_runtime(&template.source_span),
        }
    }

    fn into_runtime(self) -> OriginAttributeTemplate {
        OriginAttributeTemplate {
            key: self.key,
            source_span: self.source_span.into_runtime(),
        }
    }
}
