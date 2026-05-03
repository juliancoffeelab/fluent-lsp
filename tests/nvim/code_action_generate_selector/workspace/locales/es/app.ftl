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
# Comentario para verificar que la reescritura no toque el comentario
commented-download =
    .tooltip = { $files ->
        [one] Descarga { $files } archivo.
       *[other] Descarga { $files } archivos.
    }
install-hint =
    Copia el enlace de descarga para la cuenta de { $gender ->
        [female] ella
        [male] el
       *[other] elle
    } en { $count } { $count ->
        [one] dispositivo
       *[other] dispositivos
    } ahora.
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
