# code_action_generate_selector

Number-selector generation code action

This scenario covers the first `textDocument/codeAction` slice for numeric selector generation.
It verifies three client-visible behaviors:

- requesting a code action on a message key with a direct variable placeable offers generated selector output
- requesting a code action on the variable itself uses the variable-targeted title path
- when the client advertises `workspace.workspaceEdit.snippetEditSupport`, the edit is returned as a snippet-capable `documentChanges` entry
- `fluent-lsp.toml` `selector_style` overrides client settings after the server is restarted

How the scenario is exercised:

- opens `workspace/locales/es/app.ftl`
- requests `textDocument/codeAction` on `coins-line`
- asserts the default generated action is `prefix`
- requests `textDocument/codeAction` on `{ $coins }`
- asserts the variable-targeted title path is used
- rewrites `workspace/fluent-lsp.toml` to `selector_style = "whole"`
- restarts the client, sends client settings preferring `prefix`, and asserts the file config still forces `whole`

Assumptions:

- Neovim advertises `workspace.workspaceEdit.documentChanges = true`
- Neovim advertises `workspace.workspaceEdit.snippetEditSupport = true`
- the server therefore returns `SnippetTextEdit` entries inside `WorkspaceEdit.documentChanges`

Source under test:

- `tests/nvim/code_action_generate_selector/workspace/locales/es/app.ftl`
- `tests/nvim/code_action_generate_selector/workspace/fluent-lsp.toml`
