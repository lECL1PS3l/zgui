# Мульти-движковость, фазы 1–2: реестр, data-дистрибуция, zapret2/GoodbyeDPI/dpibreak

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Перевести Z GUI с одного движка Flowseal на реестр из четырёх системных движков (flowseal, zapret2, goodbyedpi, dpibreak), движки живут в `data/engines/`, служба Windows обобщена на любой движок.

**Architecture:** Единая статическая таблица движков `EngineDef` в config.rs заменяет все `if engine == ENGINE_FLOWSEAL`. Roots становится map. Вшитые пресеты — статическая таблица с подстановкой `%ENGINE_ROOT%`. Служба и тестер запускают exe из реестра. Новый модуль presets.rs; conflicts следит за всеми четырьмя exe.

**Tech Stack:** Rust (Tauri 2), serde_json, zip; фронт — vanilla JS (src/main.js).

**Spec:** `docs/superpowers/specs/2026-09-23-multiengine-design.md`

## Global Constraints

- Кодировка файлов с русским текстом — UTF-8 (без BOM для .rs/.js).
- Проверки после каждой задачи: `cargo check` (0 warnings), `cargo test --lib` (все зелёные, сейчас 49).
- Один активный обход: OPS-мьютекс уже гарантирует, новых блокировок не вводим.
- state.json обратная совместимость: старое поле `roots: {flowseal: ...}` должно читаться новой версией без потерь.
- Вшитый flowseal-zip в exe СОХРАНЯЕТСЯ как fallback миграции для существующих portable-пользователей; новые движки в exe не вшиваются.

## Review Focus

1. **Битый/чужой state.json со старым roots** — программа должна поднять корень flowseal и не потерять его; тест: сериализовать старый JSON → загрузить → roots map содержит flowseal.
2. **Профиль с движком, отсутствующим в реестре** (например, после отката версии) — не паника, профиль не запускается с внятной ошибкой; тест: profile("unknown") → engines() lookup → None → graceful error string.
3. **Аргументы пресета с пробелами в службе** — build_service_cmdline кавычит; тест уже есть для winws, добавить кейс с `--lua-init=@C:\path with space\...`.
4. **data/engines/<id> без exe (пользователь удалил папку)** — UI показывает «не готов, скачать», а не краш; тест: root_info на пустую папку → ready=false.
5. **Подстановка %ENGINE_ROOT% в кастомном профиле, где её нет** — аргументы без плейсхолдера не изменяются; тест: apply_engine_root("plain") == "plain".

---

### Task 1: Реестр движков и Roots-map

**Files:**
- Modify: `src-tauri/src/config.rs` (константы, Roots, Profile)
- Test: `src-tauri/src/config.rs` (mod tests)

**Interfaces:**
- Produces: `pub struct EngineDef { pub id, label, repo, exe: &'static str }`, 
  `pub fn engines() -> &'static [EngineDef; 4]`, `pub fn engine_def(id: &str) -> Option<&'static EngineDef>`,
  `pub fn engine_ids() -> [&'static str; 4]`, `Roots::map: BTreeMap<String, String>` + `path()/set()` (сигнатуры прежние), `Profile::exe_name(&self) -> &'static str` (теперь через реестр).

- [ ] **Step 1: failing test**

```rust
#[test]
fn registry_and_roots_migration() {
    // Реестр: все четыре движка, exe-имена корректны.
    let all = engines();
    assert_eq!(all.len(), 4);
    assert!(engine_def("zapret2").map(|d| d.exe == "winws2.exe").unwrap_or(false));
    assert!(engine_def("goodbyedpi").map(|d| d.exe == "goodbyedpi.exe").unwrap_or(false));
    assert!(engine_def("dpibreak").map(|d| d.exe == "dpibreak.exe").unwrap_or(false));
    assert!(engine_def("unknown").is_none());

    // Старый state.json: roots {"flowseal": "C:\\z"} → новая map.
    let legacy = serde_json::json!({"flowseal": "C:\\z"});
    let roots: Roots = serde_json::from_value(legacy).unwrap();
    assert_eq!(roots.path("flowseal").unwrap(), PathBuf::from("C:\\z"));
    assert!(roots.path("zapret2").is_none());

    // set работает для любого зарегистрированного движка.
    let mut r = Roots::default();
    r.set("zapret2", Some("D:\\e2".into()));
    assert_eq!(r.path("zapret2").unwrap(), PathBuf::from("D:\\e2"));

    // exe_name через реестр.
    let p = Profile { id: "x".into(), name: "x".into(), engine: "goodbyedpi".into(),
        args: vec![], builtin: true, source: None, updated_at: None };
    assert_eq!(p.exe_name(), "goodbyedpi.exe");
}
```

- [ ] **Step 2: run** `cargo test --lib registry` — FAIL (функции не существуют).

- [ ] **Step 3: реализация в config.rs**

```rust
pub const ENGINE_FLOWSEAL: &str = "flowseal";
pub const ENGINE_ZAPRET2: &str = "zapret2";
pub const ENGINE_GOODBYEDPI: &str = "goodbyedpi";
pub const ENGINE_DPIBREAK: &str = "dpibreak";

pub struct EngineDef {
    pub id: &'static str,
    pub label: &'static str,
    pub repo: &'static str,
    pub exe: &'static str,
}

pub fn engines() -> &'static [EngineDef] {
    &[
        EngineDef { id: ENGINE_FLOWSEAL, label: "Flowseal (zapret winws)", repo: "Flowseal/zapret-discord-youtube", exe: "winws.exe" },
        EngineDef { id: ENGINE_ZAPRET2, label: "zapret2 (winws2)", repo: "bol-van/zapret2", exe: "winws2.exe" },
        EngineDef { id: ENGINE_GOODBYEDPI, label: "GoodbyeDPI", repo: "ValdikSS/GoodbyeDPI", exe: "goodbyedpi.exe" },
        EngineDef { id: ENGINE_DPIBREAK, label: "DPIBreak", repo: "dilluti0n/dpibreak", exe: "dpibreak.exe" },
    ]
}

pub fn engine_def(id: &str) -> Option<&'static EngineDef> {
    engines().iter().find(|d| d.id == id)
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct Roots {
    map: BTreeMap<String, String>,
}

impl Roots {
    pub fn path(&self, engine: &str) -> Option<PathBuf> {
        if engine_def(engine).is_some() {
            self.map.get(engine).map(PathBuf::from)
        } else {
            None
        }
    }
    pub fn set(&mut self, engine: &str, p: Option<String>) {
        if engine_def(engine).is_none() { return; }
        match p { Some(v) => { self.map.insert(engine.into(), v); }, None => { self.map.remove(engine); } }
    }
}

impl Profile {
    pub fn exe_name(&self) -> &'static str {
        engine_def(&self.engine).map(|d| d.exe).unwrap_or("winws.exe")
    }
}
```

Сериализация Roots: `map` сериализуется как плоский объект `{"flowseal": "..."}` —
старый JSON читается без миграции. Убрать старое поле `flowseal`.

- [ ] **Step 4:** `cargo test --lib` — все зелёные; `cargo check` — 0 warnings.
- [ ] **Step 5:** `git add -A && git commit -m "feat: engine registry, Roots map with legacy state compat"`.

### Task 2: Пресеты движков (presets.rs)

**Files:**
- Create: `src-tauri/src/presets.rs`
- Modify: `src-tauri/src/lib.rs` (mod presets;), `src-tauri/src/profiles.rs`

**Interfaces:**
- Produces: `pub struct PresetDef { pub id, engine, name: &'static str, pub args: Vec<String> }` (args через фабрику, т.к. нужен %ENGINE_ROOT%),
  `pub fn builtin_presets() -> Vec<PresetDef>`, `pub fn apply_engine_root(args: &[String], root: &Path) -> Vec<String>`
  (заменяет `%ENGINE_ROOT%` на абсолютный путь с хвостовым разделителем).

- [ ] **Step 1: failing test** в presets.rs:

```rust
#[test]
fn presets_cover_all_engines_and_substitute_root() {
    let ps = builtin_presets();
    assert!(ps.iter().any(|p| p.engine == "flowseal"));
    assert!(ps.iter().any(|p| p.engine == "zapret2" && p.args.iter().any(|a| a.contains("--lua-desync"))));
    assert!(ps.iter().any(|p| p.engine == "goodbyedpi" && p.args.contains(&"-9".to_string())));
    assert!(ps.iter().any(|p| p.engine == "dpibreak" && p.args.iter().any(|a| a.starts_with("-o"))));

    let out = apply_engine_root(&["--lua-init=@%ENGINE_ROOT%lua/zapret-lib.lua".into(), "plain".into()],
        Path::new(r"D:\e2"));
    assert!(out[0].contains(r"D:\e2\lua\zapret-lib.lua") && !out[0].contains('%'));
    assert_eq!(out[1], "plain"); // без плейсхолдера — не трогаем
}
```

- [ ] **Step 2:** run → FAIL.

- [ ] **Step 3: реализация.** Таблица (id, engine, name, args) для:

```rust
// flowseal — один пресет «General» (args от текущего general.bat, без %ENGINE_ROOT%,
// экспортируется из существующего builtin-набора raw-стратегий).
// zapret2 — портированный preset2_example из win-bundle (README bol-van/zapret2):
vec![
 "--wf-tcp-out=80,443".into(),
 "--lua-init=@%ENGINE_ROOT%lua/zapret-lib.lua".into(),
 "--lua-init=@%ENGINE_ROOT%lua/zapret-antidpi.lua".into(),
 "--lua-init=fake_default_tls = tls_mod(fake_default_tls,'rnd,rndsni')".into(),
 "--filter-tcp=80".into(), "--filter-l7=http".into(), "--out-range=-d10".into(),
 "--payload=http_req".into(),
 "--lua-desync=fake:blob=fake_default_http:ip_autottl=-2,3-20:ip6_autottl=-2,3-20:tcp_md5".into(),
 "--lua-desync=fakedsplit:ip_autottl=-2,3-20:ip6_autottl=-2,3-20:tcp_md5".into(),
 "--new".into(),
 "--filter-tcp=443".into(), "--filter-l7=tls".into(), "--out-range=-d10".into(),
 "--payload=tls_client_hello".into(),
 "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6".into(),
 "--lua-desync=multidisorder:pos=midsld".into(),
 // (сигнатура youtube с hostlist — отдельный пресет «zapret2 · YouTube»)
],
// goodbyedpi: -9 (default), -8, -7, -6, -5; «RU + DNS»: ["-9", "--dns-addr", "77.88.8.8", "--dns-port", "1253"]; legacy -1..-4.
// dpibreak: ["-o", "0,1"], ["-o", "0,5", "-a"], ["-a"].
```

Реальные сигнатуры сверить с win-bundle и README при реализации (шаг верификации ниже).

- [ ] **Step 4:** `cargo test --lib` зелёные.
- [ ] **Step 5: верификация пресетов против первоисточников** — открыть README bol-van/zapret2 (preset2_example.cmd) и WinDivert-фильтры бандла; убедиться, что каждое имя файла (%ENGINE_ROOT%lua/..., files/...) существует в бандле после скачивания (см. Task 5). Отклонения исправить в таблице.
- [ ] **Step 6:** `git commit -m "feat: builtin presets for all engines with ENGINE_ROOT substitution"`.

### Task 3: Обобщение set_root / fetch_engine / bootstrap

**Files:**
- Modify: `src-tauri/src/lib.rs` (set_root, engine_meta, fetch_engine, root_info, Bootstrap DTO, collect_warnings)

**Interfaces:**
- Produces: `Bootstrap.engines: Vec<EngineInfo>` где `EngineInfo { id, label, path: Option<String>, exe: Option<String>, ready: bool }`;
  set_root принимает любой зарегистрированный id; seed_presets вызывается после установки корня.

- [ ] **Step 1: failing-поведение** — руками: `set_root("zapret2", "D:\\empty")` возвращает ошибку «не найден winws2.exe»; `set_root("zapret2", <корень с winws2>)` — принимается. Тест на engine_meta:

```rust
#[test]
fn engine_meta_registry() {
    assert!(crate::lib_test_helpers::engine_meta("goodbyedpi").is_ok());
    assert!(crate::lib_test_helpers::engine_meta("nope").is_err());
}
```
(если lib.rs тесты недоступны из mod tests — вынести engine_meta в config.rs рядом с реестром и тестировать там).

- [ ] **Step 2–3:** `engine_meta` → thin wrapper над `engine_def` (или переезд в config.rs). `set_root`: убрать проверку `engine != ENGINE_FLOWSEAL`, exe_name из реестра, для flowseal сохраняется neutralize_author_autoupdate + seed_flowseal_configs, для остальных — только seed_presets. `root_info` уже параметризован exe_name — пробежать все движки в bootstrap: 

```rust
let engines_info: Vec<EngineInfo> = config::engine_ids().iter().map(|id| {
    let def = config::engine_def(id).unwrap();
    let info = root_info(&s.roots, id, def.exe);
    EngineInfo { id: def.id, label: def.label, path: info.path, exe: info.exe, ready: info.ready }
}).collect();
```

`fetch_engine` — без изменений логики: meta.repo из реестра; для goodbyedpi/dpibreak выбирать asset по маске `*.zip` (у dpibreak win-asset может называться по архитектуре — fallback: первый asset с .zip/.exe-файлом внутри; при отсутствии zip — сообщение «архив не найден, скачайте вручную»).

collect_warnings: цикл по всем движкам реестра вместо [ENGINE_FLOWSEAL].

- [ ] **Step 4:** `cargo check` 0 warnings, `cargo test --lib` зелёные.
- [ ] **Step 5:** `git commit -m "feat: set_root/fetch_engine/bootstrap generalized to engine registry"`.

### Task 4: Служба и конфликты на все движки

**Files:**
- Modify: `src-tauri/src/service.rs` (detect_conflicts, own_engine_pids, any_winws_running → any_engine_running), `src-tauri/src/netreset.rs` (список имён процессов)

**Interfaces:**
- Produces: `pub fn ENGINE_EXES: [&'static str; 4] = ["winws.exe", "winws2.exe", "goodbyedpi.exe", "dpibreak.exe"]`; функции конфликтов принимают этот список.

- [ ] **Step 1: failing test**:

```rust
#[test]
fn engine_exe_list_covers_registry() {
    for d in crate::config::engines() {
        assert!(service::ENGINE_EXES.contains(&d.exe), "нет в конфликтах: {}", d.exe);
    }
}
```

- [ ] **Step 2:** run → FAIL.
- [ ] **Step 3:** заменить хардкод `["winws.exe", "winws2.exe"]` в detect_conflicts/process_image_paths/own_engine_pids/any_winws_running на ENGINE_EXES; `is_own_engine` — путь начинается с data/engines/<любого id> (перебор engine_ids); netreset `$names` дополнить 'goodbyedpi','dpibreak'. build_service_cmdline уже generic (profile.exe_name() из Task 1) — добавить тест кавычек:

```rust
#[test]
fn service_cmdline_quotes_spaces() {
    let p = Profile { id: "z2".into(), name: "z".into(), engine: "zapret2".into(),
        args: vec!["--lua-init=@C:\\lua lib\\zapret-lib.lua".into()], builtin: true, source: None, updated_at: None };
    let c = service::build_service_cmdline_for_test(Path::new(r"D:\e"), &p);
    assert!(c.contains("\"--lua-init=@C:\\lua lib\\zapret-lib.lua\""));
}
```
(build_service_cmdline сделать pub(crate) или обёртка для теста).

- [ ] **Step 4:** тесты зелёные; `cargo test --lib`.
- [ ] **Step 5:** `git commit -m "feat: conflicts and netreset track all engine exes"`.

### Task 5: Дистрибуция — data/engines и упаковка

**Files:**
- Create: `scripts/fetch-engines.ps1`
- Modify: `src-tauri/src/embedded.rs` (ensure-функция для новых движков), `.gitignore` (не коммитить data/engines бинари), скрипт сборки release (если есть — найти в scripts/).

**Interfaces:**
- Produces: `pub fn ensure_engine(data: &Path, id: &str) -> Result<Option<PathBuf>, String>` — корень из Roots, иначе data/engines/<id> c exe, иначе вшитый flowseal-fallback (только flowseal).

- [ ] **Step 1: скрипт fetch-engines.ps1** — скачивает последние релизы zapret2 win-bundle, GoodbyeDPI, dpibreak в `data/engines/<id>/` (GitHub API latest → zip → распаковка тем же extract_all из embedded.rs, вызов через cargo test или отдельный PS). Скрипт запускается при подготовке релиза, НЕ на машинах пользователей (у них fetch_engine по кнопке).

- [ ] **Step 2: test на ensure_engine** (пустая data + папка с winws2.exe → Ok(root)); flowseal-fallback остаётся (существующий тест embedded_engines_contain_exe не трогаем).

- [ ] **Step 3:** `cargo test --lib`, `git commit -m "feat: engine distribution via data/engines + packaging script"`.

### Task 6: UI — выбор движка

**Files:**
- Modify: `src/main.js`, `src/index.html`, `src/styles.css` (минимально)

**Interfaces:**
- Consumes: `bootstrap.engines` (Task 3), presets из profилей (Task 2, применяются как обычные профили).

- [ ] **Step 1:** на вкладке «Стратегии» — селектор движка (радио/дропдаун из engines: label + статус готовности); профили фильтруются по выбранному движку; у неготового движка кнопка «Скачать» вызывает `fetch_engine(id)`; для goodbyedpi/dpibreak скрыть блоки, специфичные для flowseal (game filter остаётся — плейсхолдеры универсальны).
- [ ] **Step 2: ручная проверка** `npm run dev` (или tauri dev): список движков, flowseal-профили работают как раньше, у zapret2 без движка — «Скачать», после скачивания появляется exe-статус.
- [ ] **Step 3:** `git commit -m "feat: engine selector UI with per-engine download"`.

## Верификация всей фазы

1. `cargo check` — 0 warnings; `cargo test --lib` — все зелёные (49 + новые).
2. Ручной прогон на Windows от админа: установка корня zapret2 (бандл) → запуск пресета через службу → `sc query zapret` RUNNING, процесс winws2.exe жив → смена стратегии на goodbyedpi -9 → служба пересоздана, goodbyedpi.exe жив → Стоп.
3. Конфликт-чек видит чужой goodbyedpi.exe.

## Фазы 3–5 (отдельные планы после этой партии)

3. Тестер+автоподбор (матрица кандидатов). 4. OTA-пресеты. 5. Telegram-автопрокид.
