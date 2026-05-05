# fluent-lsp

Minimal Fluent-focused language server.

Current behavior:

- Reads `fluent-lsp.toml` or `.fluent-lsp.toml` from the workspace root.
- Tracks localized `.ftl` files through templated `file_masks` that include `{lang}` and `{filepath}`, so nested locale trees resolve back to the matching origin-language file.
- Builds a global Fluent workspace index after initialization and reports standard LSP work-done progress when the client supports it.
- Keeps unsaved editor buffers as index overlays and invalidates one file entry at a time for standard `didOpen`, `didChange`, `didSave`, and `didClose` notifications.
- Resolves `textDocument/definition` by extracting the message, term, or attribute under the cursor from a translated Fluent file and jumping to the matching origin-language `.ftl` file.
- Resolves `textDocument/hover` as a formatted Fluent preview for the hovered message or attribute. Translation value hover renders the source preview first, then `---`, then the current-language preview, while key and origin-language hover stay single-section.
- Publishes standard-LSP CodeLens entries for selector-bearing Fluent values, and uses `workspace/executeCommand` plus `window/showDocument` to open a temp Markdown document with the full combination list in the current document language.
- Resolves `textDocument/references` from an origin-language `.ftl` file into matching entries across translated Fluent files with the same relative locale-tree path.
- Publishes a standard diagnostic when a translation file has no origin-language counterpart.

Config shape:

```toml
origin_language = "en"
file_masks = ["locales/{lang}/{filepath}.ftl"]
```

Selector-combination CodeLens output is always enabled for selector-bearing Fluent values. When the client does not support `window/showDocument`, the fallback `window/showMessage` path truncates the rendered combination list to 10 items.

Run it over stdio:

```bash
cargo run
```

Validation:

```bash
cargo test
```

Benchmarks:

```bash
cargo build --bins
FLUENT_LSP_BENCH_MESSAGES=10 target/debug/fluent-lsp-bench benchmarks/results/indexed-2026-05-05.json target/debug/fluent-lsp
```

The benchmark harness defaults the synthetic workspace to 31 languages, 50 files per language, and 2000 messages per file. Set `FLUENT_LSP_BENCH_MESSAGES` for faster local validation runs. Current bounded results are recorded in [STAGES.md](/home/codex/workspace/fluent-lsp/STAGES.md) and [benchmarks/results/indexed-2026-05-05.json](/home/codex/workspace/fluent-lsp/benchmarks/results/indexed-2026-05-05.json).

Try it locally from this repo:

- Open the repository root as your editor workspace.
- The top-level [fluent-lsp.toml](/home/codex/workspace/fluent-lsp/fluent-lsp.toml) points at the repo-local [example/](/home/codex/workspace/fluent-lsp/example/README.md) tree.
- Use the files under `example/locales/` to try definition, formatted hover previews with separator-delimited source/current blocks, locale-tree resolution, and CodeLens-driven selector expansion.

Local fork:

- `third_party/fluent-syntax/` is a path-patched local fork of `fluent-syntax 0.12.0`.
- The intent is to have a clean place to add AST/source span support for editor features without waiting on upstream merges.

The test suite includes:

- direct LSP JSON-RPC integration coverage
- dedicated headless Neovim smoke scenarios under `tests/nvim/`
