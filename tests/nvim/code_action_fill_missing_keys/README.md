# code_action_fill_missing_keys

Whole-file empty-stub code action for incomplete translation files

This scenario covers the `Add missing keys and attributes from source` quickfix on a translation file that is missing one top-level message and one attribute inside an existing message.

How the scenario is exercised:

- opens `workspace/locales/es/app.ftl`
- starts `fluent-lsp` and waits for `codeActionProvider`
- requests `textDocument/codeAction` on the translated `hello` entry
- selects `Add missing keys and attributes from source`
- applies the returned workspace edit through Neovim’s standard LSP workspace-edit helper
- asserts the final buffer exactly matches the expected empty-stub result

Assumptions:

- the client uses standard LSP `textDocument/codeAction`
- the client can apply standard `WorkspaceEdit.changes` responses
- no custom editor integration is required

Source under test:

- `tests/nvim/code_action_fill_missing_keys/workspace/locales/en/app.ftl`
- `tests/nvim/code_action_fill_missing_keys/workspace/locales/es/app.ftl`
- `tests/nvim/code_action_fill_missing_keys/workspace/fluent-lsp.toml`
