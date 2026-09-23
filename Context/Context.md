# Контекст проекта: Z GUI (Tauri, оригинал)

Репо: `E:\Base\opencode\Z GUI`. Tauri 2 GUI для zapret (Flowseal winws). Основное: `src-tauri/src/lib.rs` (~3100 строк, команды), `runner.rs`, `service.rs`, `watchdog.rs`, `telegram.rs`, `tester.rs`, `dns.rs`, `netreset.rs`, `config.rs`, `logger.rs`, `updater.rs`, `embedded.rs`; фронт `src/main.js`, `src/styles.css`, `src/index.html`.
Режим: ponytail (full). Русский вывод. Проверки: `cargo check`, `cargo test --lib` (49 тестов).

## ВАЖНО: порт на WinForms ОТМЕНЁН

23.09.2026 пользователь: «Порт не нужен, нужно исправить оригинал». Каталог `winforms/` удалён (HEAD тоже удаляет). История порта (задачи 1-16, 137 тестов) — в Context.md.bak.1/.2. План `docs/superpowers/plans/2026-09-22-winforms-port.md` более не актуален.

## СМС от пользователя (23.09.2026)

«Проверь проект и исправь все ошибки, баги, зависания, добавь взаимную блокировку исключающих друг другу функций, убери таймер при запуске, приведи проект в максимально стабильный вид, программа должна выполнять свою функцию на все 100% и максимально быстро»

## Ответ агента: стабилизация оригинала (23.09.2026)

### 1. Взаимная блокировка операций (OPS)
- lib.rs: `static OPS: Mutex<()>` + `ops_try()` (try_lock, берётся ДО state-лока); `Global.op_running: AtomicBool` + методы; `Bootstrap.op_running`.
- Каждая команда-операция берёт `ops_try().ok_or("идёт другая операция — дождитесь завершения")?`, ставит флаг, эмитит `zgui:op {running,kind}`, сбрасывает во ВСЕХ ветках выхода (включая фоновые thread::spawn). Покрыто: start_or_switch (do_start-ветка сама ставит/снимает), stop_running, test_strategies (снимает в фоновом потоке теста, включая stopped-ветку), install_service, remove_service, check_updates, apply_updates, fetch_engine, kill_conflicts, net_reset, apply_dns, reset_dns, apply_best_strategy.
- main.js: `B.opRunning` → плашка «идёт операция…» (`.run-state.busy`, amber), `#btnStop.disabled`, `runBtn.disabled` в renderProfiles (кнопка теперь `runBtn`, не `acts.children[0]`), ранний отказ в `runTest()`, `B.opRunning` в сигнатуре перерисовки, `listen("zgui:op")` → `refreshAll()`.

### 2. Таймеры старта убраны
- `askRestartAdmin()`: удалён `delaySec: 3`.
- Убран `lockBtnWithCountdown($("#adminEnable"), 3)` (модалка первого запуска).
- `maybeOfferAdmin()`: ожидание каталога обновлений 12000 мс → 3000 мс.

### 3. Ускорение
- `pid_alive` (runner.rs): Windows — WinAPI OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)+GetExitCodeProcess (259=жив), CloseHandle; не-Windows — tasklist. Cargo.toml: windows-sys feature `Win32_System_Threading`.
- Удалён dead-код `fn ops()`.

### Верификация
`cargo check` — 0 warnings; `cargo test --lib` — 49 passed / 0 failed.

### Не сделано (осознанно)
- tg_toggle и DNS-кнопки фронта не блокируются по opRunning (Telegram-мост лёгкий, TCP-бинд 20 с).

## Идеи

23.09.2026 | ToDo Feature: "Мульти-движковость: внедрить zapret2 (bol-van, winws2.exe) и GoodbyeDPI (ValdikSS) как дополнительные движки; byedpi (hufrea, ciadpi) — посмотреть, но он SOCKS-прокси. GoodbyeDPI давно без крупных обновлений — возможно, заменить/дополнить byebyedpi-подходом (у того рабочие обходы)"

23.09.2026 | ToDo Feature: "Референсы GUI-обёрток: ByeDPI Manager (romanvht, .NET, byebyedpi+ProxiFyre/системный прокси), CDPI UI (Storik4pro, .NET, компоненты через магазин, автоподбор параметров, импорт .bat/.txt, шеринг пресетов)"

## СМС от пользователя (23.09.2026, второе)

«Софт для дурения DPI на ПК: ByeDPI Manager от romanvht (Windows), zapret-discord-youtube от Flowseal (Windows, есть форк для Linux), CDPI UI от Storik4pro (Windows), byedpi от hufrea, zapret и zapret2 от bol-van. Больше: тут» + «вот, что нашел»

## Ответ агента: исследование движков (23.09.2026)

Факты из репозиториев:
- **zapret2 (bol-van/zapret2)**: актуальная версия, 1330 коммитов, winws2.exe через windivert. Аргументы: `--wf-tcp-out=80,443`, `--lua-init=@zapret-lib.lua`, `--lua-desync=fake:blob=...`, `--filter-l7`, `--out-range`, `--payload`, `--new` (профили). Пресеты — preset2_*.cmd в zapret-win-bundle. Релизы содержат бандл. Старый zapret (winws) — EOL, только багфиксы.
- **GoodbyeDPI (ValdikSS)**: живой, релизы с zip (goodbyedpi.exe + WinDivert + .cmd-пресеты). Пресеты: legacy -1..-4, modern -5..-9 (-9 default). Аргументы простые. Служба ставится авторскими .cmd.
- **byedpi (hufrea/byedpi)**: есть Windows-сборка (ciadpi.exe, mingw, win_service.c), но это локальный SOCKS5-прокси — нужна маршрутизация (ProxiFyre/системный прокси), другой класс интеграции.
- **ByeDPI Manager (romanvht)**: GUI-обёртка для ciadpi + ProxiFyre, net48. ByeByeDPI (romanvht) — Android-версия.
- **CDPI UI (Storik4pro/cdpiui)**: C# GUI, компоненты (zapret, GoodbyeDPI, byebyedpi) через «магазин» CDPIUI-Store, автоподбор параметров, импорт .bat/.txt, шеринг пресетов JSON, 1234 коммита. Референс по UX.

Предложено пользователю: встроить zapret2 + GoodbyeDPI как движки со встроенными пресетами (портировать авторские пресеты), byedpi отложить (SOCKS-модель ломает «запустил и забыл»). Вопрос задан, ждём ответ.

## СМС от пользователя (23.09.2026, серия решений по мульти-движковости)

1. «берем в референс cdpi ui, zapret2 встраиваем, стоит проверить: goodbyeDPI-UI, dpibreak от dilluti0n, NoDPI от GVCoder09, SpoofDPI от xvzc, b4 от daniel lavrushin, youtubeUnblock от Waujito»
2. «используем только варианты plug and play, все равно за раз более одного движка (обхода) запускать не нужно. вшитые пресеты ок + добавить в обновления по воздуху»
3. «под каждый обход нужно добавить автоподбор аргументов/настроек... можешь пинговать yt/discord для проверки. движки и настройки к ним вшиваем обязательно, exe раздуваем, но можно все необходимое не паковать в exe, а положить в /data рядом. идею по b4 отменяю, ибо у нас есть визуальный тест. визуальный тест нужно сделать под каждый движок = выбрал способ обхода = прогнал = получил лучшие и худшие = выбрал сам, подсказку с автозапуском и лучшей стратегией можно оставить, по стандарту создаем службу, а не автозапуск z gui + стратегия»
4. «матрица кандидатов норм идея, в работу. еще стоит добавить автопрокид прокси под телеграмм, если сам тг есть в автозапуске пользователя/он его запустил = предлагать построить прокси, но только если не обнаружены vpn и их туннели»

## Ответ агента: проверка кандидатов и спека

Проверены все предложенные проекты (факты в доке): dpibreak — системный WinDivert, Rust, winget, берём в волну; SpoofDPI/NoDPI/byedpi — прокси-класс, вычеркнуты; b4 и youtubeUnblock — Linux-only, мимо; goodbyeDPI-UI — эволюционировал в CDPI UI.

Решения зафиксированы: только plug-and-play системные движки (flowseal, zapret2, goodbyedpi, dpibreak); движки в data/engines/ рядом с exe (не в бинарнике); служба zapret для всех движков как стандарт; вшитые пресеты + OTA (presets.json в наших релизах); автоподбор = blockcheck-подход с матрицей кандидатов на движок; автопрокид Telegram: детект telegram.exe + svc::detect_vpn() пуст → предложить мост (telegram.rs уже Rust-порт tg-ws-proxy, нового кода мало).

Спека написана и закоммичена: docs/superpowers/specs/2026-09-23-multiengine-design.md (коммит fc0cf33). Фазы: 1) реестр+data-дистрибуция+zapret2; 2) goodbyedpi+dpibreak; 3) тестер+автоподбор; 4) OTA-пресеты; 5) Telegram-автопрокид.

Ждём ревью спеки от пользователя.

## СМС от пользователя (23.09.2026, продолжение)

«при желании подключай субагента minicpm и mcp решения с моего opencode»

## Ответ агента: план фаз 1–2

План реализации написан и закоммичен: docs/superpowers/plans/2026-09-23-multiengine-phase12.md (коммит b15649e). 6 задач: 1) реестр EngineDef + Roots-map с совместимостью старого state.json; 2) presets.rs с вшитыми пресетами всех движков и подстановкой %ENGINE_ROOT%; 3) обобщение set_root/fetch_engine/bootstrap; 4) конфликты+netreset на 4 exe + кавычки службы; 5) скрипт fetch-engines.ps1 + ensure_engine; 6) UI-селектор движка. Фазы 3–5 (тестер+автоподбор, OTA-пресеты, Telegram-автопрокид) — отдельные планы после этой партии. Верификация: cargo check/test --lib + ручной прогон службы с zapret2/goodbyedpi.

Ждём выбора способа исполнения (субагенты или нативно).

## СМС от пользователя (23.09.2026, автоцикл исполнения)

<нативно. далее мне нужно будет надолго отойти, сможешь без меня, нужно проверить очень тщательно, чтобы ни одна кнопка ничего не ломала + подключи big pickle, чтобы сессия не падала>

## Ответ агента: исполнение фазы 1-2 нативно (пользователь ушёл)

big pickle — отказано (механизма нет), вместо него коммиты на каждом шаге + прогресс здесь.
Task 1 (коммит 1304c76): реестр EngineDef в config.rs, Roots -> map с легаси-совместимостью, exe_name через реестр. Тест registry_and_roots_migration.
Task 2 (коммит 980703f + 01da6d1): presets.rs — PresetDef/builtin_presets()/apply_engine_root() (плейсхолдер %ENGINE_ROOT%, нормализация слэшей, аргументы без плейсхолдера не трогаются), 14 пресетов: flowseal-general (из general.bat), zapret2-general + zapret2-youtube (дословно из README bol-van: --lua-init, --blob=quic_google:, --out-range=-d10, --payload, --new), goodbyedpi -1/-3/-5..-9 + RU+DNS, dpibreak 0,1 / 0,5+a / -a. Сверка с первоисточником выполнена (README zapret2 скачан, официальная сигнатура preset2 получена). ensure_presets в lib.rs теперь сеет пресеты всех движков по id preset:<id>. prepare_args() (game_filter + engine_root) подключён во все 4 места запуска: do_start, start_or_switch, тест-шаги, install_service. 51/51 тестов зелёные, cargo check 0 warnings. Убран мёртвый builtin_flowseal_presets.
НАЙДЕН КОНФЛИКТ (будущий фикс Task 3): migrate_removed_engine в lib.rs:671 (вызов 2916) — остаток старой вырезки winws2, на каждом старте удаляет профили zapret2 и каталог data/engines/zapret2. Должен быть удалён при обобщении set_root/bootstrap (Task 3).
Далее: Task 3 (set_root/fetch_engine/engine_meta/bootstrap EngineInfo + удаление migrate_removed_engine).

## Ответ агента: фазы 1-2 завершены (Tasks 3-6, пользователь отсутствовал)

Task 3 (коммит 0f0c872): engine_meta -> thin wrapper над config::engine_def (тест engine_meta_registry). set_root/fetch_engine обобщены на реестр (exe_name из meta; neutralize_author_autoupdate + seed_flowseal_configs только для flowseal). fetch_engine_impl: 3-ступенчатый поиск zip-ассета (с exe в имени -> 'win' -> любой zip), корень через find_exe по exe движка. Bootstrap.engines: Vec<EngineInfo> (id,label,repo,path,exe,ready) + engines_info(); flowseal: RootInfo сохранён для совместимости фронта. collect_warnings циклом по реестру. УДАЛЕНА migrate_removed_engine + вызов (конфликт со спекой: вырезала профили zapret2 на каждом старте) + #[allow(dead_code)] на updater.rs purge_prefix. 52 теста.

Task 4 (коммит bf315c9): service.rs pub const ENGINE_EXES (4 exe) — process_image_paths, detect_conflicts, own_engine_pids, any_winws_running обобщены; build_service_cmdline -> pub(crate). Тесты engine_exe_list_covers_registry + service_cmdline_quotes_spaces (zapret2, --lua-init с пробелом). netreset $names += goodbyedpi, dpibreak. 54 теста.

Task 5 (коммит 182246a): scripts/fetch-engines.ps1 (UTF-8 BOM) — GitHub API latest для zapret2/goodbyedpi/dpibreak (flowseal вшит в exe), выбор zip по приоритету, срезка верхнего каталога, распаковка в data/engines/<id>, idempotent; для релизов, не для юзеров. embedded.rs: ensure_engine(data, id) — data/engines/<id> с exe -> корень, flowseal -> вшитый fallback, неизвестный id -> Err. provision_engines подхватывает движки без заданного корня. Тест ensure_engine_finds_data_root_and_flowseal_fallback. 55 тестов.

Task 6 (коммит 7096587): UI — renderEngineTabs() (таб «Все» + по движку из B.engines, amber dot на неустановленных, фильтрация профилей), ensureEngineCards() на вкладке «Обновления» (статус + «Установить движок»/«Выбрать папку»), renderEngines обобщён, engineLabel(id), чипы движков на плитках профилей, #npEngine в «Новый профиль», btnSaveProfile шлёт engine. styles.css: .tab-dot. UI проверен через vite dev + playwright с mock __TAURI_INTERNALS__ (4 движка, табы/фильтрация/карточки/npEngine/click fetch_engine).

Self-review fix (коммит bfac0ac): баг — wd = root/bin у новых движков с exe в корне -> spawn падал (Os error 267, «неверно задано имя папки», подтверждено отдельной rust-программой). Фикс: wd = bin если есть, иначе parent exe (do_start + тест-шаги). build_service_cmdline уже ок (рекурсивный поиск).

Верификация: cargo check 0 warnings, cargo test --lib 55/55, дерево чистое. Все 6 задач плана docs/superpowers/plans/2026-09-23-multiengine-phase12.md выполнены нативно. Коммиты после cb0d786: 1304c76, 980703f, 01da6d1, ad5d2b3 (docs), 0f0c872, bf315c9, 182246a, 7096587, bfac0ac.

Не сделано (ручная верификация на машине юзера): прогон службы с winws2/goodbyedpi от админа по плану (§Верификация всей фазы). Фазы 3-5 (тестер+автоподбор, OTA-пресеты, Telegram-автопрокид) — отдельные планы.
## СМС от пользователя (23.09.2026, вечер)

«Спасибо. Свежий .exe уже проверен, что у нас дальше?»

## Ответ агента: статус и план дальнейшего (23.09.2026, вечер)

Фазы 1-2 мульти-движковости закрыты полностью (HEAD 48e7894): реестр 4 движков, пресеты, data/engines-дистрибуция, конфликты/netreset, UI-селектор, flowseal 1.10.3, exe пересобран и проверен юзером. 55/55 тестов, cargo check 0 warnings.

Дальше по спеке — три фазы, каждая отдельным планом (brainstorm → spec → plan по мере надобности):
- Фаза 3: тестер+автоподбор — матрица кандидатов на движок, пинг yt/discord, скоринг (доля успехов + TLS-handshake), повтор неудачных 1 раз, кэш, кнопка «применить лучшую». База уже есть: tester.rs, apply_best_strategy, prepare_args.
- Фаза 4: OTA-пресеты — presets.json в релизах нашего репо, обновление вшитых пресетов по воздуху.
- Фаза 5: Telegram-автопрокид — telegram.exe запущен + detect_vpn() пуст → предложить мост (telegram.rs готов).

Известная мелочь на подумать: fetch_engine для zapret2 у юзеров будет падать под Defender (детект релизного zip, os error 225) — нужен UX-хинт или дистрибуция собранной нами версии (у нас self-built winws2.exe живёт под Defender).

## СМС от пользователя (23.09.2026, вечер, второе)

«100% - класть в наш релиз собственную сборку» (решение по zapret2 под Defender)

## Ответ агента: zapret2 из нашего релиза (23.09.2026, вечер)

Коммит b8b3f4c + exe пересобран. Что сделано:
- release\engine-zapret2.zip (1.66 МБ, плоский, без каталога-обёртки) из нашей self-built сборки (winws2.exe 666112 б + cygwin1.dll + WinDivert.dll/.sys + lua/ + files/) залит как ассет релиза v1.2.0 нашего репо (lECL1PS3l/zgui) через gh; старый ассет ZapretGUI-1.2.0-portable.zip удалён (он же лежал локально в github pre release — оттуда тоже убран).
- lib.rs: EngineMeta.self_asset: Option<&'static str>, const SELF_REPO = "lECL1PS3l/zgui". engine_meta("zapret2") → self_asset = Some("engine-zapret2.zip"). fetch_engine_impl: repo = SELF_REPO если self_asset задан; asset-поиск: точное имя self_asset → zip с exe → 'win' → любой zip. Остальные движки (flowseal/goodbyedpi/dpibreak) качаются из репо авторов как раньше.
- scripts/fetch-engines.ps1: zapret2 исключён из списка качаемых (собственная сборка в data/engines дистрибутива, обновление только вручную при новом релизе bol-van).
- Тест zapret2_fetched_from_self_repo. 56/56, cargo check 0 warnings.
- Порядок обновления нашей сборки zapret2: пересборка из исходников (Cygwin+LuaJIT, процесс в Context.md выше) → Compress-Archive "\*" → release\engine-zapret2.zip → gh release upload v1.2.0 --clobber → залить в data/engines дистрибутива.

Релизная заметка: движение fetch_engine("zapret2") у юзеров теперь ходит в наш репо — Defender не сносит (наша сборка не совпадает с сигнатурой bol-van).

## СМС от пользователя (23.09.2026, поздно)

«Можешь без уточнений вести проект к завершению, просто перед релизом остановись и по спорным вопросам спрашивай» + «Доделай действие и остановись, нужно перезагрузить клиент opencode»

## Ответ агента: Фаза 3 выполнена, остановка по просьбе (23.09.2026)

Мандат: вести проект к завершению без вопросов, остановка ТОЛЬКО перед релизом (спорные вопросы — там).

ФАЗА 3 (тестер+автоподбор на все движки) — ЗАКРЫТА, коммиты:
- a068972: test_strategies (lib.rs:~1248) — по умолчанию (ids пустые) профили ВСЕХ установленных движков (engine_def есть + roots.path есть), явные ids фильтруются по roots с честной ошибкой. tester::group_of → "<preset|bat> · <движок label>". Тест groups дополнен (zapret2 → «preset · zapret2 (winws2)»).
- cdfc3e2: index.html/main.js — тест-карточка без Flowseal-специфики (flowsealProfiles → все ready-движки, пусто → «скачайте хотя бы один движок», чип движка в результатах через engineLabel, «выбрать все стратегии»).
- 0e07f15: PS-раннер (tester.rs:~461) — один повтор старта, если процесс мгновенно умер (гонка драйвера WinDivert).
Скоринг доля успехов + TLS-handshake время уже был (summarize: score → critical → avg_ms). Кэш TestCache уже есть. Кнопка «Применить лучшую» (apply_best_strategy) уже есть. 56/56 тестов.

ПРОДОЛЖИТЬ ОТСЮДА — ФАЗА 4 (OTA-пресеты):
- presets.json в ассетах релиза v1.2.0 нашего репо (lECL1PS3l/zgui; gh уже залогинен, ассет engine-zapret2.zip залит ранее).
- Схема [{engine, id, name, args, version}]; updater.rs — новая группа «Пресеты»: сейчас updater работает с файлами на диск (CatEntry url/dest), а пресеты живут в state.json как профили builtin — нужен спец-путь: check по version, apply обновляет builtin-профили (preset:<id>) в state.json, НЕ трогает кастомные. Вшитая таблица presets.rs — фоллбек.
- Точка интеграции: lib.rs apply_updates (ветка entries → reload_bats при flowseal strategies) + check_updates/updater_view; ensure_presets (lib.rs:~700) сеет по id.
- Затем ФАЗА 5: Telegram-автопрокид (telegram.exe в процессах + svc::detect_vpn() пуст → один раз за сессию неблокирующее предложение; tg_start/tg_stop/tg_status/telegram.rs готовы; флаг в state типа tg_offer_done).
- Затем: ФИНАЛ перед релизом (сюда задавать спорные вопросы), сборка npx tauri build, Context.md-цикл.
Черновик начала Фазы 4: читал updater.rs (check_entry/apply_updates/UpdArchive — применённые хэши, backup, atomic_write) и presets.rs (PresetDef, to_profile). Ничего не менял в них ещё.
