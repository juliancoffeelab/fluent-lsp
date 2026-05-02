# workspace_locale_tree

## Feature

Locale-tree path resolution via {lang}/{filepath}

## Exercise

Opens a nested translation file and asserts that definition jumps to the matching nested origin-language file.

## Assumptions

The scenario-local workspace config uses the template-based locale tree layout.

## Source Under Test

`workspace/locales/es/dialogs/menu.ftl`

This scenario is self-contained. The source fixture and workspace config used by the smoke test live under this directory's `workspace/` tree.
