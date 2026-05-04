# Example Workspace

This directory is a repo-local workspace for trying `fluent-lsp` by opening the repository root as your editor workspace.

The top-level [fluent-lsp.toml](/home/codex/workspace/fluent-lsp/fluent-lsp.toml) points at this `example/` tree, so you can open files under `example/locales/` directly and exercise the server without setting up a separate project.

## Files To Try

- `example/locales/es/app.ftl`
  - Go to definition on `welcome-title`, `-brand-name`, or `.label`
  - Hover `welcome-title` for the compact local-language preview
  - Hover inside a value like `welcome-body` or `[female]` under `install-hint` to verify source-first hover output separated from the current preview by `---`
  - Hover `mismatch-rollout` inside `[female]`, `[0]`, or `[1]` to verify that source/local hover matches shared selector names, carries exact numeric selector values, ignores local-only selectors on the source side, and defaults source-only selectors to `*`
  - Run the CodeLens on `install-hint` to open the full selector combinations document in Spanish
  - Run the CodeLens on `download-action.tooltip` to verify attribute-level selector combinations
  - Request a code action on `coins-line` to generate a selector from a plain message
  - Request a code action on `{ $coins }` in `coins-line` to generate variable-targeted `prefix`, `whole`, and `suffix` forms
  - Request a code action on `plain-count` to verify whole-only snippet generation with a placeholder variable
  - Request a code action on `formatted-download` or `$downloads` to verify nested `NUMBER(...)` anchors use the enclosing placeable for `prefix`, `whole`, and `suffix`
  - Request a code action on `NUMBER($downloads)` to select on the function call itself
  - Request a code action on `deep-download` to verify nested function-call selectors like `WRAP(NUMBER($downloads))`
  - Request a code action on `coins-period` to verify generated branch bodies keep trailing punctuation attached
  - Request a code action on `whole-coins`, `prefix-coins`, or `suffix-coins` to exercise selector rewrite actions between the three shapes
  - Request a code action on `bare-suffix-coins` to verify only `Convert selector to prefix form` is distinct
  - Request a code action inside the `[female]` branch of `nested-whole-coins` to verify nested selector rewrites target the local branch pattern instead of the whole message
  - Type `miss` on a new line and request completion to pull `missing-demo` from the English origin file
  - Request `Add missing keys and attributes from source` to fill the missing `missing-demo` entry and the missing `source-copy-card.tooltip` attribute with parseable stubs
  - Request `Copy missing keys and attributes from source` to copy the same origin entries directly into Spanish with `# [LSP-COPY]` markers
  - Save the file while `release-notes` still has its `# [LSP-COPY]` marker to verify the warning diagnostic, then remove the marker and save again to clear it
- `example/locales/en/app.ftl`
  - Run references from `welcome-title` or `.label`
  - Hover `install-hint` on the key line for default selector choices, or inside a specific branch for explicit selector choices
  - Use the matching English `coins-line`, `whole-coins`, `prefix-coins`, `suffix-coins`, and `nested-whole-coins` entries to inspect the same code-action shapes in the origin language
  - This is the denser top-level file: terms, message values, multiple attributes, selector-generation examples, and selector-rewrite examples
  - `incomplete-rollout` is the English “missing `other`” example for diagnostics
- `example/locales/fr/app.ftl`
  - Extra translation target for references and locale comparisons
- `example/locales/lv/app.ftl`
  - Hover `zero-rollout` inside `[zero]` or `[one]` to verify category-style selector matching across source and Latvian local text
  - Enable `warn_on_missing_plural_categories` and save `incomplete-zero` to verify a Latvian missing-`zero` warning
  - Enable `error_on_unsupported_plural_categories` and save `bad-zero` to verify an exact-span error on `[few]`
- `example/locales/ar/app.ftl`
  - Use `arabic-rollout` when you want a locale whose supported plural-category set is wider than English or Latvian
  - Enable diagnostics to verify `zero/one/two/few/many/other` are treated as valid Arabic category names
- `example/locales/uk/app.ftl`
  - Extra translation target for references and locale comparisons
  - Enable `warn_on_missing_plural_categories` or `error_on_unsupported_plural_categories` to verify Unicode-backed Ukrainian categories like `few` and `many` are accepted
- `example/locales/es/dialogs/menu.ftl`
  - Go to definition on `menu-save.label` to verify locale-tree resolution for nested attributes
- `example/locales/en/dialogs/menu.ftl`
  - Hover `menu-save.label` to verify plain attribute preview rendering on a nested file
  - This is the file with top-level comment structure
- `example/locales/fr/dialogs/menu.ftl`
  - Extra nested translation target
- `example/locales/lv/dialogs/menu.ftl`
  - Extra nested translation target
- `example/locales/uk/dialogs/menu.ftl`
  - Extra nested translation target

## Assumptions

- Your editor opens this repository root as the workspace root.
- The LSP client reads `fluent-lsp.toml` from the repo root.
- The client supports standard LSP hover, definition, references, and CodeLens.
- Diagnostic settings can be sent through LSP client configuration or added to `fluent-lsp.toml`:
  - `error_on_unsupported_plural_categories = true`
  - `warn_on_missing_plural_categories = true`
  - `warn_on_selector_style_mismatch = true`
- Numeric-selector diagnostics are published on save, not on every edit, so use `:write` or an equivalent save action after changing a file.
- The `# [LSP-COPY]` warning also refreshes on save, so save after translating or removing copied source blocks.
- This example workspace is intentionally small, but it keeps multiple translation targets where they are useful for trying references and cross-locale behavior.
