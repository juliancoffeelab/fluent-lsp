coins-line = Tienes { $coins } monedas.
plain-count = Monedas disponibles.
range-summary = Entre { $min } y { $max } elementos.
nested-coins =
    { $gender ->
        [female] Ella tiene { $coins } monedas.
       *[other] Elle tiene { $coins } monedas.
    }
download-count =
    .tooltip = Descarga { $files } archivos.
formatted-download = Descarga { NUMBER($downloads) } archivos.
deep-download = Descarga { WRAP(NUMBER($downloads)) } archivos.
coins-period = Tienes { $coins }.
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
bare-suffix-coins = { $coins ->
    [one] { $coins } moneda.
   *[other] { $coins } monedas.
}
nested-whole-coins =
    { $gender ->
        [female] { $coins ->
            [one] Ella tiene { $coins } moneda.
           *[other] Ella tiene { $coins } monedas.
        }
       *[other] Elle tiene { $coins } monedas.
    }
