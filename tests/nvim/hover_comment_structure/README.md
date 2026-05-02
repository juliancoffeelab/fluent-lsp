# hover_comment_structure

## Feature

Hover comment structure preservation

## Exercise

Requests hover on a nested origin-language menu attribute and asserts that resource, group, and regular comments are preserved in the hover output without any source-body block.

## Assumptions

The local scenario source keeps the multi-level Fluent comments intact.

## Source Under Test

`workspace/locales/en/dialogs/menu.ftl`

This scenario is self-contained. The source fixture and workspace config used by the smoke test live under this directory's `workspace/` tree.
