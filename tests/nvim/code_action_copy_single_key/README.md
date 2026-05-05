# code_action_copy_single_key

Targeted source-copy quickfixes for one selected message

This scenario covers the fine-grained source-copy quickfixes that operate only on the message under the cursor:

- `Copy \`hello\` from source` replaces a local empty stub with the origin entry and a top-level marker
- `Copy missing attributes for \`download-action\` from source` inserts only the missing attributes for the selected message and adds a message-level attribute marker comment above that message

How the scenario is exercised:

- opens `workspace/locales/es/app.ftl`
- starts `fluent-lsp` and waits for `codeActionProvider`
- requests `textDocument/codeAction` on `hello = { "" }`
- applies `Copy \`hello\` from source` and records the resulting buffer
- restores the original scenario-local fixture content
- requests `textDocument/codeAction` on `download-action =`
- applies `Copy missing attributes for \`download-action\` from source` and records the resulting buffer
- requests standard `textDocument/hover` on the copied `.tooltip` key and verifies the machine marker does not surface as translator-comment hover content

Assumptions:

- the client uses standard LSP `textDocument/codeAction`
- the client can apply standard `WorkspaceEdit.changes` responses
- buffer edits propagate to the attached client without custom editor integration

Source under test:

- `tests/nvim/code_action_copy_single_key/workspace/locales/en/app.ftl`
- `tests/nvim/code_action_copy_single_key/workspace/locales/es/app.ftl`
- `tests/nvim/code_action_copy_single_key/workspace/fluent-lsp.toml`
