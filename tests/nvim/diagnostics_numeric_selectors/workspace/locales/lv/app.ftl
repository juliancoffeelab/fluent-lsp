bad-zero =
    { $count ->
        [few] slikti
       *[other] labi
    }

incomplete-zero =
    { $count ->
        [one] viena pakotne
       *[other] pakotnes
    }

theme-label = { $theme ->
    [dark] tumss
   *[light] gaiss
}
