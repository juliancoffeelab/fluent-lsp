# hover_origin_language

## Feature

Hover directly on the origin-language file

## Exercise

Requests hover on an origin-language menu attribute key and then on that attribute body. It asserts that key hover returns the menu comments and that body hover shows the plain rendered preview block.

## Assumptions

Origin-language key hover should prefer local comment context when it exists. Body hover uses the current document source and does not require translation lookup.

## Source Under Test

`workspace/locales/en/dialogs/menu.ftl`

This scenario is self-contained. The source fixture and workspace config used by the smoke test live under this directory's `workspace/` tree.
