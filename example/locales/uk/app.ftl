# Текст, який бачить людина після входу
welcome-title = Ласкаво просимо
welcome-body = Відкрийте останню збірку { -brand-name } і продовжуйте роботу з того місця, де зупинилися.
-brand-name = Nightly
button-copy =
    .label = Запустити
    .tooltip = Відкрити найновішу збірку { -brand-name }
    .aria-label = Запустити { -brand-name }
install-hint =
    Скопіюйте посилання на завантаження для { $gender ->
        [female] її
        [male] його
       *[other] їхнього
    } облікового запису на { $count } { $count ->
        [one] пристрої
        [few] пристроях
        [many] пристроях
       *[other] пристроях
    } зараз.
# Основна дія на екрані завантажень
download-action =
    .label = Встановити збірку
    .accesskey = I
    .tooltip =
        Встановіть рекомендовану збірку для { $gender ->
            [female] її
            [male] його
           *[other] їхнього
        } облікового запису на { $count } { $count ->
            [one] пристрої
            [few] пристроях
            [many] пристроях
           *[other] пристроях
        } зараз.
sync-status =
    { $count ->
        [one] { $count } завантаження готове.
        [few] { $count } завантаження готові.
        [many] { $count } завантажень готово.
       *[other] { $count } завантаження готове.
    }
secondary-copy = Більше тексту
