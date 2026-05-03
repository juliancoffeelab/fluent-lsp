zero-rollout =
    Kopsavilkums ar { $count ->
        [zero] neviena pakotne nav gatava
        [one] viena pakotne ir gatava
       *[other] { $count } pakotnes ir gatavas
    }.
