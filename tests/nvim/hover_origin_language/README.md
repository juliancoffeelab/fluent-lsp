# hover_origin_language

## Feature

Hover directly on the origin-language file

## Exercise

Requests hover on a commented origin-language menu attribute key and then on that attribute body. It also requests hover on an uncommented origin-language message key and then on that message body. It asserts that commented key hover returns the menu comments, commented body hover shows the plain rendered preview block, uncommented key hover returns no result, and uncommented body hover still shows the rendered preview block.

## Assumptions

Origin-language key hover should prefer local comment context when it exists, and should return no hover when the key has no comments rather than falling through to the message preview. Body hover uses the current document source and does not require translation lookup.

## Source Under Test

`workspace/locales/en/dialogs/menu.ftl`

This scenario is self-contained. The source fixture and workspace config used by the smoke test live under this directory's `workspace/` tree.
