# log_trace_timings

This scenario covers standard LSP trace timing output from `fluent-lsp`.

The smoke script starts `fluent-lsp` through Neovim's standard LSP client, installs a `$/logTrace` handler, enables verbose tracing with the standard `$/setTrace` notification, and issues `textDocument/definition` from the Spanish translation to the English origin file. It verifies that Neovim receives a `$/logTrace` notification whose message contains `operation=textDocument/definition` and whose verbose payload reports `hit=true`.

Assumptions:

- Neovim attaches its standard LSP client to the scenario workspace.
- The server binary is provided through `FLUENT_LSP_BIN` by the Rust smoke harness.
- Trace level changes are sent through standard `$/setTrace`; no editor-specific integration is required.

Exact source fixture used by the test:

- `workspace/locales/es/app.ftl`
- `workspace/locales/en/app.ftl`

This scenario is self-contained. The source fixtures and workspace config used by the smoke test live under this directory's `workspace/` tree.
