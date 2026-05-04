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
