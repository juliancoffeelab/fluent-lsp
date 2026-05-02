# hover_comment_structure

## Feature

Hover selector defaults

## Exercise

Requests hover on a selector-bearing origin-language message key and asserts that the hover output shows default selector choices plus the formatted message preview.

## Assumptions

Hover on the message key itself should use default selector branches for every unresolved selector.

## Source Under Test

`workspace/locales/en/app.ftl`

This scenario is self-contained. The source fixture and workspace config used by the smoke test live under this directory's `workspace/` tree.
