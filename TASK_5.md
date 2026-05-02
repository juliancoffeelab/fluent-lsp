# Task 5

This task is a follow-up cleanup pass over hover, selector expansion, and the repo-local example workspace.

## Plan 1

Remove the `Comments:` header from hover output and rename `Entry:` to `Source:`.

Implementation:
- Change hover Markdown assembly so comment blocks render directly as a Fluent fence when comments exist, without a section label above them.
- Rename the source section label from `Entry:` to `Source:` in both hover output and selector-combination documents so the wording matches the actual source snippet being shown.
- Update string-based tests and the dedicated Neovim hover smoke scenarios to assert the new rendering.

Verification:
- Rust hover integration tests for translation and origin files.
- Neovim smoke tests for hover on translation, origin, and comment-structure scenarios.

Risks:
- This is mostly string-shape churn, so the main risk is stale expected output in tests and docs.

## Plan 2

Show selector combinations in the hovered document language instead of always using the origin-language source.

Implementation:
- Split selector expansion lookup into two paths:
  - hover preview expands from the current document text
  - definition/reference/origin lookup behavior stays origin-based where that is still the intended semantic
- Keep the existing fallback behavior sane when the current translation cannot be expanded, but prefer the hovered language whenever possible.
- Apply the same language-local behavior to the CodeLens full-combination document, since the lens is attached to the current file.

Verification:
- Rust integration coverage for hover on a translation file with translated selector text.
- Rust integration coverage for CodeLens `showDocument` content in a non-origin language.
- Dedicated Neovim CodeLens smoke scenario still asserting translated output, not origin output.

Risks:
- Translation files can diverge structurally from the origin file, so expansion has to use the current file carefully and not assume the same selector tree exists everywhere.

## Plan 3

Fix attribute hover so hovering an attribute shows the attribute source, not the whole parent message or term.

Implementation:
- Teach hover-source rendering to isolate the targeted attribute when the hovered key is `message.attr` or `-term.attr`.
- Preserve surrounding comment behavior:
  - free comments above the parent entry should still show
  - inline entry comments should still show
  - the source block itself should contain only the hovered attribute definition
- Keep message and term hover behavior unchanged for non-attribute keys.

Verification:
- Add or update unit tests for rendering hovered source for message attributes and term attributes.
- Add integration coverage that hovers an attribute and checks only the attribute source block is rendered.
- Add or extend a dedicated Neovim smoke scenario that exercises attribute hover directly.

Risks:
- Fluent attributes are nested under a parent entry in the AST, so rendering only the attribute may require constructing a focused synthetic source snippet instead of serializing the whole entry unchanged.

## Plan 4

Prove that selector expansion works correctly for attributes too.

Implementation:
- Reuse the attribute-aware pattern lookup path so both hover preview and CodeLens full expansion can target attribute values.
- Add at least one example attribute with selector-based content.
- Ensure CodeLens placement and title generation work when the selectable value belongs to an attribute rather than a top-level message value.

Verification:
- Unit or integration test for selector counting on an attribute pattern.
- Integration test for attribute hover selector preview.
- Integration test for CodeLens `workspace/executeCommand` on an attribute.
- Dedicated Neovim smoke coverage for the attribute-selector path if the existing CodeLens scenario cannot cover it cleanly.

Risks:
- Attribute keys already resolve for definition and hover lookup, but the full selector feature path may still assume top-level entries in subtle ways.

## Plan 5

Make the repo-local example workspace substantially richer without adding pointless files.

Implementation:
- Keep the current compact file count, but increase feature density inside those files.
- Expand the example files so they cover:
  - plain messages
  - terms
  - attributes
  - selector-bearing message values
  - selector-bearing attributes
  - nested locale-tree files
  - top-level resource/group/comment structure in at least one file
- Make the locale texts intentionally distinct enough that hover-language bugs are obvious by inspection.
- Update `example/README.md` so each notable key is called out by feature, not just by file.

Verification:
- Sanity-check the example tree against the actual smoke scenarios so the repo-local demo stays aligned with tested behavior.
- Re-run the full Rust and Neovim test suite after the example refresh.

Risks:
- Richer example files can accidentally become noisy fixtures. The goal is denser coverage, not random filler.

## Agreed Plan

1. Hover becomes comments-only.
- No source/body text in hover.
- Source comments first.
- If local comments also exist, insert `---` and show local comments after.
- If only one side has comments, show just that side.
- For origin-language files, hover shows only source comments.

2. Add inlay hints for source previews.
- Implement `textDocument/inlayHint`.
- For plain values and attributes, show the full source snippet inline with no truncation.
- For selector-bearing values and attributes, resolve through the default branch for every selector.
- Prefix selector previews like:
  - `src [platform=*, action=*]: Press Ctrl + V`
- For non-select values:
  - `src: Welcome`
  - `src: .label = Save`

3. Fix attribute handling.
- Hover on an attribute must target the attribute only.
- Inlay hint on an attribute must preview the attribute only.
- Selector preview on an attribute must also use the attribute value only.
- No more rendering the whole parent message when hovering `.label` or `.tooltip`.

4. Keep full combinations in CodeLens + `showDocument`.
- CodeLens remains the “show all combinations” path.
- Full combinations should be rendered in the local language, not always origin English.
- Attribute selector combinations need to work there too.
- Selector examples should not encode different logic across branches; they should vary presentation only, with all variants preserving the same semantic meaning.

5. Strengthen tests.
- Add Rust integration coverage for:
  - hover comments-only shape
  - source-first/local-last comment ordering
  - inlay hints on plain message, attribute, selector message, and selector attribute
  - local-language full combinations in `showDocument`
- Add or extend Neovim smoke coverage for:
  - inlay hints
  - attribute selector behavior
  - new hover rendering

6. Make the example workspace richer.
- Keep the small file count.
- Add realistic comments and more attributes.
- Include selector-bearing attributes and nested attribute cases.
- Keep locale texts distinct enough that local-vs-source behavior is obvious.
- Repo-root example `fluent-lsp.toml` already uses `hover_selector_combinations_limit = 20`.

7. Review and fix everything touched by this feature area.
- Do a full pass over hover rendering, inlay hints, selector expansion, CodeLens output, tests, docs, and example files.
- Fix inconsistencies, stale wording, broken assumptions, and poor example semantics even if they are not called out elsewhere in this task.
- Treat this as a cleanup-and-correctness pass over the whole feature set, not a minimal patch for only the explicitly listed bugs.

## Completion

Completed in the current working tree.

- Hover is comments-only, with source comments first and local comments last.
- Source previews moved to inlay hints, including selector-bearing attributes and messages.
- Inlay hints now anchor to the emitted source line end and are filtered on that real position.
- CodeLens full selector documents use the current document language and work for attributes.
- Selector examples were rewritten so variants change presentation, not meaning.
- The repo example and test fixtures were expanded with denser real-world attributes and comments.
- Rust integration coverage and dedicated Neovim smoke coverage were updated for the new hover, inlay hint, attribute, and selector behavior.
- `cargo clippy --all-targets -- -D warnings` and `cargo test` both pass after the review fixes.
