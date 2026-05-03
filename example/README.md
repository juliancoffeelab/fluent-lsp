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
- `example/locales/en/app.ftl`
  - Run references from `welcome-title` or `.label`
  - Hover `install-hint` on the key line for default selector choices, or inside a specific branch for explicit selector choices
  - This is the denser top-level file: terms, message values, multiple attributes, and both message-level and attribute-level selectors
- `example/locales/fr/app.ftl`
  - Extra translation target for references and locale comparisons
- `example/locales/lv/app.ftl`
  - Hover `zero-rollout` inside `[zero]` or `[one]` to verify category-style selector matching across source and Latvian local text
- `example/locales/uk/app.ftl`
  - Extra translation target for references and locale comparisons
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
- This example workspace is intentionally small, but it keeps multiple translation targets where they are useful for trying references and cross-locale behavior.
