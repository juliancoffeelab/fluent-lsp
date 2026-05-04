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
    }

incomplete-zero =
    { $count ->
        [one] viena pakotne
       *[other] pakotnes
    }

coins-line = Tev ir { $coins } monetas.
plain-count = Pakotnes pieejamas.

# Hover komentaru parklajums
# Saglabat so piezimi redzamu atslegas hover skata
commented-preview = Hover komentaru prieksskata teksts.
