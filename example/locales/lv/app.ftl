zero-rollout =
    Kopsavilkums ar { $count ->
        [zero] neviena pakotne nav gatava
        [one] viena pakotne ir gatava
       *[other] { $count } pakotnes ir gatavas
    }.

bad-zero =
    { $count ->
        [few] slikti
       *[other] labi
    }.

incomplete-zero =
    { $count ->
        [one] viena pakotne
       *[other] pakotnes
    }.
