# hover_origin_language

## Feature

Hover directly on the origin-language file

## Exercise

Requests hover on an origin-language menu attribute and asserts that the formatted attribute value is shown as a plain preview block.

## Assumptions

Origin-language hover uses the current document source and does not require translation lookup.

## Source Under Test

`workspace/locales/en/dialogs/menu.ftl`

This scenario is self-contained. The source fixture and workspace config used by the smoke test live under this directory's `workspace/` tree.
