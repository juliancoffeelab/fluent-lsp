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

# Comment-only hover coverage
# Keep this translator guidance visible on key hover
commented-preview = Preview text for hover comments.
