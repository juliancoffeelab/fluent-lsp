# Task 2

Replaced the single `english_file` model with `origin_language` plus templated `file_masks` using `{lang}` and `{filepath}`. The server now matches locale files by template, derives the shared relative filepath for nested trees, resolves the origin-language counterpart dynamically, and scopes references to sibling translations with the same locale-tree path.

Pros:
- Supports nested locale trees instead of a single flat origin file.
- Removes hard-wiring to English and makes the server language-agnostic at the config level.
- Keeps resolution deterministic because origin and translation files are derived from the same mask template.

Cons:
- `file_masks` are now structured templates rather than loose globs, so existing configs must be updated.
- The current placeholder model is intentionally narrow: exactly one `{lang}` and one `{filepath}` per mask.
