- Add code action for for generating selectors for numbers.
If you execute it on a message, it should return you a snippet with $ where you type a variable to select on.
If you execute it on a variable, it should do the same, put identical message in each branch, but use your variable for matching.
In case of a variable, it can do like, suffix thing, where it would turn:
It should unroll into all plural categories for that language.
```ftl
coins = You have { $coins } coins.
```
Into
```
coins = You have { $coins ->
    [one] { $coins } coin.
    *[other] { $coins } coins.
}
```
Or whole
```
coins = { $coins
    [one] You have { $coins } coin.
    *[other] You have { $coins } coins.
}
```
Or prefix
```
coins = You have { $coins } { $coins ->
    [one] { $coins } coin.
    *[other] { $coins } coins.
}
```
This behaviour should be configurable in fluent-lsp.toml or in LSP settings protocol.
fluent-lsp.toml should override editor settings.
UPD: the config field is selector_style, make it `prefix` by default.
- Add a code action to convert from prefix/suffix form into whole.
- If parsed and clearly can collapse whole into prefix or suffix, advertise two actions for that.
- If parsed and clearly can collapse suffix form into prefix, advertise code action for that.
- Add a config field to error for unsuported plural categories in the language (off by default), needs heuristics.
- Add a config field to warn if selector matches not enough number categoris (off by default), needs heuristics on whether it matches numbers.
- Add a config field about selector_style with whole/suffix/prefix and warn if items dont use these proper.

Heuristic note:
- A selector is considered number-like the moment any branch key is either:
  - a supported plural category for the current language, like `zero`, `one`, `two`, `few`, `many`, `other`
  - or a numeric literal like `0`, `1`, `2`
- Once a selector is number-like, plural-category validation and completeness checks apply to the whole selector.
- Reusing number-category names for non-number selectors is not a case we should optimize around.

Span note:
- Variant and variant-key spans are available from the vendored `fluent-syntax` parser.
- Diagnostics for bad categories should point at the exact variant-key token span, not a regex/source-text guess.
- Rewrite/delete fixes that operate on a whole branch may use the full `Variant.span`.

## Proposed Testing Plan

### 1. Number selector generation code action

Feature shape:
- Add a code action for generating selectors for numbers.
- On a plain message, return a snippet with a `$...` placeholder for the selector variable.
- On a variable usage, generate the selector from that variable and duplicate the message text into each branch.
- Unroll into all plural categories for the file language.
- Support `whole`, `suffix`, and `prefix` output styles.
- `selector_style` config exists in both `fluent-lsp.toml` and LSP settings.
- `fluent-lsp.toml` overrides editor settings.
- Default `selector_style` is `prefix`.

Concrete examples:

Message case, default `prefix`:
```ftl
coins = You have { $coins } coins.
```

Code action menu:
```text
Generate number selector (prefix)
```

Snippet result:
```ftl
coins = You have { $${1:count} } { $${1:count} ->
    [one] coin
    *[other] coins
}
```

Variable-under-cursor case:
```ftl
coins = You have { $coins } coins.
                 ^ cursor on $coins
```

Code action menu:
```text
Generate number selector from $coins (prefix)
Generate number selector from $coins (whole)
Generate number selector from $coins (suffix)
```

Expected `prefix` edit:
```ftl
coins = You have { $coins } { $coins ->
    [one] coin
    *[other] coins
}
```

Expected `whole` edit:
```ftl
coins = { $coins ->
    [one] You have { $coins } coin.
    *[other] You have { $coins } coins.
}
```

Expected `suffix` edit:
```ftl
coins = You have { $coins ->
    [one] { $coins } coin.
    *[other] { $coins } coins.
}
```

Function-call anchor note:
```ftl
downloads = Download { NUMBER($downloads) } files.
```

Current conservative behavior:
- detect `$downloads` as the selector variable
- only advertise `whole`
- do not advertise `prefix` or `suffix` while the anchor is still inside `NUMBER(...)`

Current safe result:
```ftl
downloads = { $downloads ->
    [one] Download { NUMBER($downloads) } files.
    *[other] Download { NUMBER($downloads) } files.
}
```

Possible future extension:
- allow `prefix` or `suffix` when we can promote the anchor from the nested variable token to the enclosing safe placeable span
- for example, treat `{ NUMBER($downloads) }` as the split point rather than `$downloads` itself

Latvian example:
```ftl
packages = Ir { $packages } pakotnes.
```

Expected category expansion:
```ftl
packages = Ir { $packages } { $packages ->
    [zero] pakotnes
    [one] pakotne
    *[other] pakotnes
}.
```

Numeric-literal detection example:
```ftl
packages = { $packages ->
    [0] No packages
    [1] One package
   *[other] { $packages } packages
}
```

Expected behavior:
- this is treated as number-like because `0` and `1` are numeric literal keys
- category completeness checks still apply
- unsupported-category checks still apply if mixed with bad plural-category keys

Plural-category detection example:
```ftl
packages = { $packages ->
    [one] One package
   *[other] { $packages } packages
}
```

Expected behavior:
- this is treated as number-like because `one` and `other` are plural-category keys

Required tests:

Unit tests:
 [x] Detect that the current entry is eligible for number-selector generation.
 [x] Detect the variable-under-cursor case versus plain-message case.
 [x] Build snippet output for `prefix`.
 [x] Build snippet output for `suffix`.
 [x] Build snippet output for `whole`.
 [x] Build snippet output for a language with only `one` and `other`.
 [x] Build snippet output for a language with `zero`, `one`, and `other`.
[] Build snippet output for a language with larger sets like `one`, `few`, `many`, `other`.
 [x] Preserve surrounding indentation and trailing punctuation.
 [x] Preserve existing placeables inside the duplicated text.
 [x] Ensure the default branch is starred exactly once.
 [x] Ensure exact numeric variants are not emitted when the feature is supposed to use plural categories.
 [x] Resolve effective `selector_style` with precedence:
 [x] built-in default
 [x] editor settings only
 [x] `fluent-lsp.toml` only
 [x] both set, with file config winning

Integration tests:
 [x] `textDocument/codeAction` on a plain message returns the generate-selector action.
 [x] `textDocument/codeAction` on a number variable returns the variable-driven generate-selector action.
 [x] Returned edit/snippet for English expands to `one` and `other`.
 [x] Returned edit/snippet for Latvian expands to `zero`, `one`, `other`.
 [x] Returned edit/snippet respects `selector_style=prefix`.
 [x] Returned edit/snippet respects `selector_style=suffix`.
 [x] Returned edit/snippet respects `selector_style=whole`.
 [x] Editor settings alone affect the result when file config is absent.
 [x] File config overrides editor settings when both are present.
 [x] Non-Fluent files do not advertise the action.
 [x] Messages with no clear number candidate do not advertise the action.

Neovim smoke:
 [x] Dedicated scenario for generating a selector from a plain message.
 [x] Dedicated scenario for generating a selector from a variable occurrence.
 [x] Scenario asserting default `prefix` output.
 [x] Scenario asserting file-config override over client settings.
 [x] Scenario using Latvian to verify `[zero]`, `[one]`, `*[other]`.

Examples to add:
 [x] English example showing generated `prefix` output from `coins = You have { $coins } coins.`
 [x] English example showing generated `whole` output for the same message.
 [x] English example showing generated `suffix` output for the same message.
 [x] Latvian example showing generated categories including `zero`.

### 2. Conversion code action: prefix/suffix -> whole

Concrete examples:

Prefix input:
```ftl
coins = You have { $coins } { $coins ->
    [one] coin
    *[other] coins
}
```

Code action menu:
```text
Convert selector to whole form
```

Expected result:
```ftl
coins = { $coins ->
    [one] You have { $coins } coin.
    *[other] You have { $coins } coins.
}
```

Suffix input:
```ftl
coins = You have { $coins ->
    [one] { $coins } coin.
    *[other] { $coins } coins.
}
```

Expected result:
```ftl
coins = { $coins ->
    [one] You have { $coins } coin.
    *[other] You have { $coins } coins.
}
```

Unit tests:
 [x] Parse a prefix-form message and normalize it into whole form.
 [x] Parse a suffix-form message and normalize it into whole form.
 [x] Refuse malformed or ambiguous forms.
 [x] Preserve comments, indentation, and attribute scoping.

Integration tests:
 [x] `textDocument/codeAction` advertises conversion for valid prefix form.
 [x] `textDocument/codeAction` advertises conversion for valid suffix form.
 [x] Applying the edit produces the expected whole form.
 [x] Invalid or ambiguous entries do not advertise the action.

Neovim smoke:
 [x] Scenario for prefix -> whole.
 [x] Scenario for suffix -> whole.

Examples to add:
 [x] One whole-form example next to matching prefix/suffix forms so the intended normalization is obvious.

### 3. Conversion code action: whole -> prefix/suffix

Concrete examples:

Whole-form input:
```ftl
coins = { $coins ->
    [one] You have { $coins } coin.
    *[other] You have { $coins } coins.
}
```

Code action menu when both are possible:
```text
Convert selector to prefix form
Convert selector to suffix form
```

Expected prefix result:
```ftl
coins = You have { $coins } { $coins ->
    [one] coin
    *[other] coins
}
```

Expected suffix result:
```ftl
coins = You have { $coins ->
    [one] { $coins } coin.
    *[other] { $coins } coins.
}
```

Case where only prefix should be advertised:
```ftl
coins = { $coins ->
    [one] About { $coins } coin total.
    *[other] About { $coins } coins total.
}
```

Pseudo UI:
```text
Convert selector to prefix form
```

Unit tests:
 [x] Parse whole-form selector and detect whether prefix conversion is valid.
 [x] Parse whole-form selector and detect whether suffix conversion is valid.
 [x] Refuse conversion when branch text diverges in a way that cannot collapse cleanly.
 [x] Preserve punctuation and placeables around the collapsed selector.

Integration tests:
 [x] Advertise both actions when both prefix and suffix are valid.
 [x] Advertise only one action when only one collapse is valid.
 [x] Applying each edit yields the expected normalized form.

Neovim smoke:
 [x] Scenario where both prefix and suffix are offered.
 [x] Scenario where only one collapse action is offered.

Examples to add:
 [x] One whole-form example that can become both prefix and suffix.
 [x] One whole-form example that can become only prefix.

### 4. Conversion code action: suffix -> prefix

Concrete example:

Input:
```ftl
coins = You have { $coins ->
    [one] { $coins } coin.
    *[other] { $coins } coins.
}
```

Code action menu:
```text
Convert selector to prefix form
```

Expected result:
```ftl
coins = You have { $coins } { $coins ->
    [one] coin
    *[other] coins
}
```

Unit tests:
 [x] Detect suffix forms that can collapse to prefix.
 [x] Refuse suffix forms that cannot collapse cleanly.

Integration tests:
 [x] Advertise suffix -> prefix only when structurally valid.
 [x] Applying the edit yields the expected prefix form.

Neovim smoke:
 [x] Dedicated suffix -> prefix scenario.

### 5. Warning/error config: unsupported plural categories

Feature shape:
- Config field is off by default.
- Should report unsupported plural categories for the language.
- Needs heuristics, so tests should cover both positive and suppressed cases.

Concrete examples:

Latvian bad category example:
```ftl
zero-rollout =
    { $count ->
        [few] bad
        *[other] ok
    }
```

Pseudo diagnostic:
```text
[error] `few` is not a supported plural category for `lv`
```

Mixed literal/category example that should also count as number-like:
```ftl
zero-rollout =
    { $count ->
        [0] exact zero
        [few] bad
       *[other] ok
    }
```

Pseudo diagnostic:
```text
[error] `few` is not a supported plural category for `lv`
```

Arabic supported-set example:
```ftl
arabic-rollout =
    { $count ->
        [zero] ...
        [one] ...
        [two] ...
        [few] ...
        [many] ...
       *[other] ...
    }
```

Unit tests:
[] Map locale to supported plural-category set.
[] Detect unsupported category in a numeric selector.
[] Ignore supported categories.
[] Ignore non-numeric selectors when heuristics say “not number-like”.
[] Default config is off.
[] Use exact AST variant-key spans for the highlighted unsupported-category diagnostic range.

Integration tests:
[] Diagnostics are absent by default.
[] Diagnostics appear when enabled.
[] Diagnostics point at the unsupported category token.
[] Latvian and Arabic-like cases use the correct language-specific category sets.

Neovim smoke:
[] Scenario with diagnostics off.
[] Scenario with diagnostics on.

Examples to add:
[] A Latvian example with an intentionally unsupported category.
[] An Arabic example where the supported category set is wider than English.

### 6. Warning config: selector matches not enough number categories

Feature shape:
- Config field is off by default.
- Needs heuristics to decide whether a selector is meant to be numeric.

Concrete examples:

English incomplete selector:
```ftl
coins = { $coins ->
    [one] One coin
}
```

Pseudo diagnostic:
```text
[warning] Numeric selector for `en` is missing fallback category `other`
```

Latvian incomplete selector:
```ftl
packages = { $packages ->
    [one] viena pakotne
    *[other] { $packages } pakotnes
}
```

Pseudo diagnostic:
```text
[warning] Numeric selector for `lv` is missing category `zero`
```

Non-numeric selector that should not warn:
```ftl
theme-label = { $theme ->
    [dark] Dark
   *[light] Light
}
```

Selector that becomes numeric because of literals alone:
```ftl
packages = { $packages ->
    [0] No packages
    [1] One package
}
```

Pseudo diagnostic:
```text
[warning] Numeric selector is missing category `one`
[warning] Numeric selector is missing fallback category `other`
```

Selector that becomes numeric because of category names alone:
```ftl
packages = { $packages ->
    [one] One package
}
```

Pseudo diagnostic:
```text
[warning] Numeric selector is missing fallback category `other`
```

Exhaustiveness rule:
- Exact numeric literals like `[0]` and `[1]` do not by themselves prove that a selector is exhaustive for the locale.
- If a selector contains numeric literals but does not contain the needed plural categories for the locale, it should still warn.
- Only suppress the warning when we can actually prove the selector is exhaustive for that locale.

Concrete example:
```ftl
packages = { $packages ->
    [0] No packages
    [1] One package
   *[other] { $packages } packages
}
```

Expected behavior for English:
- still warn that `one` is missing, because exact `[1]` is not the same thing as proving the plural category coverage is complete
- do not treat `[1]` as automatically satisfying the language category requirement unless we explicitly implement that proof

Unit tests:
[] Heuristic recognizes obvious number selectors like `$count`, `$coins`, `$items`.
[] Heuristic does not flag clearly non-numeric selectors.
[] Missing required categories for the locale emit warning when enabled.
[] Complete category sets do not emit warning.
[] Use exact AST variant-key spans for missing/invalid-category related diagnostic ranges where applicable.

Integration tests:
[] Diagnostics absent by default.
[] Diagnostics emitted when enabled for incomplete numeric selectors.
[] Diagnostics suppressed for non-numeric selectors.

Neovim smoke:
[] Scenario for incomplete numeric selector warning.
[] Scenario proving non-numeric selector is ignored.

Examples to add:
[] English incomplete example missing `other`.
[] Latvian incomplete example missing `zero`.

### 7. Style warning config: whole/suffix/prefix

Feature shape:
- Config field defines preferred selector style.
- Warn when entries do not use the preferred style.

Concrete examples:

Preferred `prefix`, but actual `whole`:
```ftl
coins = { $coins ->
    [one] You have { $coins } coin.
    *[other] You have { $coins } coins.
}
```

Pseudo diagnostic:
```text
[warning] Selector style is `whole`, but workspace prefers `prefix`
```

Preferred `whole`, but actual `suffix`:
```ftl
coins = You have { $coins ->
    [one] { $coins } coin.
    *[other] { $coins } coins.
}
```

Pseudo diagnostic:
```text
[warning] Selector style is `suffix`, but workspace prefers `whole`
```

Unit tests:
[] Detect `whole` style.
[] Detect `prefix` style.
[] Detect `suffix` style.
[] Ignore entries that do not match any recognized style cleanly.
[] Emit warning only when detected style differs from preferred style.

Integration tests:
[] Diagnostics for preferred `prefix` against `whole`.
[] Diagnostics for preferred `whole` against `suffix`.
[] No diagnostics when style matches preference.
[] `fluent-lsp.toml` overrides LSP settings here too.

Neovim smoke:
[] Scenario with preferred `prefix`.
[] Scenario with preferred `whole`.

Examples to add:
[] One example trio showing the same message in whole/prefix/suffix form.

### 8. Cross-cutting coverage I would add

Pseudo UI examples:

Code action menu on a simple message:
```text
Generate number selector (prefix)
Generate number selector (whole)
Generate number selector (suffix)
```

Code action menu on a convertible whole-form selector:
```text
Convert selector to prefix form
Convert selector to suffix form
Convert selector to whole form
```

Diagnostics gutter example:
```text
packages = { $packages ->
    [one] viena pakotne
    *[other] { $packages } pakotnes
}
          ^ warning: Numeric selector for `lv` is missing category `zero`
```

Unit tests:
[] Config precedence between file config and LSP settings for every new setting.
[] Snippet placeholder escaping for `$` and braces.
[] Attribute-scoped code actions.
[] Term-scoped code actions.
[] Multi-line messages with comments above them.

Integration tests:
[] Code actions in translation files use that file language, not origin language.
[] Code actions in origin files use origin-language plural categories.
[] Nested locale-tree files behave the same as top-level files.

Neovim smoke:
[] At least one scenario should validate the actual applied edit result, not only action advertisement.
[] Every new code-action scenario needs a self-contained fixture and README under `tests/nvim/`.
