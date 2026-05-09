# Missing Spec Tests

This checklist tracks test gaps against [spec/reference.md](/Users/illiadenysenko/Workspace/lab/fluent-lsp/spec/reference.md) after removing items already covered by `tests/integration_lsp.rs`.

## Test Helper Policy

Do not add new test helpers or helper layers beyond the existing `LspProcess` struct helpers already present in `tests/integration_lsp.rs`.
Remove all existing ones, and make tests assert real data, not helpers.
Do not make tests looser to accommodate incorrect behavior.
If a test fails because the implementation does not match the spec, say that to the user and fix the code instead of weakening the test.
Some of the TODO tests will fail when first written, and that is good: a failing test means we are catching a real bug, a spec mismatch, or a hole in current behavior.
Bare test names are not enough in this file. Every `Existing coverage` entry should say what that test actually covers and where it stops.

## Coverage Matrix

This section breaks broad request behavior into concrete request paths.
Each path below carries both existing coverage and missing coverage.
Each path below needs at least ten exact tests, not one merged happy-path test.

## Configuration Paths

### Path: documented `fluent-lsp.toml` contract

Existing coverage:
- [documented_config_contract_resolves_counterparts_from_exact_file_shape](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:5359): exact `fluent-lsp.toml` shape with documented `origin_language` and `file_masks`, covering both top-level and nested counterpart resolution.

Missing coverage:
- none currently tracked beyond the path-specific request coverage below

### Path: client configuration precedence

Existing coverage:
- [client_configuration_applies_origin_language_and_file_masks_without_file_config](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:5467): client `workspace/didChangeConfiguration` drives counterpart lookup when no file config exists.
- [code_action_uses_client_selector_style_setting](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:5295): client-provided `selector_style` changes the preferred selector-generation action.
- [diagnostics_report_selector_style_mismatches_when_enabled](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:6967): client-provided selector-style settings drive mismatch diagnostics in translation files.

Missing coverage:
- [ ] Add precedence coverage that client `workspace/didChangeConfiguration` applies plural-diagnostic settings when file config does not override them.

### Path: file configuration precedence

Existing coverage:
- [file_config_overrides_client_origin_language_and_file_masks](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:5526): file config wins over client-provided path mapping and origin-language settings.
- [file_config_selector_style_overrides_client_setting](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:5606): file config wins over client selector-style preference for code actions.
- [file_config_overrides_client_style_diagnostic_settings](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:7009): file config wins over client selector-style diagnostic settings.

Missing coverage:
- [ ] Add precedence coverage that file config overrides client-provided plural-diagnostic settings, not just path mapping and selector-style settings.

## Capability Paths

### Path: exact `initialize` capability contract

Existing coverage:
- [initialize_returns_exact_capability_contract](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:271): exact `initialize` capability payload, including supported fields and the absence of unsupported ones.

Missing coverage:
- none currently tracked outside the request-specific paths below

## Index And Trace Paths

### Path: initial index build and progress

Existing coverage:
- [initialized_builds_index_and_reports_progress_when_supported](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:917): initial index build starts after `initialized` and emits work-done progress when the client supports it.

Missing coverage:
- [ ] Add a background-refresh test that picks up on-disk content or mtime changes for an already indexed file, not just add/delete events.

### Path: index-driven refresh across request paths

Existing coverage:
- [indexed_requests_reflect_live_origin_changes_without_restart](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:1172): open origin overlays immediately affect definition, hover, and completion without restarting the server.
- [indexed_requests_reflect_live_translation_changes_and_dirty_close_reverts](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:1259): dirty translation overlays immediately affect references, and `didClose` reverts back to disk state.
- [indexed_references_pick_up_disk_file_adds_and_deletes](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:1332): background refresh picks up top-level translation file add/delete events for references.
- [local_only_file_warning_updates_when_origin_counterpart_appears](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:1401): missing-origin-file diagnostics clear after the origin counterpart appears on disk.
- [translation_only_keys_warn_and_clear_when_origin_adds_counterparts](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:1471): translation-only entry and attribute warnings clear after matching origin counterparts are added on disk.

Missing coverage:
- [ ] Add a background-refresh definition test for an origin counterpart file appearing on disk after initialization.
- [ ] Add a background-refresh definition test for an origin counterpart file disappearing on disk after initialization.
- [ ] Add a background-refresh hover test that picks up changed origin comment or preview content from disk without restarting the server.
- [ ] Add a background-refresh completion test that picks up changed origin completion entries from disk without restarting the server.
- [ ] Add a background-refresh missing-origin-diagnostics test that picks up on-disk counterpart changes for an already indexed translation file without reopening it.

### Path: trace logging

Existing coverage:
- [log_trace_reports_index_and_request_timings_when_enabled](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:968): `initialize.trace = "messages"` emits timing traces for `workspace/index` and `textDocument/definition`.
- [verbose_log_trace_includes_request_details](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:1040): `initialize.trace = "verbose"` adds verbose payloads for `workspace/index` and `textDocument/definition`.
- [set_trace_enables_request_timings_after_initialize](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:1097): runtime `$/setTrace` enables verbose request tracing after initialization.

Missing coverage:
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

## `textDocument/definition` Path Matrix

### Path: translation top-level key -> origin top-level key

Existing coverage:
- [goto_definition_from_translation_resolves_to_origin_fluent_file](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:96): translation top-level keys resolve to the matching origin key range in the counterpart file.
- [documented_config_contract_resolves_counterparts_from_exact_file_shape](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:5359): documented file config shape is enough for top-level definition requests to find the right origin file.
- [client_configuration_applies_origin_language_and_file_masks_without_file_config](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:5467): client-provided origin-language and file-mask settings are honored for top-level definition when no file config exists.
- [file_config_overrides_client_origin_language_and_file_masks](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:5526): file config overrides client counterpart mapping for top-level definition lookups.

Missing coverage:
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

Existing coverage:
- [goto_definition_from_translation_resolves_to_origin_fluent_file](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:96): translation terms resolve to the matching origin term in the basic counterpart fixture.

Missing coverage:
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

Existing coverage:
- [goto_definition_from_translation_resolves_to_origin_fluent_file](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:96): translation attributes resolve to the matching origin attribute in the base fixture.
- [documented_config_contract_resolves_counterparts_from_exact_file_shape](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:5359): documented config shape also covers nested attribute definition resolution.

Missing coverage:
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

Existing coverage:
- [definition_rejects_non_file_uris_with_invalid_params](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:337): non-file URIs are rejected with an invalid-params error on definition requests.
- [definition_rejects_files_outside_the_configured_workspace](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:360): files outside the configured workspace are rejected with invalid params.
- [goto_definition_from_origin_file_returns_no_location](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:394): origin-side definition requests return no location instead of pointing back into translations.
- [goto_definition_returns_no_location_for_translation_key_missing_in_origin](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:417): missing origin top-level keys return no definition location.
- [goto_definition_returns_no_location_when_origin_counterpart_file_is_missing](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:452): missing origin counterpart files return no definition location.
- [goto_definition_returns_no_location_for_translation_attribute_missing_in_origin](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:480): missing origin attributes return no definition location.

Missing coverage:
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

Existing coverage:
- [references_from_origin_resolve_to_translated_fluent_files](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:576): origin top-level keys return matching translation-key references across counterpart files.
- [indexed_references_pick_up_disk_file_adds_and_deletes](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:1332): reference results refresh when translation files are added or removed on disk.

Missing coverage:
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

Existing coverage:
- [references_from_origin_resolve_to_translated_fluent_files](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:576): origin terms return matching translation-term references in the shared fixture.

Missing coverage:
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

Existing coverage:
- [references_from_origin_resolve_to_translated_fluent_files](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:576): origin attributes return matching translation-attribute references in the shared fixture.

Missing coverage:
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

Existing coverage:
- [references_from_origin_return_empty_list_when_no_translation_matches](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:755): unmatched origin top-level keys return an empty reference array.
- [references_from_origin_attribute_return_empty_list_when_no_translation_matches](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:795): unmatched origin attributes return an empty reference array.
- [references_from_translation_file_return_no_result](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:834): translation-side reference requests return no result instead of searching outward.
- [references_reject_non_file_uris_with_invalid_params](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:858): non-file URIs are rejected for reference requests.
- [references_reject_files_outside_the_configured_workspace](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:882): outside-workspace files are rejected for reference requests.

Missing coverage:
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

Existing coverage:
- [indexed_requests_reflect_live_translation_changes_and_dirty_close_reverts](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:1259): open translation overlays immediately affect reference results and `didClose` restores disk-backed references.
- [indexed_references_pick_up_disk_file_adds_and_deletes](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:1332): background indexing updates reference results after on-disk translation file adds and deletes.

Missing coverage:
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

Existing coverage:
- [hover_key_and_attribute_show_comment_context_across_locale_files](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:4016): top-level key hover can show ordered comment blocks across origin and local locale files.
- [hover_key_with_origin_comments_only_shows_one_origin_comment_block](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:4212): origin-only key comments produce exactly one hover comment block.
- [hover_key_with_local_comments_only_shows_one_local_comment_block](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:4265): local-only key comments produce exactly one hover comment block.
- [hover_key_without_comments_returns_no_hover](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:4318): uncommented keys return no hover at all.
- [hover_on_uncommented_key_does_not_fall_back_to_body_preview](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:4776): key hover does not silently degrade into message-body preview when comment hover is absent.

Missing coverage:
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

Existing coverage:
- [hover_from_translation_shows_local_formatted_messages](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:3041): translation-side body hover shows formatted origin and local preview blocks.
- [hover_from_origin_file_shows_one_body_preview_block_for_top_level_message](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:3931): origin-side body hover returns one preview block for the origin message only.
- [hover_body_without_origin_message_shows_one_local_preview_block](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:4352): missing origin counterparts still yield one local preview block instead of failing hover entirely.
- [hover_body_preview_stays_semantic_across_translation_locales](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:4715): body hover preserves semantic preview rendering across multiple translation locales.

Missing coverage:
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

Existing coverage:
- [hover_key_and_attribute_show_comment_context_across_locale_files](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:4016): attribute-key hover can show ordered comment blocks across origin and local locale files.

Missing coverage:
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

Existing coverage:
- [hover_from_translation_shows_local_formatted_messages](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:3041): translation-side attribute body hover shows formatted origin and local previews.
- [hover_from_origin_file_shows_formatted_attribute_text](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:3832): origin-side attribute body hover returns the formatted origin attribute preview.
- [hover_on_copied_attribute_does_not_surface_lsp_copy_marker_comments](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:2486): copied-attribute hover excludes injected `LSP-COPY` marker comments from attribute-body preview.

Missing coverage:
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

Existing coverage:
- [hover_from_translation_matches_available_selector_variables_across_source_and_local](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:3510): selector-expression hover aligns source and local selector variables and previews the chosen branch.
- [hover_from_latvian_translation_preserves_zero_category_selectors](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:3695): selector hover preserves locale-specific categories such as Latvian `zero`.
- [hover_selector_without_origin_selectors_leaves_origin_block_headerless](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:4403): origin preview stays headerless when the origin side has no selector headers to show.
- [hover_selector_without_origin_message_shows_one_local_selector_block](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:4472): missing origin selectors still yield one local selector preview block.

Missing coverage:
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

Existing coverage:
- [hover_selector_preserves_unresolved_term_references_inside_selected_branch](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:4529): selector-branch hover keeps unresolved term references verbatim in the rendered branch preview.
- [hover_selector_preserves_unresolved_non_selector_inline_references_inside_selected_branch](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:4622): selector-branch hover also preserves unresolved non-selector inline references verbatim.

Missing coverage:
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

Existing coverage:
- [completion_from_translation_uses_origin_language_keys_and_attributes](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:1629): top-level completion in translation files is sourced from origin-language keys.
- [completion_omits_already_present_top_level_keys](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:1731): top-level completion excludes keys already present in the translation file.

Missing coverage:
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

Existing coverage:
- [completion_from_translation_uses_origin_language_keys_and_attributes](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:1629): attribute completion in translation files is sourced from origin-language attributes.
- [completion_omits_already_present_attributes](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:1768): attribute completion excludes attributes already present on the message.
- [completion_uses_nested_origin_counterpart_and_skips_origin_files](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:1806): nested translation files use nested origin counterparts, while origin files themselves do not offer completion.

Missing coverage:
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

Existing coverage:
- [completion_items_include_origin_documentation_for_keys_and_attributes](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:2011): completion items include origin-derived documentation for both message and attribute suggestions.

Missing coverage:
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

Existing coverage:
- [completion_returns_empty_results_for_nested_origin_files](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:1874): nested origin files themselves return an empty completion result.
- [completion_returns_empty_results_for_unmatched_prefixes](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:1910): unmatched completion prefixes return an empty completion result.
- [completion_returns_empty_results_inside_comments](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:1947): comment positions return an empty completion result instead of suggestions.
- [completion_returns_empty_results_without_origin_counterpart_file](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:1972): missing origin counterpart files produce an empty completion result.
- [completion_rejects_non_file_uris_with_invalid_params](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:553): non-file URIs are rejected for completion requests.
- [completion_rejects_files_outside_the_configured_workspace](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:518): outside-workspace files are rejected for completion requests.
- [completion_uses_nested_origin_counterpart_and_skips_origin_files](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:1806): origin files are explicitly excluded from nested completion results.

Missing coverage:
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

Existing coverage:
- [code_action_file_wide_copy_uses_spec_title_for_whole_missing_string_example](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:2124): file-wide quick fix can copy one whole missing message with the spec title and exact resulting source.
- [code_action_file_wide_copy_uses_spec_title_for_whole_missing_message_with_attributes](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:2207): file-wide quick fix can copy a whole missing message that includes attributes.
- [whole_file_missing_entry_actions_are_absent_when_translation_is_complete](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:2294): complete translation files do not offer file-wide missing-entry quick fixes.
- [whole_file_missing_entry_actions_are_absent_without_origin_counterpart_file](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:2341): missing origin counterpart files do not offer file-wide missing-entry quick fixes.

Missing coverage:
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

Existing coverage:
- [code_action_copies_single_stub_message_without_touching_other_entries](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:2557): single-message quick fix fills one stub message and leaves unrelated entries unchanged.
- [single_message_copy_actions_are_absent_for_complete_entries](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:2831): complete messages do not offer single-message copy actions.
- [single_message_copy_action_is_absent_when_selected_key_has_no_origin_counterpart](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:2900): local-only selected keys do not offer single-message copy actions.

Missing coverage:
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

Existing coverage:
- [code_action_copies_missing_attributes_for_selected_message_only](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:2692): missing-attribute quick fix copies only the selected message's missing attributes.
- [missing_attribute_copy_action_is_absent_when_selected_message_has_no_origin_counterpart](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:2948): local-only messages do not offer missing-attribute copy actions.
- [hover_on_copied_attribute_does_not_surface_lsp_copy_marker_comments](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:2486): copied attributes remain hover-clean even after the quick fix inserts marker comments.

Missing coverage:
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

Existing coverage:
- [code_action_generates_prefix_selector_by_default](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:5183): selector generation defaults to prefix style when no preference overrides it.
- [code_action_returns_all_styles_for_variable_occurrence](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:5247): variable-occurrence generation can return the full set of selector-style rewrites.
- [code_action_uses_client_selector_style_setting](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:5295): client selector-style settings change which generation action is preferred.
- [code_action_keeps_punctuation_attached_in_prefix_generation](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:6040): generated prefix selectors keep surrounding punctuation attached correctly in the edit.

Missing coverage:
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

Existing coverage:
- [code_action_uses_snippet_text_edit_when_supported](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:5668): selector generation uses snippet text edits when the client advertises snippet support.
- [code_action_generates_whole_snippet_when_no_variable_exists](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:5730): generation falls back to a whole-form snippet when no variable anchor exists.
- [code_action_supports_attributes](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:5791): selector generation works inside attribute values, not just top-level message bodies.
- [code_action_uses_enclosing_function_placeable_as_generation_anchor](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:5837): enclosing function placeables can act as the generation anchor.
- [code_action_preserves_nested_selector_when_generating_inside_variant](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:7345): generation inside a selector variant preserves nested selectors already present in the text.

Missing coverage:
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

Existing coverage:
- [code_action_rewrites_whole_selector_to_prefix_and_suffix](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:6181): whole-style selectors can be rewritten into prefix and suffix forms.
- [code_action_rewrites_prefix_selector_to_whole](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:6254): prefix-style selectors can be rewritten back into whole form.
- [code_action_selector_rewrite_preserves_assignment_spacing](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:6303): rewrite edits preserve assignment spacing exactly.
- [code_action_rewrites_suffix_selector_to_whole_and_prefix](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:6367): suffix-style selectors can be rewritten into whole and prefix forms.
- [code_action_bare_suffix_like_selector_only_offers_prefix](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:6402): suffix-like selectors that cannot support every rewrite only offer the valid prefix rewrite.
- [code_action_rewrites_nested_whole_selector_inside_variant](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:6456): nested selectors inside variants can be rewritten without disturbing the surrounding selector.
- [code_action_rewrites_selector_inside_attribute_value](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:6599): selector rewrites also work inside attribute values.
- [attribute_rewrite_range_does_not_consume_comments_or_attribute_key](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:6666): attribute selector rewrite ranges stay inside the attribute value and do not eat comments or keys.
- [code_action_rewrites_selected_count_selector_with_trailing_suffix_text](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:6708): rewrite edits preserve trailing suffix text after the selected count selector.
- [code_action_rewrites_selected_gender_selector_to_whole_with_nested_count_preserved](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:6763): rewriting a gender selector to whole form preserves nested count selectors inside it.

Missing coverage:
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

Existing coverage:
- [origin_files_do_not_offer_translation_only_missing_entry_quick_fixes](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:2999): origin files do not offer translation-only copy quick fixes.
- [code_action_generation_is_absent_when_message_already_has_selector](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:6091): messages that already contain selectors do not offer generation actions.
- [code_action_generation_is_absent_when_attribute_already_has_selector](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:6136): attributes that already contain selectors do not offer generation actions.
- [code_action_rewrite_is_absent_when_message_has_no_selector](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:6507): messages without selectors do not offer rewrite actions.
- [code_action_rewrite_is_absent_when_attribute_has_no_selector](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:6553): attributes without selectors do not offer rewrite actions.
- [code_action_is_hidden_for_ambiguous_message_keys](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:6824): ambiguous message keys suppress selector-related code actions instead of guessing.

Missing coverage:
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

Existing coverage:
- [code_lens_opens_full_selector_combinations_document](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:4826): positive code lens requests return selector-combination lenses that can open the generated document.

Missing coverage:
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

Existing coverage:
- [code_lens_returns_empty_list_for_files_without_selector_combinations](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:5056): files without selector-combination expansion opportunities return an empty lens array.

Missing coverage:
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

Existing coverage:
- [code_lens_opens_full_selector_combinations_document](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:4826): the positive lens test also exercises the returned command title, command name, and follow-up execution flow for one representative lens.

Missing coverage:
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

Existing coverage:
- [code_lens_opens_full_selector_combinations_document](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:4826): successful execute-command flow already proves one `window/showDocument` path from a code lens.

Missing coverage:
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

Existing coverage:
- [code_lens_opens_full_selector_combinations_document](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:4826): current positive coverage includes one generated selector-combinations markdown document and its main content blocks.

Missing coverage:
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

Existing coverage:
- [execute_command_rejects_unknown_command](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:5089): unknown execute-command names are rejected.
- [execute_command_rejects_missing_document_uri_argument](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:5112): missing document URI arguments are rejected.
- [execute_command_rejects_missing_fluent_key_argument](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:5135): missing Fluent key arguments are rejected.
- [execute_command_rejects_non_file_document_uris](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:5160): non-file document URIs are rejected for command execution.

Missing coverage:
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

Existing coverage:
- none yet

Missing coverage:
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

Existing coverage:
- [parse_error_diagnostics_publish_on_save_and_clear_after_fix](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:7175): parse errors publish on save and clear after the syntax is fixed and saved again.

Missing coverage:
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

Existing coverage:
- [lsp_copy_marker_diagnostics_publish_on_save_and_clear_after_removal](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:2386): `LSP-COPY` markers publish diagnostics on save and those diagnostics clear after marker removal.
- [hover_on_copied_attribute_does_not_surface_lsp_copy_marker_comments](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:2486): copied-marker diagnostics do not leak marker comment text into hover output.

Missing coverage:
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

Existing coverage:
- [local_only_file_warning_updates_when_origin_counterpart_appears](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:1401): missing-origin-file warnings clear once the origin counterpart file appears on disk.

Missing coverage:
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

Existing coverage:
- [translation_only_keys_warn_and_clear_when_origin_adds_counterparts](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:1471): translation-only entry and attribute warnings clear when matching origin counterparts are added.
- [translation_only_warnings_are_absent_for_matching_translation_files](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:1594): matching translation files do not publish translation-only warnings.

Missing coverage:
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

Existing coverage:
- [diagnostics_report_invalid_numeric_identifier_keys_when_enabled](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:7062): unsupported numeric-identifier selector keys publish diagnostics when the setting is enabled.
- [diagnostics_ignore_non_numeric_admin_other_selector](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:7103): non-numeric semantic selector keys do not trigger the numeric-identifier diagnostic.

Missing coverage:
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

Existing coverage:
- [diagnostics_are_absent_by_default](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:6886): plural-category diagnostics stay off by default.
- [diagnostics_report_unsupported_and_missing_categories_when_enabled](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:6896): enabling plural-category checks reports unsupported and missing categories.
- [diagnostics_use_unicode_plural_categories_for_ukrainian](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:7276): plural diagnostics use the locale-specific Ukrainian category set.
- [did_close_clears_document_diagnostics](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:7312): closing a document clears its published diagnostics.

Missing coverage:
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

Existing coverage:
- [diagnostics_report_selector_style_mismatches_when_enabled](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:6967): selector-style mismatch diagnostics appear when the feature is enabled.
- [file_config_overrides_client_style_diagnostic_settings](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:7009): file config overrides client selector-style diagnostic settings.
- [diagnostics_do_not_warn_for_complete_numeric_selectors_or_matching_style](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:7135): complete numeric selectors and already matching styles do not produce selector-style diagnostics.
- [diagnostics_report_local_selector_style_mismatches_when_enabled](/Users/illiadenysenko/Workspace/lab/fluent-lsp/tests/integration_lsp.rs:7233): local-only selector style mismatches are also diagnosed when enabled.

Missing coverage:
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
