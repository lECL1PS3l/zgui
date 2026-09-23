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
