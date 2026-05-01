# fluent-lsp

Minimal Fluent-focused language server.

Current behavior:

- Reads `fluent-lsp.toml` or `.fluent-lsp.toml` from the workspace root.
- Restricts lookup to translated `.ftl` files matched by `file_masks`.
- Resolves `textDocument/definition` by extracting the message, term, or attribute under the cursor from a translated Fluent file and jumping to the matching entry in the configured English `.ftl` file.
- Resolves `textDocument/hover` on translated Fluent entries by showing the matching English entry, including its attached comments.
- Optionally appends static selector combinations to hover when `hover_selector_combinations = true`, capped by `hover_selector_combinations_limit`.
- Resolves `textDocument/references` from the configured English `.ftl` file into matching entries across translated Fluent files.

Config shape:

```toml
english_file = "locales/en/app.ftl"
file_masks = ["locales/**/*.ftl"]
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

Local fork:

- `third_party/fluent-syntax/` is a path-patched local fork of `fluent-syntax 0.12.0`.
- The intent is to have a clean place to add AST/source span support for editor features without waiting on upstream merges.

The test suite includes:

- direct LSP JSON-RPC integration coverage
- a headless Neovim smoke test with a mock config under `tests/nvim/`
