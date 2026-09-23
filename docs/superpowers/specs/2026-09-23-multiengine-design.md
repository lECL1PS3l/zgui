# Спека: мульти-движковая архитектура Z GUI

Дата: 23.09.2026. Статус: черновик на ревью.

## Цель

Z GUI работает с одним движком (Flowseal winws на EOL-zapret1). Переводим программу
на мульти-движковую архитектуру: четыре plug-and-play движка системного перехвата,
единая служба Windows, автоподбор аргументов и OTA-обновления стратегий. Один
активный обход в любой момент времени (уже гарантировано OPS-мьютексом).

## Решения (зафиксировано с пользователем)

1. Только системные plug-and-play движки (WinDivert), без прокси-класса
   (byedpi/SpoofDPI/NoDPI — мимо).
2. Все движки и их настройки вшиты в дистрибутив: лежат в `data/engines/` рядом с
   exe, НЕ внутри бинарника. Portable и installer несут полный набор.
3. Вшитые пресеты стратегий + OTA-обновление пресетов и движков из GitHub Releases.
4. Стандартный путь запуска — служба Windows `zapret` для ВСЕХ движков. GUI — панель
   управления; обход не зависит от того, открыто ли окно.
5. Автоподбор аргументов (blockcheck-подход): матрица кандидатов на движок,
   визуальный прогон, лучший/худший результат, выбор пользователем.
6. Автопрокид Telegram: если процесс Telegram запущен и VPN/туннели не обнаружены —
   предложить поднять встроенный tg-ws-proxy мост.
7. Референс UX — CDPI UI (Storik4pro).

## Движки первой волны

| id | репо | exe | примечание |
|----|------|-----|-----------|
| flowseal | Flowseal/zapret-discord-youtube | winws.exe | текущий, сохраняется |
| zapret2 | bol-van/zapret2 (win-bundle) | winws2.exe | стратегии Lua, пресеты preset2_*.cmd |
| goodbyedpi | ValdikSS/GoodbyeDPI | goodbyedpi.exe | пресеты -1..-9, dnsredir-варианты |
| dpibreak | dilluti0n/dpibreak | dpibreak.exe | минимальный, -o/-a |

Все требуют админ-права, все системные. Прокси-движки (byedpi, SpoofDPI, NoDPI) не
встраиваем: ломают модель «запустил и забыл».

## Архитектура

### Реестр движков (config.rs)

```rust
pub struct EngineDef {
    pub id: &'static str,          // "flowseal" | "zapret2" | "goodbyedpi" | "dpibreak"
    pub label: &'static str,       // человекочитаемое имя для UI
    pub repo: &'static str,        // GitHub releases для fetch_engine
    pub exe: &'static str,         // "winws.exe" и т.д.
    pub bundled: bool,             // есть в data/engines дистрибутива
}
pub fn engines() -> &'static [EngineDef]; // единая таблица
```

Roots → `BTreeMap<String, String>`. Все места `if engine == ENGINE_FLOWSEAL`
(set_root, engine_meta, root_info, bootstrap, сборка журнала) ходят через реестр.
Profile.exe_name() → lookup в реестре. Обратная совместимость state.json: старое
поле `flowseal` переносится в map при загрузке.

### Дистрибуция движков

- Вшитые zip в exe (FLOWSEAL_ENGINE и т.п.) удаляются.
- Установщик/portable поставляет `data/engines/<id>/` с распакованным движком:
  для zapret2 — winws2.exe + `lua/` (zapret-lib.lua, zapret-antidpi.lua) + 
  `windivert.filter/` + `files/` (fake-блобы, списки).
- `ensure_engine(data, id)`: путь из Roots или data/engines/<id>; если нет exe и
  есть вшитая копия в дистрибутиве — она уже на месте; иначе подсказка «скачать».
- `fetch_engine` остаётся как OTA: GitHub Releases → tmp zip → распаковка → обновление
  Roots. Общее для всех движков, различие только в EngineDef.

### Пресеты

Вшитые пресеты хранятся как статическая таблица в Rust (id, engine, name, args, 
desс) — единственный источник правды для первого запуска. Примеры:
- flowseal: существующий импорт .bat (сохраняется) + general как пресет по умолчанию;
- zapret2: портированные preset2 из win-bundle (fake+multisplit/multidisorder, quic,
  youtube с hostlist);
- goodbyedpi: -9 (default), -5..-8, 1_russia_dnsredir (с --dns-addr),
  -1..-4 legacy;
- dpibreak: default `-o 0,1`, `-o 0,5 -a`, `-a` (fake autottl).

Плейсхолдеры %GameFilterTCP%/UDP% подставляются из settings.game_filter как сейчас.

### OTA-пресеты

Ассет `presets.json` в релизах нашего репо: `[{engine, id, name, args, version}]`.
В updater — новая группа «Пресеты»: сравнение по version/хэшу, применение
перезаписывает только builtin-пресеты (кастомные не трогает). Вшитая копия — фоллбек.
Обновление пресетов = свежие тактики обхода без релиза программы.

### Служба для всех движков

`svc::install_service` строит командную строку из профиля через реестр:
`"<root>\bin\winws2.exe" --lua-init=... --lua-desync=...`. Один сервис `zapret`,
при смене стратегии/движка — пересоздание (как сейчас). do_start (процессный путь)
остаётся фоллбеком, если служба недоступна/отклонена. Специфика:
- goodbyedpi: аргументы вида `-9 --blacklist <path>` (короткие флаги — единый
  Vec<String>, ничего не меняется);
- zapret2: пути lua-файлов и fake-блобов в аргументах должны указывать в 
  data/engines/zapret2 — пресеты пишем уже с абсолютным префиксом корня движка
  (подстановка `%ENGINE_ROOT%` при применении пресета).

### Конфликты

Списки процессов расширяем: `winws.exe, winws2.exe, goodbyedpi.exe, dpibreak.exe`.
own_engine_pids сравнивает путь процесса с любым из data/engines/<id>. Чужой
goodbyedpi/dpibreak — предупреждение + «остановить», как сейчас с winws.

### Тестер + автоподбор

Обобщение tester.rs: интерфейс кандидата = (engine_id, args). Матрица кандидатов:

| движок | варьируем |
|--------|----------|
| flowseal | существующий набор стратегий |
| zapret2 | fake / multisplit / multidisorder / fakedsplit × fooling (md5sig, badseq) × autottl |
| goodbyedpi | -1..-9, --set-ttl 5..8, --wrong-chksum, --wrong-seq |
| dpibreak | -o 0,1 / 0,5 / 5,0, ± -a, --fake-ttl 4..8 |

Алгоритм (детерминированный):
1. baseline: проверка сети без обхода (уже есть: read_baseline).
2. для каждого кандидата по порядку: поднять (служба/процесс на 1 прогон), пауза
   3-5 с, health-check = HTTP(S) GET к ytimg/gstatic/discord-cdn (2-3 повтора,
   таймаут 10 с), скоринг = доля успехов + среднее время TLS-handshake; сброс.
3. неудачный кандидат — один повтор, затем пропуск.
4. результат: сортированный список лучший→худший, зелёный/красный, кнопка
   «применить лучшую», подсказка автозапуска с лучшей стратегией.
5. кэш в state.json (tester cache уже есть).

Визуальный тест на каждый движок: выбрал способ обхода → прогнал → увидел лучшие
и худшие → выбрал сам.

### Автопрокид Telegram

Триггер: процесс telegram.exe / Telegram Desktop запущен (список имён процессов,
проверка при bootstrap и периодически watchdog'ом) И svc::detect_vpn() пуст.
Тогда один раз за сессию (флаг в state) — неблокирующее предложение в UI:
«Запущен Telegram, VPN не найден — построить прокси-мост для Telegram?».
Да → существующий telegram::TgState start (tg-ws-proxy уже вшит). Нет → скрыть,
больше не спрашивать до перезапуска GUI. tg_toggle в ручном режиме остаётся.

## Фазы реализации

1. Реестр движков + map Roots + /data-дистрибуция + zapret2 (движок, пресеты,
   служба, конфликты).
2. GoodbyeDPI + dpibreak по тому же шаблону.
3. Тестер + автоподбор на все движки (матрица кандидатов).
4. OTA-пресеты (presets.json в релизах + группа в updater).
5. Автопрокид Telegram.

Каждая фаза: cargo check + cargo test --lib зелёные.

## Нерешённое / риски

- zapret2-бандл большой (десятки МБ с fake-блобами) — дистрибутив распухнет;
  принято сознательно (пользователь: «exe раздуваем», но кладём в data).
- Поведение ветки «GUI не под админом» для новых движков: тот же launcher-путь
  (spawn_and_wait_pid), тест на винде обязателен.
- Тест матрицы кандидатов медленный (десятки кандидатов × ~5 с) — прогресс-бар уже
  есть (zgui:prog), плюс ограничение времени на прогон.
