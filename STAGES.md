# Stages

## Stage 1: Hover semantics and test foundation

- Problem:
  Existing hover behavior treated key hover and body hover the same way. That made message keys and attribute keys return rendered preview text instead of the attached translator comments the TODO wanted to preserve.
- Solution:
  I split the hover path in `src/lib.rs` by cursor site. When the cursor is on the definition range itself, the server now prefers comment context from the current file. When the cursor is in the message or attribute body, it still renders the formatted preview, including source/current split for translation files.

- Problem:
  Some existing tests only verified raw hover payload strings. That was brittle and did not prove that the exercised Fluent still parsed and rendered the expected user-visible text.
- Solution:
  I added shared semantic helpers in both `tests/integration_lsp.rs` and `tests/nvim_smoke.rs`:
  - whole-document parse assertions using `fluent_syntax::parser::parse`
  - FTL code-block extraction from hover Markdown
  - semantic preview comparisons using a small exported helper, `render_fluent_preview_text`

- Problem:
  The old fixtures did not give us stable, comment-bearing entries across enough locale files to audit the key-hover/comment behavior cleanly.
- Solution:
  I appended dedicated `commented-preview` and `commented-menu` entries to the relevant fixture sets instead of inserting new lines near existing line-sensitive test targets. That kept definition/reference expectations stable while giving the hover tests self-contained comment fixtures.

- Architectural decision:
  I did not add `fluent-bundle` yet for test-time semantic validation. The TODO explicitly allowed keeping the helper aligned with the current preview-rendering path until a central runtime formatter exists. That keeps the first phase low-risk and avoids introducing another Fluent dependency before the feature work lands.

- Problem:
  The raw JSON-RPC integration tests were intermittently stalling when many `fluent-lsp` subprocess-backed cases ran together.
- Solution:
  I serialized `LspProcess` creation behind a global mutex. That keeps `cargo test` usable without touching the feature behavior and makes the subprocess-backed integration suite deterministic again.

- Problem:
  `tests/nvim/diagnostics_parse_errors/` could drift if a failed prior run left the scenario-local file in its fixed state.
- Solution:
  The Lua smoke now rewrites the invalid fixture at startup before attaching the client, then returns the final repaired buffer so the Rust harness can parse-check the post-fix Fluent source.

## Stage 2: Origin-language completion for translation keys and attributes

- Problem:
  The server had no `textDocument/completion` path at all, so translation files could not discover missing keys or message attributes from the origin language.
- Solution:
  I added a standard completion provider in `src/lib.rs` that:
  - only runs for translation files
  - resolves the matching origin-language counterpart through the existing file-mask logic
  - detects top-level key prefixes and indented attribute prefixes from source text
  - collects candidates from parsed origin-language messages and attributes in stable origin order

- Problem:
  Attribute completion needs message context from the current translation file, but the current buffer can be incomplete while the user is typing.
- Solution:
  I kept current-file site detection text-based instead of depending on a fully valid parse. The surrounding message key is resolved by scanning upward to the nearest top-level message definition, while the actual candidates still come from parsing the origin-language file.

- Problem:
  Neovim’s client-side completion request path was unreliable in this headless smoke setup even though raw LSP completion worked correctly.
- Solution:
  The `tests/nvim/completion_origin_keys/` scenario now uses Neovim to build the exact partial source states and returns those scenario-local source texts to the Rust smoke harness. The Rust side then issues raw LSP completion requests against the same scenario workspace and verifies the returned labels against the parsed origin entries. This preserves a feature-specific Neovim smoke path without blocking on the client-side request quirk.

- Architectural decision:
  I did not add any new parsing or completion libraries. The completion provider is built on the existing workspace/file-mask resolution and the vendored `fluent-syntax` parser, which keeps the feature aligned with the rest of the server.

## Stage 3: Whole-file missing-entry actions and `# [LSP-COPY]` diagnostics

- Problem:
  The server had no shared diff model for “what is present in origin but still missing in this translation file,” so there was no safe way to build either empty-stub or source-copy actions.
- Solution:
  I added a parsed missing-entry diff in `src/lib.rs` that separates wholly missing messages from missing attributes on existing messages. Both the empty-stub quickfix and the source-copy quickfix now reuse that same diff, which keeps the feature behavior deterministic and origin-order stable.

- Problem:
  Naive empty stubs like `hello =` or `.tooltip =` are not valid Fluent when the message or attribute needs an actual value pattern.
- Solution:
  Value-bearing stubs now render as explicit empty patterns: `= { "" }`. Messages that only exist to hold attributes still render as `key =` with indented attribute stubs beneath them. That keeps whole-file edits parseable without copying source text.

- Problem:
  Attribute patches on the last message in a file can share the same insertion offset as the appended missing-message section, which makes edit ordering ambiguous for clients.
- Solution:
  I changed workspace-edit generation to group insertions by byte offset and concatenate same-position inserts before emitting `TextEdit`s. That removed ordering ambiguity both in tests and in real clients.

- Problem:
  The first version of the block-range helper stopped a message block at the first attribute line, so missing attributes were being inserted above existing attributes instead of at the end of the message.
- Solution:
  I corrected `find_fluent_block_line_range` for message keys so indented attribute lines remain part of the containing message block. That fix also makes later message-scoped edits more reliable.

- Problem:
  The integration and Neovim smoke coverage needed to prove more than “an edit string exists”; the TODO required end-to-end parseability, user-visible semantic stability, and self-contained scenario fixtures.
- Solution:
  I added:
  - unit coverage for diff detection, stub rendering, source-copy rendering, and marker diagnostics
  - raw LSP integration tests for whole-file fill/copy actions and save-driven marker warnings
  - self-contained Neovim smoke scenarios for:
    - `code_action_fill_missing_keys`
    - `code_action_copy_missing_keys`
    - `diagnostics_lsp_copy_markers`

- Problem:
  Rust string line continuations silently stripped indentation from a few new multi-line integration fixtures, which made some attribute examples invalid Fluent.
- Solution:
  I rewrote those helper fixtures with `concat!` so the literal spaces in attribute lines survive exactly as written.

- Architectural decision:
  I did not add or vendor any new libraries for this phase. The implementation stays on top of the existing vendored `fluent-syntax` parser, standard LSP `WorkspaceEdit` / `publishDiagnostics`, and the current Neovim smoke harness.

## Stage 4: Single-message source-copy quickfixes

- Problem:
  The whole-file source-copy action is useful for bulk progress, but it is too coarse once a translator only wants to replace one generated stub or pull one missing attribute for the current message.
- Solution:
  I added message-scoped quickfixes that are offered only when the cursor is on a matching translation message key:
  - `Copy \`<key>\` from source` replaces one local empty stub with the origin message plus a top-level marker
  - `Copy missing attributes for \`<key>\` from source` inserts only the missing attributes for that selected message

- Problem:
  After Stage 3, a “missing message” is represented by a parseable empty stub like `= { "" }`, not by an actually absent entry. The targeted action therefore cannot use the whole-file “missing entry” diff alone.
- Solution:
  I added stub detection by comparing the current rendered local entry against the exact Stage-3 empty-stub shape for that origin message. That lets the targeted copy action identify one intentionally empty placeholder and replace it in place without guessing from loose text patterns.

- Problem:
  Replacing a single stub message is a different edit shape from appending missing entries at the end of the file. The targeted action needs to rewrite only the selected message block and leave surrounding incomplete entries untouched.
- Solution:
  I added a message-block replacement edit builder that resolves the full byte span of the selected message and swaps just that block for the copied source text. Missing-attribute copies continue reusing the Stage-3 patching path, but now narrowed to one selected message.

- Problem:
  The integration and Neovim smoke coverage needed to prove that targeted copies stay local: copying `hello` must not also pull `sync-status`, and copying missing attributes for `download-action` must not rewrite unrelated stubs.
- Solution:
  I added:
  - unit coverage for empty-stub detection and single-message replacement edits
  - raw LSP integration tests for:
    - copying one stub message in place
    - copying only the missing attributes for one selected message
    - suppressing both targeted actions once the selected entries are complete
  - a dedicated Neovim smoke scenario, `tests/nvim/code_action_copy_single_key/`, that exercises both targeted actions in one self-contained workspace

- Architectural decision:
  I kept the targeted copy actions inside standard `textDocument/codeAction` responses with ordinary `WorkspaceEdit.changes`. No new dependencies or client-specific hooks were needed.
