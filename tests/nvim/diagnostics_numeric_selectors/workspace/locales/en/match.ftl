match-rollout =
    { $count ->
        [one] one package
       *[other] { $count } packages
    }
