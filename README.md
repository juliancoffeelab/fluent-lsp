# fluent-lsp

Minimal Fluent-focused language server.

Current behavior:

- Reads `fluent-lsp.toml` or `.fluent-lsp.toml` from the workspace root.
- Tracks localized `.ftl` files through templated `file_masks` that include `{lang}` and `{filepath}`, so nested locale trees resolve back to the matching origin-language file.
- Resolves `textDocument/definition` by extracting the message, term, or attribute under the cursor from a translated Fluent file and jumping to the matching origin-language `.ftl` file.
- Resolves `textDocument/hover` on translated Fluent entries by showing the matching origin entry with comments rendered separately from the Fluent block for subtle syntax distinction.
- Resolves `textDocument/hover` on origin-language Fluent entries with the same metadata and selector expansion shown on translated hovers.
- Optionally appends static selector combinations to hover when `hover_selector_combinations = true`, capped by `hover_selector_combinations_limit`.
- Publishes standard-LSP CodeLens entries for Fluent values with selector expansions, and uses `workspace/executeCommand` plus `window/showDocument` to open a temp Markdown document with the full combination list on demand.
- Resolves `textDocument/references` from an origin-language `.ftl` file into matching entries across translated Fluent files with the same relative locale-tree path.

Config shape:

```toml
origin_language = "en"
file_masks = ["locales/{lang}/{filepath}.ftl"]
hover_selector_combinations = true
hover_selector_combinations_limit = 32
```

Run it over stdio:

```bash
cargo run
```

Validation:

```bash
cargo test
```

Try it locally from this repo:

- Open the repository root as your editor workspace.
- The top-level [fluent-lsp.toml](/home/codex/workspace/fluent-lsp/fluent-lsp.toml) points at the repo-local [example/](/home/codex/workspace/fluent-lsp/example/README.md) tree.
- Use the files under `example/locales/` to try definition, hover, references, locale-tree resolution, and CodeLens-driven selector expansion.

Local fork:

- `third_party/fluent-syntax/` is a path-patched local fork of `fluent-syntax 0.12.0`.
- The intent is to have a clean place to add AST/source span support for editor features without waiting on upstream merges.

The test suite includes:

- direct LSP JSON-RPC integration coverage
- a headless Neovim smoke test with a mock config under `tests/nvim/`
