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
- [ ] Add precedence coverage that file config overrides client-provided plural-diagnostic settings, not just path mapping and selector-style settings.

## Capabilities

- [x] Add one exact `initialize` contract test that asserts the documented capability surface as a whole.
- [x] In that `initialize` contract test, assert exact `textDocumentSync`, `completionProvider`, `codeActionProvider`, `codeLensProvider`, and `executeCommandProvider` shapes.
- [x] In that `initialize` contract test, assert unsupported capabilities remain absent, including `rename`, `semanticTokens`, `inlayHint`, and `documentSymbol`.

## `textDocument/definition`

- [ ] Add an integration test that proves `Go to Definition` from an origin-file term returns no location, not just the top-level key case.
- [ ] Add an integration test that proves `Go to Definition` from an origin-file attribute returns no location, not just the top-level key case.
- [ ] Add an integration test that proves a translation term with no origin counterpart returns no location.
- [ ] Add an integration test that proves a nested translation file with no origin counterpart returns no location, not just a top-level orphan file.
- [ ] Add an integration test that proves a nested translation attribute with no origin counterpart returns no location, not just the top-level attribute case.

## `textDocument/references`

- [ ] Add an integration test that proves `Find References` from a translation-file term returns no result, not just the top-level key case.
- [ ] Add an integration test that proves `Find References` from a translation-file attribute returns no result, not just the top-level key case.
- [ ] Add a dirty-overlay references test for attribute ranges, not just the top-level `download-action` message case.
- [ ] Add a dirty-close references test for attribute ranges, not just the top-level `download-action` message case.
- [ ] Add a background-refresh references test for nested translation files being added and removed, not just top-level `app.ftl`.

## `textDocument/hover`

### File and group comments on key hover

- [ ] Add an integration test for key hover that proves a file-level comment block and a group-level comment block keep a newline between them when they are adjacent in source.
- [ ] Add an integration test for key hover that proves a file-level comment block and a group-level comment block keep a newline between them when they are not adjacent in source.
- [ ] Add an integration test for key hover that proves a group-level comment block and a message-level comment block keep a newline between them when they are adjacent in source.
- [ ] Add an integration test for key hover that proves a group-level comment block and a message-level comment block keep a newline between them when they are not adjacent in source.
- [ ] Add an integration test for key hover that proves file-level comments apply to every key they scope over, not just the first key after the comments.
- [ ] Add an integration test for key hover that proves group-level comments apply to every key they scope over, not just the first key after the comments.

### Selector hover

- [x] Add an integration test that proves selector hover preserves unresolved term references inside the selected branch instead of substituting them.
- [x] Add an integration test that proves selector hover preserves unresolved non-selector inline Fluent references inside the selected branch, not just `$variable` references.

## `textDocument/completion`

### Empty result shape

- [ ] Tighten the top-level origin-file completion test so it asserts the exact empty completion payload instead of only asserting that the extracted label list is empty.
- [ ] Tighten the nested origin-file completion test so it asserts the exact empty completion payload instead of only asserting that the extracted label list is empty.
- [ ] Tighten the unmatched-prefix completion test so it asserts the exact empty completion payload instead of only asserting that the extracted label list is empty.
- [ ] Tighten the missing-origin-counterpart completion test so it asserts the exact empty completion payload instead of only asserting that the extracted label list is empty.

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

### Exact action sets

- [ ] Tighten the representative generation test for a variable occurrence so it asserts the exact returned rewrite-title set instead of only checking that three expected titles are present.
- [ ] Tighten the attribute-generation test so it asserts the exact returned rewrite-title set instead of only checking that one expected title is present.
- [ ] Tighten the whole-selector rewrite test so it asserts the exact returned rewrite-title set instead of only checking that the prefix and suffix titles are present.
- [ ] Tighten the suffix-selector rewrite test so it asserts the exact returned rewrite-title set instead of only checking that the whole and prefix titles are present.
- [ ] Tighten the nested-selector rewrite test so it asserts the exact returned rewrite-title set instead of only checking that one additional title is present.

## `textDocument/codeLens`

- [ ] Add one selector-focused contract test that proves `Show all N selector combinations` is derived from the actual combination count, not just the current 6-combination fixture.
- [ ] Add one negative integration test that a message with selectors but no meaningful expansion opportunity does not produce a code lens.
- [ ] Add one exact contract test for the full returned code-lens set on the main fixture file instead of only checking `items.len() >= 3` and one `install-hint` lens.
- [ ] Add one exact contract test for the code-lens command arguments array so the emitted `workspace/executeCommand` call shape is verified before execution.

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

### Default-off behavior

- [ ] Replace the current `diagnostics_are_absent_by_default` smoke test with an exact save-triggered assertion that disabled plural/category diagnostics publish no warnings by default.

### Unicode plural categories

- [ ] Add a unit test that explicitly proves English treats `[zero]` as an unsupported plural category and emits `` `zero` is not a supported plural category for `en` `` when unsupported-category diagnostics are enabled.
- [ ] Add a JSON-RPC integration test for the same English `[zero]` unsupported-category diagnostic path.
- [ ] Add end-to-end Unicode-plural coverage for a locale with extended category sets, so categories such as `two`, `few`, and `many` are exercised beyond the unit-only category-table assertions.
- [ ] Add exact severity and range assertions for each Latvian missing-category diagnostic, not just the unsupported `few` diagnostic.
- [ ] Add exact severity and range assertions for the Ukrainian missing-category diagnostic.
- [ ] Add a diagnostic test that proves the origin file may use a different plural-category set without producing translation-side category warnings.

### Selector style mismatch

- [ ] Add exact severity and range assertions for each translation-file selector-style mismatch diagnostic instead of checking only the sorted messages.
- [ ] Add exact severity and range assertions for the local-origin selector-style mismatch diagnostic instead of checking only its message.

## Index And Refresh

- [ ] Add a background-refresh test that picks up on-disk content or mtime changes for an already indexed file, not just add/delete events.
- [ ] Add a background-refresh definition test for an origin counterpart file appearing on disk after initialization.
- [ ] Add a background-refresh definition test for an origin counterpart file disappearing on disk after initialization.
- [ ] Add a background-refresh hover test that picks up changed origin comment or preview content from disk without restarting the server.
- [ ] Add a background-refresh completion test that picks up changed origin completion entries from disk without restarting the server.
- [ ] Add a background-refresh missing-origin-diagnostics test that picks up on-disk counterpart changes for an already indexed translation file without reopening it.

## Trace Logging

- [ ] Add trace coverage for `textDocument/references`.
- [ ] Add trace coverage for `textDocument/hover`.
- [ ] Add trace coverage for `textDocument/completion`.
- [ ] Add trace coverage for `textDocument/codeAction`.
- [ ] Add trace coverage for `textDocument/codeLens`.
- [ ] Add trace coverage for `workspace/executeCommand`.
- [ ] Add verbose trace coverage that `textDocument/references` reports `count=<count>`.
- [ ] Add verbose trace coverage that `textDocument/codeAction` reports `count=<count>`.
- [ ] Add verbose trace coverage that `textDocument/codeLens` reports `count=<count>`.
- [ ] Add verbose trace coverage that `workspace/executeCommand` reports `command=<command> ok=true`.

## Path-By-Path Coverage Matrix

This section breaks broad request behavior into concrete request paths.
Each path below needs at least ten exact tests, not one merged happy-path test.

## `textDocument/definition` Path Matrix

### Path: translation top-level key -> origin top-level key

- [ ] Add a test where the translation key is the first non-empty line in the file.
- [ ] Add a test where the translation key is the last non-empty line in the file.
- [ ] Add a test where the cursor is on the first character of the key name.
- [ ] Add a test where the cursor is on the middle character of the key name.
- [ ] Add a test where the cursor is on the last character of the key name.
- [ ] Add a test where the cursor is on leading whitespace on the key line and returns no location.
- [ ] Add a test where the cursor is on the `=` sign and returns no location.
- [ ] Add a test where the translation key has a preceding file-level comment and still resolves the exact origin range.
- [ ] Add a test where the translation key has a preceding group-level comment and still resolves the exact origin range.
- [ ] Add a test where the origin counterpart key is the last entry in the origin file and still resolves the exact start range.

### Path: translation term -> origin term

- [ ] Add a test where the term is the first non-empty line in the file.
- [ ] Add a test where the term is the last non-empty line in the file.
- [ ] Add a test where the cursor is on the leading `-` character.
- [ ] Add a test where the cursor is on the first alphanumeric character after `-`.
- [ ] Add a test where the cursor is on the last character of the term name.
- [ ] Add a test where the cursor is on leading whitespace on the term line and returns no location.
- [ ] Add a test where the cursor is on the `=` sign and returns no location.
- [ ] Add a test where the translation term has a preceding file-level comment and still resolves correctly.
- [ ] Add a test where the translation term has a preceding group-level comment and still resolves correctly.
- [ ] Add a test where the origin counterpart term is followed by another term with a similar prefix and the exact target still resolves.

### Path: translation attribute key -> origin attribute key

- [ ] Add a top-level attribute test where the attribute is the first attribute on the message.
- [ ] Add a top-level attribute test where the attribute is the last attribute on the message.
- [ ] Add a nested-file attribute test where the attribute is the first attribute on the message.
- [ ] Add a nested-file attribute test where the attribute is the last attribute on the message.
- [ ] Add a test where the cursor is on the leading `.` of the attribute key.
- [ ] Add a test where the cursor is on the first character after `.`.
- [ ] Add a test where the cursor is on the last character of the attribute name.
- [ ] Add a test where the cursor is on indentation before the attribute and returns no location.
- [ ] Add a test where the cursor is on the attribute `=` sign and returns no location.
- [ ] Add a test where the message has multiple attributes and the exact matching origin attribute range is returned.

### Path: no-location and request-error outcomes

- [ ] Add a test where `Go to Definition` from an origin top-level key returns no location.
- [ ] Add a test where `Go to Definition` from an origin term returns no location.
- [ ] Add a test where `Go to Definition` from an origin attribute returns no location.
- [ ] Add a test where a translation top-level key has no origin counterpart and returns no location.
- [ ] Add a test where a translation term has no origin counterpart and returns no location.
- [ ] Add a test where a translation attribute has no origin counterpart and returns no location.
- [ ] Add a test where a top-level translation file has no origin counterpart file and returns no location.
- [ ] Add a test where a nested translation file has no origin counterpart file and returns no location.
- [ ] Add a test where a non-file URI request returns the exact invalid-params error.
- [ ] Add a test where a file outside the configured workspace returns the exact invalid-params error.

## `textDocument/references` Path Matrix

### Path: origin top-level key -> translation top-level key references

- [ ] Add a test where the origin key is the first non-empty line in the file.
- [ ] Add a test where the origin key is the last non-empty line in the file.
- [ ] Add a test where the cursor is on the first character of the key name.
- [ ] Add a test where the cursor is on the middle character of the key name.
- [ ] Add a test where the cursor is on the last character of the key name.
- [ ] Add a test where two translation files contain the key and both exact ranges are returned in stable order.
- [ ] Add a test where one translation file is missing the key and only matching files are returned.
- [ ] Add a test where `includeDeclaration = true` is requested and the exact behavior is asserted.
- [ ] Add a test where leading file comments in translation files do not shift the asserted reference ranges incorrectly.
- [ ] Add a test where the referenced translation key is the last entry in the translation file.

### Path: origin term -> translation term references

- [ ] Add a test where the origin term is the first non-empty line in the file.
- [ ] Add a test where the origin term is the last non-empty line in the file.
- [ ] Add a test where the cursor is on the leading `-` character.
- [ ] Add a test where the cursor is on the first alphanumeric character after `-`.
- [ ] Add a test where the cursor is on the last character of the term name.
- [ ] Add a test where two translation files contain the term and both exact ranges are returned.
- [ ] Add a test where one translation file contains a similarly prefixed term and it is not returned.
- [ ] Add a test where the translation term is preceded by a file-level comment and the exact range still points at the term.
- [ ] Add a test where the translation term is preceded by a group-level comment and the exact range still points at the term.
- [ ] Add a test where the translation term is the last entry in the translation file.

### Path: origin attribute -> translation attribute references

- [ ] Add a top-level attribute test where the attribute is the first attribute on the message.
- [ ] Add a top-level attribute test where the attribute is the last attribute on the message.
- [ ] Add a nested-file attribute test where the attribute is the first attribute on the message.
- [ ] Add a nested-file attribute test where the attribute is the last attribute on the message.
- [ ] Add a test where the cursor is on the leading `.` of the origin attribute.
- [ ] Add a test where the cursor is on the first character after `.`.
- [ ] Add a test where the cursor is on the last character of the attribute name.
- [ ] Add a test where multiple translation files contribute the same attribute and all exact ranges are returned.
- [ ] Add a test where the translation message exists but the selected attribute does not and that file is excluded.
- [ ] Add a test where the selected attribute exists in nested translation files and the exact nested ranges are returned.

### Path: no-match and caller-side no-result outcomes

- [ ] Add a test where an origin top-level key has no translation matches and returns an exact empty array.
- [ ] Add a test where an origin term has no translation matches and returns an exact empty array.
- [ ] Add a test where an origin attribute has no translation matches and returns an exact empty array.
- [ ] Add a test where `Find References` from a translation top-level key returns no result.
- [ ] Add a test where `Find References` from a translation term returns no result.
- [ ] Add a test where `Find References` from a translation attribute returns no result.
- [ ] Add a test where the cursor is on leading whitespace of an origin key line and returns no result.
- [ ] Add a test where the cursor is on the `=` sign of an origin key line and returns no result.
- [ ] Add a test where a non-file URI request returns the exact invalid-params error.
- [ ] Add a test where a file outside the configured workspace returns the exact invalid-params error.

### Path: indexed references with dirty overlays and background refresh

- [ ] Add a dirty-overlay test where opening a translation file introduces a new top-level key reference immediately.
- [ ] Add a dirty-overlay test where opening a translation file introduces a new term reference immediately.
- [ ] Add a dirty-overlay test where opening a translation file introduces a new attribute reference immediately.
- [ ] Add a dirty-change test where editing an open translation file removes a top-level key reference immediately.
- [ ] Add a dirty-change test where editing an open translation file removes a term reference immediately.
- [ ] Add a dirty-change test where editing an open translation file removes an attribute reference immediately.
- [ ] Add a `didClose` test where a top-level key reference reverts to disk state immediately.
- [ ] Add a `didClose` test where a term reference reverts to disk state immediately.
- [ ] Add a `didClose` test where an attribute reference reverts to disk state immediately.
- [ ] Add a background-refresh test where a nested translation file is added on disk and begins contributing references without restart.

## `textDocument/hover` Path Matrix

### Path: top-level key hover

- [ ] Add a test where origin-only top-level key comments render as one exact block.
- [ ] Add a test where local-only top-level key comments render as one exact block.
- [ ] Add a test where origin and local top-level key comments render as two exact blocks in order.
- [ ] Add a test where file-level and group-level comments keep a newline between them when adjacent.
- [ ] Add a test where file-level and group-level comments keep a newline between them when separated in source.
- [ ] Add a test where a file-level comment applies to the first key in its scope.
- [ ] Add a test where a file-level comment applies to a later key in its scope.
- [ ] Add a test where hovering empty space on a key line returns no hover.
- [ ] Add a test where hovering the `=` sign on a key line returns no hover.
- [ ] Add a test where a top-level key with no comments on either side returns no hover and does not degrade into body preview.

### Path: top-level message body hover

- [ ] Add a translation-file body hover test where origin and local previews both render exactly.
- [ ] Add an origin-file body hover test where only the origin preview block is returned.
- [ ] Add a local-only body hover test where the origin message is missing and one local block is returned.
- [ ] Add an empty-local-value body hover test where the local block is exactly `<empty>`.
- [ ] Add a test where an unresolved term reference is preserved in the origin preview block.
- [ ] Add a test where an unresolved term reference is preserved in the local preview block.
- [ ] Add a test where hovering the first character of the body returns the same preview as hovering the middle.
- [ ] Add a test where hovering the last character of the body returns the same preview as hovering the middle.
- [ ] Add a test where hovering whitespace inside the body returns the same preview block set.
- [ ] Add a test where the body text is on the last line of the file and still returns the exact preview blocks.

### Path: attribute key hover

- [ ] Add an origin-only attribute-key comment test where one exact origin comment block is returned.
- [ ] Add a local-only attribute-key comment test where one exact local comment block is returned.
- [ ] Add an origin+local attribute-key comment test where two exact blocks are returned in order.
- [ ] Add a test where group-level and message-level comments keep a newline between them when adjacent.
- [ ] Add a test where group-level and message-level comments keep a newline between them when separated in source.
- [ ] Add a test where a group-level comment applies to the first attribute-bearing message in its scope.
- [ ] Add a test where a group-level comment applies to a later attribute-bearing message in its scope.
- [ ] Add a test where hovering indentation before the attribute key returns no hover.
- [ ] Add a test where hovering the attribute `=` sign returns no hover.
- [ ] Add a test where an uncommented attribute key returns no hover and does not degrade into attribute-body preview.

### Path: attribute body hover

- [ ] Add an origin-file attribute-body hover test where one exact origin preview block is returned.
- [ ] Add a translation-file attribute-body hover test where origin and local preview blocks are returned in order.
- [ ] Add a local-only attribute-body hover test where the origin attribute is missing and one local block is returned.
- [ ] Add an empty-local-attribute body hover test where the local block is exactly `<empty>`.
- [ ] Add a test where an unresolved term reference in an attribute value is preserved in the origin preview block.
- [ ] Add a test where an unresolved term reference in an attribute value is preserved in the local preview block.
- [ ] Add a test where hovering the first character of the attribute body returns the same preview as hovering the middle.
- [ ] Add a test where hovering the last character of the attribute body returns the same preview as hovering the middle.
- [ ] Add a test where hovering whitespace inside the attribute body returns the same preview block set.
- [ ] Add a test where the attribute body is on the last line of the file and still returns the exact preview blocks.

### Path: selector hover on selector expression

- [ ] Add a matching-selector test where origin and local selector headers list the same chosen variable values.
- [ ] Add a mismatched-selector-set test where shared selector variables are matched by name only.
- [ ] Add an origin-without-selectors test where the origin block is headerless and the local block still has a selector header.
- [ ] Add a numeric-selector test where an explicit numeric key such as `0` is preserved in the header.
- [ ] Add a plural-category selector test where a key such as `one` is preserved in the header.
- [ ] Add a test where the selector expression is the first line of the message and still resolves the exact hover range.
- [ ] Add a test where the selector expression is the last selector in the message and still resolves the exact hover range.
- [ ] Add a test where hovering the opening `{` of the selector returns the same selected branch preview.
- [ ] Add a test where hovering the selector variable name returns the same selected branch preview.
- [ ] Add a test where hovering whitespace inside the selector header returns the same selected branch preview.

### Path: selector hover on variant branch text

- [ ] Add a test where an unresolved term reference in the selected origin branch is preserved verbatim.
- [ ] Add a test where an unresolved term reference in the selected local branch is preserved verbatim.
- [ ] Add a test where an unresolved non-selector inline message reference in the selected origin branch is preserved verbatim.
- [ ] Add a test where an unresolved non-selector inline message reference in the selected local branch is preserved verbatim.
- [ ] Add a test where hovering the first character of a branch returns the same selected-branch preview as hovering the middle.
- [ ] Add a test where hovering the last character of a branch returns the same selected-branch preview as hovering the middle.
- [ ] Add a test where hovering whitespace inside a branch returns the same selected-branch preview.
- [ ] Add a test where the selected branch is the first branch in the selector and still resolves the exact hover range.
- [ ] Add a test where the selected branch is the last branch in the selector and still resolves the exact hover range.
- [ ] Add a test where the chosen selector key is repeated in both origin and local selector headers exactly once.

## `textDocument/completion` Path Matrix

### Path: top-level message completion list

- [ ] Add a test where completion at the first message in a translation file returns the exact top-level key list.
- [ ] Add a test where completion at the last message in a translation file returns the exact top-level key list.
- [ ] Add a test where the typed prefix matches exactly one origin key and only that label is returned.
- [ ] Add a test where the typed prefix matches multiple origin keys and the full returned label order is asserted exactly.
- [ ] Add a test where an already present top-level key is excluded while all remaining matches are preserved in order.
- [ ] Add a test where the cursor is at the first character of a new key fragment and still returns the exact label list.
- [ ] Add a test where the cursor is at the last character of a new key fragment and still returns the exact label list.
- [ ] Add a test where nested origin keys are not leaked into top-level completion results.
- [ ] Add a test where a preceding file-level comment does not change the exact completion list.
- [ ] Add a test where a preceding group-level comment does not change the exact completion list.

### Path: attribute completion list

- [ ] Add a test where bare-dot completion on a message with two missing attributes returns the exact ordered attribute list.
- [ ] Add a test where bare-dot completion on a message with one existing attribute excludes the existing attribute exactly.
- [ ] Add a test where a typed attribute prefix matches exactly one attribute and only that label is returned.
- [ ] Add a test where a typed attribute prefix matches multiple attributes and the full returned order is asserted exactly.
- [ ] Add a nested-file attribute completion test where the nested origin counterpart is used and the exact list is asserted.
- [ ] Add a test where top-level origin attributes are not leaked into a nested file completion result.
- [ ] Add a test where the cursor is on the leading `.` and still returns the exact attribute list.
- [ ] Add a test where the cursor is after the first attribute character and still returns the exact attribute list.
- [ ] Add a test where the attribute block is at the end of file and still returns the exact attribute list.
- [ ] Add a test where comments inside the message do not change the exact attribute list when the cursor is on a valid attribute fragment.

### Path: completion documentation payload

- [ ] Add a message-completion-doc test where origin-only comments render as one exact markdown block.
- [ ] Add a message-completion-doc test where origin comments preserve comment-line newlines exactly.
- [ ] Add an attribute-completion-doc test where origin-only comments render as one exact markdown block.
- [ ] Add an attribute-completion-doc test where attribute documentation preserves comment-line newlines exactly.
- [ ] Add a test where completion documentation preserves unresolved term references in the preview text.
- [ ] Add a test where completion documentation preserves unresolved non-selector inline references in the preview text.
- [ ] Add a test where completion documentation for a key with no comments still returns the exact preview markdown contract.
- [ ] Add a test where completion documentation for an attribute with no comments still returns the exact preview markdown contract.
- [ ] Add a test where the returned documentation kind is exactly `markdown` for message completions.
- [ ] Add a test where the returned documentation kind is exactly `markdown` for attribute completions.

### Path: empty-result and request-error outcomes

- [ ] Add a top-level origin-file completion test that asserts the exact empty completion payload.
- [ ] Add a nested origin-file completion test that asserts the exact empty completion payload.
- [ ] Add an unmatched-prefix completion test that asserts the exact empty completion payload.
- [ ] Add an inside-comment completion test that asserts the exact empty completion payload.
- [ ] Add a missing-origin-counterpart completion test that asserts the exact empty completion payload.
- [ ] Add a test where the cursor is on blank whitespace in a translation file and the exact empty completion payload is asserted.
- [ ] Add a test where the cursor is on a completed key name with no extension point and the exact empty completion payload is asserted.
- [ ] Add a test where the cursor is on a completed attribute name with no extension point and the exact empty completion payload is asserted.
- [ ] Add a non-file URI completion request test with the exact invalid-params error.
- [ ] Add a file-outside-workspace completion request test with the exact invalid-params error.

## `textDocument/codeAction` Path Matrix

### Path: file-wide missing-string quick fix

- [ ] Add a file-wide copy test where one whole missing top-level message is added with the exact `LSP-COPY` marker placement.
- [ ] Add a file-wide copy test where one whole missing message with multiple attributes is added with one exact per-attribute marker line each.
- [ ] Add a file-wide copy test where a missing top-level term is copied with the exact resulting source.
- [ ] Add a file-wide copy test where missing entries are appended after an existing trailing comment and the exact spacing is asserted.
- [ ] Add a file-wide copy test where unrelated complete entries before the insertion point remain byte-for-byte unchanged.
- [ ] Add a file-wide copy test where unrelated complete entries after the insertion point remain byte-for-byte unchanged.
- [ ] Add a file-wide copy test where the selected cursor position is on the first message in the file and the same file-wide action is still returned.
- [ ] Add a file-wide copy test where the selected cursor position is on the last message in the file and the same file-wide action is still returned.
- [ ] Add a file-wide copy test where the translation file is already complete and the exact returned action set excludes the file-wide quick fix.
- [ ] Add a file-wide copy test where there is no origin counterpart file and the exact returned action set excludes the file-wide quick fix.

### Path: single-message missing-string quick fix

- [ ] Add a single-message copy test where the selected stub message is the first entry in the file.
- [ ] Add a single-message copy test where the selected stub message is the last entry in the file.
- [ ] Add a single-message copy test where the copied message has no attributes and uses the exact whole-message `LSP-COPY` marker form.
- [ ] Add a single-message copy test where the copied message preserves unresolved term references in the inserted text.
- [ ] Add a single-message copy test where the copied message preserves unresolved non-selector inline references in the inserted text.
- [ ] Add a single-message copy test where surrounding unrelated entries remain byte-for-byte unchanged before the selection.
- [ ] Add a single-message copy test where surrounding unrelated entries remain byte-for-byte unchanged after the selection.
- [ ] Add a single-message copy test where a complete selected message does not return the single-message quick fix.
- [ ] Add a single-message copy test where a selected local-only message does not return the single-message quick fix.
- [ ] Add a single-message copy test where the exact returned quick-fix title set is asserted rather than only checking one expected title.

### Path: single-attribute missing-attribute quick fix

- [ ] Add a missing-attribute copy test where the selected message is the first message in the file.
- [ ] Add a missing-attribute copy test where the selected message is the last message in the file.
- [ ] Add a missing-attribute copy test where one missing attribute is inserted with the exact per-attribute `LSP-COPY` marker.
- [ ] Add a missing-attribute copy test where multiple missing attributes are inserted in origin order with exact markers.
- [ ] Add a missing-attribute copy test where existing local attributes remain byte-for-byte unchanged.
- [ ] Add a missing-attribute copy test where copied attributes preserve unresolved term references in inserted text.
- [ ] Add a missing-attribute copy test where copied attributes preserve unresolved non-selector inline references in inserted text.
- [ ] Add a missing-attribute copy test where a complete selected message does not return the missing-attribute quick fix.
- [ ] Add a missing-attribute copy test where a selected local-only message does not return the missing-attribute quick fix.
- [ ] Add a missing-attribute copy test where the exact returned quick-fix title set is asserted rather than only checking one expected title.

### Path: selector generation from a variable occurrence

- [ ] Add a generation test where the variable occurrence is in top-level body text and the exact returned rewrite-title set is asserted.
- [ ] Add a generation test where the variable occurrence is in attribute-body text and the exact returned rewrite-title set is asserted.
- [ ] Add a generation test where the selected variable occurrence is the first variable in the message.
- [ ] Add a generation test where the selected variable occurrence is the last variable in the message.
- [ ] Add a generation test where the preferred style is `prefix` and the exact first action title and edit are asserted.
- [ ] Add a generation test where the preferred style is `whole` and the exact first action title and edit are asserted.
- [ ] Add a generation test where file config overrides client selector-style preference and the exact first action title is asserted.
- [ ] Add a generation test where punctuation remains attached in the exact returned edit.
- [ ] Add a generation test where the generated edit is parseable Fluent source and exact rendered output is asserted.
- [ ] Add a generation test where the exact returned rewrite action count is asserted, not just presence of one expected title.

### Path: selector generation from non-default anchors

- [ ] Add a generation test where the enclosing anchor is a function placeable and the exact edit is asserted.
- [ ] Add a generation test where the enclosing anchor is inside an attribute value and the exact edit is asserted.
- [ ] Add a generation test where there is no variable and the whole-form snippet is generated with the exact placeholder text.
- [ ] Add a generation test where the client supports snippet edits and `documentChanges` is returned instead of `changes`.
- [ ] Add a generation test where the client does not support snippet edits and plain `changes` is returned instead of `documentChanges`.
- [ ] Add a generation test where the selected anchor is the first placeable in the message.
- [ ] Add a generation test where the selected anchor is the last placeable in the message.
- [ ] Add a generation test where nested selectors inside variants are preserved exactly while generating around a new anchor.
- [ ] Add a generation test where an ambiguous message key returns no generation actions at all.
- [ ] Add a generation test where the exact returned rewrite-title set is asserted rather than only checking one expected title.

### Path: selector rewrite

- [ ] Add a whole->prefix rewrite test where the exact returned rewrite-title set is asserted.
- [ ] Add a whole->suffix rewrite test where the exact returned rewrite-title set is asserted.
- [ ] Add a prefix->whole rewrite test where the exact returned rewrite-title set is asserted.
- [ ] Add a suffix->whole rewrite test where the exact returned rewrite-title set is asserted.
- [ ] Add a suffix->prefix rewrite test where the exact returned rewrite-title set is asserted.
- [ ] Add a bare suffix-like selector test where only the prefix rewrite is returned and the exact set is asserted.
- [ ] Add a nested-selector rewrite test where the exact returned rewrite-title set is asserted.
- [ ] Add an attribute selector rewrite test where the exact returned rewrite-title set is asserted.
- [ ] Add a rewrite test where assignment spacing such as `= {` is preserved exactly in every returned rewrite variant.
- [ ] Add a rewrite test where surrounding comments and attribute keys are not consumed by the edit range.

### Path: absence and hiding outcomes

- [ ] Add a generation-absence test where a message that already has a selector returns no generation actions.
- [ ] Add a generation-absence test where an attribute that already has a selector returns no generation actions.
- [ ] Add a rewrite-absence test where a message with no selector returns no rewrite actions.
- [ ] Add a rewrite-absence test where an attribute with no selector returns no rewrite actions.
- [ ] Add a hiding test where an ambiguous top-level message key returns no selector-generation actions.
- [ ] Add a hiding test where an ambiguous nested selector case returns no selector-generation actions.
- [ ] Add an origin-file quick-fix absence test where translation-only copy actions are not offered.
- [ ] Add a complete-entry absence test where single-message copy is not offered.
- [ ] Add a complete-entry absence test where single-attribute copy is not offered.
- [ ] Add a file-complete absence test where file-wide missing-string copy is not offered.

## `textDocument/codeLens` Path Matrix

### Path: positive selector-combination lens

- [ ] Add a test where a message with one selector returns the exact title with the correct combination count.
- [ ] Add a test where a message with two selectors returns the exact title with the correct combination count.
- [ ] Add a test where a message with three selectors returns the exact title with the correct combination count.
- [ ] Add a test where the lensed message is the first entry in the file.
- [ ] Add a test where the lensed message is the last entry in the file.
- [ ] Add a test where the exact returned lens range starts at the message key column.
- [ ] Add a test where the exact returned lens command name is asserted.
- [ ] Add a test where the exact returned lens argument array is asserted.
- [ ] Add a test where multiple lensed messages in one file return an exact full lens array in stable order.
- [ ] Add a test where comments above the message do not shift the asserted lens target range incorrectly.

### Path: no-lens outcomes

- [ ] Add a test where a plain message with no selectors returns an exact empty lens array.
- [ ] Add a test where a selector message with no meaningful expansion opportunity returns an exact empty lens array.
- [ ] Add a test where an ambiguous selector message returns an exact empty lens array.
- [ ] Add a test where a message with only one meaningful branch returns an exact empty lens array.
- [ ] Add a test where a file with only terms returns an exact empty lens array.
- [ ] Add a test where a file with only attributes but no selector expansion opportunity returns an exact empty lens array.
- [ ] Add a test where an origin file with no expandable selectors returns an exact empty lens array.
- [ ] Add a test where a nested file with no expandable selectors returns an exact empty lens array.
- [ ] Add a test where the request is for a non-existent but open empty file and returns an exact empty lens array.
- [ ] Add a test where comments alone in a file return an exact empty lens array.

### Path: lens contract shape

- [ ] Add a contract test that asserts the full returned lens array exactly for the main fixture file.
- [ ] Add a contract test that asserts the exact command title for every returned lens, not just one representative lens.
- [ ] Add a contract test that asserts the exact command name for every returned lens, not just one representative lens.
- [ ] Add a contract test that asserts the exact argument ordering for every returned lens, not just after execution.
- [ ] Add a contract test that asserts `resolveProvider = false` and that no resolve request is needed.
- [ ] Add a contract test that asserts the returned lens ranges are stable when unrelated comments are inserted earlier in the file.
- [ ] Add a contract test that asserts the returned lens ranges are stable when unrelated messages are inserted later in the file.
- [ ] Add a contract test that asserts nested-file lenses use the nested logical file path in their arguments.
- [ ] Add a contract test that asserts origin-file lenses use the origin document URI in their arguments when they exist.
- [ ] Add a contract test that asserts no duplicate lens is returned for one logical message.

## `workspace/executeCommand` Path Matrix

### Path: `window/showDocument` success path

- [ ] Add a test where a valid command request opens a temp markdown document through `window/showDocument`.
- [ ] Add a test where the exact `external = false` value is asserted.
- [ ] Add a test where the exact `takeFocus = true` value is asserted.
- [ ] Add a test where the exact selection start line is asserted.
- [ ] Add a test where the exact selection start character is asserted.
- [ ] Add a test where the temp document file name suffix includes the Fluent key exactly once.
- [ ] Add a test where the command request succeeds when the source document is the first entry in the file.
- [ ] Add a test where the command request succeeds when the source document is the last entry in the file.
- [ ] Add a test where the response to `workspace/executeCommand` is exact `null` after a successful `showDocument` acknowledgment.
- [ ] Add a test where the exact trace or log side effects of a successful command execution are asserted once trace coverage is added.

### Path: generated selector-combinations markdown document

- [ ] Add a test where the markdown document starts with the exact expected heading for the selected key.
- [ ] Add a test where the markdown document contains the exact current-language line.
- [ ] Add a test where the markdown document contains the exact source-language line.
- [ ] Add a test where the markdown document contains the exact logical-file line.
- [ ] Add a test where the markdown document contains the exact source text block.
- [ ] Add a test where the markdown document contains the exact current text block.
- [ ] Add a test where unresolved term references are preserved in source-language combination blocks.
- [ ] Add a test where unresolved term references are preserved in current-language combination blocks.
- [ ] Add a test where unresolved non-selector inline references are preserved in source-language combination blocks.
- [ ] Add a test where unresolved non-selector inline references are preserved in current-language combination blocks.

### Path: invalid-params and malformed-argument outcomes

- [ ] Add a test where an unknown command returns the exact invalid-params error.
- [ ] Add a test where a missing document URI argument returns the exact invalid-params error.
- [ ] Add a test where a missing Fluent key argument returns the exact invalid-params error.
- [ ] Add a test where a non-file document URI returns the exact invalid-params error.
- [ ] Add a test where a non-string document URI argument returns the exact invalid-params error.
- [ ] Add a test where a non-string Fluent key argument returns the exact invalid-params error.
- [ ] Add a test where the document URI and key arguments are reversed and the exact invalid-params error is asserted.
- [ ] Add a test where too many arguments are supplied and the exact behavior is asserted.
- [ ] Add a test where the key names a message that does not exist and the exact behavior is asserted.
- [ ] Add a test where the document path points outside the configured workspace and the exact behavior is asserted.

### Path: no-`showDocument` capability and failure outcomes

- [ ] Add a test where `window/showDocument` is unavailable and the exact spec-compliant failure path is asserted.
- [ ] Add a test where `window/showDocument` is unavailable and `window/showMessage` is explicitly not sent.
- [ ] Add a test where the command request still returns an exact response object when `showDocument` is unavailable.
- [ ] Add a test where the temp document is not silently generated without a client open request when `showDocument` is unavailable.
- [ ] Add a test where a client `showDocument` response with `success = false` is handled through the exact spec-compliant failure path.
- [ ] Add a test where a client `showDocument` error response is handled through the exact spec-compliant failure path.
- [ ] Add a test where a missing selected key in the source document does not silently degrade into a partial document.
- [ ] Add a test where an origin-only document still follows the exact same command failure contract when `showDocument` is unavailable.
- [ ] Add a test where a nested document still follows the exact same command failure contract when `showDocument` is unavailable.
- [ ] Add a test where the exact command trace payload is asserted once trace coverage is added.

## Diagnostics Path Matrix

### Path: parse-error diagnostics

- [ ] Add a parse-error test where the invalid token is on the first non-empty line in the file.
- [ ] Add a parse-error test where the invalid token is on the last non-empty line in the file.
- [ ] Add a parse-error test where the exact severity is asserted.
- [ ] Add a parse-error test where the exact message string is asserted.
- [ ] Add a parse-error test where the exact start line is asserted.
- [ ] Add a parse-error test where the exact start character is asserted.
- [ ] Add a parse-error test where fixing the syntax and saving clears diagnostics exactly.
- [ ] Add a parse-error test where an open dirty edit produces the exact diagnostic after save.
- [ ] Add a parse-error test where `didClose` after a parse error clears diagnostics exactly.
- [ ] Add a parse-error test where nested files produce the same exact parse-error contract.

### Path: `LSP-COPY` marker diagnostics

- [ ] Add a whole-message marker test where the exact warning message is asserted.
- [ ] Add a whole-message marker test where the exact severity is asserted.
- [ ] Add a whole-message marker test where the exact range points at the marker line.
- [ ] Add an attribute-marker test where the exact warning message is asserted.
- [ ] Add an attribute-marker test where the exact severity is asserted.
- [ ] Add an attribute-marker test where the exact range points at the copied attribute key line.
- [ ] Add a multi-marker test where two exact diagnostics are returned in stable order.
- [ ] Add a marker-removal test where removing only one of two markers leaves exactly one diagnostic.
- [ ] Add a marker-removal test where removing all markers clears diagnostics exactly.
- [ ] Add a hover test where copied-attribute hover still excludes marker comments from hover content after diagnostics are present.

### Path: missing origin counterpart file diagnostics

- [ ] Add a top-level translation file test where the exact missing-origin-file warning message is asserted.
- [ ] Add a top-level translation file test where the exact severity is asserted.
- [ ] Add a top-level translation file test where the exact range is asserted.
- [ ] Add a nested translation file test where the exact missing-origin-file warning message is asserted.
- [ ] Add a nested translation file test where the exact severity is asserted.
- [ ] Add a nested translation file test where the exact range is asserted.
- [ ] Add a background-refresh test where creating the origin counterpart clears the warning exactly.
- [ ] Add a background-refresh test where deleting the origin counterpart introduces the warning exactly.
- [ ] Add a test where unrelated diagnostics in the same file remain while the missing-origin-file warning clears.
- [ ] Add a test where reopening the same translation file after counterpart creation does not resurrect the warning.

### Path: translation-only entry and attribute diagnostics

- [ ] Add a translation-only top-level entry test where the exact warning message is asserted.
- [ ] Add a translation-only top-level entry test where the exact severity is asserted.
- [ ] Add a translation-only top-level entry test where the exact range is asserted.
- [ ] Add a translation-only term test where the exact warning contract is asserted if terms are supposed to be checked.
- [ ] Add a translation-only attribute test where the exact warning message is asserted.
- [ ] Add a translation-only attribute test where the exact severity is asserted.
- [ ] Add a translation-only attribute test where the exact range is asserted.
- [ ] Add a background-refresh test where adding the origin entry clears only the entry warning exactly.
- [ ] Add a background-refresh test where adding the origin attribute clears only the attribute warning exactly.
- [ ] Add a fully matching translation-file test where an exact empty diagnostic array is published after save.

### Path: unsupported numeric selector key diagnostics

- [ ] Add an English invalid-selector-key test where the exact message is asserted.
- [ ] Add an English invalid-selector-key test where the exact severity is asserted.
- [ ] Add an English invalid-selector-key test where the exact range is asserted.
- [ ] Add a test where the invalid key is the first branch in the selector.
- [ ] Add a test where the invalid key is the last branch in the selector.
- [ ] Add a test where an exact numeric key such as `[0]` does not trigger the invalid-key diagnostic.
- [ ] Add a test where a plural category such as `[one]` does not trigger the invalid-key diagnostic.
- [ ] Add a test where a non-numeric semantic selector such as `[admins]` under a non-numeric selector does not trigger this diagnostic.
- [ ] Add a nested selector test where the exact invalid-key diagnostic still points at the offending nested branch.
- [ ] Add a test where disabling unsupported-category diagnostics suppresses this warning exactly.

### Path: plural-category diagnostics

- [ ] Add an English `[zero]` test where the exact unsupported-category message is asserted.
- [ ] Add an English `[zero]` test where the exact severity is asserted.
- [ ] Add an English `[zero]` test where the exact range is asserted.
- [ ] Add a Latvian unsupported-category test where the exact message, severity, and range are asserted.
- [ ] Add a Latvian missing-category test for `[zero]` where the exact message, severity, and range are asserted.
- [ ] Add a Latvian missing-category test for `[one]` where the exact message, severity, and range are asserted.
- [ ] Add a Ukrainian missing-category test where the exact message, severity, and range are asserted.
- [ ] Add a locale-specific coverage test for a language with `two`, `few`, and `many` where exact diagnostics are asserted.
- [ ] Add a test where the origin file uses a different plural-category set and the translation side still follows locale-specific rules only.
- [ ] Add a default-off test where plural-category diagnostics are absent until the relevant settings are enabled.

### Path: selector-style mismatch diagnostics

- [ ] Add a translation-file whole-style mismatch test where the exact message is asserted.
- [ ] Add a translation-file whole-style mismatch test where the exact severity is asserted.
- [ ] Add a translation-file whole-style mismatch test where the exact range is asserted.
- [ ] Add a translation-file suffix-style mismatch test where the exact message is asserted.
- [ ] Add a translation-file suffix-style mismatch test where the exact severity is asserted.
- [ ] Add a translation-file suffix-style mismatch test where the exact range is asserted.
- [ ] Add a local-origin mismatch test where the exact message is asserted.
- [ ] Add a local-origin mismatch test where the exact severity is asserted.
- [ ] Add a local-origin mismatch test where the exact range is asserted.
- [ ] Add a file-config-overrides-client-settings test where the exact mismatch diagnostics follow file config, not client settings.
