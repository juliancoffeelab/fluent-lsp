# code_action_copy_missing_keys

Whole-file source-copy code action with `# [LSP-COPY]` markers

This scenario covers the `Copy missing keys and attributes from source` quickfix on a translation file that is missing one top-level message and one attribute inside an existing message.

How the scenario is exercised:

- opens `workspace/locales/es/app.ftl`
- starts `fluent-lsp` and waits for `codeActionProvider`
- requests `textDocument/codeAction` on the translated `hello` entry
- selects `Copy missing keys and attributes from source`
- applies the returned workspace edit through Neovim’s standard LSP workspace-edit helper
- asserts the final buffer exactly matches the expected copied-source result
- verifies that a copied whole message keeps its top-level `# [LSP-COPY]` marker
- verifies that a copied missing attribute uses a message-level marker comment (`# [LSP-COPY .tooltip]`) above the owning message rather than an indented pseudo-comment inside the attribute list

Assumptions:

- the client uses standard LSP `textDocument/codeAction`
- the client can apply standard `WorkspaceEdit.changes` responses
- no custom editor integration is required

Source under test:

- `tests/nvim/code_action_copy_missing_keys/workspace/locales/en/app.ftl`
- `tests/nvim/code_action_copy_missing_keys/workspace/locales/es/app.ftl`
- `tests/nvim/code_action_copy_missing_keys/workspace/fluent-lsp.toml`
