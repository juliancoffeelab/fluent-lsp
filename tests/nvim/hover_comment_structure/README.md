# hover_comment_structure

## Feature

Hover comment-vs-body split in an origin-language file

## Exercise

Requests hover on the origin-language `commented-preview` key and then on its value text. It asserts that key hover returns only the entry comments, while body hover returns only the rendered preview text.

## Assumptions

Hover on a definition key should prefer comment context when comments exist, while body hover should stay on rendered preview content.

## Source Under Test

`workspace/locales/en/app.ftl`

This scenario is self-contained. The source fixture and workspace config used by the smoke test live under this directory's `workspace/` tree.
