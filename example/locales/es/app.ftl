# Texto que ve la persona usuaria al entrar
welcome-title = Bienvenido
welcome-body = Abre la ultima build de { -brand-name } y sigue donde lo dejaste.
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

# Objetivo simple para generar selector a partir de una variable directa
coins-line = Tienes { $coins } monedas.

# Objetivo simple para generar selector sin variable existente
plain-count = Monedas disponibles.

# Caso con referencia anidada dentro de NUMBER(...)
formatted-download = Descarga { NUMBER($downloads) } archivos.

# Caso con llamadas a funciones anidadas alrededor de la variable
deep-download = Descarga { WRAP(NUMBER($downloads)) } archivos.

# Caso para verificar que la puntuacion siga pegada al texto generado
coins-period = Tienes { $coins }.

# Objetivo de reescritura en forma whole
whole-coins = { $coins ->
    [one] Tienes { $coins } moneda.
   *[other] Tienes { $coins } monedas.
}

# Objetivo de reescritura en forma prefix
prefix-coins = Tienes { $coins } { $coins ->
    [one] moneda.
   *[other] monedas.
}

# Objetivo de reescritura en forma suffix
suffix-coins = Tienes { $coins ->
    [one] { $coins } moneda.
   *[other] { $coins } monedas.
}

# Forma ambigua sin prefijo externo: solo el colapso a prefix es distinto
bare-suffix-coins = { $coins ->
    [one] { $coins } moneda.
   *[other] { $coins } monedas.
}

# Objetivo de reescritura anidado dentro de otra rama selectora
nested-whole-coins =
    { $gender ->
        [female] { $coins ->
            [one] Ella tiene { $coins } moneda.
           *[other] Ella tiene { $coins } monedas.
        }
       *[other] Elle tiene { $coins } monedas.
    }

source-copy-card =
    .label = Revisar copia de origen

# [LSP-COPY]
release-notes = Latest release notes
