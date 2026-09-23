# WinForms Port of Zapret GUI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Полный перенос GUI Zapret (v1.2.0) с Tauri 2 + WebView2 на C# WinForms (.NET Framework 4.8): один exe, ноль установленных компонентов, без WebView2.

**Architecture:** Вся логика (запуск winws, служба, DNS, тест, обновления, TG-мост, watchdog, журнал) портируется построчно из `src-tauri/src/*.rs` в проект WinForms без изменений поведения. Rust-крейт `tg-ws-proxy-rs` не переписывается: компилируется в отдельный exe, шитый в ресурсы .NET-приложения и запускаемый как процесс (текущая модель). UI фронтенда (`src/index.html` + `src/main.js`, ~2100 строк) воспроизводится на WinForms: 8 вкладок + тёмная тема, логика отображения копируется из JS.

**Tech Stack:** C# / WinForms / .NET Framework 4.8 (есть в каждой Win10/11, ставить ничего не нужно). Сборка: `dotnet build` (Visual Studio Build Tools + .NET Framework 4.8 targeting pack). Тесты: xUnit (net48). Упаковка в один exe: ILRepack (ILMerge-форк). Встроенные ресурсы: `resources/flowseal-main.zip`, `resources/engine-flowseal.zip`, `tg-ws-proxy.exe`. Внешние вызовы: процесс run-утилиты, `sc.exe`, PowerShell (`New-Service`, `Checkpoint-Computer`, netsh), schtasks — через скрытые процессы, как в Rust.

**Spec:** Существующая Tauri-версия — эталон поведения (исходники `src-tauri/src/*.rs`, `src/main.js`, `src/index.html`). План аргументируется из исходников; исполняющий читает оба.

## Global Constraints

- .NET Framework 4.8, WinForms; target platforms: Windows 10 (21H2+) и Windows 11. Без external runtime-зависимостей.
- Один итоговый exe `ZapretGui.exe`: логика + ресурсы (2 zip + tg-ws-proxy.exe) шиты внутрь, на диск распаковывается только data-папка рядом с exe.
- Поведение = Tauri-версия 1:1. Порядок блокировок, тексты сообщений, ключи реестра, имена tasks/scheduled (ZapretGUI, azapret), имя службы `zapret`, имя движка `flowseal`, `winws.exe` — без изменений.
- Русский текст в UI, журнале и предупреждениях — дословно из Rust/JS (строки в исходниках). Кодировка .cs — UTF-8 (без BOM), .ps1-скрипты — UTF-8 с BOM (PowerShell 5.1 без BOM ломает кириллицу).
- Вся связь с планировщиком/службой/сетью идёт теми же механизмами, что в Rust: скрытый запуск (`CREATE_NO_WINDOW`), PowerShell-скрипты из временной папки `data\logs`, `sc.exe query`, `reg add` (стратегия службы в `HKLM\System\CurrentControlSet\Services\zapret\zgui-strategy`).
- Не отклонять P/Invoke там, где Rust использовал WinAPI (ShellExecuteW RunAs, CurrentThread.UICulture). HttpClient вместо reqwest, ZipFile вместо zip-крейта.
- Тесты xUnit только для чистой логики (владелец обхода, парсинг, человеческие тексты, каталог, порты). Процессные и сетевые пути — acceptance-проверка вручную по чек-листу.
- ПОРЯДОК ВАЖЕН: владелец winws считался в Rust до взятия лока `state` (иначе дедлок) — `current_owner` (lib.rs:221-240). В C# — то же: вызывать вне lock.
- Права администратора: UAC-элевация обязательна для службы/DNS/netsh/планировщика; приложение работает и без прав (тогда ряд функций помечен). `always_admin` → автоперезапуск RunAs при старте.

## Review Focus

- Владелец обхода (app/service/test/external/none) должен считаться верно на переполненных состояниях: app запущен И служба установлена И чужой winws жив → приоритет app; тест активен → test; только чужой winws без наших PID → external. Тест: `wiwsOwnerOf` таблица (порт из lib.rs:186-207).
- Профили: входной .bat с тёткими/кривыми кавычками, кириллицей и `#`-комментариями разбирается без потерь; `--port` не обязателен; дефолты fill из human-таблиц. Группы («побочки», «игры») из каталога — при первом запуске.
- Конфликты: VPN/прокси системы и чужой winws детектятся до старта; auto-kill подтверждается; пометка «внешний winws» снимается, когда PID умирает.
- DNS-benchmark: таймаут > 3с на провайдер не вешает benchmark; порядок ответов и (TCP, UDP) логируется; ping-кэш сохраняется между перезапусками.
- Обновления: частично скачанный zip, битый hash и отсутствие записи в whitelist не ломают `apply_updates`; смена ipset_mode на «none» не трогает `ipset-all.txt` (условие updater.rs:220).
- Критично важен первый bootstrap на чистой машине: при отсутствии движка — самораспаковка flowseal-main + engine, seed каталога и конфигов, дефолтные профили, без единого исключения.

---
## File Structure

Портируемый проект — в подпапке `winforms/` корня репозитория (исходники Tauri остаются нетронутыми как эталон):

```
winforms/
  ZapretGui.sln
  Directory.Build.props          # net48, WarningLevel, LangVersion 7.3
  build-release.ps1              # dotnet build + ILRepack + подпись utils
  src/
    ZapretGui.Core/              # библиотека: ВСЯ логика, без UI (тестируется)
      AppState.cs                # Global: Locks, State, Busy, hooks (port lib.rs:14-141, 2600-2868)
      Bootstrap.cs               # сервис сборки Bootstrap-снимка (lib.rs:143-269)
      Owner.cs                   # WinwsOwner + winws_owner_of + owner_name (lib.rs:168-217)
      Events.cs                  # Emit<T>-мост Core→UI (замена tauri emit)
      Config/
        Settings.cs              # Settings + дефолты (config.rs:29-81)
        Profile.cs               # Profile + exe_name (config.rs:83-99)
        UpdEntry.cs              # UpdEntry (config.rs:111-120)
        StateStore.cs            # атомарная запись/чтение литерала конфигурата (config.rs:121-263)
        PortableData.cs          # portable_data_dir, sha256, file_sha256 (config.rs:270-294)
        BatImport.cs             # парсинг .bat профиля, сохранение, дефолты (profiles.rs)
        Roots.cs                 # Roots + path/set (config.rs:9-27)
      Runtime/
        Processes.cs             # hidden spawn, pid_alive, kill (runner.rs)
        Uac.cs                   # is_elevated, relaunch_as_admin (RunAs), PS_HEADER, write_ps1
        Service.cs               # install/remove/state/strategy (service.rs:384-460)
        Autostart.cs             # apply_boot_task/remove (runner.rs:208), legacy azapret (netreset? нет) 
        Conflicts.cs             # conflict_check, kill_conflicts, vpn_check (service.rs/conflicts)
      Net/
        Dns.cs                   # dns_providers таблица, apply_dns (DoH reg), reset_dns (dns.rs)
        DnsBench.cs              # TCP/UDP проба, кэш, human text (dns.rs benchmark)
        GameFilter.cs            # game_filter_ports (port/repl из profiles.rs)
        NetReset.cs              # net_reset, restore point, virtual_adapters (netreset.rs)
      Tele/
        TgBridge.cs              # извлечение tg-ws-proxy.exe, spawn/stop с портом (telegram.rs)
      Tester/
        Tester.cs                # runner-генератор, progress, cache, cancel, apply_best (tester.rs + lib.rs:1126-1200)
      Updater/
        Updater.cs               # check_all, apply_updates, sync_ipset, neutralizацию (updater.rs:39-360)
      WatchdogWatchdog.cs        # https-проба, status (watchdog.rs)
      Log/
        LogRing.cs               # in-memory кольцо + файл, entries/read/clear (logger.rs)
        Humanize.cs              # humanError/humanize-таблицы + reusable human_readable (human.rs)
      Embedded/
        Embedded.cs              # include-эквивалент: ресурсные zip, ENGINE_VERSION 1.10.2, SNAPSHOT_INFO, ENGINE_INFO, seed (embedded.rs:1-249)
      Util/
        Text.cs                  # decode utf8/bom/16LE, tail_file, trim-BOM (config.rs:300-423)
        Httpx.cs                 # HttpClient: UA "zgui/0.1 (zapret-gui updater)", timeout 45s, redirects
        Json.cs                  # compact serialize с camelCase (свой сериализатор полей вручную)
    ZapretGui.App/               # exe: WinForms UI (порт index.html + main.js)
      Program.cs                 # args: --boot, --elevate-echo; автоперезапуск admin при always_admin
      MainForm.cs                # NavSidebar (9 иконок) + панель контента, bootstrap-поллинг 4с
      Theme.cs                   # grey/dark/light: цвета из index.html CSS переменных
      Pages/                     # ProfilePage, TestPage, DnsPage, SettingsPage, UpdatesPage, LogPage, TelegramPage, Conflicts overlay
      Controls/                  # Chip, BtnBusy, Toast, Confirm modal, EngineCard
    ZapretGui.Tests/             # xUnit (net48): чистая логика
```

## Порядок задач

1. Скелет решения + CI-сборка + итоговый ILRepack-пайплайн.
2. Порт pure-данных: Settings, UpdEntry, Roots, portable_data_dir.
3. Парсинг .bat и сохранение профилей (BatImport) + тесты.
4. Каталог/дефолты профилей (profiles.rs) + human-тексты (human.rs) + тесты.
5. Владелец обхода (Owner) + тесты таблицы.
6. Процессы/элевация/журнал (Processes, Uac, LogRing) + smoke-тесты.
7. Embedded: распаковка движка, seed каталога, neutralize_author_autoupdate + тесты extraction.
8. Запуск/остановка профиля, статус, файрвол хороших портов (start/stop, status из lib.rs:902-1050).
9. Служба: install/remove/state/strategy + autostart (Service, Autostart) — accept вручную.
10. Конфликты и VPN-детект (Conflicts).
11. DNS: провайдеры, apply/reset, benchmark, кэш + тесты benchmark-lib.
12. Обновления: check/apply/sync_ipset/архив + тесты whitelist-условий.
13. Telegram-мост: bin-target в crates/tg-ws-proxy-rs, сборка exe, встраивание, TgBridge + ручной accept.
14. Тестер стратегий: runner, progress, cache, apply_best + тесты.
15. Watchdog: HTTPS-проба, авто-recovery передач + статус.
16. UI-каркас: MainForm, темы, навигация, bootstrap-поллинг.
17. Страница Профили + запуск/останов + конфликт-оверлей.
18. Страница Тест + прогресс-бар + результаты + кэш.
19. Страница DNS + benchmark-кнопка.
20. Страница Обновления + проверка/применение.
21. Страница Настройки (автозапуск/служба) + onboards/предложения.
22. Страница Telegram + связь с мостом.
23. Страница Журнал + детали-подвал + экспорт отчёта.
24. Финальный однопроходный прогон на чистой машине: чек-лист Tauri vs WinForms.

---

### Task 1: Скелет решения и single-exe пайплайн

**Files:**
- Create: `winforms/ZapretGui.sln`
- Create: `winforms/Directory.Build.props`
- Create: `winforms/src/ZapretGui.Core/ZapretGui.Core.csproj`
- Create: `winforms/src/ZapretGui.App/ZapretGui.App.csproj`
- Create: `winforms/src/ZapretGui.Tests/ZapretGui.Tests.csproj`
- Create: `winforms/build-release.ps1`
- Create: `winforms/src/ZapretGui.App/Program.cs`
- Create: `winforms/src/ZapretGui.Core/Util/Json.cs`
- Create: `winforms/src/ZapretGui.Core/Util/Text.cs`

**Interfaces:**
- Consumes: nichts (первая задача).
- Produces: рабочий каркас: Core-библиотека, App-exe (пустое окно), Tests (один xUnit-тест), `build-release.ps1` выдаёт один `winforms/artifacts/ZapretGui.exe` (ILRepack: Core внутрь App, ресурсы шиты).

- [ ] **Step 1: Создай решение и проекты**

csproj Core: `<TargetFrameworkVersion>v4.8</TargetFrameworkVersion>`, `<AssemblyName>ZapretGui.Core</AssemblyName>`. App: exe, reference Core. Tests: xunit + dotnet-test-xunit, reference Core. Сложи в `winforms/` и убедись `dotnet build` зелёный.

- [ ] **Step 2: Program.cs — точка входа и аргументы**

Портировать семантику запуска из lib.rs:2862-2868 (`--boot` → boot_pending), и автоперезапуск admin если settings.always_admin (lib.rs:2755-2770) ещё не реализован (вызовешь в Task 6): пока просто пустое окно `new MainForm()`.

- [ ] **Step 3: Json.cs и Text.cs (базовые утилиты)**

Портировать:
- `Text.cs`: `DecodeText(byte[])->string` (BOM UTF-8/UTF-16LE распознать, иначе UTF-8 lossy — порт config.rs:300-423, прочитай точный порядок), `TailFile(path, n)`, `TrimBom`.
- `Json.cs`: компактный сериализатор DTO с camelCase-именами полей и кодировкой строк (без кириллических \u-эскейпов, как serde). Реализуй на ручных StringBuilder-циклах по полям, не строй Dictionary.

- [ ] **Step 4: Failing test для Json.cs**

`ZapretGui.Tests/JsonTests.cs`:

```csharp
[Fact]
public void SettingsJson_UsesCamelCase_and_ObeysDefaults()
{
    var s = Settings.Default();
    var j = Json.Serialize(s);
    Assert.Contains("\"updateIntervalHours\":72", j);
    Assert.Contains("\"autostartMode\":\"none\"", j);
    Assert.DoesNotContain("\\u0", j); // кириллица не эскейпится
}
```

(нужен `Settings.Default()` из Task 2 — собери Task 1 и Task 2 порядком: Json/Text собраны как empty-классы, тест падает до появления Settings; реализуй после Task 2 и закрой задачью 2. Если неудобно — отложи этот тест в Task 2.)

- [ ] **Step 5: build-release.ps1 — один exe через ILRepack**

Скрипт: `dotnet build src/ZapretGui.App -c Release`, затем ILRepack: `ilrepack /out:artifacts/ZapretGui.exe src/ZapretGui.App/bin/Release/ZapretGui.App.exe src/ZapretGui.Core/bin/Release/ZapretGui.Core.dll /target:win /wildcards`. ILRepack ищется в `%USERPROFILE%\.nuget\packages\ilrepack\...` — скрипт находит bin по версии. Output: один exe. Ресурсы (zip, tg-proxy) подключишь в Task 7/13 как EmbeddedResource — в exe они уедут автоматически.

- [ ] **Step 6: Убедись, что сборка зелёная и exe запускается**

`dotnet build` без ошибок; `winforms/artifacts/ZapretGui.exe` существует; окно открывается без исключений.

- [ ] **Step 7: Commit**

```bash
git add winforms
git commit -m "winforms: scaffold solution, single-exe ILRepack pipeline"
```

---

### Task 2: Порт данных и литеры конфигурации

**Files:**
- Create: `winforms/src/ZapretGui.Core/Config/Settings.cs`
- Create: `winforms/src/ZapretGui.Core/Config/UpdEntry.cs`
- Create: `winforms/src/ZapretGui.Core/Config/Roots.cs`
- Create: `winforms/src/ZapretGui.Core/Config/PortableData.cs`
- Create: `winforms/src/ZapretGui.Core/Config/StateStore.cs`

**Interfaces:**
- Consumes: `Json.cs`, `Text.cs` (Task 1).
- Produces: `Settings` (поля config.rs:30-54 — camelCase при сериализации, Default со значениями из config.rs:64-81), `UpdEntry` (config.rs:113-120), `Roots` (config.rs:9-27, методы Path/Set), `PortableData.PortableDataDir()` (config.rs:270-284, `Directory.CreateDirectory` с текстом ошибки `"не удаётся создать portable-папку {path}: {err}. Поместите zgui.exe в доступную для записи папку."`), `PortableData.Sha256Hex`, `PortableData.FileSha256`, `StateStore` (прочитай config.rs:121-263: где хранится литерал, атомарная запись через tmp+rename, авто-миграция interval 6→72 и темы; верни те же пути и тот же формат файла). StateStore.Load отдаёт `State` (Settings + Rules + profiles; типы полей State возьми из config.rs:144-263).

- [ ] **Step 1: Failing test — дефолты, camelCase и BOM**

Добавь в `JsonTests.cs`:

```csharp
[Fact]
public void DefaultSettings_HaveProdDefaults()
{
    var s = Settings.Default();
    Assert.Equal(72, s.UpdateIntervalHours);
    Assert.Equal("none", s.AutostartMode);
    Assert.False(s.AlwaysAdmin);
    Assert.Equal(1443, s.TgPort);
    Assert.Equal("grey", s.Theme);
}
```

- [ ] **Step 2: See it fail**

`dotnet test winforms/src/ZapretGui.Tests` — красный (Settings ещё нет).

- [ ] **Step 3: Реализация**

Все классы из списка выше по контрактам. `Settings` — POCO; camelCase добивается в `Json.cs` маппингом имён (объяви `[JsonField("updateIntervalHours")]` атрибуты). Значения по умолчанию из config.rs:64-81 дословно.

- [ ] **Step 4: Pass tests**: `dotnet test` зелёный (JsonTests + DefaultSettings).

- [ ] **Step 5: Тест StateStore — миграция 6→72**

Прочитав config.rs:200-215: напиши тест: файл со `updateIntervalHours:6` без `intervalMigrated` → Load вернёт 72 и пропишет `intervalMigrated=true` обратно в файл.

- [ ] **Step 6: Commit**

```bash
git add winforms/src/ZapretGui.Core
git commit -m "winforms: port Settings/UpdEntry/Roots/StateStore/config state"
```

---

### Task 3: Парсинг .bat профилей (BatImport)

**Files:**
- Create: `winforms/src/ZapretGui.Core/Config/BatImport.cs`

**Interfaces:**
- Consumes: `Profile`, `Settings`, `PortableData`, `Humanize` (Task 4 — для человеческих текстов; пока заглушки).
- Produces: `BatImport.ParseArgsFromBat(Profile)` — чтение `{id}.bat` из `root` по тем же правилам, что profiles.rs; `BatImport.Save(TemporaryProfile, ...)`; `BatImport.DefaultFill(...)` — подстановка дефолтов для позиционных частей при их отсутствии. Точные позиции (--filter-tcp, --dpi-desync, etc.) — выпиши из profiles.rs парсера: прочитай `parse_flowseal_bat` / `import_profile_from_bat` (ищи в `src-tauri/src/profiles.rs`). Сохранение `.bat` — через тот же writer, что в Rust (кавычки, экранирование `%`, переносы строк `\r\n`), и `# comment`-пил обращения.

- [ ] **Step 1: Failing test — кривой bat разбирается в профиль**

Собери тестовый bat с полными и без `--port`, с кириллическими значениями и комментарием; и проверь, что:
- `ParseArgsFromBat` вернул тот же набор параметров;
- значение `--port` отсутствует в args, и дефолт человеческий подставлен;
- комментарий отброшен, кавычки корректно сняты.

- [ ] **Step 2: See it fail.**

- [ ] **Step 3: Реализация парсера.** Портирование построчно. Если в Rust парсер — regex-based, перенеси те же regex с той же грамматикой кавычек. Энкодинг входного файла — через `Text.DecodeText`.

- [ ] **Step 4: Pass tests.**

- [ ] **Step 5: Тест round-trip сохранения**: Profile → Save → Parse → идентичный Profile. Группы эскейпа `%` и `"` покрыть.

- [ ] **Step 6: Commit**

```bash
git add winforms/src/ZapretGui.Core/Config/BatImport.cs winforms/src/ZapretGui.Tests
git commit -m "winforms: port bat profile parser"
```

---

### Task 4: Каталог профилей, группы, человеческие тексты

**Files:**
- Create: `winforms/src/ZapretGui.Core/Config/Profiles.cs`
- Create: `winforms/src/ZapretGui.Core/Log/Humanize.cs`

**Interfaces:**
- Consumes: `Settings`, `Profile`, `PortableData`, `Embedded` (Task 7 — имена каталога; пока заглушки), `BatImport`.
- Produces: `Profiles.List`, `Profiles.Groups` (группы из каталога официальных профилей — названия и приоритеты в `collect_entries`-подобном месте profiles.rs/catalog), `Profiles.TryDelete`, `Humanize.HumanError(string) -> string` (человеческий текст для известных кодов: "The system cannot find the file specified", "Access is denied", etc.) и `Humanize.Replace/system` тексты для `PathTooLong`, как в human.rs: прочитай exact list. Выходная строка должна быть кириллицей, дословной.

- [ ] **Step 1: Failing test — группа каталога при первом bootstrap**

Тест: после `SeedCatalog` (Task 7) группа «игры» содержит профили с `Engine=="flowseal"` и их порядок соответствует порядку в каталоге (не алфавитному). До Task 7 — тест на `Profiles.Groups` пуст без seed, но фильтры не падают.

- [ ] **Step 2: See it fail.**

- [ ] **Step 3: Реализация каталога.** Портирование: список профилей-каталога (id/название/группа/args) из официального flowseal-main — где он лежит и как читается, найдёшь в profiles.rs (вероятно, читает каталог из расшаренного ресурса или скачивает; уточнить по исходнику при реализации). Используй те же строки.

- [ ] **Step 4: Тест HumanError**: для каждого из 4-6 известных кодов вернётся кириллический текст (не равен входной строке), для неизвестного — текст содержит исходный код.

- [ ] **Step 5: Pass tests + Commit**

```bash
git add winforms/src/ZapretGui.Core/Config/Profiles.cs winforms/src/ZapretGui.Core/Log/Humanize.cs winforms/src/ZapretGui.Tests
git commit -m "winforms: port profile catalog, groups, human error texts"
```

---

### Task 5: Владелец обхода (Owner)

**Files:**
- Create: `winforms/src/ZapretGui.Core/Owner.cs`
- Create: `winforms/src/ZapretGui.Tests/OwnerTests.cs`

**Interfaces:**
- Consumes: `Settings`, `Profile`.
- Produces: `enum WinwsOwner { None, App, Service, Test, External }`; `Owner.WinwsOwnerOf(bool testing, string appProfile, bool? serviceRunning, string serviceStrategy, bool anyWinws, bool ownWinws) -> WinwsOwner` — порт lib.rs:186-207 ПОСТРОЧНО (приоритет: testing → app → service → external → none); `Owner.OwnerName(WinwsOwner) -> string` (lib.rs:209-217: "none", "app", "service", "test", "external"); `Owner.CurrentOwner(...)` — сборе параметров как lib.rs:221-240 (порядок: testing flag → runner_alive → runtime.Pid жив → svc.any_winws → own_engine_pids).

- [ ] **Step 1: Failing test — таблица приоритетов**

```csharp
[Theory]
[InlineData(true, null, null, null, false, false, "test")]
[InlineData(false, "profA", null, null, true, true, "app")]
[InlineData(false, null, true, "svcProf", true, true, "service")]
[InlineData(false, null, false, null, true, false, "none")]
[InlineData(false, null, false, null, true, true, "external")]
public void OwnerPriorityTable(bool tst, string app, bool? srv, string strat, bool any, bool own, string expected)
{
    Assert.Equal(expected, Owner.OwnerName(Owner.WinwsOwnerOf(tst, app, srv, strat, any, own)));
}
```

- [ ] **Step 2: See it fail.**

- [ ] **Step 3: Реализация** — поточечный порт lib.rs:186-217.

- [ ] **Step 4: Pass + Commit**

```bash
git add winforms/src/ZapretGui.Core/Owner.cs winforms/src/ZapretGui.Tests/OwnerTests.cs
git commit -m "winforms: port winws owner state machine"
```

---

### Task 6: Процессы, элевация, журнал

**Files:**
- Create: `winforms/src/ZapretGui.Core/Runtime/Processes.cs`
- Create: `winforms/src/ZapretGui.Core/Runtime/Uac.cs`
- Create: `winforms/src/ZapretGui.Core/Log/LogRing.cs`

**Interfaces:**
- Consumes: `PortableData`, `Settings` (для путей журнала).
- Produces: `Processes.SpawnHidden(string exe, string[] args, string workdir, out int pid)` — `ProcessStartInfo` с `CreateNoWindow=true`, `WindowStyle=Hidden`, `StandardOutput/Error` в файл рядом с журналом; `Processes.PidAlive(int) -> bool` (убитый процесс → false; для умершего — `try { p.Refresh(); return !p.HasExited }`); `Processes.RunOutputHidden(exe, args) -> (exitCode, stdout)`; `Processes.KillTree(int pid)`; `Processes.CurrentExeDir`; `Processes.FindExe(root, "winws.exe")` — порт config.rs:294-300 (ищи рекурсивно по папке `bin` среди подпапок, приоритет root/bin, отсекай систем-дифы). `Uac.IsElevated()` — P/Invoke `CheckTokenMembership` / `OpenProcessToken`, как windows-sys про это; `Uac.RelaunchAsAdmin()` — `ShellExecuteW("runas")` (RunnerPS: lib.rs 2740-2760 использует ShellExecuteW); `Uac.PsHeader` + `Uac.WritePs1` (порт runner.rs PS_HEADER и write_ps1: текст файла с BOM, кавычки). `LogRing` — кольцо последних N (N из logger.rs), методы `Entries(after)` (уникальные по seq, как lib.rs:2316), `Write(level, scope, msg)` с лог-файлом `data\logs\zgui.log` плюс rotate (max size из logger.rs), `Clear`, `Read()` — текст со временем.

- [ ] **Step 1: Failing test — PidAlive по умершему PID**

Старт `cmd.exe /c exit 0` (hidden), дождись выхода, сразу `Processes.PidAlive(pid)` должен быть `false`.

- [ ] **Step 2: See it fail.**

- [ ] **Step 3: Реализация** по контрактам. WritePs1 всегда пишет UTF-8 с BOM.

- [ ] **Step 4: Тест LogRing**: `Write` 3 записи → `Entries(0)` вернёт 3 в порядке; `Clear` обнуляет; файл на диске содержит записи.

- [ ] **Step 5: Pass + Commit**

```bash
git add winforms/src/ZapretGui.Core/Runtime winforms/src/ZapretGui.Core/Log/LogRing.cs winforms/src/ZapretGui.Tests
git commit -m "winforms: port hidden processes, UAC elevation, log ring"
```

---

### Task 7: Встроенные ресурсы движка

**Files:**
- Create: `winforms/src/ZapretGui.Core/Embedded/Embedded.cs`
- Modify: `winforms/src/ZapretGui.App/ZapretGui.App.csproj` (EmbeddedResource для `assets/flowseal-main.zip` и `assets/engine-flowseal.zip`)

**Interfaces:**
- Consumes: `PortableData`, `Text` (чтение), `Settings`.
- Produces: константы `Embedded.EngineVersion = "1.10.2"`, `Embedded.SnapshotInfo = "Official Flowseal main snapshot bundled at build time"`, `Embedded.EngineInfo = "Flowseal 1.10.2 release archive bundled in the executable"` (embedded.rs:9-12); `Embedded.EnsureEmbeddedEngine(dataDir) -> string?` — порт embedded.rs:16-33: достань `engine-flowseal.zip` из ресурсов, распакуй во `data\engines\flowseal` (имя папки сверь с кодом), верни root или null; если уже распаковано — не трогай. `Embedded.ExtractAll(bytes, dest)` — ZipFile: безопасный относительный путь (порт embedded.rs safe_relative:216-229 — отсечение `..`, полных путей), кол-во файлов. `Embedded.NeutralizeAuthorAutoupdate(root)` — порт embedded.rs:34-44 (правка флага автообновления в конфиге движка; точные строки). `Embedded.SeedCatalog(dataDir)` — порт embedded.rs:139-155 + `extract_missing`/`flowseal_destination`/`copy_missing`/`copy_tree_missing` (embedded.rs:156-257): распаковывай/копируй только отсутствующие файлы.

- [ ] **Step 1: Failing test — extraction с безопасным путём**

Zip-stream с `file.txt` и `../../evil.txt` → `ExtractAll` создал только `file.txt`; `Count == 1`.

- [ ] **Step 2: See it fail.**

- [ ] **Step 3: Реализация** по контрактам.

- [ ] **Step 4: Failing test — EnsureEmbeddedEngine распаковывает при отсутствии**

Скопируй `winforms/src/ZapretGui.App/assets/engine-flowseal.zip` (добавь в тестный проект как экземпляр файла) в temp `data`, очисти `engines` → `EnsureEmbeddedEngine` вернёт root с `winws.exe` внутри.

- [ ] **Step 5: Pass + Commit**

```bash
git add winforms/src/ZapretGui.Core/Embedded winforms/src/ZapretGui.App/ZapretGui.App.csproj winforms/src/ZapretGui.Tests
git commit -m "winforms: port embedded engine extraction and catalog seed"
```

---

### Task 8: Запуск/остановка профиля, статус, firewall-порты

**Files:**
- Create: `winforms/src/ZapretGui.Core/Runtime/ProfileRunner.cs`
- Create: `winforms/src/ZapretGui.Core/Net/GameFilter.cs`

**Interfaces:**
- Consumes: `Processes`, `Settings`, `Profile`, `PortableData`, `Owner`, `Embedded`.
- Produces: `ProfileRunner.Start(Profile, RootPath, Settings, DataDir) -> Result<RuntimeState,string>` — порт lib.rs:902-1010 (lib.rs start_profile): собери `winws.exe` полный путь, читай `{id}.bat` с аргументами (BatImport), добавь `--port`/фильтры из settings (lib.rs:921), спрячь запуск, запиши `runtime` в state, ожидай PID; «уже запущено» — идемпотентно. `ProfileRunner.Stop()` — порт lib.rs:1012-1050: kill tree по runtime.Pid, сброс runtime. `ProfileRunner.CurrentStatus()` — порт lib.rs:1089-1120: состояние из runtime (+ alive). `GameFilter.Ports(string filter) -> (tcp:string, udp:string)` — порт `game_filter_ports` из profiles.rs: верни строку с портами (например "443,80..." пусто если off) дословно.

- [ ] **Step 1: Failing test — GameFilter**

`GameFilter.Ports("games")` не пустой, `GameFilter.Ports("off")` пустой. Точные значения выпиши из профилей.

- [ ] **Step 2: See it fail.**

- [ ] **Step 3: Реализация.** Старт через `Processes.SpawnHidden` + свойство `started_at` (unix, из `DateTimeOffset.UtcNow.ToUnixTimeSeconds()`).

- [ ] **Step 4: Pass tests** (процессную часть проверяй вручную позже).

- [ ] **Step 5: Commit**

```bash
git add winforms/src/ZapretGui.Core/Runtime/ProfileRunner.cs winforms/src/ZapretGui.Core/Net/GameFilter.cs winforms/src/ZapretGui.Tests
git commit -m "winforms: port profile start/stop/status and game filter ports"
```

---

### Task 9: Служба zapret и автозапуск

**Files:**
- Create: `winforms/src/ZapretGui.Core/Runtime/Service.cs`
- Create: `winforms/src/ZapretGui.Core/Runtime/Autostart.cs`

**Interfaces:**
- Consumes: `Processes`, `Uac`, `Settings`, `Profile`, `PortableData`.
- Produces: `Service.Install(root, Profile, args, dataDir)` — порт service.rs:384-419 построчно: собери том же порядке PowerShell-скрипт (`PS_HEADER`, Stop/delete/New-Service/Start, Description 'Zapret DPI bypass software (zgui)'), пиши во `data\logs\svc_install_{pid}.ps1` (UTF-8 BOM), запусти elevated, удали скрипт, затем `reg add "...Services\zapret" /v zgui-strategy /t REG_SZ /d {profile.id} /f`. Возврат ошибки с кодом (service.rs:402-404). `Service.Remove(dataDir)` — порт service.rs:421-433. `Service.State()` → `(installed:bool, running:bool?)` — порт service.rs:435-443 (sc.exe query, STATE RUNNING). `Service.Strategy()` — чтение `zgui-strategy` из реестра. `Autostart.ApplyBootTask(root, exe, user)` — порт блок-задача (runner.rs:208 body `Register-ScheduledTask -TaskName ... -RunLevel Highest ... --boot`); task name в коде; `Autostart.RemoveBootTask()`, `Autostart.LegacyBootRegistered()` (порт: legacy key/task из runner.rs — `azapret`), `Autostart.Sync(appBoot, serviceInstalled, alwaysAdmin)` — логика lib.rs:1670-1740 (выбери кнопку «Служба» vs «При входе», служба первична).

- [ ] **Step 1: Failing test — State пустого состояния**

В тестовой среде (не elevated): `Service.State()` возвращает `(false, null)` без исключений; `Autostart.LegacyBootRegistered() == false` если планировщик недоступен.

- [ ] **Step 2: See it fail.**

- [ ] **Step 3: Реализация** по контрактам: `WritePs1` + `RunElevatedScript` как в Uac.

- [ ] **Step 4: Тест-настройка sync-логики**: `Autostart.Sync` принимает `(appboot:bool, svc:bool, alwaysAdmin:bool) -> план действий` — покрыть таблицу (svc без profile → appboot; appboot только при отсутствии svc; alwaysAdmin → RunAs-перезапуск, не задача планировщика). Реализуй как чистую функцию `SyncPlan(bootApp, svcInstalled, profileSelected, alwaysAdmin)` и протестируй 4-5 сценариев.

- [ ] **Step 5: Pass + Commit**

```bash
git add winforms/src/ZapretGui.Core/Runtime/Service.cs winforms/src/ZapretGui.Core/Runtime/Autostart.cs winforms/src/ZapretGui.Tests
git commit -m "winforms: port zapret service autostart sync and legacy migration"
```

---

### Task 10: Конфликты и VPN-детект

**Files:**
- Create: `winforms/src/ZapretGui.Core/Runtime/Conflicts.cs`

**Interfaces:**
- Consumes: `Processes`, `Settings`.
- Produces: `Conflicts.Check()` → `ConflictReport` — порт service.rs:17+ и одноимённой команды (`conflict_check`): Detect внешний winws (по процессу `winws.exe` не нашим PID), наличие VPN-адаптеров (по `netsh interface show interface` / WMI), прокси системы (реестровый ключ `HKCU\...\Internet Settings` + ProxyEnable). `Conflicts.VpnCheck() -> bool`. `Conflicts.KillConflicts()` — порт `kill_conflicts`: убить чужие winws + предложить выключить VPN (через netsh? — проверь реализацию в service.rs). Сообщения для UI — кириллические, из Rust.

- [ ] **Step 1: Failing test — чистая функция пересечения**

Порт решения «кто сейчас крутит winws кроме нас» выдели в чистую функцию `DetectForeignWinws(allPids, ownPids) -> IList<int>` и протестируй: {1,2,3} собств {1,2} → {3}.

- [ ] **Step 2: See it fail.**
- [ ] **Step 3: Реализация.**
- [ ] **Step 4: Pass + Commit**

```bash
git add winforms/src/ZapretGui.Core/Runtime/Conflicts.cs winforms/src/ZapretGui.Tests
git commit -m "winforms: port conflict and VPN detection"
```

---

### Task 11: DNS — провайдеры, apply/reset, benchmark

**Files:**
- Create: `winforms/src/ZapretGui.Core/Net/Dns.cs`
- Create: `winforms/src/ZapretGui.Core/Net/DnsBench.cs`

**Interfaces:**
- Consumes: `Processes`, `Settings`, `PortableData`.
- Produces: `Dns.Providers` — таблица `DnsProvider` (id, name, primary, secondary, note, description, dohTemplate) — скопируй из dns.rs:8+ дословно, включая русские `note`/`description`. `Dns.Apply(id)` — DoH через реестровые значения `HKLM\SYSTEM\CurrentControlSet\Services\Dnscache\Parameters\DohWellKnownServers` + netsh (порт dns.rs apply_dns: как в Rust, включая откат при ошибке), `Dns.Reset()` — порт reset_dns. `DnsBench.Run(providerIds, onRow)` — TCP и UDP проба на `primary` порт 53 с таймаутом 3 сек/провайдер, параллельно до 4; результат `(id, tcpMs?, udpMs?)`; кэш в `data\dns_bench.json` (порт dns_benchmark и кэша); `DnsBench.HumanText(row)` — «пинг ~{ms} мс» строки.

- [ ] **Step 1: Failing test — benchmark не зависает**

Фабрика UDP-сокета с коротким таймаутом; вызов `DnsBench.Run(new[]{"nonexistent-proxy"}, 1)` на несуществующий адрес → возврат с `tcpMs == null` (не тикает дольше 3.5с). Нужен `DnsBench.Probe(ip, mode)` чистый метод с инъекцией таймаута — протестируй его.

- [ ] **Step 2: See it fail.**
- [ ] **Step 3: Реализация.**
- [ ] **Step 4: Pass + Commit**

```bash
git add winforms/src/ZapretGui.Core/Net winforms/src/ZapretGui.Tests
git commit -m "winforms: port dns providers, apply/reset, benchmark"
```

---

### Task 12: Обновления

**Files:**
- Create: `winforms/src/ZapretGui.Core/Updater/Updater.cs`

**Interfaces:**
- Consumes: `Httpx`, `PortableData`, `Settings`, `Roots`, `Embedded`, `Text`.
- Produces: `Updater.CollectEntries(dataDir, roots) -> List<CatEntry>` — порт updater.rs:39-203 (whitelist из `refs/heads` — точный URL и список dest-путей выпиши из кода), включая `dest`, `catalog_only`; `Updater.CheckAll(dataDir, roots, settings) -> List<UpdEntry>` — порт updater.rs:260-283 (параллельно, таймаут 45с, UA `zgui/0.1 (zapret-gui updater)`); условие `label=="ipset-all.txt" && settings.ipset_mode!="loaded"` → skip (updater.rs:220). `Updater.Apply(ids)` — порт updater.rs:284-355: скачай в temp, unzip-в dest, проверь hash (remote_hash), верни обновлённые `UpdEntry` статусами. `Updater.SyncIpset(root, data, settings)` — порт updater.rs:356-360 (`loaded`/`none`/`any` выбор файла). Архивная защита: не трогать бинарники и user-файлы (whitelist).

- [ ] **Step 1: Failing test — ipset-условие**

`CanUpdate(entry, settings)`: `ipset-all.txt` с `IpsetMode=="none"` → false; с `"loaded"` → true; другой id → true. Чистая функция.

- [ ] **Step 2: See it fail.**
- [ ] **Step 3: Реализация** (сеть — Httpx, не тестируется юнитами; sync_ipset-перебор — тест на расположение файла).
- [ ] **Step 4: Тест SyncIpset**: с `IpnetMode="none"` не трогает `ipset-all.txt` (созданный заранее файл остаётся).
- [ ] **Step 5: Pass + Commit**

```bash
git add winforms/src/ZapretGui.Core/Updater winforms/src/ZapretGui.Tests
git commit -m "winforms: port updater with whitelist and ipset sync"
```

---

### Task 13: Telegram-мост как отдельный exe

**Files:**
- Create: `src-tauri/crates/tg-ws-proxy-rs/src/bin/zgui-bridge.rs` (или `main.rs` в новом bin-таргете того же крейта)
- Create: `winforms/src/ZapretGui.Core/Tele/TgBridge.cs`
- Modify: `winforms/src/ZapretGui.App/ZapretGui.App.csproj` (EmbeddedResource `assets/tg-ws-proxy.exe`)
- Modify: `winforms/build-release.ps1` (шаг cargo build bin)

**Interfaces:**
- Consumes: `Processes`, `PortableData`, `Uac`.
- Produces: `TgBridge.Status()` — проверка жив ли мост (по PID из state или по порту), `TgBridge.StartAsync(port, title)` — извлечь мост (если нет в `data\telegram\`), запустить `tg-ws-proxy.exe --port {port} ...` скрытно из `data\telegram\` (прочитай telegram.rs командную строку 1:1), `TgBridge.Stop()`, `TgBridge.Stats()` — порт tg_stats. `TgBridge.CheckUpdate()` — порт tg_check_update (сеть GitHub через Httpx, та же версия-логика). Диалог интерфейса с мостом (WS на `127.0.0.1:{port}`) — через `ClientWebSocket` (Bcl).

- [ ] **Step 1: Добавь bin-target в крейт**

Создай `src-tauri/crates/tg-ws-proxy-rs/src/bin/zgui-bridge.rs` вызывающий существующие модули (аналог того, как lib.rs/telegram вызывает мост; посмотри, как Tauri-версия запускает поток: какой API моста публичный и какие args принимает CLI — clap derive уже объявлен). `cargo build -p tg-ws-proxy-rs --bin zgui-bridge --release` даёт `target/release/zgui-bridge.exe`; переименуй в `tg-ws-proxy.exe` для ресурса. Не трогай `lib.rs` поведение Tauri-версии.

- [ ] **Step 2: Тест-заглушка** (мост — нативный, юнит-тест невозможен): `TgBridge.Status()` без запущенного моста → «not running», без исключений.

- [ ] **Step 3: Реализация TgBridge.** Командная строка args — дословно из telegram.rs (прочитай его: порт, домены, fake-tls флаги). Чтобы не открывалось окно — `CreateNoWindow`+hidden.

- [ ] **Step 4: build-release.ps1** добавляет шаг cargo build и кладёт `tg-ws-proxy.exe` в ресурсы до ILRepack.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/crates/tg-ws-proxy-rs winforms/src/ZapretGui.Core/Tele winforms/build-release.ps1 winforms/src/ZapretGui.App
git commit -m "winforms: build tg-ws-proxy as embedded sub-exe"
```

---

### Task 14: Тестер стратегий

**Files:**
- Create: `winforms/src/ZapretGui.Core/Tester/Tester.cs`

**Interfaces:**
- Consumes: `ProfileRunner`, `Settings`, `Profiles`, `PortableData`, `LogRing`.
- Produces: `Tester.Run(profileIds, mode)` — порт из tester.rs + lib.rs:1126-1200 `test_strategies`: сгенерируй тестовые runner-скрипты (порт `write_test_runner`), запускай по очереди, собирай прогресс (парсер делегаций — порт `test_marker`/парсеров из tester.rs), `Tester.Progress()` → `TestProgress` (структура из tester.rs, поля + camelCase), `Tester.Cache`/`LoadCache`/`SaveCache` — порт кэша результатов `data\test_results.json`, `Tester.Cancel()`, `Tester.ApplyBest(profileIds)` — порт apply_best_strategy (lib.rs:1634-1700 выбор стратегии по результатам кэша + запуск / установка autostart). `TestProgress` события → `Events`.

- [ ] **Step 1: Failing test — маркер прогресса**

Парсер вывода runner для одного профиля: текст runner с одним маркером → прогресс 60% (порт логики `test_marker` из tester.rs; точные проценты выпиши из кода).

- [ ] **Step 2: See it fail.**
- [ ] **Step 3: Реализация.**
- [ ] **Step 4: Тест кэша**: после `SaveCache` симуляция (2 результата) → `LoadCache` вернёт те же (ms/группы/даты), порядок сортировки по лучшему ms совпадает с Rust.
- [ ] **Step 5: Pass + Commit**

```bash
git add winforms/src/ZapretGui.Core/Tester winforms/src/ZapretGui.Tests
git commit -m "winforms: port strategy tester runner, progress, cache"
```

---

### Task 15: Watchdog

**Files:**
- Create: `winforms/src/ZapretGui.Core/WatchdogWatchdog/Watchdog.cs` → (путь без двойного слова: `winforms/src/ZapretGui.Core/Watchdog/Watchdog.cs`)

**Interfaces:**
- Consumes: `Httpx`, `Settings`, `ProfileRunner`.
- Produces: `Watchdog.Status()` → `WatchdogStatus` (watchdog.rs:34+; структура: включён/URL/последняя проверка/процент всей передачи), `Watchdog.Probe()` — HTTPS GET на сайт (таймаут 10с?), если всё потеряно — перезапуск профиля (порт перезапуска из watchdog.rs), события `Watchdog.Restarted`.
  Прочитай watchdog.rs полностью при реализации: там есть список URL-ов и логика «сколько связи умерло».

- [ ] **Step 1: Тест-заглушка**: `Status()` без запущенного watchdog → «off», без исключений.
- [ ] **Step 2: Реализация.**
- [ ] **Step 3: Pass + Commit**

```bash
git add winforms/src/ZapretGui.Core/Watchdog winforms/src/ZapretGui.Tests
git commit -m "winforms: port watchdog probe and restart"
```

---

### Task 16: UI-каркас — MainForm, тема, поллинг

**Files:**
- Create: `winforms/src/ZapretGui.App/MainForm.cs`
- Create: `winforms/src/ZapretGui.App/Theme.cs`
- Create: `winforms/src/ZapretGui.App/Controls/Chip.cs`, `Controls/BtnBusy.cs`, `Controls/Toast.cs`
- Modify: `winforms/src/ZapretGui.App/ZapretGui.App.csproj`

**Interfaces:**
- Consumes: `Bootstrap` (Task: собери Bootstrap-снимок), `Events`, все Core-сервисы.
- Produces: окно 1180×780 (min 960×640, tauri.conf.json), `MainForm` — слева вертикальная панель навигации (9 пунктов), справа контент; `Timer 4с` → `TryApply(Bootstrap)` (renderLog-оптимизация как `renderSettings` sig-проверка); `Theme.Apply(theme) — grey/dark/light; цвета из `index.html` css variables переносятся в константы (theme таблицы). `Toast(kind, text)` — html-аналог toast().

- [ ] **Step 1: Заглушка окна с 9 вкладками и тёмной темой.**
- [ ] **Step 2: Поллинг bootstrap** — просто печатать в консоль состояние (для отладки); страницы подключаются далее.
- [ ] **Step 3: Pass (запуск без исключений) + Commit**

```bash
git add winforms/src/ZapretGui.App
git commit -m "winforms: main form shell, themes, bootstrap polling"
```

---

### Task 17: Страница Профили + запуск/конфликты

**Files:**
- Create: `winforms/src/ZapretGui.App/Pages/ProfilesPage.cs`
- Create: `winforms/src/ZapretGui.App/Pages/ConflictOverlay.cs`
- Modify: `winforms/src/ZapretGui.App/MainForm.cs`

**Interfaces:**
- Consumes: `ProfileRunner`, `Owner`, `Profiles`, `Conflicts`, `Embedded`, `Events`.
- Produces: список профилей (скролл, карточки как в main.js `renderProfiles` 673+), кнопка запуска/остановки каждой; состояние по владельцу (`B.owner`); при запуске: `Conflicts.Check()` → overlay «конфликт» modal (main.js:349-455 `showConflict`/`autoKillAndProceed`) → kill / выкл / отмена; запуск через Core; на обновление bootstrap через 4с обновить кнопки. Тот же набор фильтров по движку, что в main.js:691.

- [ ] **Step 1: Реализация списка и кнопок (логика локальная, без реального запуска).**
- [ ] **Step 2: Интеграция с Owner** — красный индикатор «app», «service» когда CPU соответствует.
- [ ] **Step 3: Overlay конфликтов** — модальное окно с «Устранить и продолжить» / «Отмена» (поведение main.js:373-455 — кн boolки и пересчёт).
- [ ] **Step 4: Manual accept] — запусти профиль от админа, убедись, что winws виден и PID жив; кнопка «Остановить» убивает.
- [ ] **Step 5: Commit**

```bash
git add winforms/src/ZapretGui.App/Pages winforms/src/ZapretGui.App/MainForm.cs
git commit -m "winforms: profiles page with run/stop and conflict overlay"
```

---

### Task 18: Страница Тест

**Files:**
- Create: `winforms/src/ZapretGui.App/Pages/TestPage.cs`

**Interfaces:**
- Consumes: `Tester`, `TestProgress`, `Events`.
- Produces: эквивалент вкладки теста index.html#test: список чекбоксов профилей, кнопка «Прогнать» (mode аппер: все? из main.js `runTest` 997+), прогресс-бар (`renderTestProgress`), результаты таблица группо=сортов (main.js:886-995: группы, лучший ms, mars), infо-кнопка «подробно» (trackDetails). Кэш результатов загружается при открытии.

- [ ] **Step 1: Список профилей + прогресс.**
- [ ] **Step 2: Таблица результатов + лучший.**
- [ ] **Step 3: Запуск через Tester.Run (реальный прогон — потом) + кнопка Отмена (Tester.Cancel).**
- [ ] **Step 4: Manual accept — прогон на живых стратегиях (2 профиля).**
- [ ] **Step 5: Commit**

```bash
git add winforms/src/ZapretGui.App/Pages/TestPage.cs
git commit -m "winforms: strategy test page with progress and results"
```

---

### Task 19: Страница DNS

**Files:**
- Create: `winforms/src/ZapretGui.App/Pages/DnsPage.cs`

**Interfaces:**
- Consumes: `Dns`, `DnsBench`.
- Produces: select провайдеров (main.js:1368-1383 + name·primary/secondary + ping), кнопка «Сменить» (`apply_dns`), «Сброс», кнопка benchmark (main.js:1399-1430) с обновлением ping в списке, инфо/описание провайдера (main.js:1385-1397). Требует elevation — кнопки disabled при `B.elevated=false`.

- [ ] **Step 1: Список + стили.**
- [ ] **Step 2: Benchmark — прогресс нитей параллельно, обновление в select.**
- [ ] **Step 3: Apply/Reset — вызов Core.** Если не elevated — предупреждение «нужны права администратора» (toast).
- [ ] **Step 4: Manual accept — применить Cloudflare, проверить реестр DoH; сброс вернул.**
- [ ] **Step 5: Commit**

```bash
git add winforms/src/ZapretGui.App/Pages/DnsPage.cs
git commit -m "winforms: DNS page with providers, benchmark, apply/reset"
```

---

### Task 20: Страница Обновления

**Files:**
- Create: `winforms/src/ZapretGui.App/Pages/UpdatesPage.cs`

**Interfaces:**
- Consumes: `Updater`, `Settings`, `Roots`.
- Produces: список обновлений (main.js:1155-1240 `renderUpdates`): категории (confs/engines), последняя проверка, checkbox-выбор, кнопки «Проверить»/「Обновить выбранные」, прогресс на каждой строке (`updateProgress` 1240), блокировка кнопки до конца (busy), empty-текст для сброса.

- [ ] **Step 1: Список + чекбоксы.**
- [ ] **Step 2: Проверка — вызов CheckAll, отрисовка entries (статусы: новый/ок/ошибка).**
- [ ] **Step 3: Применение — по одному, прогресс бар строки, disabled до конца.**
- [ ] **Step 4: Manual accept — через реальный GitHub (российские зеркала, если основной недоступен — UI показывает статусы).**
- [ ] **Step 5: Commit**

```bash
git add winforms/src/ZapretGui.App/Pages/UpdatesPage.cs
git commit -m "winforms: updates page"
```

---

### Task 21: Страница Настройки + онбординги

**Files:**
- Create: `winforms/src/ZapretGui.App/Pages/SettingsPage.cs`
- Create: `winforms/src/ZapretGui.App/Pages/Onboarding.cs`

**Interfaces:**
- Consumes: `Settings`, `Owner`, `Service`, `Autostart`, `Uac`, `PortableData`.
- Produces: карточка «Автозапуск» (main.js:1302-1366 `renderAutostart`: select профиль + chip служба/вкл/выкл + note) и «Стратегия службы», интервал обновления (select), игровой фильтр (select), ipset (select: loaded/any/none), чекбокс always-admin и `администра`-предложение (main.js:457-505 + askRestartAdmin), кнопка "Всё равно запустить" при non-admin. Все контролы привязаны к `Settings` — рендер из bootstrap, save через `set_settings`-эквивалент Core.

- [ ] **Step 1: Все поля форм с renderSettings-логикой (не трогать поля при совпадении sig).**
- [ ] **Step 2: Обработчики сохранения (saveSoon-задержка как main.js:1777).**
- [ ] **Step 3: Onboarding — первое включение always_admin → диалог с confirm + relaunch_as_admin.**
- [ ] **Step 4: Manual accept — смена темы через настройки; автозапуск служба/профиль корректно переключает chip.
- [ ] **Step 5: Commit**

```bash
git add winforms/src/ZapretGui.App/Pages/SettingsPage.cs winforms/src/ZapretGui.App/Pages/Onboarding.cs
git commit -m "winforms: settings page with autostart and admin onboarding"
```

---

### Task 22: Страница Telegram

**Files:**
- Create: `winforms/src/ZapretGui.App/Pages/TelegramPage.cs`

**Interfaces:**
- Consumes: `TgBridge`, `Settings`.
- Produces: статус моста (main.js:1466-1500 `renderTg`: жив/нет, версия, порт), кнопка Старт/Стоп (main.js:1539-1577 `tgToggle`/`tgConnect`), чекбокс автозапуск TG при старте GUI, поле порта, кнопка «Проверить обновление моста». Подключение к Telegram через открытый порт WS (тот же URL из telegram.rs).

- [ ] **Step 1: Панель статуса + кнопки.**
- [ ] **Step 2: Старт/Стоп реального моста (извлечь exe из ресурса, запустить).**
- [ ] **Step 3: Проверка обновлений через GitHub.**
- [ ] **Step 4: Manual accept — мост поднялся, порт слушает, статус «работает».**
- [ ] **Step 5: Commit**

```bash
git add winforms/src/ZapretGui.App/Pages/TelegramPage.cs
git commit -m "winforms: telegram assistant page"
```

---

### Task 23: Страница Журнал + детали

**Files:**
- Create: `winforms/src/ZapretGui.App/Pages/LogPage.cs`

**Interfaces:**
- Consumes: `LogRing`, `PortableData`.
- Produces: журнал в реальном времени (main.js:106-183: рендер-кеш по составу, автоскролл вниз, часы local), кнопка «Открыть папку» (`explorer`), кнопка «Очистить», «Сохранить отчёт» (main.js:177+ открытие папки; отчёт-куда-код — это `report_save`? проведи как в main.js:106 `logWrite` область журнала). Детали: человеческие проценты времени (`fmtSize` 216).

- [ ] **Step 1: Список логов + автоскролл.**
- [ ] **Step 2: Кнопки Open/Очистить/Сохранить.**
- [ ] **Step 3: Manual accept — при запуске профиля строки журнала появляются.**
- [ ] **Step 4: Commit**

```bash
git add winforms/src/ZapretGui.App/Pages/LogPage.cs
git commit -m "winforms: live log page"
```

---

### Task 24: Финальный прогон и чек-лист

**Files:**
- Modify: (без новых потоков; только фиксы)
- Create: `winforms/ACCEPTANCE.md` — чек-лист

**Interfaces:**
- Consumes: все.

- [ ] **Step 1: Чистый прогон на свежем профиле Windows 10 — bootstrap без ошибок** (распаковка движка, seed каталога и конфигов, профили созданы).
- [ ] **Step 2: Запуск профиля из UI — winws в списке процессов, PID жив, отстанова работает, owner=app.**
- [ ] **Step 3: Тест стратегий — запуск, прогресс идёт, лучший применяется.**
- [ ] **Step 4: Служба: установка → автозапуск при входе работает, chip «служба»; удаление службы.**
- [ ] **Step 5: DNS: benchmark считает, apply/reset меняет реестр.**
- [ ] **Step 6: Обновления: check/apply на реальном GitHub (или вручную-файлы).**
- [ ] **Step 7: Telegram-мост: поднят, статус «работает», остановка чистит.**
- [ ] **Step 8: Сверка всех текстов UI/журнала/конфликтов с Tauri-версией — дословно.**
- [ ] **Step 9: Заполни ACCEPTANCE.md результатами (PASS/FAIL для каждого).**
- [ ] **Step 10: Commit**

```bash
git add winforms/ACCEPTANCE.md
git commit -m "winforms: acceptance checklist results"
```

---

## Self-Review

**1. Spec coverage:** задач покрывают все 52 tauri-команды: bootstrap (2,5,16), log_* (6,23), set_root/fetch_engine (7,8), save/delete_profile (3,4), start/stop/status (8), test_* (14), tg_* (13,22), watchdog (15), cancel/apply_best (14), install/remove/conflict/vpn (9,10), net_reset/reboot (в Conflicts/Service 9,10 — но `net_reset`, `virtual_adapters`, `reboot_now`, `net_create_restore_point` из командной группы netreset.rs НЕ выделены отдельной задачей — Z-зазор: добавить в Task 10 `Conflicts` → переименовать в `Runtime/NetworkOps` или в Task 9 добавить NetReset). ФИКС: расширь Task 9: добавь шаги для `NetReset.Run` (порт netreset.rs: жизнь сценария — netsh winsock reset + Checkpoint-Computer body netreset.rs:54, PS-скрипты как в service), `NetReset.CreateRestorePoint`, `NetReset.VirtualAdapters`, `NetReset.RebootNow` (порт reboot_now — перезапуск ПК с подтверждением), `NetReset.ShowAdapters` (main.js:2015). Из `app_info` (2290) — в Task 16 добавить строку «О программе» (v, portable, bundled). `relaunch_as_admin` — Task 6 Uac уже. `mark_admin_onboarded` — Task 21. `read_log`, `open_logs`, `report_save`, `open_path/open_url`, `ack_boot`, `dns_*` — Task 23 и 6/11.

**2. Placeholder scan:** фраз вида «уточни при реализации», «прочитай исходник» допустимы только потому, что план — порт: эталон поведения существует и должен читаться. Таких больше, чем хотелось бы, но каждый имеет точный файл:строку источника. Остальной контент — конкретные контракты и тесты.

**3. Type consistency:** `WinwsOwner`/`Owner.WinwsOwnerOf` — Task 5, используется Task 8/17/21 — сигнатура совпадает. `Settings`/`Profile`/`UpdEntry` — Task 2, все ниже используют те же поля. `PortableDataDir` — Task 2, Tasks 3,6,7,8,9,11,12,13,15. Guards однородны.

**4. Review Focus:** владелец (Task 5), .bat-парсинг (3), конфликты (10), DNS-tаймаут (11), ipset/whitelist (12), чистая машина (24). Все покрыты тестами или acceptance.

---

## Execution Handoff

План завершён. Исполняется в отдельной сессии дочерней моделью с доступом к репозиторию (Tauri-исходники — эталон поведения). Рекомендуется `subagent-driven-development`: задачи независимы по интерфейсам, 24 задачи, ошибка срока выполнения первой сто процентов окупает проверку каждой.