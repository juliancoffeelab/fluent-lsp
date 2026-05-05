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

audience-rollout =
    Summary for { $audience ->
        [admins] admins
        [members] members
       *[others] other people
    } on { $platform ->
        [desktop] desktop
       *[mobile] mobile
    } with { $count } { $count ->
        [one] item
       *[other] items
    }.

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

coins-line = You have { $coins } coins.
plain-count = Coins available.
range-summary = Between { $min } and { $max } items.
nested-coins =
    { $gender ->
        [female] She has { $coins } coins.
       *[other] They have { $coins } coins.
    }
download-count =
    .tooltip = Download { $files } files.
# Comment used to prove rewrite edits do not consume surrounding comments
commented-download =
    .tooltip = { $files ->
        [one] Download { $files } file.
       *[other] Download { $files } files.
    }
formatted-download = Download { NUMBER($downloads) } files.
deep-download = Download { WRAP(NUMBER($downloads)) } files.
coins-period = You have { $coins }.
whole-coins = { $coins ->
    [one] You have { $coins } coin.
   *[other] You have { $coins } coins.
}
prefix-coins = You have { $coins } { $coins ->
    [one] coin.
   *[other] coins.
}
suffix-coins = You have { $coins ->
    [one] { $coins } coin.
   *[other] { $coins } coins.
}
bare-suffix-coins = { $coins ->
    [one] { $coins } coin.
   *[other] { $coins } coins.
}
nested-whole-coins =
    { $gender ->
        [female] { $coins ->
            [one] She has { $coins } coin.
           *[other] She has { $coins } coins.
        }
       *[other] They have { $coins } coins.
    }

# Comment-only hover coverage
# Keep this translator guidance visible on key hover
commented-preview = Preview text for hover comments.

empty-preview = English empty preview fallback.
