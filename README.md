# fluent-lsp

Minimal Fluent-focused language server.

Current behavior:

- Reads `fluent-lsp.toml` or `.fluent-lsp.toml` from the workspace root.
- Restricts lookup to files matched by `file_masks`.
- Resolves `textDocument/definition` by extracting the key under the cursor and jumping to the matching message in the configured English `.ftl` file.

Config shape:

```toml
english_file = "locales/en/app.ftl"
file_masks = ["src/**/*.ts", "src/**/*.tsx"]
```

Run it over stdio:

```bash
cargo run
```

Validation:

```bash
cargo test
```

The test suite includes:

- direct LSP JSON-RPC integration coverage
- a headless Neovim smoke test with a mock config under `tests/nvim/`
