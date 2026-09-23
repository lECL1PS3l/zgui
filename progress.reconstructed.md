# Zapret GUI — журнал прогресса

## Цель
Portable GUI (Tauri v2, Rust + Web UI) для zapret2 (winws2) и Flowseal/zapret-discord-youtube (winws):
единый лаунчер профилей-стратегий обоих движков + автообновление **конфигов** с официальных репозиториев.
**Движки обоих авторов вшиты в exe** — программа автономна, интернет нужен только для обновлений
доменов/конфигов. Данные рядом с exe, официальные конфиги тоже вшиты.

## Критерий «готово»
- [x] Бэкенд компилируется без ошибок/предупреждений (`cargo check` чистый, 0 предупреждений).
- [x] Release-сборка собирается: `src-tauri\target\release\zgui.exe` (v0.3.0, portable).
- [x] Стили подключаются в production (CSS в dist, `import "./styles.css"` в main.js).
- [x] Юнит-тест встраивания конфигов проходит (`embedded::tests::seeds_the_official_config_catalog`).
- [x] Тест стратегий и геоблок-источники реализованы, unit-тесты tester (4) проходят.
- [x] Движки обоих авторов вшиты в exe, авто-распаковка при первом запуске (тест `embedded_engines_contain_exe`).
- [x] Автосоздание конфига из результатов теста (`build_auto_profile`, тест `auto_profile_adds_hostlist`).
- [x] Предупреждения о VPN и оригинальных .bat/.lua (bootstrap.warnings, баннер).
- [ ] Пользователь запускает exe: окно открывается, движки уже «готов».
- [ ] «Скачать движок» для flowseal и zapret2: скачивание с GitHub, распаковка, корень определяет winws.exe/winws2.exe.
- [ ] Обновления: каталог конфигов проверяется, файлы применяются с бэкапом в `.backups`.
- [ ] Запуск профиля: UAC-повышение, pid определяется, остановка давит процесс.

## Стек и решения
- Tauri v2 (Rust), фронт — vanilla JS + Vite, тёмная тема. Крейты: reqwest (blocking), zip, sha2, tokio, url.
- Оба движка: flowseal (winws.exe) и zapret2 (winws2.exe).
- Конфиги (whitelist): flowseal — `lists/*` (кроме user-/ipset-all при ручном ipset), `.service/{version,ipset-service,hosts}`,
  стратегии `*.bat` (кроме service.bat) в каталог GUI; zapret2 — `config.default` + `lua/{zapret-lib,antidpi,auto,obfs,pcap,tests}.lua` + списки.
- Бэкапы: engine-файлы → `<dest>/.backups/<ts>/`, каталоговые (catalog_only) → `catalog/.backups/<ts>/`.
- Применённые хэши: `catalog/applied.json`. Статусы: ok / avail / new / modified / err / skip-user.
- Запуск: elevated PS-launcher (Start-Process -Verb RunAs -PassThru), pid-файл, taskkill /F /T.
- Служба: `sc create zapret` (binPath-exe + args, `zgui-strategy` в реестре), удаление — sc delete.
- Фоновые Windows-команды (`sc`, `reg`, `tasklist`, `powershell`) — с `CREATE_NO_WINDOW`
  (`hidden_command` в runner.rs), чтобы не мигали консоли.
- Portable: `config::portable_data_dir()` — `data/` рядом с exe; WebView2-кэш — `data/webview`
  (окно создаётся вручную из `WebviewWindowBuilder::data_directory`).

## Сделано
- Исследованы репо bol-van/zapret2 и Flowseal/zapret-discord-youtube (структуры, lists/lua/.service/*.bat, релизы).
- Каркас: package.json, vite.config.js, tauri.conf.json (identifier `dev.zgui.desktop`, окно `create:false` — создаётся в setup), Cargo.toml, build.rs, capabilities/default.json.
- Rust: config.rs (state.json, Roots, Settings, Profile, UpdEntry, sha256/atomic_write/tail/find_exe, portable_data_dir),
  profiles.rs (парсер *.bat flowseal, пресеты zapret2, game filter ports), runner.rs (elevated-лаунчер, pid, hidden_command),
  service.rs (sc create/delete/query, reg zgui-strategy), updater.rs (каталог конфигов, check_all, apply_updates,
  applied.json, apply_hosts в системный hosts), lib.rs (все команды + watcher 2с + автопроверка + fetch_engine с unzip).
- Иконки: assets/app-icon.png (System.Drawing) → npx tauri icon → src-tauri/icons.
- Фронтенд: index.html + src/main.js (импорт styles.css) + src/styles.css (Стратегии / Обновления / Журнал / Настройки).
- Исправлены 19+ ошибок компиляции (поколение 1-5): reqwest blocking feature, raw-string `"#` в hosts-скрипте,
  движ-паттерны Result, заимствования, временные State, PathBuf/String, dead code (service_args удалён,
  catalog_only применён в backup, builtin_flowseal_presets подключён в ensure_presets).
- Первое тестирование (скриншот): CSS не подключался + мигали консоли → исправлено (импорт CSS, hidden_command).
- Portable: все данные в `data\` рядом с exe (catalog/engines/logs/tmp/webview). Проверка записи + понятная ошибка.
- Встроенные конфиги: `src-tauri/resources/flowseal-main.zip` (1 633 838 Б) и `zapret2-master.zip` (761 918 Б)
  скачаны системным curl; `embedded.rs` — `include_bytes!` + `seed_catalog` (только недостающие файлы, каталог
  `data/catalog/sources/`), `copy_missing`/`copy_tree_missing` в движки, `seed_flowseal_configs`/`seed_zapret2_configs`,
  `reload_bats_from_disk`, запись `catalog/source-info.txt`.
- `app_info()` отдаёт `{version, portable:true, bundledConfigs}`; в UI метка «portable».
- updater: добавлен `zapret2/config.default`, статус auto (приоритет + сообщение). WebView2-кэш портативен.
- Юнит-тест `seeds_the_official_config_catalog` — проходит (general.bat, list-general.txt, hosts, zapret-lib.lua, files/fake).
- Версия 0.2.0: **тест стратегий** (`tester.rs`, аналог теста в GUI Flowseal) — последовательный запуск каждого
  `*.bat`-профиля, TCP-проба контрольных доменов (443→80), подсчёт очков, сводка + **выбор лучшей** стратегии
  (`tests.json`, `set_best_strategy` → автостарт). Команды `test_strategies/test_status/test_cache/set_best_strategy`,
  событие `zgui:test`, 6 unit-тестов (summarize/groups/parsing/cache).
- Версия 0.2.0: **геоблок-списки** в каталоге обновлений — группа `geoblock domains` (itdoginfo/allow-domains:
  inside-raw, geoblock, block, news, youtube, discord, telegram, twitter, meta) и `geoblock ip`
  (runetfreedom/russia-blocked-geoip: text/ru-blocked, ru-blocked-community) → `data/catalog/geoblock/`.
  Домены из этих списков используются как контрольные в тесте стратегий.
- UI: карточка «Тест стратегий» (чекбоксы по каждому .bat, выбрать все, прогресс, результат по доменам,
  рекомендация лучшей + кнопка автостарта); бейджи «лучшая»/счёт у профилей; пояснение, что профиль Flowseal = .bat.
- Версия 0.3.0: **движки обоих авторов вшиты в exe** — `resources/engine-flowseal.zip` (1 508 077 Б,
  релиз 1.10.2) и `engine-zapret2.zip` (9 967 425 Б, релиз 1.0.5.2); `embedded::ensure_embedded_engine`
  распаковывает в `data/engines/<engine>` при первом запуске (`provision_engines` → root + seed конфигов).
  GUI автономен: интернет нужен только для обновлений доменов/конфигов.
- Версия 0.3.0: **автосоздание конфига** — `profiles::build_auto_profile` берёт лучшую стратегию из теста,
  подмешивает домены geoblock в `lists/list-auto.txt` (оба движка) и сохраняет профиль `auto-optimized`
  («Авто (на основе …)»). Команда `generate_auto_config`, кнопки «Создать конфиг авто».
- Версия 0.3.0: **предупреждения при запуске** — баннер на «Стратегиях» (`bootstrap.warnings`): VPN конфликтует
  с zapret (не блокируем, только предупреждаем); оригинальные .bat/.lua в корне движка надо отключить.
- Версия 0.5.0: **иконка** — круглый Z-бейдж без чёрных углов (`assets/app-icon-round.png`, прозрачный фон),
  перегенерён `npx tauri icon`.
- Версия 0.5.0: **тест стратегий в один UAC** — вместо 22 запросов `Start-Process -Verb RunAs` теперь один
  элевированный PS-раннер (`tester::write_test_runner` → `logs/test-runner.ps1`) последовательно запускает
  все `.bat` (winws без повторного RunAs), сам проверяет домены через `System.Net.Sockets.TcpClient`
  и пишет `logs/test-out.json`; Rust (`spawn_elevated_script`) поллит прогресс и транслирует в `zgui:test`.
- Версия 0.5.0 (фикс): **устранён дедлок** при «Запустить тест» — `set_test` вызывался, держа `testing.lock()`
  как временный аргумент, и лочил мьютекс повторно (вечное зависание + отсутствие UAC). Теперь прогресс
  формируется локально, guard сбрасывается до вызова `set_test`.
- Версия 0.6.0 (фикс теста, настоящая причина): сгенерированный `test-runner.ps1` писался как UTF-8 **без BOM**,
  PowerShell 5.1 читал кириллицу как ANSI → ParserError, скрипт не выполнялся (UAC был, тестов нет).
  Исправлено: `runner::write_ps1` (UTF-8 **с BOM**), а сам раннер сделан **ASCII-only**; `set_test`-дедлок
  устранён ещё в 0.5.0. Хвостовой маркер теперь `DONE` в UTF-8 (раньше `Out-File -Append` писал UTF-16 →
  JSON не парсился). Счёт доменов через `@(...).Count`.
  Добавлен **end-to-end тест** `runner_script_is_ascii_and_runs` — генерирует раннер, запускает его и
  проверяет `started/score` (ловит регрессию).
- Версия 0.6.0: **опция «Всегда запускать GUI от администратора»** — `Settings.always_admin`,
  авто-перезапуск с UAC при старте (`--elevated` защищает от цикла), кнопка «Перезапустить от админа»,
  в шапке метка `admin`; `app_info.elevated`. Frontend настроек переведён на snake_case (`always_admin` и т.д.).
- Версия 0.7.0: **VPN запрещён при тестах** — `service::detect_vpn` ищет клиентов (Happ, Amnezia(WG),
  WireGuard, OpenVPN, Outline, sing-box, Xray/v2ray, Clash/mihomo, Nekoray, Shadowsocks, Hysteria, tun2socks,
  Windscribe, NordVPN, …) + службы-туннели (WireGuardTunnel*, OpenVPNService*, AmneziaWG). В `test_strategies`
  при активном VPN возвращается `VPN_RUNNING`; фронт показывает окно «VPN мешает тесту» с кнопкой
  **«Выгрузить VPN и продолжить»** (UAC → `kill_conflicts` → автоповтор теста). В общей модалке конфликтов —
  отдельный жёлтый блок VPN и остановка VPN-служб. Команда `vpn_check`. Тесты: `detects_vpn_process_names`,
  `vpn_report_flags`, `conflict_report_flags`.
- Версия 0.6.1 (фикс): **неверный корень встроенных движков** — zip-архивы релизов содержат каталог-обёртку
  (`zapret-discord-youtube-1.10.2/`, `zapret2-v1.0.5.2/`), но `extract_all` считал структуру неоднородной
  (запись самого каталога без `/`), не срезал обёртку → корень указывал на `engines/<engine>`, а лаунчер
  собирал пути `root\bin`/`root\lists` не туда → **все стратегии «process exited immediately»**.
  Исправлено: `engine_root_for` находит реальный корень (каталог с `bin/` или `binaries/`), `provision_engines`
  нормализует уже сохранённый корень; тест `embedded_engines_contain_exe` проверяет наличие `bin/`/`binaries/`.
- Версия 0.8.0: исправлен реальный корень Flowseal/zapret2 после распаковки release zip. Раньше в `state.json`
  сохранялся каталог-обёртка `engines/<engine>`, а не `zapret-discord-youtube-<version>` / `zapret2-<version>`;
  из-за этого `bin/`, `lists/` и payload-файлы строились по несуществующему пути — стратегии завершались с
  `process exited immediately`. Теперь `set_root`, `fetch_engine_impl` и `provision_engines` нормализуют путь
  через `engine_root_for_public`, seed-конфиги кладутся в фактический корень. Тестовый PowerShell-раннер
  формирует единую quoted command line для аргументов с пробелами (например, путь `Z GUI`).
- Версия 0.8.0: отключается Flowseal `utils/check_updates.enabled` (`neutralize_author_autoupdate`). Этот флаг
  заставлял авторские `.bat` вызывать `service.bat check_updates`, который открывает GitHub release page в браузере.
  Наша GUI обновляет конфиги самостоятельно; флаг переименовывается в `.zgui_disabled` при запуске.
- Версия 0.8.0: фоновые PowerShell-скрипты теперь пишутся UTF-8 BOM (`write_ps1`), shell-окна запускаются скрыто,
  включая launcher winws и перезапуск GUI с UAC.
- Версия 0.8.0: добавлен раздел «Защищённый DNS»: IPv4-провайдеры Google, Cloudflare, AdGuard, Comss.one, Quad9,
  OpenDNS, Control D, NextDNS, DNS.SB, Mullvad; системный Windows DoH через `netsh dns ... encryption`,
  `autoupgrade=yes`, `udpfallback=no`, кнопка возврата к автоматическому DNS. DoT не имитируется — для него
  нужен отдельный локальный резолвер.
- Версия 0.8.0: состояние выбора Flowseal `.bat` исправлено: `null` означает «все выбраны по умолчанию»,
  а ручной набор хранится в `testPicked` и не сбрасывается периодическим `refreshAll`.
- Версия 0.8.0: добавлен встроенный расширенный список `resources/test-domains-russia.lst` (сотни доменов
  из предоставленного набора). Тест использует до 100 доменов и запускает TCP probes параллельно, чтобы
  расширенная проверка не умножала задержку на число доменов. До обновления geoblock-каталога тест всё равно работает.
- Версия 0.8.0: `hosts` удалён из updater/UI/Tauri-команд, журнал удалён из UI при сохранении внутренних логов.
  Движки вынесены на вкладку «Обновления», профили защищают авторские записи от удаления, args раскрываются
  по запросу, «Как служба» переименовано в toggle «Автозапуск».
- Версия 0.8.0: тест отображает агрегаты YouTube, Discord, Microsoft/Xbox, Google и Cloudflare; детали доменов
  находятся в spoiler. Успех зависит от большинства Discord/YouTube и обязательного скрытого `music.youtube.com`.
  Google AI исключён из Google-группы.
- Версия 0.8.0: **резкая остановка теста** — кнопка «Остановить тест» в карточке теста. Раннер пишет `test-runner.pid`
  (свой PID) и `test-current.pid` (активный winws); команда `cancel_test` ставит стоп-флаг `test-stop.flag` и через
  один UAC бьёт оба дерева `taskkill /F /T`. Поллер/раннер замечают флаг и завершаются без «лучшей стратегии».
  При перезапуске GUI `test_status` подхватывает живой фоновый раннер (running=true, кнопка стоп активна).
- Версия 0.8.0: ручной список доменов финализирован по решению пользователя (328 → 105):
  авто-отсев политики/СМИ, блогов/сообществ, науки/книг, сторонних API, торрентов, 18+,
  VPN/крипты/обхода, аналитики, почты/мессенджеров, `.ua`, хостинга/инфраструктуры,
  корпоративных/справки. Оставлены: крит-пакеты (twitter/x, instagram, discord, facebook,
  google, microsoft), кино/аниме/сериалы (даже пиратские), погода, фото/арт, софт/техно,
  хобби/игры, genius.com.
- Версия 0.8.0: тест разделён на два режима (`test_strategies(mode)`):
  * **основной** — обязательные (20) + ручной список, фильтруется по `zapret-reachable.lst`
    (если калибровка есть), выбирает лучшую стратегию;
  * **геоблок-тест** (кнопка «Геоблок-тест») — весь онлайн-список без лимита, все выбранные
    стратегии, базовая проба без Zapret (`test-baseline.json`), результат → `catalog/geoblock/
    zapret-reachable.lst` (обходится Zapret) и `vpn-only.lst` (не прошёл, но базовая проба видела),
    на «лучшую» не влияет.
- Версия 0.8.0: `load_domains_from_lists` больше не подмешивает онлайн-геоблок; добавлены
  `load_geoblock_domains`, `read_baseline`, `save_reachability`, `load_reachable`, `REQUIRED_DOMAINS`.
- Unit-тестов: 24 (DNS 2, tester 11, runner 1, service 4, profiles 5, embedded 3) — все проходят.
- Версия 0.8.0: **Telegram-прокси (tg-ws)** — встроен MIT-крейт `tg-ws-proxy-rs` 2.3.4
  (источник: AmantesNihilo/zapret-universal-interface, см. `THIRD_PARTY_NOTICES.md`).
  Скопирован в `src-tauri/crates/tg-ws-proxy-rs` (убраны android/openwrt/docs/bin/workspace).
  Новый модуль `src/telegram.rs` (`TgState`): запуск через `server::run_with_listen` в tokio,
  остановка oneshot-shutdown, статус (running/port/link) и статистика (`STATS.summary`).
  Команды `tg_status/tg_stats/tg_start/tg_stop`; `open_url` (только `tg://`) для кнопки
  «Подключить Telegram». Настройки `tg_autostart`/`tg_port` в `state.json` (дефолт 1443),
  автозапуск при старте приложения. UI — отдельная вкладка «Telegram»: минималистично
  (статус, Включить/Выключить, «Подключить Telegram», порт/автозапуск под «Дополнительно»).
  Тесты: `telegram::tests` (3, включая реальный бинд на порту 0). Итого 28 passed.
- Версия 0.8.0 (фикс Telegram): `start()` стал async и **ждёт фактического бинда** сокета
  (oneshot от `on_listen`, таймаут 20 с) — раньше UI показывал «работает» с `Порт null`,
  потому что статус читался до бинда. Теперь ошибка «порт занят» приходит сразу, а порт и
  `tg://`-ссылка заполнены.
- Версия 0.8.0: решено по лицензиям — проект под **MIT**, берём код только MIT-проектов
  (ZUI/tg-ws-proxy-rs, ZapretControl, Line, FreeConnect); GPL-3.0 и без-лицензии — только идеи
  (см. `docs/feature-spec-forks.md`). Unit-тестов: 27.
- Версия 0.8.0 (фикс Telegram #2): кнопка «Подключить Telegram» не работала — `cmd /c start`
  (а затем `rundll32 url.dll`) **портили query-строку с `&`**, Telegram показывал «Некорректная
  ссылка на прокси». Теперь `open_url` вызывает **ShellExecuteW** напрямую (проверено вручную:
  диалог «Прокси-сервер» с верными server/port/key).
- Версия 0.8.0: **проверка обновления Telegram-моста** — `updater::check_tg_bridge()` сравнивает
  вкомпилированную версию крейта (`tg_ws_proxy_rs::VERSION`) с версией в апстриме ZUI
  (`crates/tg-ws-proxy-rs/Cargo.toml`) + показывает последний коммит Flowseal/tg-ws-proxy.
  Команда `tg_check_update`, плашка на вкладке «Telegram» («Мост актуален (версия …)» или
  «Доступно обновление…»). Пул CF-доменов при этом обновляется в крейте автоматически (раз в час).
  Тесты: `updater::tg_tests` (2).
- Версия 0.8.0: **fix Telegram**: `open_url` через `cmd/rundll32` портил query с `&` → Telegram
  показывал «Некорректная ссылка». Теперь прямой `ShellExecuteW`. `start()` ждёт бинда (без `Порт null`).
- Версия 0.8.0: **Watchdog** (`watchdog.rs`) — раз в 60 с при запущенном профиле проверяет TCP 443
  до YouTube и Discord; 3 неудачи подряд → toast «обход не работает»; восстановление → «обход снова
  работает». Авто-восстановления нет. Команда `watchdog_status`, событие `zgui:watchdog`.
- Версия 0.8.0: **DNS-раздел** — добавлен **XBox DNS** (111.88.96.50 / 111.88.96.51,
  DoH `https://xbox-dns.ru/dns-query`). У провайдеров поле `description` + кнопка «Замерить задержку»
  (DNS-время отклика, UDP A-запрос, медиана из 3; результат в выпадашке и таблицей). Команда `dns_benchmark`.
- ВАЖНО (сборка): release собирать **только** через `npm run tauri build -- --no-bundle`.
  Голый `cargo build --release` НЕ перегенерирует tauri-контекст и может вкомпилировать
  `devUrl` (`localhost:1420`) → в окне браузерная заглушка «localhost отказано в подключении».
  Если такое случилось: удалить `target\release\build\zgui-*` и пересобрать через `tauri build`.
- TODO (DNS): удалить **Control D** и **NextDNS** из списка провайдеров — они требуют
  предварительной настройки профиля на сайте (не подходят для простого пользователя).
- Unit-тестов: 33. Release-сборка без бандла: `src-tauri\target\release\zgui.exe` (0.8.0).
- Версия 0.8.0 (предрелизный аудит): разобрано «21/22 одинаковых» и «ALT5 = 0». Это не баг: все шаги теста
  имеют уникальные argv (22 сигнатуры), тест меряет TCP-доступность 100 доменов — счёт «насыщается» (общие
  87/100, у `general` 86/100; 13 доменов падают у всех, включая мусорный `.ua`). `general (ALT5)` запускается
  (`started=true`, error пуст), но его правила desync на ВЕСЬ IPv4 tcp 80/443 без hostlist +
  `--dpi-desync-any-protocol=1 --dpi-desync-cutoff=n4` рвут все хендшейки (timeout ровно 3 c). В самом .bat —
  пометка `NOT RECOMMENDED`. GUI показал 0/100 корректно.
- Аудит → исправлено 3 дефекта класса «спойлеров»:
  1. Спойлеры (`details`) хранили состояние неверно: ключевание по `id` вместо индекса было сделано, но закрытие
     не фиксировалось — набор обновлялся только при чтении DOM (`if (d.open) …add`), а `delete` не вызывался.
     Один раз открытый спойлер поэтому открывался заново при каждой перерисовке (поллинг теста ~1с, автообновление
     статусов). Исправлено: `trackDetails(details, set)` вешает событие `toggle` (open→add, close→delete); чтение
     DOM-состояния убрано. Применено ко всем трём спойлерам: `profile-details`, `test-details`, `update-group`
     (последний имел тот же скрытый дефект — группа обновлений сама раскрывалась).
  2. `load_domains_from_lists` принимал мусор из .lst (`.ua`, `.googlevideo.com`, IP, `*.wild`, хвостовые точки).
     Добавлен `usable_test_host` (минимум 2 метки, буквенный символ, без `*`/ведущих точек) — и для live-списков,
     и для builtin. Тесты `usable_host_rejects_junk`, обновлён `load_domains_parses`.
  3. Чтение авторских `.bat` только как UTF-8: UTF-16/BOM-файлы давали пустой argv и стратегия молча пропадала.
     Добавлен `profiles::decode_strategy_bytes` (UTF-8, UTF-8 BOM, UTF-16 LE BOM), применён в `refresh_catalog`
     и `reload_bats_from_disk`. Тест `decodes_utf16_and_bom`.

## Баги/риски на проверку при первом запуске
- Кодировка русских названий в результатах теста проверяется через UTF-8 BOM для `test-plan.json` и `test-runner.ps1`.
- fetch_engine распаковывает zip верхнего каталога; у flowseal внутри `bin/` — exe ищется рекурсивно (find_exe).
- Структура пресетов zapret2 написана по документации (может потребовать подгонки под конкретные флаги новой версии).
- UAC дважды: при старте стратегии и при install/remove службы — ожидаемо.
- `collect_entries` пока строит каталог обновлений только при заданных корнях движков — встроенный каталог
  обновляется после установки движка (TODO: разрешить обновление встроенного каталога без корней).
- Вшитые конфиги — снимок на дату сборки; обновляются штатным «Проверить обновления» после установки движка.
- Portable-перенос на другой ПК безхвостовый: кроме WebView2-кэша (перенесён в data\webview), ничего в %LocalAppData% не пишется.

## Следующие шаги
1. Пользователь запускает `src-tauri\target\release\zgui.exe` (0.8.0): тёмная тема, нет мигающих консолей, `data\` рядом с exe.
2. Проверить «Скачать движок» для обоих движков (ot_ честный).
3. «Проверить обновления» → должны появиться записи; «Применить доступные».
4. Запуск/останов профиля и toggle «Автозапуск».
5. При необходимости — portable ZIP-пакет (`exe + data\`), без установщика.

## Тест (план)
- Запусти `src-tauri\target\release\zgui.exe` (0.8.0). Ожидай: тёмная тема, шапка «portable»; **движки уже стоят**
  (статус «готов» у Flowseal и Zapret2 без скачивания — распакованы из exe в `data\engines\`).
- Если запущен чужой zapret (winws/winws2 или служба от service.bat) — при старте появится окно
  «Обнаружено конфликтующее ПО» с кнопкой «Выгрузить процессы» (UAC).
- Сверху на «Стратегиях» — баннер: «Не рекомендуется запускать Zapret вместе с VPN» + предупреждение про .bat/.lua.
- «Тест стратегий» → «Запустить тест»: прогон `.bat` с UAC, чипы доменов, «Лучшая стратегия».
- «Создать конфиг авто» → профиль «Авто (на основе …)» с `lists/list-auto.txt` из доменов geoblock; запусти его.
- Обновления → «Проверить»/«Применить доступные»: группы flowseal/zapret2/geoblock.
- Профиль → «Запустить»: перед стартом снова проверка конфликтов; UAC, `running pid N`; «Остановить».

## Последняя проверка
- `cargo check` — чисто (0 предупреждений); `cargo test --lib` — **33 passed**.
- `npm run build` — dist собирается, CSS подключён.
- `npm run tauri build -- --no-bundle` — OK: `src-tauri\target\release\zgui.exe` (0.8.0, 24 409 600 Б, движки внутри).

## СТАТУС НА 19.09.2026 (точка продолжения)

### Сессия 19.09 (вечер): фикс опасного confirm + редизайн Deep Space Ops
- **КРИТИЧНО ИСПРАВЛЕНО**: системный `confirm()` в WebView2 не показывается и молча возвращает
  «да» → «Восстановить интернет» запускался без подтверждения, ПК перезагружался без спроса.
  Теперь **своя модалка** `showConfirm()` с алгоритмом (шаги 1–5), перезагрузка — только кнопкой
  «Перезагрузить сейчас» в отдельном окне; «Позже» ничего не делает. Заменены все 7 `confirm()`.
- Модалка конфликтов: скролл + sticky-кнопки (раньше уезжала за экран при 25+ xray.exe),
  группировка «xray.exe — 25 процессов».
- Миграция интервала автопроверки 6→72 ч (флаг `interval_migrated`).
- **РЕДИЗАЙН «Deep Space Ops»**: звёздное поле (3 CSS-слоя + параллакс + мерцание, топо-линии),
  палитра через переменные (акцент #3B82F6, фон #050508), glassmorphism (sidebar/topbar/cards/
  tiles/modals/toasts, blur 10–14px), hover-свечение кнопок, толстый градиентный прогресс-бар,
  шрифтовые стеки Inter/JetBrains Mono (без сетевых загрузок). `prefers-reduced-motion` уважается.
- Тесты: 34 passed. Release пересобран через `tauri build` (3m14s).

### Сессия 19.09 (после прогона геоблок-теста всеми конфигами) — нужно проверить
- **ФИКС БЛОКЕРА запуска профиля**: `Start-Process -Verb RunAs` несовместим с
  `-RedirectStandardOutput/-RedirectStandardError` → «не удалось запустить процесс».
  Теперь запуск через `cmd /c "exe args > log 2>&1"` с RunAs; таймаут PID 60 с;
  `start_profile` async (`spawn_blocking`) — UI не виснет.
  ПРОВЕРИТЬ: «Запустить» → UAC появляется → winws/winws2 стартует.
- **Калибровка**: 1203 домена прогнаны всеми конфигами → обходится 998; «недоступные» = не прошли
  ни у одной стратегии (205). `vpn-only.lst` пересчитан (был 0 из-за старой логики: требовался
  base_ok, а baseline проходил почти везде). Из ручного списка вырезаются 5:
  `instagram.com`, `cdninstagram.com`, `kino.pub`, `kinozal.tv`, `omv-extras.org`
  (обязательные 20 не вырезаются никогда).
- **Точка восстановления** (`netreset::create_restore_point`, команда `net_create_restore_point`):
  перед «Восстановить интернет», не чаще раза в сутки, согласие; при неудаче — выбор продолжить/отмена.
- **Выгрузка процессов без переспроса**: `conflictConsent` + `autoKillAndProceed` (повтор до успеха,
  пауза 2 с, перепроверка), действие продолжается автоматически; уведомление по завершении.
- **Плитки стратегий**: сетка `minmax(190px,1fr)` (≈4 в ряд), чипы, кнопки «Запустить» + «⋯»
  (модалка `profileModal`: параметры, автозапуск, удаление).
- **Настройки**: подсказки к Game Filter и ipsets (жёлтый блок); автопроверка 72 ч по умолчанию;
  кнопка «Проверить обновления сейчас».
- **Жёлтый стиль `.hint`** (жёлтая рамка, БЕЛЫЙ текст) — описания netreset, теста стратегий,
  настроек; DNS-описание тоже белым.
- Тесты: 34 passed. Release пересобран через `tauri build` (2m39s).

### Ранее в этой сессии
- **Правка AmneziaVPN** (`service.rs`): в `VPN_SERVICES` добавлены `AmneziaVPN` и
  `AmneziaVPN-service`; порядок — сначала Stop-Service (ожидание 2 с), потом taskkill процессов.
- **Тест стратегий вынесен в отдельное меню** слева («Тест стратегий», `view-tests`).
- **«Восстановить интернет»** (`netreset.rs`): стоп/удаление службы zapret, стоп VPN-служб +
  kill процессов, `sc stop/delete WinDivert`, `netsh winhttp reset proxy` + реестровый прокси,
  `ipconfig /flushdns`, `netsh winsock reset` + `netsh int ip reset`. Wi-Fi/провайдер не трогаются.
  `route -f` и `/release` намеренно НЕ включены. Команды `net_reset`, `virtual_adapters`, `reboot_now`.

### Сделано и проверено пользователем (ранее)
- Telegram-прокси: работает (приём + отправка подтверждены).
- Telegram-прокси: включается/выключается, `tg://`-ссылка открывается (ShellExecuteW),
  Telegram работает (получение + отправка сообщений подтверждены).
- DNS: XBox DNS добавлен, «Тест пинга» работает (видно пинг по провайдерам),
  описания жёлтым блоком, кнопки переименованы («Вернуть стандартный DNS», «Тест пинга»).
- Интерфейс открывается (после пересборки через `tauri build`).

### Сделано в коде, но НЕ проверено пользователем
- **Watchdog** (`watchdog.rs`): проверка YouTube/Discord раз в 60 с при запущенном профиле,
  3 неудачи → toast. Индикатор в шапке (`#wdState`). Нужно проверить вживую: запустить профиль,
  подождать/сымитировать сбой. `watchdog_status` + событие `zgui:watchdog`.
- **Проверка обновления TG-моста**: `tg_check_update`, плашка на вкладке Telegram.
  Нужно проверить, что показывает «Мост актуален (версия 2.3.4-zui.2)».

### НЕ сделано (обсуждалось)
- **user-friendly проверка «UI не загрузился»** (браузерная заглушка `localhost отказано`).
  Идея: в release при навигации на `localhost` показывать понятное нативное сообщение вместо
  заглушки Edge. Не реализовано.
- Показ статистики `tg_stats` в UI (команда есть, UI минимальный — по решению владельца).

### Незавершённые фичи из спеки (`docs/feature-spec-forks.md`)
По приоритету владельца: 1) Telegram ✅, 2) Watchdog ✅ (не проверен), 3) **Редактор списков** — не начат,
4) **Мониторинг winws** (отдельная вкладка) — не начат, 5) **Диагностика** (чеклист; сюда же
`heal_orphan_proxy`) — не начата, 6) **История/валидация тестов** (fingerprint) — не начата.

### Важные технические заметки
- **Сборка release**: только `npm run tauri build -- --no-bundle`. Голый `cargo build --release`
  может вкомпилировать `devUrl` → заглушка localhost. Лечение: удалить `target\release\build\zgui-*`
  и пересобрать через `tauri build`.
- **Лицензия**: проект MIT, берём код только MIT-проектов (ZUI/tg-ws-proxy-rs, ZapretControl, Line,
  FreeConnect). GPL/без-лицензии — только идеи. См. `THIRD_PARTY_NOTICES.md`.
- **Git**: репозитория НЕТ (`.git` отсутствует). Бэкап: `E:\Base\opencode\Z GUI_backup_20260918_151759`.
- **Данные**: `src-tauri\target\release\data\` (engines, catalog, logs, webview). Кэш WebView2 —
  `data\webview\EBWebView` (следы `localhost:1420` только в `site_engagement`, безвредны).
- **Неиспользуемые временные файлы**: `src-tauri\crates\tg-ws-proxy-rs` — рабочий крейт (не удалять).

### Известные открытые вопросы
- Финальная очистка встроенного списка доменов — уже сделана (105 доменов). Пересмотр не требуется.
- Калибровка геоблока (`zapret-reachable.lst`/`vpn-only.lst`) — пользователь запускает вручную;
  после него основной тест фильтруется.
- Control D и NextDNS удалены из DNS (требовали настройки на сайте).

---

## 19.09.2026 — версия 0.9.0 (правки по 7 замечаниям владельца)

Критерий «готово»: все 7 пунктов закрыты, `cargo test --lib` зелёный, фронт собирается,
release собран и владелец проверил визуально.

### 1. Запуск стратегий bol-van (zapret2) — ИСПРАВЛЕНО
- Причина «иероглифов» и «ничего не происходит»: в пресете z2-youtube была строка
  `--lua-init=fake_default_tls = tls_mod(fake_default_tls,'rnd,rndsni')`.
  winws2 **переразбивает командную строку по пробелам** → в Lua попадал обрывок `fake_default_tls`
  → `LUA ERROR: ... '=' expected near <eof>`; без пробелов — `bad argument #2 to 'tls_mod'`
  (`fake_default_tls` не Lua-глобал, это blob из `lua-desync`). Строка удалена (`profiles.rs`).
- Диагностика подтвердила: z2-general и z2-quic стартуют нормально
  (`windivert initialized. capture is started.`).
- Логи читались как UTF-8 (`String::from_utf8_lossy`), а PowerShell пишет **UTF-16** →
  «иероглифы». Добавлены `config::decode_text` / `read_text_auto` (UTF-16 LE/BE с BOM и без,
  UTF-8 BOM/без, иначе OEM/CP866 через `MultiByteToWideChar`); `tail_file` и
  `runner::spawn_and_wait_pid` переведены на них.
- `do_start`: старые `out_log`/`err_log` удаляются перед запуском (раньше показывался прошлый лог);
  в ошибку теперь попадают **и stderr, и stdout**.
- `runner::write_launcher`: статус пишется **одним** `Set-Content -Encoding UTF8`, при
  `LAUNCH_ERROR` лаунчер сразу возвращает ошибку (не ждём 60 с таймаута).
- `Cargo.toml`: включён feature `Win32_Globalization` у `windows-sys`.

### 2. Убрана кнопка «Обновить статус»
- Удалена из `index.html` (`#btnRefresh`) и её обработчик в `main.js`. `refreshAll()` сохранён,
  вызывается из навигации/после действий (null-safe).

### 3. Telegram: цвет текста «Выключить»
- `#btnTgToggle` = `btn primary danger`; `.btn.danger` (объявлен позже) перебивал цвет primary →
  синий фон + оранжевый текст. Добавлено `.btn.primary.danger { color: #ffffff; }` — по просьбе
  владельца текст белый. Файлы Telegram-моста НЕ трогались.

### 4. Выпадающие списки — тёмные
- `:root { color-scheme: dark; }` + правила `select option/optgroup` (фон #0b0d13, текст светлый,
  выбранный — акцент). Нативное меню больше не светло-серое.

### 5. DNS — отдельный пункт меню
- В сайдбар добавлен `data-view="dns"`; карточка `#dnsCard` вынесена из «Настроек» в отдельную
  секцию `#view-dns`. При переходе вызывается `loadDnsProviders()`.

### 6. Фон: свои топо-линии + звёзды
- `src/assets/topo.svg` — **собственный** рисунок: концентрические эллипсы, искривлённые
  фильтром `feTurbulence`+`feDisplacementMap` (три центра). Никаких сторонних/скачанных файлов
  → юридически чисто (лицензия MIT, всё своё). `.topo` использует его, `mix-blend-mode: screen`,
  медленный дрейф `topo-drift` (180 с), `prefers-reduced-motion` учитывается.
- Прозрачность UI повышена: `--bg2/--panel/--panel2`, sidebar и topbar стали полупрозрачнее
  (0.32–0.42) + `backdrop-filter: blur(16px)` — звёзды и топо видны сквозь панели.

### 7. Иконка — адаптирован свой `Z.png`
- Источник: `C:\Users\vinta\OneDrive\Desktop\PS\#1 Selfmade\#3 Projects\Z.png` (1024×1024, ARGB).
- Скопирован в `assets/app-icon.png`, перегенерированы все иконки через `npx tauri icon`
  (`src-tauri/icons/*`, включая `icon.ico`/`icon.icns`/Android/iOS).
- В сайдбаре логотип-`Z` заменён на `src/assets/z-logo.png` (`<img>` + `object-fit: contain`).

### Журналы (просьба владельца «пиши в 3 журнала»)
- `progress.md` — этот файл (журнал работы).
- `docs/feedback.md` — журнал замечаний владельца (что просили и как закрыто).
- `docs/TODO.md` — журнал задач (что сделано / что осталось).

### Проверка
- `cargo check` — без ошибок/предупреждений.
- `cargo test --lib` — **34 passed**.
- `npm run build` — ок (dist пересобран; `z-logo-*.png` и `topo.svg` попали в сборку).
- Версия поднята до **0.9.0** в `package.json`, `Cargo.toml`, `tauri.conf.json`.

---

## 19.09.2026 — вторая волна правок (0.9.0, до пересборки)

### A. Тест стратегий зависал на фазе запуска, когда GUI уже админ
- Причина: `start_test` всегда звал `rn::spawn_elevated_script` (`Start-Process -Verb RunAs`).
  Из уже повышенного процесса дочерний процесс мог не стартовать → прогресс навсегда
  «запускаю тест (подтвердите права администратора один раз) (0/1)», выход только через 120 с.
- Исправлено: `runner::spawn_script_direct` — прямой запуск `powershell -File <script>`
  (без UAC) при `rn::is_elevated()`; текст фазы launch без упоминания прав.
- Вотчдог: если `logs/test-runner.pid` не появился за 20 с → тест завершается с понятной
  ошибкой (PID есть, но нет вывода — ждём 120 с). Ошибка различает elevated/не-elevated.
- Замечание владельца: «general ALT5 скорей всего не работает, но тест не перестаёт запускать».

### B. Модалка/подложка выбивалась по цвету
- `.modal` — фон в тон приложению (`var(--overlay)`) + лёгкий blur; `.modal-box` — токены темы
  (`var(--panel2)`), а не нейтрально-серый `rgba(22,22,30,.88)`.

### C. Карточка «Данные и каталог» вырезана
- Удалена из `index.html`, `renderDataInfo()` и её вызов убраны из `main.js`. Владелец: «ничего не даёт».

### D. Три темы оформления + пункт «Внешний вид»
- `config.rs`: в `Settings` добавлено `theme` (`grey` по умолчанию, `dark`, `light`),
  `#[serde(default = "default_theme")]`; команда `set_theme` (валидирует значение);
  `set_settings` тему не перезаписывает (меняется только через `set_theme`).
- `styles.css`: три палитры через `[data-theme]`; все поверхности вынесены в переменные
  (`--sidebar-bg`, `--topbar-bg`, `--btn-bg`, `--code-bg/-text`, `--option-bg`, `--overlay`,
  `--modal-solid`, `--toast-bg`, `--brand-grad`, `--shadow`, `--space-extra`, `--stars-opacity`,
  `--topo-opacity`). Звёзды/топо в светлой теме отключены (opacity 0).
- Тема по умолчанию — **«Графит»** (в стиле GitHub/Discord), как просил владелец.
- `main.js`: `applyTheme`/`initTheme`/`currentTheme`/`pickTheme`, кэш в `localStorage`
  (без мигания до bootstrap), карточки тем в `#themeGrid`, применение из `B.settings.theme`.
- `index.html`: пункт меню «Внешний вид» (`view-appearance`) с тремя карточками-образцами.

### E. Фон/топо-линии — сделано
- Владелец: прежний фон был «рваным»; прислал `C:\Users\vinta\Downloads\topography.zip`
  с нормальным (стилизованным) вариантом контуров — 1 path, 91 подпуть, гладкие линии.
- `src/assets/topo.svg` заменён на файл из архива; `fill="#000"` → `fill="#fff"`
  (чёрные линии на тёмном фоне не видны, а `mix-blend-mode: screen` с чёрным = невидимо).
- `styles.css` `.topo`: было `background-size: cover` (растягивало тайл) →
  `background-size: 600px 600px; background-repeat: repeat; background-position: 0 0`.
  Тайл проверен рендером 2×2 — бесшовный, стыки сходятся.
- Владелец сам приложил файл, лицензионный вопрос снят его решением.

### Проверка второй волны
- `cargo check` — чисто; `cargo test --lib` — **34 passed**; `npm run build` — ок.
- `npm run tauri build -- --no-bundle` — **успешно** (после закрытия запущенного `zgui.exe`,
  который держал файл): `src-tauri\target\release\zgui.exe`, 19.09.2026 20:27.

## Третья волна (19.09.2026) — zapret2 и правки UI

### F. ГЛАВНОЕ: zapret2 «процесс сразу завершился (нет вывода)» — НАЙДЕНА ПРИЧИНА
Диагностика (движок: `data\engines\zapret2\zapret2-v1.0.5.2\binaries\windows-x86_64\winws2.exe`):
- `mdig.exe`/`ip2net.exe` из того же каталога работают и печатают usage → cygwin-окружение в порядке,
  `cygwin1.dll` и `WinDivert.dll` на месте, PE-импорты winws2.exe корректны (subsystem=CONSOLE).
- **Причина: winws2 НЕ разбивает argv по пробелам.** GUI отдавал группу флагов одним аргументом
  `"--filter-tcp=443 --filter-l7=tls ... --new"`, winws2 молча завершался с кодом 1 (0 байт в stdout/stderr).

  | вариант | argv | результат |
  |---|---|---|
  | W1 | `"--filter-tcp=443 --filter-l7=tls ..."` одним аргументом | exit 1, без вывода |
  | W2 | те же флаги отдельными аргументами | exit 0, работает |
  | W3 | `"--filter-tcp=443 --filter-l7=tls"` одним аргументом | exit 1 |

- Комментарий в `profiles.rs` («winws2 переразбивает по пробелам») был **неверным** — удалён.
- Фикс: `pf::flatten_engine_args(engine, args)` — для zapret2 режет каждый аргумент по пробелам,
  но оставляет целыми `--lua-init=` (Lua-код), `--wf-raw=`, `--wf-raw-part=` (выражения WinDivert).
  Применено в `do_start` и в подготовке шагов тест-раннера.
- Вторая причина, из-за которой фикс не доходил: у владельца в `state.json` лежал **старый** `z2-youtube`
  с битой строкой `--lua-init=fake_default_tls = tls_mod(...)`, а пресеты добавлялись только если
  профиля нет. `ensure_presets` теперь **синхронизирует** встроенные пресеты zapret2 (args/name/source).

### G. Убраны из UI
- **Comss.one DNS удалён** (`dns.rs`) — владелец: «заблочен в РФ». Провайдеров стало 8, тесты обновлены.
- Модалка: подложка под кнопками выбивалась во всех темах — `.modal-box` теперь использует тот же
  `var(--modal-solid)`, что и приклеенный блок `.modal-actions`.
- Баннер «Важно перед запуском»: текст плохо читался поверх топо-линий — добавлены плотная подложка
  (`linear-gradient(amber .14), var(--modal-solid)`) и `backdrop-filter: blur(10px)`.

### H. Фон
- Лёгкое размытие контуров: `.topo { filter: blur(1.6px) }` (владелец просил «10%»).

### I. Иконки Lucide вместо эмодзи
- Лицензия ISC (совместима с MIT), проверено `npm view lucide-static license`.
- 7 иконок вендорены в `src/assets/icons/lucide-*.svg` (v1.47.0, `stroke="currentColor"`),
  пакет `lucide-static` после копирования удалён из `node_modules`/`package.json`.
- `main.js`: импорт через `?raw` + `NAV_ICONS` + `renderNavIcons()` (вызывается рядом с `initTheme()`).
- `styles.css`: `.nav-ico` → flex 18px, `.nav-ico svg` 17px.
- `THIRD_PARTY_NOTICES.md`: добавлен раздел Lucide Icons.
- Дополнительно: `✕` в модалках (`#conflictClose`, `#pmClose`, `#cmX`) тоже заменён на `lucide-x`
  (заполняется тем же `renderNavIcons()`), эмодзи в `index.html` больше нет (проверено сканом).

### Проверка третьей волны
- `cargo test --lib` — **35 passed** (добавлен тест `flattens_zapret2_groups_but_keeps_lua_code`).
- `npm run build` — ок (15 модулей, иконки попали в бандл).
- `npm run tauri build -- --no-bundle` — **успешно**: `src-tauri\target\release\zgui.exe` (21:10).
- Повторная сборка (только правка `✕`→lucide-x) **не прошла**: владелец в этот момент запустил
  `zgui.exe` (PID 10964) — «failed to remove file … Отказано в доступе (os error 5)».
  Ждём закрытия приложения, затем дособерём (фронтенд `dist/` уже обновлён).
- Отдельно проверено вручную: с разбитыми аргументами winws2 реально стартует
  («Windivert initialized. capture is started.»).
- Замечание: два окна `winws2.exe`, висевшие у владельца, — диагностические остатки от этих тестов,
  их нужно просто закрыть. Они же подтвердили, что движок рабочий.
- Побочное наблюдение: при перенаправлении вывода winws2 в файл лог остаётся пустым
  (cygwin пишет в консоль). Если нужны логи запуска — потребуется отдельное решение.

## Четвёртая волна (19.09.2026) — фризы UI, блок автозапуска, пресет автора zapret2

### J. Фриз программы (жалоба: «после теста и пары переключений темы подвисла»)
Диагностика по коду (не догадки):
1. **Главная причина — фон.** Слои фона (`.topo` + `.stars-far/mid/near`) были с бесконечными
   CSS-анимациями (`star-drift`, `star-twinkle`, `topo-drift`), а поверх них — «стекло»
   с `backdrop-filter`: сайдбар и топбар (16px), `.card` (12px), **`.profile` (10px) — а это
   25 плиток стратегий**, модалки (18px), тосты. Пока backdrop анимируется, Chromium обязан
   заново размывать backdrop у КАЖДОГО такого элемента на КАЖДОМ кадре. При переключении темы
   (полный перерисов всего окна) это давало многосекундный ступор, а DWM переставал получать
   кадры — отсюда «пропавшая» иконка/превью в панели задач.
   **Фикс:** все фоновые анимации убраны (фон стал статичным → размытие backdrop кэшируется),
   лишние `will-change: transform` сняты, мёртвые `@keyframes` удалены. Внешний вид не изменился.
2. **Лишние перерисовки.** `refreshAll()` опрашивает `bootstrap` каждые 4 с и полностью
   пересобирал DOM: список профилей (25+ плиток), дерево обновлений, а также переписывал поля
   формы (затирал ввод пользователя). События теста (`zgui:test`) приходят каждые ~0.7 с и каждый
   раз заново строили весь список результатов (сотни узлов: чипы групп + по домену на стратегию).
   Во время теста добавлялись ещё и повторные `renderTestResults`.
   **Фикс:** сигнатуры последней отрисовки (`sigProfiles`, `sigUpdates`, `sigSettings`,
   `sigTestResults`) — перерисовка только при реальном изменении данных.
3. **Sync-команды блокировали UI-поток.** В Tauri v2 команда без `async` выполняется прямо
   в IPC-обработчике, т.е. на главном потоке (проверено по исходникам tauri 2.11.5:
   `body_blocking` вызывает функцию напрямую; асинхронные уходят в `respond_async` →
   `async_runtime::spawn`). Любой диск/перечисление процессов (например `conflict_check`,
   `bootstrap`, чтение логов) замораживали окно.
   **Фикс:** 39 команд переведены в `#[tauri::command(async)]`. Проверено, что в проекте нигде нет
   `block_on`/`blocking_lock` (иначе был бы паник на runtime-потоке).

### K. Настройки: блок «Запуск и права администратора»
- По просьбе владельца флажок «Всегда запускать GUI от администратора», кнопки
  «Автозапуск GUI при входе» и «Перезапустить от админа» вынесены из общей карточки
  в отдельную карточку внутри `#view-settings`. `«Сохранить»` осталась в карточке настроек
  (она сохраняет и этот флажок) — в подсказке это указано.

### L. Чипы теста: галочки/крестики → иконки
- `✓`/`✗` в чипах групп и доменов заменены на `lucide-check`/`lucide-x`
  (`chipMark()` в `main.js`), CSS `.chip-ico`. Плюс `lucide-check.svg` в `src/assets/icons/`.

### M. Пресет zapret2 «По умолчанию автора»
- Владелец: «ни одна из стратегий zapret2 не работает, но, возможно, дело не в программе».
- Наши три пресета были написаны вручную. В движке найден `config.default` с каноническим
  `NFQWS2_OPT` самого автора zapret2 — эти аргументы и добавлены новым пресетом
  `z2-default` («По умолчанию автора (zapret2)»): `fake_default_tls:tcp_md5:tcp_seq=-10000` +
  `multidisorder:pos=1,midsld`, для http — `multisplit:pos=method+2`.
  Ин-profile `--out-range/--in-range` (connbytes 1:20 / 1:10) опущены — на Windows это только
  экономия CPU. Теперь есть честный тест: если и набор автора не работает — причина в DPI
  провайдера, а не в наших аргументах. `ensure_presets` добавит пресет при следующем запуске.

### Проверка четвёртой волны
- `cargo check --release` — чисто; `cargo test --lib` — **35 passed**; `npm run build` — ок (17 модулей).
- `npm run tauri build -- --no-bundle` — **успешно**: `src-tauri\target\release\zgui.exe`
  (24 951 808 байт, 19.09.2026 21:38) после того, как владелец закрыл приложение.
- Пресет `z2-default` в `state.json` ещё не прописан (там 3 старых пресета) — `ensure_presets`
  добавит его при первом запуске новой сборки.
- Владелец проверил: пресет **запускается** (winws2 жив, pid 9064, WinDivert работает), но
  обход не помогает. Вывод: это уровень DPI провайдера, а не путь запуска; zapret2 оставляем
  как есть, рабочий вариант — стратегии Flowseal.

### N. Настройки: флажок админа применяется сразу
- `collectSettings()` в `main.js` (собирает форму), используется и кнопкой «Сохранить»,
  и обработчиком `change` на `#cfAlwaysAdmin` → `set_settings` сразу, без «Сохранить».

### O. Иконка приложения: мелкие размеры
- Жалоба: «в минимальном размере иконка не читается». Причина объективная: в `icon.ico`
  были кадры 16/24/32, все — авто-даунскейл из 512px, а штрих «Z» тонкий (яркие пиксели —
  всего ~6 % площади), на 16px он расплывался в кашу.
- Пересобран `src-tauri/icons/icon.ico`: кадры **16, 20, 24, 32, 40, 48, 64, 128, 256**
  (PNG-кадры внутри ICO — тот же формат, что был). Для размеров ≤ 32 штрих делается чисто
  белым и утолщается (градиентная маска 30..85 + MaxFilter), фон остаётся тёмной плиткой.
  Проверено ASCII-рендером 16x16 — «Z» читается (верхняя планка, диагональ, нижняя).
  Скрипт: `C:\Users\vinta\AppData\Local\Temp\opencode\mkicon.py`.
- Заодно перезаписаны `32x32.png`/`24x24.png`/`16x16.png` в `src-tauri/icons/`.
- **Грабля**: `tauri build` НЕ перезапускает build-скрипт, если изменился только `icons/icon.ico` —
  в exe остаётся старая иконка (проверено: в ресурсах было 6 кадров вместо 9). Лечится касанием
  `tauri.conf.json` перед сборкой. Проверка встроенной иконки: в PE-ресурсах считать `RT_ICON`
  (у старой было 6 кадров, у новой — 9), либо `[System.Drawing.Icon]::ExtractAssociatedIcon(exe)`.
- Итог: release 19.09.2026 22:03, `zgui.exe` 24 950 784 байта — **9 кадров иконки на месте**,
  ОС-экстрактор отдаёт тот самый жирный «Z» (верхняя/нижняя планки сплошные).
- **Уточнение (позже в тот же вечер)**: кадры пересобраны ещё раз — теперь их **10**
  (16, 20, 24, 32, 40, 48, 64, 96, 128, 256) и все они **DIB**, без PNG-кадров внутри ICO
  (PNG-кадры часть оболочек/экстракторов не любит). Проверено в собранном exe:
  `RT_ICON` = 10 записей, заголовки DIB (`28 00 00 00`, удвоенная `biHeight`), а
  `PrivateExtractIcons` (тот же API, что у проводника) на 16/24/32 отдаёт чёткий жирный «Z».
  Release 22:17, `zgui.exe` 25 341 952 байта.

## Пятая волна (19.09.2026) — журнал в программе, понятные ошибки, релиз 1.0.0

Запрос владельца: «сделай user-friendly проверки везде, чтобы тестеры не пугались;
логгер/отчёт отдельным пунктом меню сразу после «Настроек»; проверь готовность релиза 1.0
и полную портативность; сделай полный диздок».

### P. Встроенный журнал (новый модуль `logger.rs`)
- Кольцевой буфер в памяти (1500 записей) + файл `data/logs/zgui.log` с ротацией при 1 МБ
  (`zgui.1.log`), перехват паник, событие `zgui:log` в интерфейс.
- Команды: `log_entries` (только новые записи по `seq`), `log_write`, `log_clear`,
  `log_dir_open`, `report_save`.
- **Отчёт** (`report_save`): версия, время, версия Windows, права админа, путь exe и данных,
  состояние обоих движков (найден ли exe), число профилей, запущенная стратегия, служба,
  настройки (тема, интервал, game filter, ipsets, автозапуск, порт Telegram) и весь журнал.
  Файл `data/logs/отчёт-<дата>.txt`, сразу открывается в проводнике.
- **Важная грабля**: логгер намеренно **не знает про Tauri**. Первая версия хранила
  `AppHandle` — и тестовый бинарник (`cargo test --lib`) стал падать на загрузке с
  `STATUS_ENTRYPOINT_NOT_FOUND` (0xC0000139). Причина: тесты логгера делали достижимым код
  с `AppHandle` → линковался tao/wry → импорт `comctl32!TaskDialogIndirect`, а он есть только
  в comctl32 v6 (активируется манифестом, которого у тестового exe нет). Решение: логгер
  принимает колбэк `Arc<dyn Fn(Entry)>`, а `run()` подставляет замыкание с `emit`.
  Проверено: `cargo test --lib` снова зелёный (43 теста).

### Q. Понятные ошибки (`human.rs` + обёртка `invoke` в `main.js`)
- Таблица перевода в Rust и такая же в JS: `os error 5` → «нужны права администратора»,
  `os error 32` → «файл занят другой программой», сетевые → «нет связи с сервером (проверьте
  интернет или выключите VPN)», HTTP 403/404/5xx, `ADMIN_REQUIRED`, «движок сразу завершился»,
  паника. Понятные русские сообщения не перезаписываются.
- Все вызовы бэкенда из UI идут через свою `invoke()`: сырая ошибка пишется в журнал
  (`scope = "команда <имя>"`), пользователю отдаётся переведённый текст. Одна правка вместо
  сотни мест вызова.
- Логи в бэкенде: старт/стоп стратегии, старт/финиш теста, проверка и применение обновлений,
  установка движка, корень движка, служба (установка/удаление), DNS (применение/сброс),
  Telegram (старт/стоп), восстановление сети, watchdog («обход не работает / снова работает»).

### R. Защиты и предстартовые проверки
- **Отмена UAC больше не закрывает программу**: `relaunch_as_admin` теперь ждёт результат
  (`Start-Process -PassThru` + `exit 0/1`), при отказе продолжаем без прав и показываем
  предупреждение (раньше процесс просто завершался — выглядело как «программа пропала»).
- **Уникальные id профилей**: `make_id` давал `prefix-<секунды>`, и две стратегии, созданные
  подряд, получали одинаковый id (вторая «перетирала» первую при поиске по id). Теперь
  миллисекунды + счётчик.
- **Дубли имён профилей** отклоняются с понятным сообщением.
- **Битый `state.json`** копируется в `state.json.bad-<ts>` и попадает в журнал (раньше молча
  превращался в «программу без настроек»).
- **Две копии программы**: `data/zgui.lock` (PID + проверка живости) — предупреждаем, что
  настройки могут конфликтовать, но не блокируем запуск (замок может остаться от убитого
  процесса).
- Тексты ошибок запуска движка: «стратегия сразу завершилась — подробности в «Журнале»»
  (+ подсказка про права, если запускали не от админа), корень движка — «нажмите «Скачать
  движок»».

### S. Экран «Журнал» (пункт меню сразу после «Настроек»)
- Новая иконка `lucide-scroll-text.svg` (ISC), `#view-logs` в `index.html`, стили `.log-*`
  в `styles.css`.
- Фильтры по уровню (все/ошибки/предупреждения/успешно/инфо), поиск по тексту, кнопки
  «Сохранить отчёт», «Открыть папку логов», «Скопировать», «Очистить» (с подтверждением),
  счётчик «показано X из Y», автоскролл только если пользователь у низа списка.
- Обновление: событие `zgui:log` в реальном времени + поллинг `log_entries` раз в 2 с, пока
  экран открыт (при уходе с экрана поллинг выключается).

### T. Релиз 1.0.0 и портативность
- Версия поднята в `package.json`, `package-lock.json` (`npm version 1.0.0 --no-git-tag-version`),
  `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`.
- Удалён мусорный пустой `lib.rs` в корне проекта.
- Добавлен `LICENSE` (MIT) — на него уже ссылался `THIRD_PARTY_NOTICES.md`, а файла не было.
  В `THIRD_PARTY_NOTICES.md` обновлён список иконок Lucide (теперь 10 файлов).
- Портативность подтверждена по коду: `bundle.active: false` (один exe), рабочая папка — от
  `current_exe()`, движки/конфиги/список доменов вшиты (`include_bytes!`), профиль WebView2
  тоже в `data/webview`. Внешняя зависимость — только WebView2 Runtime (на Win10/11 уже есть).
- Написан **`docs/design.md`** — полный дизайн-документ: архитектура, модули, 50 команд,
  события, раскладка `data/`, движки, экраны, фоновые задачи, обработка ошибок, журнал, UAC,
  что программа меняет в системе, портативность, безопасность (в т.ч. почему `csp: null`),
  сборка/релиз, тесты, лицензии, роадмап, глоссарий.

### Проверка пятой волны
- `cargo check` — чисто, без предупреждений; `cargo test --lib` — **43 passed** (добавлены
  тесты логгера и переводчика ошибок).
- `npm run build` — ок; `npm run tauri build -- --no-bundle` — **успешно**:
  `src-tauri\target\release\zgui.exe`, 25 480 192 байта, 19.09.2026 22:58,
  `FileVersion/ProductVersion = 1.0.0`.
- Живьём (запуск, экран «Журнал», отчёт) — за владельцем, чеклист в `docs/TODO.md`.

## Шестая волна (19.09.2026, вечер) — звёзды вернулись, автосохранение, цвет иконки
Три просьбы владельца в одном заходе: «звёздочки перестали двигаться — можно вернуть?»,
«сохранение по нажатию флага, кнопка "Сохранить" не нужна», «иконка потеряла цвет».

### U. Анимация фона вернулась (без прежнего фриза)
Причина прошлого фриза (см. волну IV, п. J): при движущемся фоне Chromium пересчитывает
`backdrop-filter` у КАЖДОГО «стеклянного» элемента на каждом кадре. Дороже всего были
карточки и 25 плиток стратегий. Поэтому:
- у `.card` и `.profile` `backdrop-filter` **убран**, плотность даёт вуаль:
  `background: linear-gradient(var(--card-veil), var(--card-veil)), var(--panel)`;
  новый CSS-переменная `--card-veil` (grey/dark `rgba(0,0,0,.22)`, light `rgba(255,255,255,.6)`);
- стекло осталось у сайдбара/топбара/модалок/тостов (их мало);
- вернулись анимации: `.stars` → `star-drift 150s ease-in-out infinite alternate`,
  `.stars-near` → `star-twinkle 7.5s`, `.topo` → `topo-drift 200s`; ключевые кадры добавлены;
  `will-change: transform`; движение маятником (alternate), т.к. слои тайлятся разными
  размерами (280/380/520 px) и полный оборот дал бы видимый стык;
- выключатели: светлая тема (`html[data-theme="light"]`), `body.fx-off`, `body.fx-paused`
  (окно свёрнуто — `visibilitychange` в `main.js`), `@media (prefers-reduced-motion: reduce)`.

### V. Настройки применяются сразу, кнопки «Сохранить» больше нет
- `index.html`: кнопка `#btnSaveSettings` удалена, в заголовке карточки чип «применяются сразу».
- `main.js`: обработчик кнопки заменён на автосохранение — `change` для `#cfGameFilter`,
  `#cfIpset`, `#cfAutostart`; для `#cfInterval` — `input` с паузой 700 мс + `change` сразу;
  таймер `saveSettingsTimer` объявлен рядом с `sigSettings`.
- `lib.rs::set_settings` больше **не показывает тост** (иначе всплывашка на каждое переключение)
  и не принимает `AppHandle`; в отчёт добавлена строка `анимация фона: {bg_fx}`.
- Новый флажок «Анимация фона» в «Внешний вид» (`#cfBgFx`) + `applyBgFx()` в `main.js`
  (класс `body.fx-off`, кэш в `localStorage["zgui.fx"]`, применяется сразу).
- Бэкенд: `Settings.bg_fx` (`#[serde(default = "default_bg_fx")]`, дефолт `true`).

### W. Иконка «потеряла цвет» — найдена причина
Диагностика по файлам (не догадки): в исходнике `Z.png`/`icon.png` у «Z» **верхняя половина
белая, нижняя синяя `#009AFF`**. Разбор кадров `icon.ico` показал, что в мелких кадрах
(16/20/24/32/40/48) синей половины нет вовсе: старый `mkicon.py` строил маску по яркости
(`> 60`, синий туда попадал) и заливал её **чисто белым** — цвет терялся.
- `mkicon.py` переписан: квантование в 3 цвета (плитка `24,27,33` / белый / синий),
  раздельные маски белого и синего с уплотнением (`MaxFilter`), сборка на 8x + `BOX`-даунскейл,
  альфа/скруглённые углы из исходника; размеры ≥64 по-прежнему LANCZOS из `icon.png`.
- Грабли: `np.int16` в расчёте расстояний до палитры переполнялся (255² = 65025 > 32767) →
  кадры становились целиком белыми; лечится `np.int32`.
- Перегенерированы `icon.ico` (10 DIB-кадров) и `16x16.png`/`24x24.png`/`32x32.png`.
  ASCII-превью подтверждает синюю половину на всех размерах.
- **Ещё не проверено**: цвета внутри самого `icon.ico` (мой `icoparse.py` читал DIB как RGBA
  вместо BGRA — «blue» в нём не показатель, верить декоду Pillow) и **не объяснена** первая
  жалоба (в панели задач рядом с «Z» был generic-значок окна).

### Состояние на момент остановки (перезапуск opencode)
- Правки в коде **сделаны**, но **ничего не собрано**: `cargo check` / `npm run build` /
  `npm run tauri build -- --no-bundle` НЕ запускались. В exe лежит **старая** иконка и старая
  вёрстка — владельцу показывать нечего.
- Дальше: `cargo check` → `cargo test --lib` (43) → `npm run build` →
  тронуть `src-tauri\tauri.conf.json` (иначе tauri-build не перечитает `icon.ico`) →
  `npm run tauri build -- --no-bundle` (закрыть запущенный `zgui.exe`) → проверить иконку в
  Проводнике/панели задач и движение звёзд в темах «Космос»/«Графит».
<!-- УТЕРИ ПРИ ОБРЕЗКЕ 21.09: строки 736-794 отсутствуют в БД (не читались ни в одной сессии) -->
[утраченный блок: строки 736-794]


























































### 4.1. Прочее по фидбеку
- Старый кэш «Обновлений» в `state.json` продолжал показывать группы ZAPRET2 CONFIG/LUA/LISTS
  (проверка от 19.09): миграция теперь вычищает записи групп `zapret2*` и ключи
  `zapret2*` из `catalog/applied.json`.
- Иконка: владелец решил разбираться на другом уровне — код не трогаем (как есть).
- Скорость фона по просьбе владельца уменьшена вдвое: far 480 с, mid 320 с, near 200 с
  (мерцание 10 с), topo 600 с. Дельты (кратные тайлам) не менялись — скорость = дельта/время.
- «Обновления»: карточка Flowseal была в полколонки (осталась от двух движков) —
  `.engines` теперь одна колонка, карточка на всю ширину.
- Светлая тема: у логотипа в сайдбаре тёмная плитка квадратная, а контейнер резал её
  скруглением 12px (на светлом фоне читалось как «иконка режется»). В `src/assets/z-logo.png`
  вшито скругление (радиус 11/36 от размера = внутренний радиус рамки), теперь клип
  контейнера не режет тёмные пиксели. **Владелец: срез всё ещё виден, готовит новую иконку —
  пока не трогаем.**
- Подвал сайдбара (`#appMeta`): оставлена только версия (`v1.0.1`), убраны `portable · admin`
  и путь `release\data` (просьба владельца). Сборка exe не запускалась — ждём владельца.

### 7. Новый пак `Z-GUI-icons.zip` (20.09, 15:33) — интеграция как есть
- Владелец подготовил вручную: `windows/app.ico` (6 кадров 16/24/32/48/64/256, PNG-in-ICO,
  66 911 Б) + Square-логи; инструкция DeepSeek: интегрировать, не пересоздавая.
- Проверка исходников tauri-codegen 2.6.3: иконка окна на Windows берётся из первого
  `.ico` в `bundle.icon` (у нас `icons/icon.ico`), `icon.png` — только Unix. Значит,
  таскбар/окно = этот ICO.
- Встроено: `icon.ico` — **байт-в-байт** копия `windows/app.ico`; PNG 16/24/32/48/64 —
  кадры того же ICO, 128 — LANCZOS из 256, `128x128@2x`/`icon.png` = 256;
  Square 44/71/150/310 — из пака, остальные — даунскейл 256; `z-logo.png` сайдбара — 256.
  `icon.icns` не трогали (macOS не цель, bundler выключен).
- Проверка встроенного: `RT_ICON` = 6, `RT_GROUP_ICON` = 1; **SHA-256 всех 6 кадров
  в exe совпадают с кадрами исходного ICO** (byte-exact). Release 15:33,
  `zgui.exe` 14 398 976 Б (меньше: ICO 67 КБ вместо 300+ КБ DIB-пересборки).
- Владелец сгенерировал полный набор (QM Icon Studio: windows/app.ico + Square-логи,
  macos/AppIcon.icns, custom/icon-16…1024, web/android/ios — последние не нужны).
  Новый арт: тёмная плитка со скруглением **в самом файле** и синий «Z» (градиент
  #fafafa → #001eff).
- Встроено: `icons/icon.ico` — 6 DIB-кадров (16/24/32/48/64/256) из `windows/app.ico`;
  16/24/32/64/128/256/512 PNG — из `custom/`; `icon.icns` — из `macos/`;
  Square 44/71/150/310 — из пака, 30/89/107/142/284 и StoreLogo — даунскейл из 512.
  `src/assets/z-logo.png` заменён на `custom/icon-256x256.png`.
- `.brand-logo` в CSS: убраны рамка/фон/`overflow:hidden` — раньше контейнер резал
  квадратную плитку радиусом 12px, отсюда «иконка вырезана» в светлой теме; теперь
  видны собственные скругления арта.
- Проверка: `RT_ICON` = 6, `ExtractAssociatedIcon` из exe отдаёт новый «Z»;
  release 15:07, `zgui.exe` 14 673 920 Б.

### 5. Движок zapret2 (winws2) вырезан полностью — решение владельца
- Повод: стратегии не обходили блоки у тестеров + Windows Defender **удалил**
  `resources/engine-zapret2.zip` (внутри winws2.exe — «нежелательная программа»,
  os error 225 при сборке). Вместо исключений AV — отказ от движка.
- Удалено: `ENGINE_ZAPRET2`, поле `Roots.zapret2`, встроенный `engine-zapret2.zip` и
  `zapret2-master.zip` (файлы), `ensure_embedded_engine` для zapret2, `seed_zapret2_configs`,
  `builtin_zapret2_presets` (`z2-*`), `flatten_engine_args` (+ тест), группы обновлений
  zapret2, карточка движка и вкладка в UI, выбор движка в «Новом профиле», упоминания в
  отчёте/дизайне. `engine_root_for` теперь только Flowseal (`bin/`).
- Миграция `migrate_removed_engine`: удаляет профили winws2 из `state.json`, сбрасывает
  автостарт на удалённый профиль, чистит `data/engines/zapret2`, `data/catalog/zapret2`,
    `Win32_System_LibraryLoader`; описание пакета почищено от zapret2.
  - `lib.rs`: `apply_native_window_icon(&window)` — `LoadIconWithScaleDown(hinst,
    MAKEINTRESOURCE(32512), 48, 48)` (comctl32 v6; манифест v6 в exe есть — проверено),
    фолбэк `LoadImageW`, затем `WM_SETICON` для `ICON_SMALL` и `ICON_BIG`.
    48 px выбран потому, что панель задач рисует 24–48 px: уменьшение всегда чётче
    растяжения. Сам `icon.ico` не меняется.
- **Проверка без сборки exe**: `cargo check` — чисто; `cargo test --lib` — 44 passed.
  Владелец параллельно готовит новый арт «с резкими сторонами буквы» (теория DeepSeek
  про свечение/мягкие края у мелких кадров). Release-сборка — после его иконки.
- Повод: стратегии не обходили блоки у тестеров + Windows Defender **удалил**
  интегрирован тем же способом (`icon.ico` — байт-в-байт, PNG из `custom/`, `z-logo` = 256).
  Release собран: `zgui.exe` 14 402 048 Б, `RT_ICON` = 6, SHA-256 всех кадров = исходным.
  В exe присутствует код нативной иконки (лог-строки найдены в бинарнике).

### 9. Автозапуск: служба ⇄ GUI — взаимная блокировка (20.09, 16:38)
- **Проблема**: если установлена служба zapret и включён «Автозапуск GUI», то `ack_boot`
  при входе вызывал `do_start`, а тот через `do_stop` **останавливал службу** и поднимал
  winws как процесс приложения → «служба не работает» (тест владельца сорвался бы).
- **Фикс-предохранитель** (`ack_boot`): если служба zapret установлена — GUI-старт профиля
  пропускается, состояние службы обновляется из `sc query`, в журнал пишется причина.
- **Взаимная блокировка по решению владельца** (нельзя держать оба механизма):
  - бэкенд: `install_service` отклоняется при включённом `boot_app`; `set_boot_app(true)`
    отклоняется при установленной службе — с понятными русскими сообщениями;
  - UI «Настройки»: при установленной службе чекбокс «Автозапуск GUI» и селект «Автостарт
    профиля» заблокированы, под ними пояснение (`#bootGuiNote`); когда ничего не включено —
    подсказка, как включить (GUI+профиль либо служба);
  - UI профиля: кнопка «Автозапуск (служба)» показывает реальное состояние службы
    (`B.service.strategy === p.id`), а не `autostart_profile` (раньше путала: «включён»
    показывался просто за выбранный в настройках профиль); при включённом GUI-автозапуске
    кнопка заблокирована с подсказкой.
- Сборка: `zgui.exe` 14 402 560 Б, 20.09.2026 16:38; `cargo check` чисто, 44 теста.

### Чек-лист перезагрузки (тест владельца)
1. Профиль → «⋯» → «Автозапуск (служба)»: установка без ошибок; `sc query zapret` = RUNNING.
   (Если включён «Автозапуск GUI» — кнопка/чекбокс заблокированы: сначала отключить один механизм.)
2. Перезагрузка. После входа: служба поднялась сама (winws в Диспетчере задач, `sc query zapret` = RUNNING).
3. GUI (если открыт/автозапуск выключен): показывает «через службу», сам winws не дублирует.
4. «Журнал»: либо «обход поднимает служба zapret — GUI-старт профиля пропущен», либо
   «автозапуск: старт профиля …» — смотря какой механизм выбран.
5. Обратный тест (по желанию): удалить службу → включить «Автозапуск GUI» + выбрать профиль
   в «Автостарте профиля» → перезагрузка → GUI и winws поднимаются сами.
- Повод: стратегии не обходили блоки у тестеров + Windows Defender **удалил**
  `resources/engine-zapret2.zip` (внутри winws2.exe — «нежелательная программа»,
  os error 225 при сборке). Вместо исключений AV — отказ от движка.
- Удалено: `ENGINE_ZAPRET2`, поле `Roots.zapret2`, встроенный `engine-zapret2.zip` и
  `zapret2-master.zip` (файлы), `ensure_embedded_engine` для zapret2, `seed_zapret2_configs`,
  `builtin_zapret2_presets` (`z2-*`), `flatten_engine_args` (+ тест), группы обновлений
  zapret2, карточка движка и вкладка в UI, выбор движка в «Новом профиле», упоминания в
  отчёте/дизайне. `engine_root_for` теперь только Flowseal (`bin/`).
- Миграция `migrate_removed_engine`: удаляет профили winws2 из `state.json`, сбрасывает
  автостарт на удалённый профиль, чистит `data/engines/zapret2`, `data/catalog/zapret2`,
  `data/catalog/sources/zapret2`.
- Побочные обновления: `service::detect_conflicts` по-прежнему ловит чужие `winws2.exe`
  (это не наш движок, а конфликтующее ПО), в netreset остаётся имя процесса для kill-list.
<!-- УТЕРИ ПРИ ОБРЕЗКЕ 21.09: строки 904-929 отсутствуют в БД (не читались ни в одной сессии) -->
[утраченный блок: строки 904-929]

























- Жалоба: при проверке обновлений ничего не происходило визуально, уведомление приходило
  только после. Причина: `check_updates`/`apply_updates` отвечают сразу (работа в фоне),
  `zgui:prog` при этом не шлётся, кнопка «мигала» и снова становилась активной.
- Решение: `setUpdatesBusy()`/`clearUpdatesBusy()` в `main.js` — при нажатии показываем
  **бегущую полосу** (`#progBar.indeterminate`, CSS `prog-slide`) с подписью
  «Проверяю обновления… это может занять до минуты» / «Применяю обновления…», все кнопки
  обновлений блокируются; снимается по событию `zgui:updates`, по ошибке или по страховочному
  таймауту (3–5 мин). Точные проценты (установка движка) по-прежнему показываются полосой.
- Сборка: `zgui.exe` 14 402 560 Б, 20.09.2026 23:10; `cargo check` чисто, 44 теста.

### 14. Единая проверка обновлений + порядок вкладок (20.09, 23:31)
- **Одна кнопка вместо двух**: верхняя карточка «Обновления» теперь называется «Проверка
  обновлений» и содержит единственную кнопку «Проверить обновления» — она проверяет
  **всё**: конфиги (`check_updates`), движок (новая команда `engine_check_update` —
  сравнение папки релиза с последним тегом GitHub) и Telegram-мост (`tg_check_update`).
  Дублирующая кнопка из карточки конфигов убрана (`#btnCheckNow` удалён).
- Карточка движка Flowseal перемещена **в самый низ** вкладки; у неё появилась строка
  статуса: «Движок актуален (1.10.2)» или «Доступно обновление движка: X (у вас Y)».
- Вкладка «Стратегии»: баннер «Важно перед запуском» — **в самом верху**; фон теперь как
  у обычных карточек, осталась только жёлтая обводка (просьба владельца).
- Бэкенд: `updater::check_engine_latest()`, команда `engine_check_update` (в журнал
  пишется результат), `engine_version_from_root` (версия из имени каталога релиза).
- Сборка: `zgui.exe` 14 423 040 Б, 20.09.2026 23:31; `cargo check` чисто, 44 теста.

### 15. Служба по кнопке, версия движка, человеческие тексты про права (20.09, 23:55)
- **«Запускать службой» не работал**: чекбокс был заблокирован, пока включён автозапуск через
  программу. Теперь он доступен, как только выбран профиль; при включении **сам снимает**
  автозапуск через программу (очищает профиль + удаляет задачу планировщика) и ставит службу.
  Перед установкой — предупреждение «Windows запросит права администратора для её создания».
- **«Движок обновлён, а плашка „доступно обновление“ висит»**: версия установленного движка
  нигде не сохранялась (`engine_version_from_root` не мог разобрать имя каталога `flowseal`).
  Добавлено `AppState.engine_version` (serde default): пишется из тега релиза при
  установке/обновлении, для вшитого движка — `embedded::ENGINE_VERSION`; `engine_check_update`
  сравнивает сохранённую версию с последним тегом. После установки движка фронт сам
  перепроверяет версию (`updateProgress` → `fetch:*` done) — плашка снимается.
  Плюс `cleanup_stale_engine_dirs`: удаляет оставшиеся вложенные каталоги релиза
  (`zapret-discord-youtube-1.10.2` внутри корня после обновления). Нюанс: у владельца движок
  уже 1.10.3 (обновлялся до этой правки), версия не записана — после первого «Обновить движок»
  она зафиксируется, и плашка станет «актуален».
- **Термин «UAC» убран из интерфейса**: теперь понятные формулировки с указанием, для чего
  права: «Windows запросит права администратора — они нужны, чтобы запускать обход»,
  «Ставлю службу — Windows запросит права администратора для её создания»,
  «Скачиваю движок Flowseal…» (загрузка движка прав не требует). То же в `human.rs`
  (os error 5) и в подсказке при мгновенном завершении стратегии.
- Сборка: `zgui.exe` 14 427 136 Б, 20.09.2026 23:55; `cargo check` чисто, 44 теста.

### 16. Ревью кода + защита от подмен + релиз 1.1.0 (21.09, 00:35)
- **Автопроверки**: `cargo clippy --all-targets` — было 20 предупреждений, все исправлены
  (в т.ч. авто-фиксом с предварительным бэкапом `src-tauri/src`); теперь **0**. Осталось
  исправить реальную мелочь — в `updater.rs` два одинаковых `else if` слиты в одно условие.
  `cargo check` чисто, `cargo test --lib` — 44 passed. Один прогон дал 42/2 — совпал с
  параллельным clippy (тесты спавнят PowerShell/пробы портов), повторные прогоны стабильны;
  отметка: не гонять тесты параллельно с clippy.
- **Мёртвые зависимости удалены**: `url`, `futures` (в коде не использовались) — меньше
  поверхность supply-chain.
- **Защита от подмены/заразы** (просьба владельца):
  - `src-tauri/resources/checksums.sha256` — SHA-256 вшитых архивов (движок/конфиги/список домен),
  - `scripts/security-check.ps1` (UTF-8 BOM, иначе PS 5.1 ломает кириллицу — грабли повторены):
    сверка хешей ресурсов, вывод всех `include_bytes!`/`Command::new` для глазами-проверки,
    поиск типовых вредоносных паттернов (`Invoke-Expression`, base64+exec, `eval`, `atob`…),
    `cargo audit` + `npm audit`, скан `zgui.exe` через Windows Defender, SHA-256 релиза.
  - Поставлен `cargo-audit`: **0 уязвимостей**, 7 предупреждений (unmaintained/unsound у
    транзитивных крейтов: `unic-*`, `proc-macro-error`, `glib` — Linux-only, на Windows не
    компилируется). `npm audit` — 0.
  - Defender на релизном exe: **угроз нет**.
- **README.md** (корень): что это, как запустить, автозапуск, проверка SHA-256, отправка отчёта,
  лицензии. `THIRD_PARTY_NOTICES.md`: добавлен раздел про движок Flowseal (**MIT**, bol-van +
  Flowseal) и **WinDivert** (LGPLv3, в составе архива движка).
- **Релиз 1.1.0**: версия в `package.json`/`package-lock.json`/`Cargo.toml`/`tauri.conf.json`
  (проверено: FileVersion/ProductVersion = 1.1.0, `RT_ICON` = 6).
  `zgui.exe` 14 426 624 Б, 21.09.2026 00:35,
  SHA-256 `ce1e5e0d7ba5a3338faf7c8129184f8dba3955b82d46d1a3985cf4703cd47811`.
- **Пакет для тестеров**: `release/ZapretGUI-1.1.0-portable/` (exe + README + LICENSE +
  THIRD_PARTY_NOTICES + `checksums.txt`) и `release/ZapretGUI-1.1.0-portable.zip` (7.3 МБ).
  Текстовые файлы в пакете сохранены в UTF-8 **с BOM** — чтобы старый «Блокнот» на Windows
  не показывал кириллицу кракозябрами. Старый тестовый архив `zgui-v1.0.0-test-msvc-x86_64.zip`
  удалён из корня по решению владельца.
- **Известный долг (не блокирует релиз)**: `lib.rs` ~2600 строк (кандидат на распил);
  7 «unmaintained/unsound» предупреждений в транзитивных крейтах; журнал `progress.md` объёмный.

### 5. Движок zapret2 (winws2) вырезан полностью — решение владельца
- Повод: стратегии не обходили блоки у тестеров + Windows Defender **удалил**
  `resources/engine-zapret2.zip` (внутри winws2.exe — «нежелательная программа»,
  os error 225 при сборке). Вместо исключений AV — отказ от движка.
- Удалено: `ENGINE_ZAPRET2`, поле `Roots.zapret2`, встроенный `engine-zapret2.zip` и
  `zapret2-master.zip` (файлы), `ensure_embedded_engine` для zapret2, `seed_zapret2_configs`,
  `builtin_zapret2_presets` (`z2-*`), `flatten_engine_args` (+ тест), группы обновлений
  zapret2, карточка движка и вкладка в UI, выбор движка в «Новом профиле», упоминания в
  отчёте/дизайне. `engine_root_for` теперь только Flowseal (`bin/`).
- Миграция `migrate_removed_engine`: удаляет профили winws2 из `state.json`, сбрасывает
  автостарт на удалённый профиль, чистит `data/engines/zapret2`, `data/catalog/zapret2`,
  `data/catalog/sources/zapret2`.
- Побочные обновления: `service::detect_conflicts` по-прежнему ловит чужие `winws2.exe`
  (это не наш движок, а конфликтующее ПО), в netreset остаётся имя процесса для kill-list.

### Проверка
- `cargo check` — чисто, 0 предупреждений.
- `cargo test --lib` — **44 passed** (убраны `auto_profile_adds_hostlist`,
  `flattens_zapret2_groups...`; добавлены `boot_task_script_registers_and_removes`,
  `relaunch_preserves_boot_argument`, `build_cmdline_finds_nested_exe`).
- `npm run build` — ок (18 модулей).
- `npm run tauri build -- --no-bundle` — успешно: `src-tauri\target\release\zgui.exe`
  **14 645 248 Б** (было 25.5 МБ: минус движок zapret2 ~10 МБ и каталог ~0.8 МБ),
  FileVersion/ProductVersion **1.0.1**, `RT_ICON` = 6. Пересборка после правок звёзд
  и чистки кэша обновлений — 20.09.2026 14:30.
- Версия 1.0.1: `package.json`, `package-lock.json`, `Cargo.toml`, `tauri.conf.json`.

### Осталось за владельцем (живая проверка)
- Перезагрузка с включённым «Автозапуск GUI при входе» + выбранным профилем автостарта:
  GUI и winws должны подняться сами, без UAC.
- Установка службы (`Автозапуск` у профиля) → `sc query zapret` = RUNNING.
- Иконка в панели задач/проводнике; плавающие звёзды; отсутствие вкладки Zapret2.

## 21.09.2026 — права администратора (первый вход) + баг «служба winws как чужой» + AV-тест
Критерий «готово»: (а) на первом запуске GUI предлагает «Всегда запускать от администратора»
и один перезапуск вместо 5 подряд UAC-окон; (б) winws, поднятый нашей службой/раннером,
не помечается «чужой процесс»; (в) документированный тест, что антивирус не мешает
журналу и обновлению движка.

### Первый вход: права администратора
- `config.rs`: в `Settings` поле `admin_onboarded` (флаг «предложение уже показано»).
- `lib.rs`: в `Bootstrap` добавлено `elevated` (реально elevated ли GUI сейчас);
  команды `set_admin_prefs(always)` (ставит `always_admin` + онбординг) и
  `mark_admin_onboarded()`; зарегистрированы в `invoke_handler`.
- `index.html`: модалка `#adminModal` (копия карточки из скриншота: чекбокс
  «Всегда запускать от администратора» + «Включить и перезапустить» / «Не сейчас»,
  пояснение про 5 запросов и один перезапуск).
- `main.js`: `maybeOfferAdmin()` вызывается в init до `checkConflicts(true)`;
  если показали модалку — проверку конфликтов откладываем до закрытия. Если гуй уже
  elevated — молча ставим `admin_onboarded`. «Не сейчас»/крестик — `mark_admin_onboarded` + `checkConflicts`.
  `askRestartAdmin()` — общий диалог перезапуска; переиспользуется: первый вход,
  чекбокс `#cfAlwaysAdmin` при включении, кнопка `#btnElevateNow`.

### Баг: служба winws считалась «чужим процессом»
- Причина: `detect_conflicts` пропускал только `pid == наш`, а winws, поднятый нашей
  службой zapret или elevated-раннером, имеет другой PID → «чужой процесс zapret».
- Фикс в `service.rs`: получение путей exe через WMI (`Get-CimInstance Win32_Process`,
  вывод `PID|путь`, разделитель `'|'` — запрещён в именах файлов Windows), хелпер
  `is_own_engine(path, data_dir)` (регистронезависимо, префикс с завершающим `\`,
  чтобы `data` не ловило `data2`). Процесс, чей exe под нашим `data/`, не конфликт.
  Фолбэк: своя служба установлена и запущена, а путь WMI не отдал (нет прав) —
  считаем winws «своим».
- Новый юнит-тест `service::tests::own_engine_recognised_by_data_prefix`.

### Блокировка записи/движка антивирусом (тест)
- `scripts/defender-test.ps1`: статус защиты (real-time, PUA, исключения), повторяет
  паттерн записи журнала (`tmp → rename`) в `data/logs`, «обновление движка» — копия
  winws.exe/WinDivert в сандбокс под `data/tmp` с паузой и сверкой хеша, история угроз
  Defender по нашей папке; ВЕРДИКТ OK/проблемы + что делать.
- `docs/antivirus-test.md`: ручной чеклист (запуск стратегии → stdout/stderr в логах,
  сохранение отчёта, обновление catalog с бэкапами, обновление движка), таблица
  «ожидаемый результат / если нет», инструкция по исключениям и PUA-детекту.

### Проверка
- `cargo check --all-targets` — чисто (одно время была ошибка: `'{0}|{1}'` в format! —
  экранировано `'{{0}}|{{1}}'`).
- `cargo test --lib` — **45 passed** (+ `own_engine_recognised_by_data_prefix`).
- Быстрая сборка фронта к релизу не делалась: правки только в HTML/JS-статистике
  модалки и логике (сборку владелец обычно гоняет командой `npm run tauri build`).
- `npm run tauri build -- --no-bundle` выполнен тут же: vite ok (18 модулей),
  `src-tauri\target\release\zgui.exe` собрался за 3м10с.
- `scripts/defender-test.ps1` переделан: сам находит рабочую папку
  (сначала `src-tauri\target\release`), не закрывает окно (пауза Enter, `-NoPause`
  для конвейера), ловит критические ошибки, уточнён раздел истории угроз
  (выводит пути ресурсов; без админа — контекстная проверка по JSON с пометкой).
  Живой прогон на свежей сборке: **ВЕРДИКТ OK (6 ok, 0 провалов)** — журнал
  пишется, winws.exe/WinDivert64.sys в сандбоксе переживают паузу.
- Причина «консоль закрылась сразу» у владельца: (а) в пакете `release/...` скрипта
  нет — он в репозитории; (б) PowerShell 5.1 читает `.ps1` без BOM как cp1251,
  и Cyrillic-байты дают «умные кавычки», ломающие разбор. Скрипты сохранены
  в UTF-8 **с BOM** (как тестовые файлы пакета).
- Разобрано происхождение 15 записей `Trojan:Script/Wacatac.*` в истории Defender:
  это **наши unit-тесты** — харнесс `zgui_lib-*.exe` (target\debug\deps) распаковывает
  встроенные бинари zapret2 (`mdig`, `nfqws2`) в `%TEMP%\zgui-engine-test-*`, и ML-детект
  срабатывает по ним. К собранному exe отношения не имеет; скрипт такие записи
  не считает проблемой (pattern только winws/WinDivert/наша папка).

### Владелец: отладка на target-release (релиз отложен)
- Релиз по просьбе владельца **отложен**; пакет `release/ZapretGUI-1.1.0-portable`
  не пересобирался (там старый exe и нет скриптов — не тестировать по нему).
- Рабочая сборка для отладки: `src-tauri\target\release\zgui.exe` (свежая,
  с новыми правками: модалка админа, фикс detect_conflicts, AV-скрипт в репозитории).

### Подготовка к релизу на GitHub (локальный чеклист)
- `release/prereview/` — bundle для внешнего ревью (DeepSeek): README (root+пакет),
  RELEASE-NOTES-draft.md, PROMPT.md (текст запроса, юр. контекст по «обход блокировок»),
  docs/design|antivirus-test|TODO|feature-spec, configs (Cargo/tauri/package+lock),
  scripts (security-check, defender-test), LICENSE, THIRD_PARTY_NOTICES.
- `scripts/security-check.ps1` (ему добавлен BOM) — **OK**: хэши вшитых ресурсов целы,
  подозрительных паттернов нет, cargo audit 0 уязвимостей (7 unmaintained/unsound —
  известный долг), Defender-скан `zgui.exe` — чисто.
- Свежий exe (после всех правок админа/detect_conflicts): `src-tauri\target\release\zgui.exe`
  14 454 272 Б, SHA-256 `e48d2a170c363b4c4d90b2ac8b6e75902dcec60a7edeeee7853b8e346851c233`.
- Пакет пересобран: в `release/ZapretGUI-1.1.0-portable/` новый `zgui.exe`, из `data/`
  убраны личные артефакты тестера (`state.json`, `zgui.lock`, `logs/*`, `tmp/*`) —
  остались предзасеянные `data/engines` и `data/catalog`; `checksums.txt` обновлён.
  Zip `release/ZapretGUI-1.1.0-portable.zip` пересобран (20 974 451 Б, 409 записей,
  без state.json/local). ВНИМАНИЕ: zip вырос с 7.3 МБ → 20.9 МБ из-за распакованного
  движка в `data/engines` (в пакете 1.1.0 его не было). Вариант «легче» — пакет без
  предраспакованного движка (первый запуск распакует из exe).
- Решения за владельцем: версия релиза (1.1.0 / 1.1.1 / 1.2.0 — проставить в трёх местах
  и в имени папки), публикация GitHub Release (черновик через `gh`), отдавать ли bundle
  DeepSeek до или после версии.

### ПЛАН (решение владельца от 21.09, отложено — НЕ выполнять без команды)
- Версия релиза: **1.2.0** (решение принято). При пересборке проставить в
  `Cargo.toml`, `package.json`, `package-lock.json`, `tauri.conf.json` + имя папки/архива.
- Лёгкий архив: **ОДОБРЕНО** — пакет вообще **без `data/`** (движок и каталог
  пересоздаются из вшитых ресурсов при первом запуске: `ensure_embedded_engine`,
  `seed_catalog`). Ожидаемый вес zip ~16 МБ вместо 20.9 МБ.
- Дальше: правки по ревью DeepSeek (`deepseek_markdown_20260921_eefef0.md`) →
  пересборка → `gh release create --draft`.

### Ревью DeepSeek (21.09) — триаж
Готово (внесено здесь же): п.1 (формулировки «обхода» в 2 README + checksums.txt),
п.3–4 (релиз-ноуты: согласование «нашей программы», «запрос прав»), п.16 (CSP —
заметка в релиз-ноуты), п.5–6,10–11 (defender-test.ps1: ожидание 25 с + проверка
хаша, исполняемость winws --version в сандбоксе, маска `release\ZapretGUI-*-portable`,
сортировка истории по дате), п.7,8 (шапка «только Defender», отсылка на
security-check для скана zgui.exe), п.18,24,25 (README пакета: возврат к админ-
настройкам, ссылка WebView2, убраны ссылки на repo-docs), п.23 (CHANGELOG.md).
Осталось на владельца: п.12 (версия — уже решена 1.2.0, проставим при сборке),
п.13 (cargo tree — tokio net), п.14–15 (tauri.conf: targets/csp null, identifier —
выбрать домен), п.17,22 (SmartScreen: читать ли код/подпись; в README уже описано),
п.21 (скриншоты для релиза), п.26 (THIRD_PARTY_NOTICES: что реально перенесено),
п.27 (переименовать resources/checksums.sha256 → embedded.sha256), п.28 (корневой
README: бейджи/сборка/contributing). Блокер сборки: нет.

### Осталось проверить живьём (владелец)
- Первый запуск на чистом профиле: модалка «Права администратора» → «Включить и
  перезапустить» — один UAC, GUI стартует elevated и без дальнейших запросов.
- После запуска winws через «Автозапуск» (службу) — УСПЕШНЫЙ старт профиля не должен
  показывать «чужой процесс zapret».
- `scripts\defender-test.ps1` в папке portable — ВЕРДИКТ OK; повторная проверка после
  реального обновления движка.

## 21.09 — поиск причины окна «Системная ошибка» после теста

- Пользователь прислал скрин: окно `winws.exe - Системная ошибка`, «система не обнаружила
  cudwin1.dll». Это был диалог от песочницы `defender-test.ps1` — туда копировался только
  `winws.exe`, а загрузчику нужны соседние DLL.
- Проверено фактами: `winws.exe` (203 776 байт) импортирует cygwin1.dll и WinDivert.dll
  (строки cudwin в exe/WinDivert.dll/cygwin1.dll отсутствуют); из полного `bin\` движка
  `winws.exe --version` отрабатывает с кодом 0, диалога нет. VLM на мелком тексте диалога
  читает имя DLL неточно (проходы давали cudwin1/sugwin1) — точное имя не важно,
  диагноз подтверждён фактами.
- Исправлено в `defender-test.ps1`: перед пробой исполнения в сандбокс копируется весь
  `bin\` движка. Повторный прогон: «winws.exe исполнился (код 0)», ВЕРДИКТ OK (7 ok),
  диалога нет.
- Старый хвост в антивирусном тесте давал ложный «держится в процессе» (зависал на
  диалоге) — теперь честный exit-код. `docs\antivirus-test.md` обновлены (7 ok, ловушка
  «Системная ошибка», нужные DLL).
- Почищены хвосты песочниц `data\tmp\av-engine-*`.
- Вывод для релиза: у движка есть соседние DLL — при проверке запуска всегда копировать
  весь `bin\`; при обновлении движка в приложении это и так делается (extract_all
  распаковывает всю папку `bin`).
- Создан `CHANGELOG.md` (п.23 ревью DeepSeek, черновик под отчёт релиза).
- Снимок `release/prereview/` освежён под правки (README-root/-package, defender-test,
  CHANGELOG); `RELEASE-NOTES-draft.md` не обновился из-за блокировки файла — повторить
  при следующей синхронизации.

## 21.09 — сборка 1.2.0 (лёгкий portable, без data/)

- Владелец разблокировал финальную сборку и выбрал «сразу 1.2.0».
- Версия поднята в 4 файлах: `src-tauri\Cargo.toml`, `src-tauri\tauri.conf.json`,
  `package.json`, `package-lock.json` (1.1.0 → 1.2.0); снапшот `release\prereview\config\`
  обновлён. Релиз-ноуты про prореview — RELEASE-NOTES-draft актуализировать при финализации.
- `cargo test --lib` — 45/45, чисто. Пересборка `npm run tauri build -- --no-bundle`:
  ProductVersion = 1.2.0. Новый exe: 14 454 272 байт,
  SHA-256 `8400D6D09B07EBE4832ECA43A6720F983AF9C4323B2893567AA9791426CDBD59`, ts 21.09 05:33.
- `security-check.ps1` на свежие бинари: OK (хеши ресурсов целы, Defender-скан чист,
  cargo audit 0 уязвимостей / 7 unmaintained-предупреждений).
- Собран лёгкий пакет `release\ZapretGUI-1.2.0-portable\`: zgui.exe + README.md + LICENSE +
  THIRD_PARTY_NOTICES.md + checksums.txt (новый хеш), БЕЗ data/. Первый запуск:
  `seed_catalog` + `ensure_embedded_engine` (lib.rs:2399 и lib.rs:507) распакуют движок
  и каталог из встроенных ресурсов.
- Архив `release\ZapretGUI-1.2.0-portable.zip`: 7 309 369 байт, 5 записей, верхний каталог
  `ZapretGUI-1.2.0-portable/` — как в 1.1.0 (730 КБ exe победа над 20.9 МБ).
- Осталось проверить вручную (чеклист «Тест: …»): первый запуск из папки пакета без data/,
  админ-карточка, стратегия, обновления. Затем финал: gh релиз v1.2.0.
<!-- УТЕРИ ПРИ ОБРЕЗКЕ 21.09: строки 1210-1254 отсутствуют в БД (не читались ни в одной сессии) -->
[утраченный блок: строки 1210-1254]












































  программы» + кнопка «Остановить» активна.

### Проверка

- `cargo check --all-targets` — чисто; `cargo clippy --all-targets` — 0
  предупреждений (заодно `map_or` → `is_none_or` в lib.rs).
- `cargo test --lib` — **47 passed** (+`sync_ipset_materializes_loaded_list`,
  `summarize_breaks_ties_by_critical_groups_then_latency`; `probe_localhost_fails`
  переведён на tokio).
- `npm run build` — ок (18 модулей). `npm run tauri build -- --no-bundle` — ок:
  `src-tauri\target\release\zgui.exe` 14 490 112 Б, ProductVersion 1.2.0,
  SHA-256 `FFAC732E3F4EB184B1F53FC4DD2B5A0A29A877BDC18FCD05646067A144C9DB88`, ts 21.09 11:27.
- Не проверено живьём: реальный прогон теста на сети владельца, наполнение
  ipset при первом запуске (сработает автоматически: `sync_ipset` при старте),
  детект ручного .bat. Пакет `release\ZapretGUI-1.2.0-portable` НЕ пересобирался
  (нужен новый хеш exe и повторный тест).
- Нюанс: `sync_ipset` при каждом старте перезаписывает ipset движка копией из
  каталога GUI (даже если владелец обновил список через service.bat) — осознанный
  компромисс: GUI остаётся единственным менеджером конфигов.

### Догон по скрину владельца (21.09, 11:49) — тест «умер» на новом exe

Скрин: «winws запущен вне программы» + у 19 стратегий `process exited immediately
(exit ) A copy of winws is already running with the same filter`, у ALT/ALT3 —
`Не удается найти тип [System.Net.Http.HttpClientHandler]`. Три причины:

1. **`stop_all_own` вызывался условно** (`if had_runtime || had_service`) — внешний
   winws (runtime не знает о нём) не глушился, и все winws выходили сразу: фильтр
   WinDivert занят. Теперь `stop_all_own` вызывается всегда + после него проверка
   `any_winws_running()`: если winws жив (чужой движок/отказ UAC) — тест честно
   отменяется с понятной ошибкой, а не выдаёт мусор.
2. **PS 5.1 не резолвит `System.Net.Http.HttpClientHandler` без загрузки сборки**
   (проверено на этой машине: напрямую — FAIL, после `Add-Type -AssemblyName
   System.Net.Http` — OK). В скрипт раннера добавлен `Add-Type`.
3. **`Start-Probe($client, $host)`**: `$host` — read-only автоматическая переменная
   PS, присвоение параметра падало. Переименовано в `$target`.

Плюс: в раннере пустой exit-код (`exit `) — теперь `WaitForExit()` перед чтением
`ExitCode`; юнит-тест раннера усилен (`r.error.is_none()` + проверка `Add-Type` и
`https://$target/`) — именно он поймал баги 2 и 3, старая версия теста их
пропускала (score==0 проходил и при упавшей пробе).

Проверка: `cargo test --lib` — 47/47, clippy чисто. Пересборка:
`zgui.exe` 14 490 624 Б, ProductVersion 1.2.0,
SHA-256 `67C984C2DB75524FBE883F9A3BDB9F0167F22BDCD1D400ADD778F510E843B1F5`, ts 21.09 12:01.
Живьём проверено отдельно (PS-скрипт): `https://example.com/` → http 200,
`https://127.0.0.1/` → отказ; т.е. проба успех/неуспех различает.

## 21.09 — полное ревью «в остальном» (фронт + хвостовые модули)

Критерий «готово»: отчёт по пяти осям (корректность/читаемость/архитектура/
безопасность/производительность) + исправление обязательных находок; проверка
тестами и сборкой. Живой прогон — за владельцем.
<!-- УТЕРИ ПРИ ОБРЕЗКЕ 21.09: строки 1308-1372 отсутствуют в БД (не читались ни в одной сессии) -->
[утраченный блок: строки 1308-1372]
































































Zip: 7 327 727 Б, SHA-256 `F8C4BF26B49AB30F9157A23367AD3560BAEB204CFD897AC26E23FB079C6D5BD0`.

## 21.09 — подготовка пакета к релизу

Критерий «готово»: портативный zip замкнут сам на себя (exe + checksums +
README + LICENSE + THIRD_PARTY_NOTICES), содержимое и хеши сверены, changelog
и пакетный README актуальны. Публикация на GitHub — отдельным шагом (нужен gh
или ручная загрузка). Диф смотреть по проверенным командам: cargo test (47/47),
clippy (0), security-check (0 уязвимостей, Defender чист).

- `git status`: 8 файлов изменено, не коммичено (все правки сессии). Тегов в
  репо нет; `origin = lECL1PS3l/zgui`; `gh` CLI не установлен; CI-пайплайнов нет.
- CHANGELOG.md `[1.2.0] — не выпущено (тестируется)` → в раздел «Исправлено»
  добавлены итоги сессии (HTTPS-проба, ipset из .service, stop_all_own, Add-Type,
  UI-фиксы).
- Пакетный README: добавлен раздел «Антивирусы и WinDivert» (не хватало — без
  него пользователи теряются в карантинах PUA).
- `checksums.txt` не менялся (хеш только exe); zip пересобран (README внутри),
  распаковка проверена: 5 файлов, exe = `0E7544…E95`, README 8102 Б.
- Итоговый zip: 7 328 131 Б, SHA-256 `AA8AD9B2D7A30257455462B1591E7CF078D3AE6D56B86942BC80FA9BCB55C51A`.
- Осталось на решение владельца: коммит + тег `v1.2.0` + push, релиз-страница
  GitHub (выложить zip + checksums.txt).

## 21.09 — plug-and-play: единая модель запуска/автозапуска (реализовано)

Критерий «готово»: любое действие даёт осмысленное состояние (без тупиков),
программа не называет «внешним» свой winws (в т.ч. во время теста),
«Применить лучшую» = автозапуск + запуск сейчас одним кликом.

Спека: `docs/superpowers/specs/2026-09-21-unified-run-state-design.md` (дизайн
утверждён владельцем).

Сделано (бэкенд, `lib.rs`):
- `WinwsOwner{none|app|service|test|external}` + чистая `winws_owner_of(...)` +
  `current_owner(g)`; поле `owner` в `bootstrap`. Приоритет: тест →
  процесс программы → служба → внешний → пусто.
- Watchdog: `external` теперь `current_owner == External` — во время теста
  больше не ложный «вне программы» (жалоба 3).
- `start_or_switch`: запуск не блокируется настройками — служба есть →
  пересоздаём её под профиль (repoint) через идемпотентный `svc::install_service`;
  иначе прямой winws. Во время теста запуск запрещён (не убиваем winws теста).
- `apply_best_strategy` (замена `set_best_strategy`): пишет профиль автостарта +
  сразу запускает сейчас (жалоба 1).
- `autostart_plan(service_installed, have_profile)` +
  `sync_autostart(g)` — единая точка приведения механизма автозапуска к одному
  (задача планировщика ⇔ программный режим и выбран профиль). Вызывается из
  `set_settings`, `install_service`, `remove_service`, `apply_best_strategy`.
- `install_service`/`remove_service` без тупиков: служба снимает задачу
  планировщика сама; удаление службы сохраняет профиль — автозапуск переходит
  на программу. Команда `set_boot_app` удалена (её роль у `sync_autostart`).
- `tester::runner_alive(data)` — маркер живого раннера для владельца.

Сделано (фронт, `main.js`): тулбар по `owner` (тест/служба/внешний/процесс),
плитки профилей активны и для службы, кнопка теста «Применить: автозапуск +
запустить сейчас», карточка автозапуска без тупиков, `zgui:test` обновляет
тулбар.

Проверено: `cargo check --all-targets` чисто, `cargo clippy --all-targets` 0,
`cargo test --lib` 49/49 (добавлены `owner_precedence`,
`autostart_task_only_in_program_mode`), `npm run build` ок.

Правки владельца после проверки на чистой сборке (21.09):
- Индикатор watchdog в шапке теперь «стратегия активна / не отвечает» (было
  «обход работает / не работает» — вводило в заблуждение). Watchdog наблюдает
  за владельцем обхода (программа ИЛИ служба), а не только за процессом GUI, и
  гаснет сразу при остановке (обработчик `zgui:status` → `renderWatchdog(null)`).
- Архив `ZapretGUI-1.2.0-portable.zip` теперь содержит ТОЛЬКО `zgui.exe`:
  README/LICENSE/THIRD_PARTY_NOTICES/checksums убраны (всё есть в GitHub).
  Прочие изменения в блоке сборки ниже.
- Проверено: `clippy` 0, `cargo test --lib` 49/49.

Сборка: `npm run tauri build -- --no-bundle` (GUI закрыт) →
`src-tauri\target\release\zgui.exe`, 14 484 992 Б, ts 21.09 18:19,
SHA-256 `0EB3CFD7B6E171B2E015BAA1751D96611864CD6E1407AA8791DBF6655BB0ACE3`.
Release-пакет (только exe) обновлён, zip пересобран и проверен
(1 файл — `zgui.exe`): 7 320 286 Б,
SHA-256 `34AE93CB220EB2FCEA992B040D4673723FEAB054516D5E9E360864A0AF374CA3`.

### Сверка независимого ревью (21.09)

Внешний агент оставил отчёт `docs/superpowers/reviews/2026-09-21-code-review-findings.md`
(2 Major, 6 Minor, 4 Nit). Все находки проверены по коду — подтвердились, кроме
двух, оставленных осознанно (ниже). Правки внесены:

- **Major (main.js):** «— не запускать —» при установленной службе теперь сбрасывает
  `autostart_mode/profile` через `set_settings` ДО `remove_service` — иначе бэкенд
  сохранял профиль и включал программный автозапуск (тост врал «выключено»).
- **Major (lib.rs `set_settings`):** `sync_autostart` вызывается только при смене
  полей автозапуска; иначе любое сохранение настроек у не-админа повторяло UAC на
  создание задачи.
- **Minor (lib.rs `do_stop`):** остановка службы сразу пишет `service_running =
  Some(false)` + `status` — тулбар/плитка гаснут без задержки 10 с.
- **Minor (lib.rs `fetch_engine`):** захват `busy` — атомарный `compare_exchange`,
  закрыт TOCTOU двух быстрых «Скачать движок».
- **Minor (lib.rs `start_or_switch`):** ветка repoint/`do_start` — по факту
  `svc::service_state()`, а не по записи в `state.json` (устаревшее состояние
  вело ко второму механизму обхода).
- **Minor (lib.rs `provision_boot`/`delete_profile`):** задача планировщика
  снимается, когда профиль автозапуска недоступен (вырезанный движок, удалённый
  профиль); `delete_profile` сбрасывает устаревшую ссылку `autostart_profile`.
- **Nits:** `pid_alive(0) == false` (System Idle Process); watchdog проверяет
  «уже запущен» до сборки reqwest-клиента; удалено мёртвое поле `Bootstrap.external`;
  `AutostartPlan` свёрнут в `autostart_wants_task -> bool`.

Оставлено осознанно (не баги релиза):
- блокирующий UAC в `install_service`/`remove_service`/`set_settings` (мелкое
  замедление опроса при открытом UAC-диалоге; правка — вынести в `spawn_blocking`);
- `is_own_engine` считает «своим» только движок под `data/` — ручной winws из
  кастомного корня вне `data/` не глушится (нужен проброс настроенных корней).

Открытый вопрос владельца: `ack_boot` при «служба установлена, но остановлена»
не поднимает её сам (Windows это делает при загрузке) — пропуск намеренный?

Проверено: `cargo clippy --all-targets` 0, `cargo test --lib` 49/49,
`npm run build` ок.

Пересборка после правок (21.09, GUI закрыт): `npm run tauri build -- --no-bundle` →
`src-tauri\target\release\zgui.exe`, 14 491 648 Б, ts 21.09 19:15,
SHA-256 `AF929C988E23539D563BDF286F6B2274E17885883CD3A1671FB178C1FBA707F5`.
Release-пакет обновлён (в `ZapretGUI-1.2.0-portable\` и `.zip` только `zgui.exe`):
zip 7 321 475 Б, SHA-256 `01FA9873265CB140472143A3475E78A77FEC7AFF24BB73A90E787636EC0DC33A`.
Живой прогон на новом exe — за владельцем; скан `scripts\security-check.ps1`
пройден на новом хеше (Defender чист, 0 уязвимостей).

Релиз опубликован (21.09, ~22:30): тег `v1.2.0` → `6c1f11e`, ассет `zip`
(01fa…), GitHub Release на https://github.com/lECL1PS3l/zgui/releases/tag/v1.2.0.
Документ-журнал `progress.md` обновлён; `docs/superpowers/` хранится локально
(не коммичится, в релиз не входит).

## 21.09 (вечер) — проблема тестера: нет WebView2 Runtime

Тестер на «чистой» сборке Windows получил стандартное окно Tauri:
«Could not find the WebView2 Runtime». Это не баг проекта — отсутствует системный
компонент (Win10 LTSC, урезанные сборки, Server, свежая установка без обновлений,
или WebView2 установлен только для другого пользователя). Внешний агент (DeepSeek)
дал корректный диагноз, но часть фактов не бьётся с проектом: `README-package.md`
не существует (в zip только `zgui.exe`), ссылка на WebView2 в `README.md` уже была,
а релиз v1.2.0 уже опубликован.

Решение владельца: пакет и релиз **не трогаем**, ограничиваемся документацией.
- `README.md`: новый раздел «Известные проблемы» — пошаговое решение ошибки
  WebView2 (ссылка на `developer.microsoft.com/en-us/microsoft-edge/webview2` и
  прямой `go.microsoft.com/fwlink/p/?LinkId=2124703`); шаг 3 «Быстрого старта»
  ведёт в этот раздел.
- `github pre release/RELEASE-NOTES.md`: блок «Известные проблемы» со ссылкой на
  установщик.
- Сборка/релиз не пересобирались.

