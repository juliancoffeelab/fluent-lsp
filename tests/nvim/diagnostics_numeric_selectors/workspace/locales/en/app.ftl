bad-key =
    Count { $count } { $count ->
        [admins] nope
        [one] ok
       *[other] ok
    }

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

local-whole-coins = You have { $coins } { $coins ->
    [one] coin
   *[other] coins
} now.
