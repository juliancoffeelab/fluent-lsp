# Missing Spec Tests

This checklist tracks test gaps against [spec/reference.md](/Users/illiadenysenko/Workspace/lab/fluent-lsp/spec/reference.md) after removing items already covered by `tests/integration_lsp.rs`.

## Test Helper Policy

Do not add new test helpers or helper layers beyond the existing `LspProcess` struct helpers already present in `tests/integration_lsp.rs`.
Remove all existing ones, and make tests assert real data, not helpers.
Do not make tests looser to accommodate incorrect behavior.
If a test fails because the implementation does not match the spec, say that to the user and fix the code instead of weakening the test.

## Configuration

- [x] Add one end-to-end config-contract integration test that uses the documented `fluent-lsp.toml` shape exactly.
- [x] In that config-contract test, assert `origin_language = "en"` and `file_masks = ["locales/{lang}/{filepath}.ftl"]` drive counterpart resolution exactly as documented.
- [x] Add precedence coverage that client `workspace/didChangeConfiguration` applies core path-mapping settings when file config does not override them.
- [x] Add precedence coverage that file config overrides client-provided `origin_language` and `file_masks`.

## Capabilities

- [x] Add one exact `initialize` contract test that asserts the documented capability surface as a whole.
- [x] In that `initialize` contract test, assert exact `textDocumentSync`, `completionProvider`, `codeActionProvider`, `codeLensProvider`, and `executeCommandProvider` shapes.
- [x] In that `initialize` contract test, assert unsupported capabilities remain absent, including `rename`, `semanticTokens`, `inlayHint`, and `documentSymbol`.

## `textDocument/hover`

### Selector hover

- [x] Add an integration test that proves selector hover preserves unresolved term references inside the selected branch instead of substituting them.
- [x] Add an integration test that proves selector hover preserves unresolved non-selector inline Fluent references inside the selected branch, not just `$variable` references.

## `textDocument/codeAction`

### Missing-entry quick-fix titles

- [x] Add an integration test that asserts the spec title exactly: `Copy missing strings in file`.
- [x] Add an integration test that asserts the old file-wide copy title is absent: `Copy missing keys and attributes from source`.
- [x] Add an integration test that asserts the old stub title is absent: `Add missing keys and attributes from source`.
- [x] Add an integration test that asserts the spec title exactly: `Copy missing string \`hello\``.
- [x] Add an integration test that asserts the old single-message copy title is absent: `Copy \`hello\` from source`.
- [x] Add an integration test that asserts the spec title exactly: `Copy missing attribute \`download-action.tooltip\``.
- [x] Add an integration test that asserts the old single-attribute copy title is absent: `Copy missing attributes for \`download-action\` from source`.

### File-wide copy examples from the spec

- [x] Add an integration test for the spec example where a whole missing string is added by the file-wide copy action.
- [x] In that whole-missing-string test, assert exact `Before` -> `After` source comparison.
- [x] Add an integration test for the spec example where a whole missing message with attributes is added by the file-wide copy action.
- [x] In that whole-missing-message-with-attributes test, assert exact `Before` -> `After` source comparison.
- [x] In that whole-missing-message-with-attributes test, assert exact resulting source including one `# [LSP-COPY .<attribute>]` marker per copied attribute.
- [x] In that same test, assert unrelated existing local entries remain unchanged.

### Selector rewrite formatting

- [x] Add an integration test that proves selector rewrite preserves assignment spacing such as `= {`.

## `textDocument/codeLens`

- [ ] Add one selector-focused contract test that proves `Show all N selector combinations` is derived from the actual combination count, not just the current 6-combination fixture.
- [ ] Add one negative integration test that a message with selectors but no meaningful expansion opportunity does not produce a code lens.

## `workspace/executeCommand`

- [ ] Add a test that enforces the spec behavior for the selector-combinations command when `window/showDocument` is unavailable.
- [ ] Assert that the command does not fall back to `window/showMessage` for selector combinations.
- [ ] Assert that the command fails or otherwise follows the spec-only path instead of silently degrading.
- [ ] Add an exact contract test for `workspace/executeCommand` argument ordering and values: document URI first, Fluent key second.
- [ ] Add an invalid-params test for a non-string document URI argument.
- [ ] Add an invalid-params test for a non-string Fluent key argument.

### Selector combinations document

- [ ] Add an integration test that proves selector-combinations document output preserves unresolved term references in rendered combination blocks.
- [ ] Add an integration test that proves selector-combinations document output preserves unresolved non-selector inline Fluent references in rendered combination blocks.

## Diagnostics

### Unicode plural categories

- [ ] Add a unit test that explicitly proves English treats `[zero]` as an unsupported plural category and emits `` `zero` is not a supported plural category for `en` `` when unsupported-category diagnostics are enabled.
- [ ] Add a JSON-RPC integration test for the same English `[zero]` unsupported-category diagnostic path.
- [ ] Add end-to-end Unicode-plural coverage for a locale with extended category sets, so categories such as `two`, `few`, and `many` are exercised beyond the unit-only category-table assertions.

## Index And Refresh

- [ ] Add a background-refresh test that picks up on-disk content or mtime changes for an already indexed file, not just add/delete events.

## Trace Logging

- [ ] Add trace coverage for `textDocument/references`.
- [ ] Add trace coverage for `textDocument/hover`.
- [ ] Add trace coverage for `textDocument/completion`.
- [ ] Add trace coverage for `textDocument/codeAction`.
- [ ] Add trace coverage for `textDocument/codeLens`.
- [ ] Add trace coverage for `workspace/executeCommand`.
- [ ] Add verbose trace coverage for payload shapes that are currently untested, especially `count=<count>` and `command=<command> ok=true`.
