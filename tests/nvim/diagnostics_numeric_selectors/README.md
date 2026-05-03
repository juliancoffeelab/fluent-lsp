# diagnostics_numeric_selectors

Numeric selector diagnostics

This scenario covers the diagnostics layer for numeric selectors.
It verifies these client-visible behaviors:

- diagnostics are absent by default
- enabling unsupported-category and missing-category diagnostics produces warnings/errors for Latvian numeric selectors
- unsupported plural categories point at the offending category key and use error severity
- missing locale categories use warning severity
- Ukrainian cardinal categories are resolved from Unicode plural-rule data, so `few` and `many` are accepted
- numeric selectors reject arbitrary identifier keys such as `[admins]`
- enabling preferred-style diagnostics reports both top-level and local mismatched selector styles when the preferred style is `prefix`
- closing a diagnosed buffer clears its diagnostics instead of leaving stale markers behind
- a whole-style English file with a complete category set stays quiet when style warnings and missing-category warnings are enabled

How the scenario is exercised:

- writes a local `fluent-lsp.toml` with only `origin_language` and `file_masks`
- opens `workspace/locales/lv/app.ftl`
- asserts there are no diagnostics before the first save
- sends `workspace/didChangeConfiguration` enabling:
  - `error_on_unsupported_plural_categories`
  - `warn_on_missing_plural_categories`
- saves the Latvian file and checks the resulting four diagnostics/messages/severities
- opens `workspace/locales/uk/app.ftl`
- attaches the existing client to the new buffer
- saves the Ukrainian file and checks that it reports only a missing `one` category and does not reject `few` or `many`
- opens `workspace/locales/en/app.ftl`
- attaches the existing client to the new buffer
- sends `workspace/didChangeConfiguration` enabling:
  - `error_on_unsupported_plural_categories`
  - `warn_on_selector_style_mismatch`
  - `selector_style = "prefix"`
- saves the English file and checks the diagnostics:
  - one invalid numeric-key error on `[admins]`
  - two `whole`-style warnings, one of them from a local selector occurrence
  - one `suffix`-style warning
- opens an empty buffer, deletes the diagnosed English buffer, and waits for its diagnostics to clear
- opens `workspace/locales/en/match.ftl` and confirms it stays at zero diagnostics when the selector style matches and the category set is complete

Assumptions:

- Neovim builtin diagnostics are enabled
- the server publishes diagnostics on save and clears them on `didClose`

Source under test:

- `tests/nvim/diagnostics_numeric_selectors/workspace/locales/en/app.ftl`
- `tests/nvim/diagnostics_numeric_selectors/workspace/locales/en/match.ftl`
- `tests/nvim/diagnostics_numeric_selectors/workspace/locales/lv/app.ftl`
- `tests/nvim/diagnostics_numeric_selectors/workspace/locales/uk/app.ftl`
- `tests/nvim/diagnostics_numeric_selectors/workspace/fluent-lsp.toml`
