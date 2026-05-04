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
