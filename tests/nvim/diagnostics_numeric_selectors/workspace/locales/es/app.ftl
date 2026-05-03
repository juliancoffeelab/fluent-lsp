whole-coins = { $coins ->
    [one] Tienes { $coins } moneda.
   *[other] Tienes { $coins } monedas.
}

prefix-coins = Tienes { $coins } { $coins ->
    [one] moneda.
   *[other] monedas.
}

suffix-coins = Tienes { $coins ->
    [one] { $coins } moneda.
   *[other] { $coins } monedas.
}
