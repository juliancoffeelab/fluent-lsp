arabic-rollout =
    { $count ->
        [zero] لا عناصر
        [one] عنصر واحد
        [two] عنصران
        [few] بضعة عناصر
        [many] عناصر كثيرة
       *[other] { $count } عنصر
    }.
