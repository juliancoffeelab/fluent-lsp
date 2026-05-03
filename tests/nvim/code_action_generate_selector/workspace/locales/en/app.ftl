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
formatted-download = Download { NUMBER($downloads) } files.
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
