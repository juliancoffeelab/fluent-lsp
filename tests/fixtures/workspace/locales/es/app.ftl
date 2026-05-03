# Texto que ve la persona usuaria al entrar
welcome-title = Bienvenido
welcome-body = Abre la build mas reciente de { -brand-name } y sigue donde lo dejaste.
-brand-name = Nightly
button-copy =
    .label = Lanzar
    .tooltip = Abre la build mas nueva de { -brand-name }
    .aria-label = Lanzar { -brand-name }
install-hint =
    Copia el enlace de descarga para la cuenta de { $gender ->
        [female] ella
        [male] el
       *[other] elle
    } en { $count } { $count ->
        [one] dispositivo
       *[other] dispositivos
    } ahora.
# Accion principal en la pantalla de descargas
download-action =
    .label = Instalar build
    .accesskey = I
    .tooltip =
        Instala la build recomendada para la cuenta de { $gender ->
            [female] ella
            [male] el
           *[other] elle
        } en { $count } { $count ->
            [one] dispositivo
           *[other] dispositivos
        } ahora.
sync-status =
    { $count ->
        [one] Hay { $count } descarga lista.
       *[other] Hay { $count } descargas listas.
    }
secondary-copy = Mas texto

audience-rollout =
    Resumen para { $audience ->
        [admins] administradores
        [members] miembros
       *[others] otras personas
    } en { $platform ->
        [desktop] escritorio
       *[mobile] movil
    } con { $count } { $count ->
        [one] elemento
       *[other] elementos
    }.

mismatch-rollout =
    Resumen para { $gender ->
        [female] ella misma
        [male] el mismo
       *[other] elle misme
    } con { $count ->
        [0] ningun paquete
        [1] un paquete
       *[other] { $count } paquetes
    } listo.

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
