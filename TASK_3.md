# Task 3

Extended hover so it works on origin-language files as well as translations. When the hovered file is already in the origin language, the server uses the current document as both the lookup source and the hover source, then renders the same comments and selector-combination metadata that translation hovers receive.

Pros:
- Removes the asymmetric behavior where origin files had references but no useful hover.
- Reuses the same rendering and selector-expansion path as translation hover, which keeps behavior consistent.
- Avoids redundant file lookup when hovering the origin file itself.

Cons:
- The hover content is still origin-centric; it does not enumerate reverse translation coverage from that hover.
- Definition behavior remains translation-to-origin only, since that was outside the requested hover change.
