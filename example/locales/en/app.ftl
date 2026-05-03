# Shown on the welcome screen
welcome-title = Welcome

# Supporting copy below the main heading
welcome-body = Open the latest { -brand-name } build and pick up where you left off.

# Product brand
# Keep this in title case
-brand-name = Nightly

# CTA copy
# Keep it short
button-copy =
    .label = Launch
    .tooltip = Open the newest { -brand-name } build
    .aria-label = Launch { -brand-name }

# Shortcut reminder near the download button
install-hint =
    Copy the download link for { $gender ->
        [female] her
        [male] his
       *[other] their
    } account on { $count } { $count ->
        [one] device
       *[other] devices
    } now.

# Primary install action in the downloads panel
download-action =
    .label = Install build
    .accesskey = I
    .tooltip =
        Install the recommended build for { $gender ->
            [female] her
            [male] his
           *[other] their
        } account on { $count } { $count ->
            [one] device
           *[other] devices
        } now.

# Status summary above the downloads list
sync-status =
    { $count ->
        [one] { $count } download is ready.
       *[other] { $count } downloads are ready.
    }

secondary-copy = More text

mismatch-rollout =
    Summary for { $platform ->
        [desktop] desktop
       *[mobile] mobile
    } users with { $count ->
        [0] no packages
        [1] one package
       *[other] { $count } packages
    } ready.

zero-rollout =
    Zero summary: { $count ->
        [zero] no packages ready
        [one] one package ready
       *[other] { $count } packages ready
    }.

# Plain generation target with a direct variable placeable
coins-line = You have { $coins } coins.

# Plain generation target without any existing variable
plain-count = Coins available.

# Generation target with a nested variable reference inside a function call
formatted-download = Download { NUMBER($downloads) } files.

# Generation target with nested function calls around the variable
deep-download = Download { WRAP(NUMBER($downloads)) } files.

# Generation target that keeps punctuation attached in branch bodies
coins-period = You have { $coins }.

# Rewrite target in whole form
whole-coins = { $coins ->
    [one] You have { $coins } coin.
   *[other] You have { $coins } coins.
}

# Rewrite target in prefix form
prefix-coins = You have { $coins } { $coins ->
    [one] coin.
   *[other] coins.
}

# Rewrite target in suffix form
suffix-coins = You have { $coins ->
    [one] { $coins } coin.
   *[other] { $coins } coins.
}

# Ambiguous zero-prefix suffix/whole shape: only prefix collapse is distinct
bare-suffix-coins = { $coins ->
    [one] { $coins } coin.
   *[other] { $coins } coins.
}

# Nested rewrite target inside an outer selector branch
nested-whole-coins =
    { $gender ->
        [female] { $coins ->
            [one] She has { $coins } coin.
           *[other] She has { $coins } coins.
        }
       *[other] They have { $coins } coins.
    }
