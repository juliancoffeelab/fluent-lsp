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
    } en { $count ->
        [one] un dispositivo
       *[other] varios dispositivos
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
        } en { $count ->
            [one] un dispositivo
           *[other] varios dispositivos
        } ahora.
sync-status =
    { $count ->
        [one] Hay 1 descarga lista.
       *[other] Hay varias descargas listas.
    }
secondary-copy = Mas texto
