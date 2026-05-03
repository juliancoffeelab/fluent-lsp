# code_action_generate_selector

Number-selector generation code action

This scenario covers numeric selector generation and selector-rewrite code actions.
It verifies these client-visible behaviors:

- requesting a code action on a message key offers all applicable selector styles
- message-level generation uses snippet placeholders when the client supports snippet text edits
- requesting a code action on the variable itself offers variable-targeted prefix/whole/suffix actions
- plain messages without an existing variable only offer whole-form generation
- ambiguous message keys with multiple candidate variables do not advertise generation
- root entries that already branch but have no clear local anchor do not advertise generation
- attributes participate in generation
- variables nested inside function arguments participate using the enclosing placeable as the split anchor, so `NUMBER($downloads)` can generate prefix/whole/suffix forms without splitting the call itself
- when the cursor is on a function call rather than on the nested variable, the generated selector can target the function expression itself
- punctuation tails stay attached in generated variant bodies instead of turning `.` into ` .`
- whole-form selectors can be rewritten into prefix and suffix forms when the shared structure is lossless
- prefix-form selectors can be rewritten into whole form
- suffix-form selectors can be rewritten into whole form and collapsed into prefix form
- whole selectors that already look suffix-like with no outer prefix text only advertise the distinct prefix collapse
- nested selector branches are rewritten within their local variant pattern instead of breaking the outer select
- rewrite actions also work inside attribute values
- rewrite edits for attribute values do not consume surrounding comments or the attribute key itself
- selectors in the middle of a larger pattern can still rewrite their containing pattern when the cursor is on that selector
- the `install-hint` `$count` selector can collapse into a local suffix form without disturbing the earlier `$gender` selector
- the `install-hint` `$gender` selector can rewrite the whole containing pattern, preserving the nested `$count` selector inside each branch
- `fluent-lsp.toml` `selector_style` overrides client settings after the server is restarted

How the scenario is exercised:

- opens `workspace/locales/es/app.ftl`
- requests `textDocument/codeAction` on `coins-line`
- asserts the message-level `prefix` action is a snippet edit
- requests `textDocument/codeAction` on `{ $coins }`
- asserts variable-targeted prefix/whole/suffix actions are all present
- requests `textDocument/codeAction` on `plain-count`
- asserts only whole-form generation is offered
- requests `textDocument/codeAction` on `download-count.tooltip`
- asserts attribute generation is available
- requests `textDocument/codeAction` on `formatted-download`, on `$downloads`, and on `NUMBER($downloads)`
- asserts a variable inside `NUMBER(...)` uses the enclosing placeable as the split anchor
- asserts a function call under the cursor can be used as the selector expression itself
- requests `textDocument/codeAction` on `deep-download`
- asserts nested function calls like `WRAP(NUMBER($downloads))` can also be selected as the selector expression
- requests `textDocument/codeAction` on `coins-period`
- asserts generated variant text keeps `.` attached without an extra leading space
- requests `textDocument/codeAction` on `whole-coins`
- asserts `Convert selector to prefix form` and `Convert selector to suffix form` rewrite actions are available with the expected edits
- requests `textDocument/codeAction` on `prefix-coins`
- asserts `Convert selector to whole form` rewrites the generated prefix structure back into a whole selector
- requests `textDocument/codeAction` on `suffix-coins`
- asserts both `Convert selector to whole form` and `Convert selector to prefix form` are available
- requests `textDocument/codeAction` on `bare-suffix-coins`
- asserts only `Convert selector to prefix form` is available because the original text already serves as the zero-prefix suffix/whole shape
- requests `textDocument/codeAction` inside `nested-whole-coins`
- asserts rewrite targeting applies to the nested selector pattern inside the selected variant, not only to root-level messages
- requests `textDocument/codeAction` on `commented-download.tooltip`
- asserts `Convert selector to prefix form` and `Convert selector to suffix form` are available for an attribute value
- asserts the returned edit range starts at the attribute value itself, not at the preceding comment or attribute key
- requests `textDocument/codeAction` on the `$count` selector inside `install-hint`
- asserts the server offers a local `suffix` rewrite on the containing pattern while preserving the earlier `$gender` selector
- requests `textDocument/codeAction` on the `$gender` selector inside `install-hint`
- asserts the server offers a local `whole` rewrite that nests the existing `$count` selector inside each generated branch
- requests `textDocument/codeAction` inside `nested-coins`
- asserts the nested rewrite stays within the selected branch pattern
- rewrites `workspace/fluent-lsp.toml` to `selector_style = "whole"`
- restarts the client, sends client settings preferring `prefix`, and asserts the file config still forces `whole`

Assumptions:

- Neovim advertises `workspace.workspaceEdit.documentChanges = true`
- Neovim advertises `workspace.workspaceEdit.snippetEditSupport = true`
- the server therefore returns `SnippetTextEdit` entries inside `WorkspaceEdit.documentChanges`

Source under test:

- `tests/nvim/code_action_generate_selector/workspace/locales/es/app.ftl`
- `tests/nvim/code_action_generate_selector/workspace/locales/en/app.ftl`
- `tests/nvim/code_action_generate_selector/workspace/fluent-lsp.toml`
