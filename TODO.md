- Add autocomplete for keys/attributes from origin language
I.e. if you have this in English
```ftl
hello = Hello World
```
Ukrainian
```ftl
he-TAB -> hello
```
Nothing crazy, pretty much the way usual LSPs work, have part of a key, complete it.
Same for attributes.
- Add code action to complete whole file for missing keys/attributes.
- Add code action to complete whole file for missing keys/attributes by copypasting
them directly, but with `# [LSP-COPY]` comment
- Add warning for `# [LSP-COPY]` comments to catch this
- Add code action on a single key to copypase from source, and yes, add the same comment.

All features must have tests, integration tests using nvim.
Any test that produces Fluent source or user-visible Fluent preview text must verify semantic correctness, not only raw LSP payload shape: parse the whole file and check that the exercised message or attribute renders the expected text.

## Proposed Plan

### 0. Audit existing integration/smoke coverage for semantic Fluent validation

Goal:
- Close the current gap where some tests only assert ranges, snippets, Markdown blocks, or diagnostic counts without proving that the resulting Fluent still parses and renders correctly.

Audit targets:
- Existing edit-producing coverage in `tests/integration_lsp.rs` and `tests/nvim/code_action_generate_selector/`.
- Existing preview-producing coverage in hover and selector-combinations tests, including `hover_translation`, `hover_selector_mismatch`, `hover_selector_zero_lv`, `hover_comment_structure`, `hover_origin_language`, and the selector-combinations integration paths.
- Existing save/fix diagnostics coverage in `diagnostics_parse_errors`, so the "fixed" buffer is also proven valid after the diagnostic clears.

Required helper coverage:
- Add a shared test helper that runs `fluent_syntax::parser::parse` on the whole post-edit file or buffer and fails with the full source when parsing breaks.
- Add a shared semantic assertion helper that formats the exercised message or attribute and compares the user-visible text to the expectation.
- Prefer `FluentBundle::format_pattern` for that semantic assertion helper; until the repo has that wired in centrally, keep the helper aligned with the current preview-rendering path so we can swap the implementation later without rewriting the tests.
- Use selector-aware expectations when a test depends on a specific variant branch.

Exit criteria:
- Existing integration and Neovim smoke tests that assert generated edits also assert full-document parseability after the edit is applied.
- Existing integration and Neovim smoke tests that assert rendered output also compare the relevant message text semantically, not only by substring or Markdown shape.
- New feature work below should reuse these helpers rather than adding more payload-only assertions.

### 0.1. Audit hover comment behavior across all languages

Goal:
- Catch regressions where key/attribute hover stops showing comment context, or where message-body hover stops showing the source/current message preview split.

Required behavior to lock down:
- Hover on a message key should show that entry's comments.
- Hover on an attribute key should show that attribute/message comment context when available.
- Hover on the message body itself should show the source/current rendered message preview instead of comment-only output.
- These expectations must hold across every locale fixture we keep in the test workspace, not only one translation language.

Audit targets:
- Existing integration hover coverage in `tests/integration_lsp.rs`.
- Existing Neovim smoke scenarios `hover_translation`, `hover_origin_language`, `hover_comment_structure`, `hover_selector_mismatch`, and `hover_selector_zero_lv`.
- Locale sets in both top-level and nested `dialogs/` fixture paths.

Required test additions:
- Add coverage that iterates or enumerates all fixture languages and checks hover-on-key for comment presence on the same logical entry.
- Add coverage that separately checks hover-on-message-body for source/local preview rendering on the same entry.
- Include at least one attribute-hover case, not only top-level message keys.
- Include both origin-language and translation-file behavior where relevant.

Exit criteria:
- Integration tests prove hover-on-key and hover-on-attribute still include comments for every exercised locale.
- Integration tests prove hover-on-message-body still shows source/current message previews for every exercised locale.
- Neovim smoke tests cover the same split so a client-facing regression is caught even if raw LSP payload tests still pass.

### 1. Autocomplete for keys and attributes from the origin language

Feature shape:
- Use standard LSP completion only. No custom Neovim/editor integration.
- Offer completions in translation files based on the matching origin-language file.
- Complete top-level message keys and, when the cursor is inside a message block, complete that message's available attributes.
- Match by simple typed prefix. This should feel like a normal language-server completion, not a special command.
- The completion should insert the full key name. It should not rewrite surrounding file structure by itself.

Concrete examples:

Origin file:
```ftl
hello = Hello World

menu-save =
    .label = Save
```

Translation file before completion:
```ftl
he
```

Expected completion item:
```text
hello
```

Translation file before attribute completion:
```ftl
menu-save =
    .l
```

Expected completion item:
```text
.label
```

If the user has only typed `.` inside the message block:
```ftl
menu-save =
    .
```

Expected completion menu:
```text
.label
.tooltip
```

More realistic translation-side example:
```ftl
welcome-title = Launched

down
```

Expected completion menu:
```text
download-action
download-count
```

Implementation plan:
- Reuse the existing workspace/file-mask resolution to find the matching origin-language counterpart for the current translation file.
- Parse the origin file and collect top-level message keys plus per-message attribute names.
- Determine the current token under the cursor using source text rather than regexing raw lines loosely.
- Return LSP completion items only when the cursor is on a plausible key/attribute token site.
- For attribute completion, first resolve the surrounding message context, then offer only that message's origin-language attributes.
- Prefer a compact completion list: label is the candidate key or attribute, insert text is the exact token to insert, detail can indicate whether it came from a message or attribute.
- Do not advertise these completions in origin-language files.

Unit tests:
- Collect top-level message keys from the origin file.
- Collect attribute names for a specific origin-language message.
- Resolve completions for a partial message-key prefix.
- Resolve completions for a partial attribute prefix inside the matching message block.
- Resolve completions for bare `.` inside the matching message block.
- Suppress attributes from unrelated messages.
- Suppress completions when the cursor is inside a message value rather than on a key line.
- Suppress completions in origin-language files.
- Keep completion ordering stable.

Integration tests:
- `textDocument/completion` on a partial translation key returns the matching origin key.
- `textDocument/completion` on `.l` inside a translation message returns `.label` when the matching origin message has that attribute.
- `textDocument/completion` on bare `.` inside a translation message returns all origin attributes for that message.
- Nested locale-tree files use the matching nested origin file, not the top-level file.
- Origin-language files do not advertise these translation-key completions.
- Unmatched prefixes return an empty completion list.
- The completion candidates are sourced from entries found via whole-resource parsing, not loose line scanning.

Neovim smoke:
- Add a self-contained scenario under `tests/nvim/completion_origin_keys/`.
- Exercise key completion on a top-level translation file.
- Exercise attribute completion on a nested translation file.
- Assert the returned completion labels match the expected origin entries.
- Keep the scenario-local source fixtures parseable and assert the advertised completion labels map to real parsed origin entries.
- Document the source-under-test in the scenario README.

Examples to add:
- `example/locales/en/app.ftl`: keep a few obvious keys like `hello-world`, `download-action`, and `download-count`.
- `example/locales/es/app.ftl`: leave one partial key line for completion testing.
- `example/locales/en/dialogs/menu.ftl` and `example/locales/es/dialogs/menu.ftl`: include a message where typing `.l` or bare `.` inside `menu-save =` can complete attributes like `.label`.

### 2. Code action to complete a whole file for missing keys and attributes

Feature shape:
- Offer a standard `textDocument/codeAction` on translation files.
- Detect keys and attributes present in the origin file but missing in the current translation file.
- Insert empty translation stubs rather than copying source text.
- Keep the result syntactically valid Fluent and preserve attribute structure.

Concrete examples:

Origin file:
```ftl
hello = Hello World

menu-save =
    .label = Save
    .tooltip = Save this file
```

Translation file before:
```ftl
hello = Hola Mundo
```

Expected code action title:
```text
Add missing keys and attributes from source
```

Expected result:
```ftl
hello = Hola Mundo

menu-save =
    .label =
    .tooltip =
```

Case with a partially translated entry:

Origin file:
```ftl
download-action =
    .label = Download
    .tooltip = Download this build
```

Translation file before:
```ftl
download-action =
    .label = Descargar
```

Expected result:
```ftl
download-action =
    .label = Descargar
    .tooltip =
```

Implementation plan:
- Build a shared missing-entry diff between origin and translation files.
- Represent missing whole messages separately from missing attributes on existing messages.
- Generate empty stubs that match Fluent structure without copying source text.
- Append new entries in origin-file order so repeated runs stay stable.
- For missing attributes on an existing message, patch only that message block instead of duplicating the whole entry.

Unit tests:
- Detect a wholly missing message.
- Detect a missing attribute on an existing message.
- Preserve existing translated keys.
- Preserve origin ordering in generated stubs.
- Generate valid empty message/attribute stubs.
- Avoid duplicating entries already present locally.

Integration tests:
- `textDocument/codeAction` advertises the whole-file action when the translation file is incomplete.
- Applying the returned edit adds missing top-level messages.
- Applying the returned edit adds missing attributes to an existing message.
- Applying the returned edit leaves the whole translation file parseable via `fluent_syntax::parser::parse`.
- Applying the returned edit does not change the rendered text of untouched translated messages.
- No action is advertised when the translation file is already complete.

Neovim smoke:
- Add `tests/nvim/code_action_fill_missing_keys/`.
- Open a translation file with one missing message and one missing attribute.
- Request the code action, apply the edit, and assert the buffer text matches the expected stubs.
- After applying the action, assert the full buffer still parses and that an untouched translated message still formats to the same text as before.
- README must name the exact scenario-local fixture file and how the action is triggered.

Examples to add:
- `example/locales/en/app.ftl`: include a message with multiple attributes.
- `example/locales/fr/app.ftl` or `example/locales/es/app.ftl`: omit one message and one attribute so the action has something to fill.

### 3. Code action to complete a whole file by copying source text with `# [LSP-COPY]`

Feature shape:
- Separate code action from the empty-stub action above.
- Insert the origin-language content directly into the translation file.
- Prepend each copied message with a top-level `# [LSP-COPY]` marker comment so the copied text is visible and machine-detectable later.
- When only a missing attribute is copied, place the marker inside the message block at the same indentation level as the attribute.
- Keep the action standard LSP code-action/edit behavior.

Concrete examples:

Origin file:
```ftl
hello = Hello World

menu-save =
    .label = Save
```

Translation file before:
```ftl
hello = Hola Mundo
```

Expected code action title:
```text
Copy missing keys and attributes from source
```

Expected result:
```ftl
hello = Hola Mundo

# [LSP-COPY]
menu-save =
    .label = Save
```

Case with only a missing attribute:
```ftl
download-action =
    .label = Descargar
    # [LSP-COPY]
    .tooltip = Download this build
```

Implementation plan:
- Reuse the same missing-entry diff used by the empty-stub action.
- Render copied entries from the origin AST/source text rather than reconstructing them loosely.
- Attach a top-level `# [LSP-COPY]` marker directly above each copied message.
- For copied attributes, insert an indented `# [LSP-COPY]` marker inside the parent message immediately above the copied attribute.
- Keep the output deterministic so later diagnostics and edits can target the marker cleanly.

Unit tests:
- Copy a wholly missing message with the marker comment.
- Copy a missing attribute with the marker comment.
- Preserve origin text exactly inside the copied body.
- Keep marker placement stable across repeated runs.
- Avoid adding duplicate markers for already copied content.

Integration tests:
- The copy action is advertised separately from the empty-stub action.
- Applying the edit inserts the expected comment marker and copied source text.
- Applying the edit leaves the whole translation file parseable via `fluent_syntax::parser::parse`.
- Applying the edit makes the copied message or attribute format to the same user-visible text as the origin entry.
- Existing translated entries are not overwritten.
- Already-complete files do not advertise the action.

Neovim smoke:
- Add `tests/nvim/code_action_copy_missing_keys/`.
- Exercise one missing message and one missing attribute.
- Apply the edit and assert that `# [LSP-COPY]` comments are inserted in the expected places.
- After applying the action, assert the full buffer parses and the copied entry formats to the expected text.

Examples to add:
- Example workspace entry where one locale intentionally relies on copied source text for a missing item.
- Example README note explaining that copied content is intentionally marked and should be translated later.

### 4. Warning diagnostic for `# [LSP-COPY]` markers

Feature shape:
- Add a warning diagnostic whenever a translation file still contains an `# [LSP-COPY]` marker.
- The warning is there to catch untranslated copied content left behind.
- Diagnostics should use standard `publishDiagnostics`.
- The warning should point at the marker comment line, not guess at the following message span.

Concrete examples:

Input:
```ftl
# [LSP-COPY]
hello = Hello World
```

Expected diagnostic:
```text
[warning] Entry still contains an `# [LSP-COPY]` marker
```

Marker on an attribute:
```ftl
download-action =
    .label = Descargar
    # [LSP-COPY]
    .tooltip = Download this build
```

Expected behavior:
- warn on the comment line
- do not emit a second duplicate warning for the same copied block

Implementation plan:
- Scan source text for top-level `# [LSP-COPY]` marker lines during document diagnostics.
- Also scan for indented `# [LSP-COPY]` marker lines inside message blocks.
- Emit warning diagnostics independently from plural/style diagnostics so the feature can be enabled on its own.
- Keep matching strict to the literal marker to avoid false positives on normal translator comments.

Unit tests:
- Detect a standalone copied-message marker.
- Detect a copied-attribute marker.
- Point the diagnostic at the marker line span.
- Ignore ordinary comments.
- Allow multiple independent markers in one file.

Integration tests:
- Saving a file with a marker publishes the warning.
- Removing the marker and saving clears the warning.
- Marker-bearing fixtures remain whole-file parseable before and after the marker is removed.
- A file without markers stays clean.

Neovim smoke:
- Add `tests/nvim/diagnostics_lsp_copy_markers/`.
- Open a file with one copied-message marker and one copied-attribute marker.
- Save and assert the warning count/messages.
- Remove the marker, save again, and assert the warning clears.
- After the marker is removed, assert the affected message still parses and formats to the expected text.

Examples to add:
- Example translation file containing one intentional `# [LSP-COPY]` block.
- Example README note telling users to save after editing so diagnostics refresh.

### 5. Code action on a single key to copy from source with `# [LSP-COPY]`

Feature shape:
- Offer a targeted code action when the cursor is on a missing key site or on an existing key that is missing one or more attributes.
- Copy only the selected message or attribute from the origin file.
- Prefix the inserted copied content with a top-level `# [LSP-COPY]` marker.
- For copied attributes, use an indented marker inside the parent message.
- This is the fine-grained version of the whole-file copy action, not a replacement for it.

Concrete examples:

Origin file:
```ftl
hello = Hello World

download-action =
    .label = Download
    .tooltip = Download this build
```

Translation file before:
```ftl
hello =

download-action =
    .label = Descargar
```

Cursor on `hello`, expected code action title:
```text
Copy `hello` from source
```

Expected result:
```ftl
# [LSP-COPY]
hello = Hello World

download-action =
    .label = Descargar
```

Cursor on `download-action`, expected code action title:
```text
Copy missing attributes for `download-action` from source
```

Expected result:
```ftl
download-action =
    .label = Descargar
    # [LSP-COPY]
    .tooltip = Download this build
```

Implementation plan:
- Reuse the same origin/translation diff model as the whole-file actions.
- Scope the advertised action to the message or attribute under the cursor.
- For a wholly missing key, insert the copied entry in the right origin-order position.
- For a missing attribute, patch only the relevant message block.

Unit tests:
- Advertise the single-key copy action on a wholly missing message.
- Advertise the single-key copy action on a message missing one attribute.
- Suppress the action when the key is already complete.
- Generate the expected copied text with marker placement.

Integration tests:
- `textDocument/codeAction` on a missing key advertises the single-key copy action.
- Applying the edit inserts only the selected copied entry.
- `textDocument/codeAction` on an incomplete message advertises the missing-attribute copy action.
- Applying the edit inserts only the missing attributes.
- Applying each edit leaves the whole translation file parseable via `fluent_syntax::parser::parse`.
- Applying each edit makes the copied message or attribute format to the same user-visible text as the origin entry.

Neovim smoke:
- Add `tests/nvim/code_action_copy_single_key/`.
- Exercise one wholly missing key and one partially missing key.
- Apply the action and assert the buffer text exactly matches the expected copied block.
- After applying the action, assert the full buffer parses and the copied entry formats to the expected text.

Examples to add:
- One top-level message example.
- One attribute example using `.tooltip` inside `download-action =` or `.label` inside `menu-save =`.

## Cross-cutting testing plan

Shared implementation/testing notes:
- Keep all behaviors inside standard LSP: `textDocument/completion`, `textDocument/codeAction`, and `textDocument/publishDiagnostics`.
- Every user-visible feature needs:
  - unit coverage for parsing, matching, and edit generation
  - raw LSP integration coverage in `tests/integration_lsp.rs`
  - a dedicated Neovim smoke scenario under `tests/nvim/<feature_name>/`
  - a scenario-local `README.md` documenting feature, exercise, assumptions, and source-under-test
- For edit-producing tests, "done" means the test applies the LSP edit, re-parses the whole resulting document with `fluent_syntax::parser::parse`, and fails loudly on any syntax error.
- For preview-producing tests, "done" means the test also compares the exercised message or attribute's rendered text through a shared semantic formatter helper, preferably backed by `FluentBundle::format_pattern`.
- Navigation-only tests can stay focused on locations and ranges, but their scenario-local source fixtures should still be syntactically valid Fluent.
- Prefer self-contained workspace fixtures inside each Neovim scenario directory instead of relying on external fixture trees for the exact file under test.

Suggested new Neovim scenario directories:
- `tests/nvim/completion_origin_keys/`
- `tests/nvim/code_action_fill_missing_keys/`
- `tests/nvim/code_action_copy_missing_keys/`
- `tests/nvim/diagnostics_lsp_copy_markers/`
- `tests/nvim/code_action_copy_single_key/`

Suggested example-workspace updates:
- Extend [example/README.md](/home/codex/workspace/fluent-lsp/example/README.md) with a short section for completion and source-copy workflows.
- Add at least one file pair where the translation intentionally omits a key.
- Add at least one file pair where the translation has the message but not one attribute.
- Add at least one file pair where a copied source block with `# [LSP-COPY]` is left in place so the warning can be exercised manually.
