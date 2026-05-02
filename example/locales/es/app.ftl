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
