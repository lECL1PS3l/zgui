import "./styles.css";
import { invoke as rawInvoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { T } from "./texts.js";

// Иконки Lucide (ISC, https://lucide.dev) — вендорены в src/assets/icons,
// цвет наследуется через stroke="currentColor", поэтому работают во всех темах.
import icoZap from "./assets/icons/lucide-zap.svg?raw";
import icoFlask from "./assets/icons/lucide-flask-conical.svg?raw";
import icoDownload from "./assets/icons/lucide-download.svg?raw";
import icoSend from "./assets/icons/lucide-send.svg?raw";
import icoShield from "./assets/icons/lucide-shield-check.svg?raw";
import icoPalette from "./assets/icons/lucide-palette.svg?raw";
import icoSettings from "./assets/icons/lucide-settings.svg?raw";
import icoX from "./assets/icons/lucide-x.svg?raw";
import icoCheck from "./assets/icons/lucide-check.svg?raw";
import icoScroll from "./assets/icons/lucide-scroll-text.svg?raw";
import icoWrench from "./assets/icons/lucide-wrench.svg?raw";
import icoInfo from "./assets/icons/lucide-info.svg?raw";
import topoUrl from "./assets/topo.svg?url";

const NAV_ICONS = {
  strategies: icoZap,
  tests: icoFlask,
  updates: icoDownload,
  telegram: icoSend,
  dns: icoShield,
  appearance: icoPalette,
  settings: icoSettings,
  tools: icoWrench,
  logs: icoScroll,
  about: icoInfo,
  scanner: icoShield,
};

function renderNavIcons() {
  for (const btn of $$(".nav-item")) {
    const slot = btn.querySelector(".nav-ico");
    const svg = NAV_ICONS[btn.dataset.view];
    if (slot && svg) slot.innerHTML = svg;
  }
  for (const btn of $$(".modal-x")) btn.innerHTML = icoX;
}

// Иконка ✓/✗ для чипов теста (вместо текстовых символов).
function chipMark(ok) {
  const s = document.createElement("span");
  s.className = "chip-ico";
  s.innerHTML = ok ? icoCheck : icoX;
  return s;
}

const $ = (s) => document.querySelector(s);
const $$ = (s) => [...document.querySelectorAll(s)];

// Подставляет тексты из словаря во все элементы с data-i18n-атрибутами.
function applyTexts(root = document) {
  root.querySelectorAll("[data-i18n]").forEach((el) => { const v = T[el.dataset.i18n]; if (v != null) el.textContent = v; });
  root.querySelectorAll("[data-i18n-html]").forEach((el) => { const v = T[el.dataset.i18nHtml]; if (v != null) el.innerHTML = v; });
  root.querySelectorAll("[data-i18n-title]").forEach((el) => { const v = T[el.dataset.i18nTitle]; if (v != null) el.title = v; });
  root.querySelectorAll("[data-i18n-placeholder]").forEach((el) => { const v = T[el.dataset.i18nPlaceholder]; if (v != null) el.placeholder = v; });
  root.querySelectorAll("[data-i18n-alt]").forEach((el) => { const v = T[el.dataset.i18nAlt]; if (v != null) el.alt = v; });
}

// ------------------------------------------------------------- ошибки и журнал

// Технические ошибки расшифровываем на человеческий язык. Таблица повторяет
// логику src-tauri/src/human.rs — интерфейс и бэкенд говорят одинаково.
const ERROR_RULES = [
  [/os error 5|access is denied|administrator|admin_required/i,
    T.err_admin],
  [/os error 32|being used by another process/i,
    T.err_busy],
  [/os error 112|not enough space/i, T.err_space],
  [/os error 2|os error 3|cannot find/i,
    T.err_not_found],
  [/error sending request|error trying to connect|dns error|timed out|connection refused|connection reset|network is unreachable/i,
    T.err_net],
  [/http 403|\b403 forbidden\b/i,
    T.err_403],
  [/http 404|\b404 not found\b/i, T.err_404],
  [/http 5\d\d/i, T.err_5xx],
  [/invalid args|expected u16|invalid type|invalid value/i,
    T.err_invalid],
  [/process exited immediately/i, T.err_exit],
  [/launch_error/i, T.err_launch],
  [/panic|panicked/i, T.err_panic],
];

const TECH_RE = /os error|error|failed|denied|http |invalid|panic|refused|timed out/i;
const CYR_RE = /[а-яё]/i;

function humanError(raw) {
  const s = String(raw == null ? "" : raw).trim();
  if (!s) return T.err_unknown;
  // Своё понятное сообщение (на русском и без технических маркеров) не портим.
  if (CYR_RE.test(s) && !TECH_RE.test(s)) return s;
  for (const [re, msg] of ERROR_RULES) if (re.test(s)) return msg;
  return T.err_unexpected(s.length > 220 ? s.slice(0, 220) + "…" : s);
}

const logState = { items: [], seq: 0, level: "all", query: "", timer: null };

function logPush(entry) {
  if (!entry) return;
  if (!entry.ts) entry.ts = Date.now();
  if (entry.seq) logState.seq = Math.max(logState.seq, entry.seq);
  logState.items.push(entry);
  if (logState.items.length > 2000) logState.items.shift();
  const v = $("#view-logs");
  if (v && v.classList.contains("active")) renderLog();
}

/// Запись в журнал из интерфейса — попадает и в файл, и в окно «Журнал».
/// `code` (E-ГРУППА-000) виден только в журнале/отчёте — в тосты не подмешивается.
function logWrite(level, scope, msg, code) {
  const text = code ? `${String(msg)} [${code}]` : String(msg);
  rawInvoke("log_write", { level, scope, msg: text }).catch(() => {});
  logPush({ ts: Date.now(), level, scope, msg: text });
}

/// Коды для ошибок команд (бэкенд-строки попадают в журнал через обёртку invoke).
/// Расшифровки — в `src-tauri/src/codes.rs` (блок «КОДЫ В ЭТОМ ОТЧЁТЕ»).
const CMD_CODES = {
  start_profile: "E-START-003",
  stop_running: "E-STOP-001",
  test_strategies: "E-TEST-003",
  cancel_test: "E-TEST-003",
  install_service: "E-SVC-003",
  remove_service: "E-SVC-004",
  check_updates: "E-UPD-001",
  apply_updates: "E-UPD-002",
  apply_dns: "E-DNS-001",
  reset_dns: "E-DNS-002",
  net_reset: "E-NET-002",
  discord_cache_clear: "E-TOOL-001",
  replace_fake: "E-TOOL-002",
  hosts_update: "E-TOOL-003",
  tg_start: "E-TG-002",
  tg_stop: "E-TG-003",
  tg_check_update: "E-TG-004",
  open_url: "E-TG-001",
  set_settings: "E-SYS-002",
  set_theme: "E-SYS-002",
};

/// Обёртка над вызовом бэкенда: понятный текст ошибки + запись в журнал.
async function invoke(cmd, args) {
  try {
    return await rawInvoke(cmd, args);
  } catch (e) {
    const raw = typeof e === "string" ? e : (e && e.message) || String(e);
    const human = humanError(raw);
    logWrite("err", "команда " + cmd, raw, CMD_CODES[cmd] || "E-CMD-001");
    const err = new Error(human);
    err.raw = raw;
    throw err;
  }
}

function logVisible() {
  const q = logState.query.trim().toLowerCase();
  return logState.items.filter((e) => {
    if (logState.level !== "all" && e.level !== logState.level) return false;
    if (!q) return true;
    return (String(e.msg) + " " + String(e.scope)).toLowerCase().includes(q);
  });
}

function logTime(ts) {
  const d = new Date(ts);
  const p = (n) => String(n).padStart(2, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

function renderLog() {
  const box = $("#logList");
  if (!box) return;
  const items = logVisible();
  const stick = box.scrollHeight - box.scrollTop - box.clientHeight < 40;
  box.replaceChildren();
  if (!items.length) {
    const empty = document.createElement("div");
    empty.className = "log-empty";
    empty.textContent = logState.items.length ? T.log_empty_search : T.log_empty;
    box.appendChild(empty);
  } else {
    for (const e of items) {
      const row = document.createElement("div");
      row.className = "log-row " + (e.level || "info");
      const t = document.createElement("span");
      t.className = "lt";
      t.textContent = logTime(e.ts);
      const lv = document.createElement("span");
      lv.className = "lv";
      lv.textContent = e.level || "info";
      const sc = document.createElement("span");
      sc.className = "ls";
      sc.textContent = e.scope || "";
      const ms = document.createElement("span");
      ms.className = "lm";
      ms.textContent = e.msg || "";
      row.append(t, lv, sc, ms);
      box.appendChild(row);
    }
    if (stick) box.scrollTop = box.scrollHeight;
  }
  const foot = $("#logFoot");
  if (foot) foot.textContent = T.log_shown(items.length, logState.items.length);
}

async function logPoll() {
  try {
    const items = await rawInvoke("log_entries", { after: logState.seq });
    for (const it of items || []) logPush(it);
  } catch (_) {}
}

async function openLogs() {
  // Разовый добор истории; дальше новые записи приходят событием zgui:log.
  await logPoll();
  renderLog();
}

let B = null; // bootstrap snapshot
let profileFilter = "all";
let testState = null; // TestProgress
let testCache = null;
// null means the initial/default state: all Flowseal .bat strategies selected.
let testPicked = null;
let dnsProviders = [];
let dnsBench = {};
// Спойлеры (<details>) пересоздаются при каждой перерисовке, поэтому их открытое состояние
// сохраняется отдельно и восстанавливается после render*. Ключ — id строки, а не индекс:
// порядок списков меняется (сортировка результатов теста, фильтр вкладок), и индексы «уезжают».
// Состояние синхронизируется через событие "toggle" (см. trackDetails): закрытие убирает id,
// иначе один раз открытый спойлер сам открывался бы заново при каждой перерисовке.
let openTestDetails = new Set();
let openProfileDetails = new Set();
let openUpdateGroups = new Set();

// Вешает обработчик toggle, чтобы Set всегда отражал реальное состояние спойлера.
const trackDetails = (details, set) => {
  details.addEventListener("toggle", () => {
    if (details.open) set.add(details.dataset.id);
    else set.delete(details.dataset.id);
  });
};

const fmtSize = (n) =>
  n > 1024 * 1024 ? (n / 1024 / 1024).toFixed(1) + " MB" : n > 1024 ? (n / 1024).toFixed(0) + " KB" : n + " B";

const statusLabel = {
  ok: T.st_ok,
  avail: T.st_avail,
  new: T.st_new,
  modified: T.st_modified,
  err: T.st_err,
  "skip-user": T.st_skip,
  unknown: T.st_unknown,
};

// ------------------------------------------------------------- тема оформления
const THEMES = ["grey", "dark", "light"];
let themeLockUntil = 0;
// Тема, выбранная пользователем: защищает от отката, пока bootstrap с опозданием
// присылает снапшот, снятый ДО сохранения темы (гонка set_theme ⇄ опрос).
let themePicked = null;

function applyTheme(theme) {
  const t = THEMES.includes(theme) ? theme : "grey";
  document.documentElement.dataset.theme = t;
  try { localStorage.setItem("zgui.theme", t); } catch (_) {}
  $$("#themeGrid .theme-card").forEach((c) => c.classList.toggle("active", c.dataset.theme === t));
  syncFxEnabled();
}

function initTheme() {
  // Кэш в localStorage — чтобы не мигало белым/чёрным до ответа bootstrap.
  let t = "grey";
  try { t = localStorage.getItem("zgui.theme") || "grey"; } catch (_) {}
  applyTheme(t);
}

function currentTheme() {
  // Пока идёт блокировка после выбора — не слушаем запоздавший bootstrap.
  if (themePicked && Date.now() < themeLockUntil) return themePicked;
  const fromState = B && B.settings && B.settings.theme;
  if (THEMES.includes(fromState)) return fromState;
  try { return localStorage.getItem("zgui.theme") || "grey"; } catch (_) { return "grey"; }
}

async function pickTheme(theme) {
  // После смены темы кнопки блокируются на 3 секунды (тема применяется не мгновенно).
  if (Date.now() < themeLockUntil) return;
  applyTheme(theme);
  // Снапшот обновляем сразу: иначе запоздавший bootstrap
  // применял бы старую тему из ещё не сохранённого state — тема «отпрыгивала».
  if (B && B.settings) B.settings.theme = theme;
  themePicked = theme;
  themeLockUntil = Date.now() + 3000;
  $$("#themeGrid .theme-card").forEach((c) => lockBtnFill(c, 3));
  try {
    await invoke("set_theme", { theme });
  } catch (e) {
    toast("err", T.theme_err(e));
  }
}

// ------------------------------------------------------------- узор фона
const patternMods = import.meta.glob("./assets/patterns/*.svg", { eager: true, query: "?url", import: "default" });
const PATTERN_URLS = { topo: topoUrl };
for (const [p, url] of Object.entries(patternMods)) {
  PATTERN_URLS[p.split("/").pop().replace(".svg", "")] = url;
}
// topo — текущий узор, дальше — набор Hero Patterns (тот же автор).
const PATTERN_ORDER = ["topo", "bubbles", "tic-tac-toe"];
// Размер тайла в превью (px): подбираем под наглядность каждого узора.
const PATTERN_PREVIEW = { topo: 360, bubbles: 56, "tic-tac-toe": 44 };
// Размер тайла в теме (px): узоры «отдалены», как топо. Кратно дельте дрейфа (calc в styles.css).
const PATTERN_THEME_SIZE = { topo: 600, bubbles: 140, "tic-tac-toe": 95 };
// Множитель прозрачности узора (доля от базовой темы): новые узоры приглушены.
const PATTERN_OPACITY = { topo: 1, bubbles: 0.57, "tic-tac-toe": 0.57 };

function currentPattern() {
  try { return localStorage.getItem("zgui.pattern") || "topo"; } catch (_) { return "topo"; }
}

function applyPattern(id) {
  const p = PATTERN_URLS[id] ? id : "topo";
  document.documentElement.style.setProperty("--topo-image", `url("${PATTERN_URLS[p]}")`);
  document.documentElement.style.setProperty("--topo-size", `${PATTERN_THEME_SIZE[p] || 600}px`);
  document.documentElement.style.setProperty("--pat-opacity", `${PATTERN_OPACITY[p] ?? 1}`);
  try { localStorage.setItem("zgui.pattern", p); } catch (_) {}
  $$("#patternGrid .pattern-card").forEach((c) => c.classList.toggle("active", c.dataset.pattern === p));
}

function renderPatternGrid() {
  const grid = $("#patternGrid");
  if (!grid) return;
  grid.innerHTML = "";
  for (const id of PATTERN_ORDER) {
    if (!PATTERN_URLS[id]) continue;
    const b = document.createElement("button");
    b.type = "button";
    b.className = "theme-card pattern-card";
    b.dataset.pattern = id;
    const sw = document.createElement("span");
    sw.className = "pattern-swatch";
    sw.style.backgroundImage = `url("${PATTERN_URLS[id]}")`;
    const px = PATTERN_PREVIEW[id] || 108;
    sw.style.backgroundSize = `${px}px ${px}px`;
    b.append(sw);
    b.addEventListener("click", () => applyPattern(id));
    grid.appendChild(b);
  }
  applyPattern(currentPattern());
}

// На светлой теме фона нет: галочки анимации и узора заблокированы.
function syncFxEnabled() {
  const light = document.documentElement.dataset.theme === "light";
  const fx = $("#cfFxOff");
  if (fx) fx.disabled = light;
  const pat = $("#cfPatOff");
  if (pat) pat.disabled = light;
}

function initFx() {
  let off = false;
  try { off = localStorage.getItem("zgui.fx") === "off"; } catch (_) {}
  document.body.classList.toggle("fx-off", off);
  const cb = $("#cfFxOff");
  if (cb) cb.checked = off;
  let patOff = false;
  try { patOff = localStorage.getItem("zgui.pat") === "off"; } catch (_) {}
  document.body.classList.toggle("pat-off", patOff);
  const pcb = $("#cfPatOff");
  if (pcb) pcb.checked = patOff;
  syncFxEnabled();
}

function toast(kind, text) {
  const el = document.createElement("div");
  el.className = `toast ${kind}`;
  el.textContent = text;
  $("#toasts").appendChild(el);
  setTimeout(() => el.remove(), 4800);
}

function btnBusy(btn, b) {
  if (!btn) return;
  btn.classList.toggle("busy", b);
  btn.disabled = b;
}

/// Экранирование текста для вставки в innerHTML: сырые ошибки ОС/PowerShell
/// могут содержать `<`, `&` — скрипты блокирует CSP, но HTML-инъекция в UI
/// ни к чему.
function escHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c],
  );
}

function shortPath(p) {
  if (!p) return "—";
  return p;
}

// ------------------------------------------------------------- bootstrap

async function refreshAll() {
  try {
    B = await invoke("bootstrap");
    onBootstrap();
    checkTgOffer();
  } catch (e) {
    toast("err", T.boot_err(e));
  }
}

// Предупреждение «настройки не сохраняются» показываем один раз за сессию.
let saveFailedWarned = false;

function onBootstrap() {
  applyTheme(currentTheme());
  if (B.saveFailed && !saveFailedWarned) {
    saveFailedWarned = true;
    toast("warn", T.save_failed);
  }
  renderRunBar();
  ensureEngineCards();
  renderProfileTabs();
  renderTestTabs();
  renderEngines();
  renderProfiles();
  renderTestCard();
  renderAutostart();
  renderSettings();
  const bt = B && B.updates && B.updates.lastCheck;
  $("#lastCheck").textContent = bt ? T.last_check(tsText(Number(bt))) : T.never_checked;
  renderUpdates($("#view-updates") === document.querySelector(".page.active") ? "force" : "lazy");
}

// ---- своё окно подтверждения (системный confirm() в WebView не показывается) ----
let cmResolve = null;

function showConfirm({ title, html, okLabel = T.btn_continue, cancelLabel = T.btn_cancel, danger = false, okKind = "", lockSec = 0 }) {
  return new Promise((resolve) => {
    // Повторный вызов перекрывает прошлый диалог: его await должен получить
    // отказ, а не висеть вечно.
    if (cmResolve) cmResolve(false);
    cmResolve = resolve;
    $("#cmTitle").textContent = title;
    $("#cmBody").innerHTML = html;
    const ok = $("#cmOk");
    ok.textContent = okLabel;
    ok.className = "btn " + (okKind || (danger ? "danger" : "primary"));
    ok.classList.remove("fill-lock");
    ok.disabled = false;
    $("#cmCancel").textContent = cancelLabel;
    $("#confirmModal").classList.remove("hidden");
    // 3 секунды на «прочитать риски»: кнопка заблокирована с заливкой-шкалой.
    if (lockSec > 0) lockBtnFill(ok, lockSec);
  });
}

function closeConfirm(val) {
  $("#confirmModal").classList.add("hidden");
  const r = cmResolve;
  cmResolve = null;
  if (r) r(val);
}

// ------------------------------------------------------------- блокировки кнопок

/// Таймеры блокировки: повторный вызов на том же элементе отменяет прежний,
/// иначе старый таймер сработает позже и разблокирует «не в своё время».
const lockTimers = new WeakMap();

/// Блокирует элемент на `secs` секунд, показывая заливку-«шкалу» слева-направо
/// (без цифр таймера — по просьбе владельца).
function lockBtnFill(el, secs) {
  if (!el) return;
  const prev = lockTimers.get(el);
  if (prev) clearTimeout(prev);
  el.disabled = true;
  el.classList.add("fill-lock");
  el.style.setProperty("--lock-sec", `${secs}s`);
  const t = setTimeout(() => {
    lockTimers.delete(el);
    el.classList.remove("fill-lock");
    el.style.removeProperty("--lock-sec");
    el.disabled = false;
  }, secs * 1000);
  lockTimers.set(el, t);
}

function renderRunBar() {
  const st = $("#runState");
  const opRunning = !!(B && B.opRunning);
  const testing = !!(B && B.testing);
  const active = !!(B && B.engineActive);
  if (testing) {
    // Идёт прогон тестов: winws управляется тестом — останавливать его тулбаром нельзя.
    st.className = "run-state running";
    st.textContent = T.run_test;
  } else if (opRunning) {
    // Идёт долгая операция (обновления, DNS, служба, сброс сети):
    // старт/стоп профилей запрещён — взаимная блокировка.
    st.className = "run-state busy";
    st.textContent = T.run_op;
  } else if (active) {
    st.className = "run-state running";
    st.textContent = T.run_active;
  } else {
    st.className = "run-state idle";
    st.textContent = T.run_idle;
  }
  $("#btnStop").disabled = opRunning || !active;
}

function renderEngines() {
  const list = B && B.engines;
  if (!list) return;
  for (const info of list) {
    const eng = info.id;
    const keys = { state: eng === "flowseal" ? "fsState" : `engState-${eng}`, root: eng === "flowseal" ? "fsRoot" : `engRoot-${eng}`, card: `engine-${eng}` };
    const st = $("#" + keys.state);
    const rootEl = $("#" + keys.root);
    if (!info || !st || !rootEl) continue;

    if (info.path) {
      st.className = "state-chip " + (info.ready ? "ok" : "bad");
      st.textContent = info.ready ? T.eng_ready : T.eng_need_file(info.exe || "");
      rootEl.innerHTML = "";
      const pathEl = document.createElement("span");
      pathEl.className = "root-path";
      pathEl.textContent = shortPath(info.path);
      rootEl.appendChild(pathEl);

      const acts = document.createElement("div");
      acts.className = "root-actions";
      acts.appendChild(btn(T.btn_open_folder, "ghost", () => invoke("open_path", { path: info.path })));
      acts.appendChild(btn(T.btn_change, "ghost", async () => {
        const dir = await open({ directory: true });
        if (dir) {
          try {
            await invoke("set_root", { engine: eng, path: dir });
            await refreshAll();
          } catch (e) {
            toast("err", String(e));
          }
        }
      }));
      rootEl.appendChild(acts);
    } else {
      st.className = "state-chip warn";
      st.textContent = T.eng_not_installed;
      rootEl.innerHTML = "";
      const acts = document.createElement("div");
      acts.className = "root-actions";
      acts.appendChild(btn(T.btn_pick_folder, "ghost", async () => {
        const dir = await open({ directory: true });
        if (dir) {
          try {
            await invoke("set_root", { engine: eng, path: dir });
            await refreshAll();
          } catch (e) {
            toast("err", String(e));
          }
        }
      }));
      rootEl.appendChild(acts);
    }

    const card = $("#" + keys.card);
    const runningHere = B.runtime && B.runtime.profileId;
    if (runningHere) {
      const pf = B.profiles.find((p) => p.id === runningHere);
      if (pf && pf.engine === eng) card.style.borderColor = "rgba(74,222,128,.4)";
      else card.style.borderColor = "";
    } else card.style.borderColor = "";
  }
}

// ------------------------------------------------------------- engine registry

function engineLabel(id) {
  const e = ((B && B.engines) || []).find((x) => x.id === id);
  return e ? e.label : id;
}

// Цветовые метки движков (●): одинаковые во всех списках и плитках.
const ENGINE_COLORS = {
  flowseal: "#5865f2",
  zapret2: "#a78bfa",
  goodbyedpi: "#34d399",
  dpibreak: "#ffb833",
};

/// Кружок-метка движка для вставки рядом с названием.
function engDot(id) {
  const s = document.createElement("span");
  s.className = "eng-dot";
  s.style.background = ENGINE_COLORS[id] || "var(--muted)";
  return s;
}

/// Карточки движков на вкладке «Обновления»: статичные для flowseal (HTML),
/// остальные создаём из bootstrap.engines (id, label, статус, кнопки).
let engineCardsBuilt = "";
function ensureEngineCards() {
  const wrap = document.querySelector("#updTiles");
  if (!wrap || !B.engines) return;
  const sig = B.engines.map((e) => e.id).join(",");
  if (sig === engineCardsBuilt) return;
  engineCardsBuilt = sig;
  for (const e of B.engines) {
    if (e.id === "flowseal" || $("#engine-" + e.id)) continue;
    const card = document.createElement("div");
    card.className = "card tile engine";
    card.id = "engine-" + e.id;
    const head = document.createElement("div");
    head.className = "tile-head";
    const h = document.createElement("span");
    h.className = "tile-name";
    h.appendChild(engDot(e.id));
    h.appendChild(document.createTextNode(e.label));
    const chip = document.createElement("span");
    chip.className = "chip";
    chip.textContent = e.exe || "";
    const st = document.createElement("span");
    st.className = "state-chip";
    st.id = "engState-" + e.id;
    head.appendChild(h);
    head.appendChild(chip);
    head.appendChild(st);
    card.appendChild(head);
    const sub = document.createElement("p");
    sub.className = "muted tile-note";
    sub.textContent = T.engine_place_note;
    card.appendChild(sub);
    const root = document.createElement("div");
    root.className = "root-row";
    root.id = "engRoot-" + e.id;
    card.appendChild(root);
    wrap.appendChild(card);
  }
  // Метки-кружки и у статичных карточек (flowseal в HTML) — по id движка.
  for (const e of B.engines) {
    const nameEl = document.querySelector("#engine-" + CSS.escape(e.id) + " .tile-name");
    if (nameEl && !nameEl.querySelector(".eng-dot")) nameEl.prepend(engDot(e.id));
  }
}

// ------------------------------------------------------------- profiles

// Сигнатуры последней отрисовки: bootstrap опрашивается каждые 4 с, а полная
// перерисовка списков (25+ плиток, дерево обновлений) на каждом опросе съедала
// кадры и «подвешивала» окно. Перерисовываем только когда данные реально изменились.
let sigProfiles = "";
let sigUpdates = "";
let sigSettings = "";
let sigTestResults = "";
// Фильтр результатов теста по движку: null — все, иначе id движка.
let testEngineFilter = null;

/// Общий конструктор сегментных табов по движкам (как в «Стратегиях»).
/// `current` — активный фильтр ("all" или id движка), `onPick(filter)` — реакция.
function buildEngineTabs(wrap, current, onPick, includeAll = true) {
  if (!wrap || !B || !B.engines) return;
  wrap.innerHTML = "";
  const mk = (filter, label, ready) => {
    const t = document.createElement("button");
    t.className = "tab" + (current === filter ? " active" : "");
    t.dataset.filter = filter;
    t.textContent = label;
    if (filter !== "all") t.prepend(engDot(filter));
    if (ready === false) {
      const dot = document.createElement("span");
      dot.className = "tab-dot";
      dot.title = T.tab_not_installed;
      t.appendChild(dot);
    }
    t.addEventListener("click", () => onPick(filter));
    wrap.appendChild(t);
  };
  if (includeAll) mk("all", T.filter_all);
  for (const e of B.engines) mk(e.id, e.label, e.ready);
}

// Сигнатуры табов: перерисовываем только при смене набора движков/готовности/фильтра.
let tabsSigProfiles = "";
let tabsSigTests = "";

/// Табы движков на вкладке «Стратегии».
function renderProfileTabs() {
  const wrap = $("#profileTabs");
  if (!wrap || !B.engines) return;
  const sig = B.engines.map((e) => `${e.id}:${e.ready ? 1 : 0}`).join(",") + "|" + profileFilter;
  if (sig === tabsSigProfiles) return;
  tabsSigProfiles = sig;
  buildEngineTabs(wrap, profileFilter, (filter) => {
    profileFilter = filter;
    renderProfileTabs();
    renderProfiles();
  });
}

/// Табы движков на вкладке «Тест стратегий» — фильтруют и кандидатов, и результаты.
function renderTestTabs() {
  const wrap = $("#testTabs");
  if (!wrap || !B.engines) return;
  const sig = B.engines.map((e) => `${e.id}:${e.ready ? 1 : 0}`).join(",") + "|" + (testEngineFilter || "all");
  if (sig === tabsSigTests) return;
  tabsSigTests = sig;
  buildEngineTabs(wrap, testEngineFilter || "all", (filter) => {
    testEngineFilter = filter === "all" ? null : filter;
    // Смена движка сбрасывает выбор стратегий: иначе прогон ушёл бы по id
    // прошлого движка, а результаты отфильтровались бы по новому (пусто).
    testPicked = null;
    renderTestTabs();
    renderTestCard();
    renderTestResults();
  });
}

// Таймер отложенного автосохранения настроек (см. «Настройки»).
let saveSettingsTimer = 0;

function renderProfiles() {
  const list = $("#profileList");
  // Сигнатура — всё, от чего зависят плитки: профили, runtime, busy, настройки
  // (автозапуск), служба и результаты теста (счёт/«лучшая»). Раньше часть полей
  // не входила в sig, и чипы «автозапуск»/«служба»/счёт не обновлялись.
  const sig = JSON.stringify([
    profileFilter,
    B.profiles,
    B.runtime,
    B.busy,
    B.opRunning,
    B.settings,
    B.service,
    testCache && testCache.bestId,
    testCache && testCache.testedAt,
  ]);
  if (sig === sigProfiles) return;
  sigProfiles = sig;
  list.innerHTML = "";
  const profs = (B.profiles || []).filter((p) => profileFilter === "all" || p.engine === profileFilter);
  if (!profs.length) {
    list.innerHTML = `<div class="empty">${T.profiles_empty}</div>`;
    return;
  }
  const autoProfile = B.settings && B.settings.autostart_profile;
  const bestId = testCache && testCache.bestId;
  for (const p of profs) {
    const tile = document.createElement("div");
    tile.className = "profile";
    const isRun =
      (B.runtime && B.runtime.alive && B.runtime.profileId === p.id) ||
      (B.service && B.service.running === true && B.service.strategy === p.id);
    const opRunning = !!B.opRunning;
    if (isRun) tile.classList.add("running");
    if (bestId === p.id) tile.classList.add("best");

    // Название.
    const nm = document.createElement("div");
    nm.className = "profile-name";
    nm.textContent = p.name;
    nm.title = p.name;
    tile.appendChild(nm);

    // Чипы: движок, лучшая, счёт, шаблон, автозапуск.
    const chips = document.createElement("div");
    chips.className = "profile-chips";
    const eng = document.createElement("span");
    eng.className = "chip";
    eng.appendChild(engDot(p.engine));
    eng.appendChild(document.createTextNode(p.engine === "flowseal" ? "winws" : engineLabel(p.engine)));
    chips.appendChild(eng);
    if (bestId === p.id) {
      const b = document.createElement("span");
      b.className = "chip best";
      b.textContent = T.chip_best;
      chips.appendChild(b);
    }
    const cached = testCache && (testCache.results || []).find((r) => r.id === p.id);
    if (cached) {
      const s = document.createElement("span");
      s.className = "chip score";
      s.textContent = `${cached.score}/${cached.maxScore}`;
      chips.appendChild(s);
    }
    if (p.builtin) {
      const b = document.createElement("span");
      b.className = "chip";
      b.textContent = T.chip_builtin;
      chips.appendChild(b);
    }
    if (autoProfile === p.id) {
      const b = document.createElement("span");
      b.className = "chip best";
      b.textContent = T.chip_autostart;
      chips.appendChild(b);
    }
    if (B.service && B.service.installed && B.service.strategy === p.id) {
      const b = document.createElement("span");
      b.className = "chip best";
      b.textContent = T.chip_service;
      chips.appendChild(b);
    }
    tile.appendChild(chips);

    // Кнопки.
    const acts = document.createElement("div");
    acts.className = "profile-actions";
    const runBtn = btn(isRun ? T.btn_stop : T.btn_start, "primary", async () => {
      const doStart = async () => {
        btnBusy(runBtn, true);
        try {
          if (isRun) {
            await invoke("stop_running");
          } else {
            await invoke("start_profile", { id: p.id });
          }
          await refreshAll();
        } catch (e) {
          toast("err", String(e));
        }
        btnBusy(runBtn, false);
      };
      await doStart();
    });
    runBtn.disabled = opRunning;
    acts.appendChild(runBtn);
    const more = btn("⋯", "ghost", () => openProfileModal(p));
    more.classList.add("profile-more");
    more.title = T.profile_args_title;
    acts.appendChild(more);
    tile.appendChild(acts);

    list.appendChild(tile);
  }
}

// ---- модалка параметров стратегии ----
let pmProfile = null;

function openProfileModal(p) {
  pmProfile = p;
  $("#pmTitle").textContent = p.name;
  const bits = [];
  bits.push(T.pm_engine(p.engine === "flowseal" ? "winws" : engineLabel(p.engine)));
  if (p.updatedAt) bits.push(T.pm_updated(tsText(Number(p.updatedAt))));
  if (p.source) bits.push(p.source);
  $("#pmMeta").textContent = bits.join(" · ");
  $("#pmArgs").textContent = (p.args || []).join("\n");
  $("#profileModal").classList.remove("hidden");
}

function closeProfileModal() {
  pmProfile = null;
  $("#profileModal").classList.add("hidden");
}

// ------------------------------------------------------------- test

function flowsealProfiles() {
  // Все профили УСТАНОВЛЕННЫХ движков (стратегии для теста).
  // B может быть null в момент перезагрузки (zgui:updates) — не роняем рендер.
  const engines = ((B && B.engines) || []).filter((e) => e.ready).map((e) => e.id);
  return ((B && B.profiles) || []).filter((p) => engines.includes(p.engine));
}

/// Профили, видимые на вкладке тестов с учётом таба по движку.
function visibleTestProfiles() {
  const all = flowsealProfiles();
  return testEngineFilter ? all.filter((p) => p.engine === testEngineFilter) : all;
}

let sigTestCard = "";

function renderTestCard() {
  const pick = $("#testPick");
  if (!pick) return;
  const all = flowsealProfiles();
  // Перерисовываем только при реальном изменении (таб, набор стратегий, флаги,
  // ход теста, кэш) — иначе каждый refreshAll пересоздавал бы список и сбрасывал
  // прокрутку/фокус.
  const sig = JSON.stringify([
    testEngineFilter,
    testPicked === null ? null : Array.from(testPicked).sort(),
    all.map((p) => `${p.id}:${p.engine}`),
    testState && testState.running,
    testCache && testCache.bestId,
    ((testCache && testCache.results) || []).map((r) => `${r.id}:${r.score}`),
  ]);
  if (sig === sigTestCard) return;
  sigTestCard = sig;
  renderTestTabs();
  if (!all.length) {
    pick.innerHTML = `<div class="empty">${T.test_no_engines}</div>`;
    $("#btnRunTest").disabled = true;
    return;
  }
  // Таб движка фильтрует список кандидатов (как во вкладке «Стратегии»).
  const profs = testEngineFilter ? all.filter((p) => p.engine === testEngineFilter) : all;
  if (!profs.length) {
    pick.innerHTML = `<div class="empty">${T.test_no_strategies}</div>`;
    $("#btnRunTest").disabled = true;
    return;
  }
  $("#btnRunTest").disabled = !!(testState && testState.running);
  if ($("#btnStopTest")) $("#btnStopTest").disabled = !(testState && testState.running);
  const ids = profs.map((p) => p.id);
  $("#testSelectAll").checked = testPicked === null || ids.every((id) => testPicked.has(id));
  pick.innerHTML = "";
  const best = testCache && testCache.bestId;
  for (const p of profs) {
    const wrap = document.createElement("label");
    wrap.className = "test-item";
    const cb = document.createElement("input");
    cb.type = "checkbox";
    cb.checked = testPicked === null || testPicked.has(p.id);
    cb.addEventListener("change", () => {
      if (testPicked === null) {
        testPicked = new Set(profs.map((x) => x.id));
      }
      if (cb.checked) testPicked.add(p.id);
      else testPicked.delete(p.id);
      $("#testSelectAll").checked = profs.every((x) => testPicked.has(x.id));
    });
    wrap.appendChild(cb);
    const nm = document.createElement("span");
    nm.className = "test-name";
    nm.textContent = p.name;
    wrap.appendChild(nm);
    if (p.id === best) {
      const b = document.createElement("span");
      b.className = "chip best";
      b.textContent = T.chip_best;
      wrap.appendChild(b);
    }
    const cached = testCache && (testCache.results || []).find((r) => r.id === p.id);
    if (cached) {
      const s = document.createElement("span");
      s.className = "chip score";
      s.textContent = `${cached.score}/${cached.maxScore}`;
      wrap.appendChild(s);
    }
    pick.appendChild(wrap);
  }
  renderTestResults();
}

function renderTestProgress() {
  const bar = $("#testProgBar");
  if (!testState) {
    bar.classList.add("hidden");
    return;
  }
  if (testState.running || testState.phase !== "done") {
    bar.classList.remove("hidden");
    $("#testProgFill").style.width = Math.max(0, Math.min(100, testState.pct || 0)) + "%";
    const cur = testState.currentName ? `: ${testState.currentName}` : "";
    $("#testProgLabel").textContent = `${testState.msg || ""}${cur} (${testState.index}/${testState.total})`;
  } else {
    bar.classList.add("hidden");
  }
}

function renderTestResults() {
  const box = $("#testResults");
  const bestBox = $("#testBest");
  if (!box) return;
  // Результаты: свежий прогон важнее кэша; ключ — id профиля.
  const byId = new Map();
  for (const r of (testCache && testCache.results) || []) byId.set(r.id, r);
  for (const r of (testState && testState.results) || []) byId.set(r.id, r);
  // Строки — ВСЕ стратегии текущего вида (движок или «Все»), а не только
  // последний прогон: непроверенные показываем серым «не тестировалась».
  const rows = visibleTestProfiles().map((p) => ({ p, r: byId.get(p.id) || null }));
  const tested = rows.filter((x) => x.r);
  const untested = rows.filter((x) => !x.r);
  tested.sort(
    (a, b) =>
      b.r.score - a.r.score ||
      (b.r.criticalOk ? 1 : 0) - (a.r.criticalOk ? 1 : 0) ||
      a.p.name.localeCompare(b.p.name),
  );
  // Прогресс теста прилетает каждые ~0.7 с — перерисовываем список только при
  // реальном изменении набора строк (иначе лишние тысячи DOM-узлов на кадр).
  const sig = JSON.stringify([
    testEngineFilter,
    (testState && testState.done) || false,
    (testState && testState.bestId) || "",
    (testCache && testCache.bestId) || "",
    rows.map((x) => x.p.id + (x.r ? `:${x.r.score}:${x.r.criticalOk ? 1 : 0}:${x.r.started ? 1 : 0}` : "")),
  ]);
  if (sig === sigTestResults) return;
  sigTestResults = sig;
  if (!rows.length) {
    box.innerHTML = "";
    bestBox.classList.add("hidden");
    return;
  }
  // «Лучшая» показывается среди текущего вида (движка/фильтра). Если прогон
  // не оставил bestId (отмена/мелкий прогон), берём лучшую из УЖЕ проверенных
  // видимых стратегий: иначе плашка «ничего не подошло» противоречила списку,
  // где успешные стратегии видны из кэша (жалоба юзера).
  const fallbackBestId =
    [...tested].sort(
      (a, b) =>
        b.r.score - a.r.score ||
        (b.r.criticalOk ? 1 : 0) - (a.r.criticalOk ? 1 : 0) ||
        a.p.name.localeCompare(b.p.name),
    ).find((x) => x.r.criticalOk)?.p.id || null;
  const bestId = (testState && testState.bestId) || (testCache && testCache.bestId) || fallbackBestId;
  const bestInView = bestId && tested.some((x) => x.r.id === bestId) ? bestId : null;
  const bestRes = bestInView ? byId.get(bestInView) : null;
  const runDone = (testState && testState.done) || false;
  const wasStopped = !!(testState && testState.stopped);
  // Плашка «лучшая» показывается ВСЕГДА, пока среди видимых стратегий есть
  // успешные (прогон, кэш или остановленный тест) — иначе после остановки
  // она пропадала, хотя 4 из 5 стратегий были идеальны (жалоба юзера).
  if (bestRes) {
    bestBox.classList.remove("hidden");
    bestBox.innerHTML = "";
    const wins = document.createElement("div");
    wins.className = "test-wins muted";
    wins.textContent = T.wins_summary(tested.filter((x) => x.r.criticalOk).length, tested.length);
    bestBox.appendChild(wins);
    const t = document.createElement("div");
    t.className = "test-best-title";
    t.textContent = T.test_best(bestRes.name);
    bestBox.appendChild(t);
    const b = btn(T.btn_apply_best, "primary small", async () => {
      btnBusy(b, true);
      try {
        await invoke("apply_best_strategy", { id: bestInView });
        toast("ok", T.test_best_applied);
      } catch (e) {
        toast("err", String(e));
      }
      btnBusy(b, false);
      await refreshAll();
    });
    bestBox.appendChild(b);
  } else if (runDone && tested.length && !tested.some((x) => x.r.criticalOk)) {
    // Ни одна из проверенных не прошла ключевые сайты — только тогда «не подошла».
    bestBox.classList.remove("hidden");
    bestBox.innerHTML = "";
    const t = document.createElement("div");
    t.className = "test-best-title muted";
    t.textContent =
      T.test_no_success
      + (testEngineFilter ? " " + T.test_no_success_hint + "." : "")
      + (wasStopped ? " " + T.test_stopped_note + "." : "");
    bestBox.appendChild(t);
  } else {
    bestBox.classList.add("hidden");
  }

  box.innerHTML = "";
  for (const { p, r } of [...tested, ...untested]) {
    if (!r) {
      // Стратегия ещё не тестировалась — строка-заглушка, чтобы «Все» реально
      // показывал весь список, а не один последний прогон.
      const row = document.createElement("div");
      row.className = "test-row muted";
      const head = document.createElement("div");
      head.className = "test-row-head";
      const nm = document.createElement("span");
      nm.className = "test-row-name";
      nm.textContent = p.name;
      head.appendChild(nm);
      const grp = document.createElement("span");
      grp.className = "chip";
      grp.appendChild(engDot(p.engine));
      grp.appendChild(document.createTextNode(engineLabel(p.engine) || p.engine));
      head.appendChild(grp);
      const sc = document.createElement("span");
      sc.className = "test-row-score";
      sc.textContent = T.test_untested;
      head.appendChild(sc);
      row.appendChild(head);
      box.appendChild(row);
      continue;
    }
    const row = document.createElement("div");
    row.className = "test-row " + (r.criticalOk ? "ok" : r.started && r.score > 0 ? "part" : "bad");
    const head = document.createElement("div");
    head.className = "test-row-head";
    const nm = document.createElement("span");
    nm.className = "test-row-name";
    nm.textContent = r.name;
    head.appendChild(nm);
    const grp = document.createElement("span");
    grp.className = "chip";
    grp.appendChild(engDot(r.engine));
    grp.appendChild(document.createTextNode(engineLabel(r.engine) || r.engine));
    head.appendChild(grp);
    const sc = document.createElement("span");
    sc.className = "test-row-score";
    sc.textContent = r.started ? (r.criticalOk ? T.test_ok : T.test_crit_fail) : T.test_not_started;
    head.appendChild(sc);
    row.appendChild(head);
    if (r.error) {
      const er = document.createElement("div");
      er.className = "muted";
      er.style.fontSize = "11px";
      er.textContent = r.error;
      row.appendChild(er);
    }
    const groups = (r.groups || []).filter((g) => g.id !== "youtube-music");
    if (groups.length) {
      const summary = document.createElement("div");
      summary.className = "test-group-summary";
      for (const g of groups.sort((a, b) => a.priority - b.priority)) {
        const chip = document.createElement("span");
        chip.className = `group-chip ${g.ok ? "ok" : "bad"} ${g.critical ? "critical" : "secondary"}`;
        chip.textContent = `${g.label}: ${g.passed}/${g.total}`;
        // Точный список упавших доменов — чтобы «не прошли крит. домены» не было
        // приговором без доказательств.
        const badHosts = (r.domains || []).filter((d) => !d.ok && d.group === g.id).map((d) => d.host);
        chip.title = badHosts.length
          ? T.test_bad_hosts(badHosts.join(", "))
          : T.test_all_hosts_ok;
        chip.appendChild(chipMark(g.ok));
        summary.appendChild(chip);
      }
      row.appendChild(summary);
    }
    if (r.domains && r.domains.length) {
      const details = document.createElement("details");
      details.className = "test-details";
      details.dataset.id = r.id;
      if (openTestDetails.has(r.id)) details.open = true;
      trackDetails(details, openTestDetails);
      const summary = document.createElement("summary");
      summary.textContent = T.test_details(r.score, r.maxScore);
      details.appendChild(summary);
      const doms = document.createElement("div");
      doms.className = "test-doms";
      for (const d of r.domains) {
        const chip = document.createElement("span");
        chip.className = "dom " + (d.ok ? "ok" : "bad");
        chip.textContent = `${d.host} `;
        chip.appendChild(chipMark(d.ok));
        chip.title = `${d.detail} · ${d.ms}ms`;
        doms.appendChild(chip);
      }
      details.appendChild(doms);
      row.appendChild(details);
    }
    box.appendChild(row);
  }
}

/// Предупреждение в диалогах запуска теста: прогон стратегии может
/// кратко оборвать интернет (движок перехватывает трафик), это нормально.
const NET_HINT_HTML = T.test_net_hint;

async function runTest(already) {
  const profs = visibleTestProfiles();
  // Учитываем текущий вид (движок/таб): если выбор пуст или содержит id не из
  // текущего вида, берём все видимые стратегии. Иначе тест уходил бы по чужим id.
  let useIds = testPicked === null ? profs.map((p) => p.id) : [...testPicked].filter((id) => profs.some((p) => p.id === id));
  if (!useIds.length) useIds = profs.map((p) => p.id);
  if (!useIds.length) {
    logWrite("warn", "ui", T.nothing_selected, "W-UI-001");
    toast("warn", T.nothing_selected);
    return;
  }
  const btn = $("#btnRunTest");
  // Взаимная блокировка: тест не запускается параллельно с другой операцией.
  if (B && B.opRunning) {
    logWrite("warn", "ui", T.test_busy_other, "W-UI-002");
    toast("warn", T.test_busy_other);
    return;
  }
  if (!already) {
    const ok = await showConfirm({
      title: T.test_confirm_title,
      okLabel: T.btn_run,
      cancelLabel: T.btn_cancel,
      okKind: "warn",
      // 3 секунды на «прочитать риски»: кнопка заблокирована с заливкой-шкалой.
      lockSec: 3,
      html:
        T.test_confirm_html(useIds.length) +
        NET_HINT_HTML,
    });
    if (!ok) return;
  }
  btnBusy(btn, true);
  try {
    // VPN не блокирует тест, но с ним результаты недостоверны — предупреждаем.
    const vpn = await invoke("vpn_check").catch(() => []);
    if (Array.isArray(vpn) && vpn.length) {
      logWrite("warn", "test", T.vpn_warn, "W-TEST-009");
      toast("warn", T.vpn_warn);
    }
    await invoke("test_strategies", { ids: useIds });
    toast("info", T.test_started);
  } catch (e) {
    toast("err", String(e));
    btnBusy(btn, false);
  }
}

async function loadTestCache() {
  try {
    testCache = await invoke("test_cache");
    renderTestCard();
  } catch (_) {}
}

// ------------------------------------------------------------- updates

// Индикация «что-то происходит» для обновлений: проверка конфигов идёт в фоне
// (до десятков секунд), поэтому показываем бегущую полосу и блокируем кнопки —
// иначе непонятно, нажалась ли кнопка вообще.
let updatesBusyTimer = 0;

function setUpdatesBusy(label, timeoutMs) {
  const bar = $("#progBar");
  if (bar) {
    bar.classList.remove("hidden");
    bar.classList.add("indeterminate");
    $("#progFill").style.width = "";
    $("#progLabel").textContent = label;
  }
  for (const id of ["#btnCheck", "#btnApplyAll", "#btnApplySel"]) {
    const b = $(id);
    if (b) { b.disabled = true; b.classList.add("busy"); }
  }
  clearTimeout(updatesBusyTimer);
  // Страховка: если событие почему-то не придёт, вернём кнопки.
  updatesBusyTimer = setTimeout(clearUpdatesBusy, timeoutMs);
}

function clearUpdatesBusy() {
  clearTimeout(updatesBusyTimer);
  const bar = $("#progBar");
  if (bar) {
    bar.classList.add("hidden");
    bar.classList.remove("indeterminate");
  }
  for (const id of ["#btnCheck", "#btnApplyAll", "#btnApplySel"]) {
    const b = $(id);
    if (b) { b.disabled = false; b.classList.remove("busy"); }
  }
}

// Одной кнопкой проверяем всё: конфиги и Telegram-мост.
async function doCheck() {
  setUpdatesBusy(T.upd_checking, 180000);
  try {
    await invoke("check_updates");
    await tgCheckBridge().catch(() => {});
  } catch (e) {
    clearUpdatesBusy();
    toast("err", String(e));
  }
}

async function doApply(ids) {
  setUpdatesBusy(T.upd_applying, 300000);
  try {
    await invoke("apply_updates", { ids });
    toast("info", T.upd_apply_started);
  } catch (e) {
    clearUpdatesBusy();
    toast("err", T.upd_apply_err(e));
  }
}

// --- Плитка «Обновление программы» ---
function renderAppUpdate(info) {
  const ver = $("#appVer");
  const stt = $("#appUpdState");
  const dl = $("#btnAppUpdDownload");
  if (!info) return;
  ver.textContent = info.current ? "v" + info.current : "";
  const avail = !!info.available;
  dl.classList.toggle("hidden", !avail);
  stt.textContent = info.error ? T.app_upd_err_short : avail ? T.app_upd_avail(info.latest) : T.app_upd_ok_short;
}

async function checkAppUpdate() {
  const b = $("#btnAppUpdCheck");
  btnBusy(b, true);
  try {
    const info = await invoke("app_update_info");
    renderAppUpdate(info);
    if (info.error) toast("err", T.app_upd_err(info.error));
    else if (info.available) toast("info", T.app_upd_latest(info.latest));
    else toast("ok", T.app_upd_ok);
  } catch (e) {
    toast("err", String(e));
  } finally {
    btnBusy(b, false);
  }
}

async function downloadAppUpdate() {
  const b = $("#btnAppUpdDownload");
  btnBusy(b, true);
  try {
    const path = String(await invoke("app_update_download"));
    const dir = path.replace(/[\\/][^\\/]*$/, "");
    await invoke("open_path", { path: dir }).catch(() => {});
    toast("ok", T.app_upd_saved(path));
  } catch (e) {
    toast("err", String(e));
  } finally {
    btnBusy(b, false);
  }
}

function renderUpdates(mode) {
  const entries = (B.updates || {}).entries || [];
  const sig = JSON.stringify([mode, entries]);
  if (sig === sigUpdates) return;
  sigUpdates = sig;
  const groups = {};
  for (const e of entries) (groups[e.group] = groups[e.group] || []).push(e);

  const upd = $("#updList");
  const sel = upd.querySelectorAll("input[type=checkbox]:checked");
  const keepSel = new Set([...sel].map((c) => c.dataset.id));
  upd.innerHTML = "";

  if (!entries.length) {
    const emptyText = (B.updates || {}).lastCheck
      ? T.upd_empty
      : T.upd_loading;
    upd.innerHTML = `<div class="empty">${emptyText}</div>`;
    return;
  }
  for (const [group, items] of Object.entries(groups)) {
    const available = items.filter((e) => ["avail", "new", "modified"].includes(e.status)).length;
    const errors = items.filter((e) => e.status === "err").length;
    const details = document.createElement("details");
    details.className = "update-group";
    details.dataset.id = group;
    if (openUpdateGroups.has(group)) details.open = true;
    trackDetails(details, openUpdateGroups);
    const summary = document.createElement("summary");
    const gt = document.createElement("span");
    gt.className = "group-title";
    gt.textContent = group;
    summary.appendChild(gt);
    const state = document.createElement("span");
    state.className = "group-state " + (errors ? "bad" : available ? "avail" : "ok");
    state.textContent = errors ? T.upd_group_err(errors) : available ? T.upd_group_avail(available) : T.st_ok;
    summary.appendChild(state);
    details.appendChild(summary);
    const content = document.createElement("div");
    content.className = "update-group-body";
    for (const e of items) {
      const row = document.createElement("div");
      row.className = "upd-row";
      const cb = document.createElement("input");
      cb.type = "checkbox";
      cb.dataset.id = e.id;
      cb.checked = keepSel.has(e.id);
      cb.disabled = !["avail", "new", "modified"].includes(e.status);
      row.appendChild(cb);

      const label = document.createElement("span");
      label.className = "upd-label";
      const nameTxt = document.createElement("span");
      nameTxt.textContent = e.label;
      label.appendChild(nameTxt);
      const cid = document.createElement("span");
      cid.className = "cid";
      cid.textContent = e.id;
      label.appendChild(cid);
      if (e.error) {
        const er = document.createElement("div");
        er.className = "muted";
        er.style.fontSize = "10px";
        er.textContent = e.error;
        label.appendChild(er);
      }
      if (e.added || e.removed) {
        const dl = document.createElement("span");
        dl.className = "upd-delta muted";
        dl.textContent = `+${e.added} −${e.removed}`;
        label.appendChild(dl);
      }
      row.appendChild(label);

      const sz = document.createElement("span");
      sz.className = "upd-size";
      sz.textContent = fmtSize(e.size);
      row.appendChild(sz);

      const st = document.createElement("span");
      st.className = "status " + e.status;
      st.textContent = statusLabel[e.status] || e.status;
      row.appendChild(st);
      content.appendChild(row);
    }
    details.appendChild(content);
    upd.appendChild(details);
  }
}

function updateProgress(ev) {
  const bar = $("#progBar");
  if (!ev || ev.phase === "done" || ev.phase === "error") {
    bar.classList.add("hidden");
    bar.classList.remove("indeterminate");
    clearUpdatesBusy();
    setTimeout(() => refreshAll(), 900);
    return;
  }
  // Пошли точные проценты (например установка движка) — переключаемся на полосу.
  bar.classList.remove("hidden", "indeterminate");
  $("#progFill").style.width = Math.max(0, Math.min(100, ev.pct)) + "%";
  $("#progLabel").textContent = (ev.msg || "") + (ev.pct > 0 ? ` (${ev.pct}%)` : "");
}

// ------------------------------------------------------------- settings

// Собирает настройки из формы. Используется и кнопкой «Сохранить», и точечными
// правками (например флажком «Всегда запускать от администратора» — он применяется сразу).
function collectSettings() {
  return {
    // Бэкенд хранит часы как u32: отрицательное или дробное значение ломало
    // сохранение настроек, поэтому подрезаем прямо здесь.
    update_interval_hours: Math.max(0, Math.floor(Number($("#cfInterval").value) || 0)),
    game_filter: $("#cfGameFilter").value,
    // Диапазоны Game Filter — фиксированные (кастомные поля убраны по решению).
    game_filter_tcp: "1024-65535",
    game_filter_udp: "1024-65535",
    ipset_mode: $("#cfIpset").value,
    filter_mode: $("#cfFilterMode").value,
    anticheat_pause: $("#cfAnticheat") ? $("#cfAnticheat").checked : false,
    autostart_mode: $("#cfAutostart").value ? "profile" : "none",
    autostart_profile: $("#cfAutostart").value || null,
    tg_autostart: $("#tgAutostart")?.checked || false,
    tg_offer: $("#tgOffer") ? $("#tgOffer").checked : (B && B.settings ? !!B.settings.tg_offer : false),
    tg_port: $("#tgPort")
      ? Number($("#tgPort").value) || 1443
      : (B && B.settings ? B.settings.tg_port || 1443 : 1443),
  };
}

function renderSettings() {
  const s = B.settings || {};
  // Не трогаем поля формы, если настройки не менялись: иначе перерисовка
  // затирала бы то, что пользователь печатает прямо сейчас.
  const sig = JSON.stringify([s, (B.profiles || []).map((p) => p.id), B.service]);
  if (sig === sigSettings) { renderDnsProviders(); return; }
  sigSettings = sig;
  $("#cfInterval").value = s.update_interval_hours ?? 72;
  $("#cfGameFilter").value = s.game_filter || "off";
  $("#cfIpset").value = s.ipset_mode || "loaded";
  $("#cfFilterMode").value = s.filter_mode || "auto";
  if ($("#cfAnticheat")) $("#cfAnticheat").checked = !!s.anticheat_pause;
  if ($("#tgAutostart")) $("#tgAutostart").checked = !!s.tg_autostart;
  if ($("#tgOffer")) $("#tgOffer").checked = !!s.tg_offer;
  if ($("#tgPort")) $("#tgPort").value = s.tg_port || 1443;
  renderDnsProviders();
}

// ------------------------------------------------------------- автозапуск

// Карточка «Автозапуск» на «Стратегиях»: один механизм — программа при входе,
// альтернатива — служба при загрузке ПК. Вместе нельзя.
let sigAutostart = "";

function renderAutostart() {
  const s = (B && B.settings) || {};
  const sel = $("#cfAutostart");
  if (!sel) return;
  // Перерисовываем только при реальных изменениях (иначе открытый список
  // схлопывался бы каждым опросом bootstrap).
  const sig = JSON.stringify([
    s.autostart_profile,
    B.service,
    (B.profiles || []).map((p) => [p.id, p.name]),
  ]);
  const svcInstalled = !!(B.service && B.service.installed);
  const svcRunning = !!(B.service && B.service.running === true);
  const chosenOld = sel.value;

  const box = $("#cfSvcBoot");
  if (sig !== sigAutostart) {
    sigAutostart = sig;
    const prev = chosenOld || s.autostart_profile || "";
    sel.innerHTML = `<option value="">${T.auto_none}</option>`;
    for (const p of B.profiles || []) {
      const o = document.createElement("option");
      o.value = p.id;
      o.textContent = p.name;
      sel.appendChild(o);
    }
    sel.value = prev;
  }
  // Стратегию можно менять и при включённой службе: служба переключится
  // (см. обработчик ниже).
  sel.disabled = false;
  sel.title = "";

  const chosen = sel.value;
  if (box) {
    // Галочка — по ФАКТУ: служба реально запущена, а не «когда-то ставилась».
    box.checked = svcRunning;
    box.disabled = !svcInstalled && !chosen;
    box.title = !svcInstalled && !chosen ? T.auto_pick_first : "";
  }

  const chip = $("#bootStateChip");
  if (chip) {
    chip.textContent = svcRunning ? T.chip_service : svcInstalled ? T.auto_chip_stopped : T.auto_chip_off;
    chip.className = "chip" + (svcRunning ? " best" : " off");
  }

  const note = $("#bootStateNote");
  if (note) {
    const profile = (B.profiles || []).find((p) => p.id === s.autostart_profile);
    if (svcRunning) {
      note.textContent = T.auto_note_service(profile ? profile.name : "");
    } else if (svcInstalled) {
      note.textContent = T.auto_note_stopped(profile ? profile.name : "");
    } else if (profile) {
      note.textContent = T.auto_note_chosen(profile.name);
    } else {
      note.textContent = T.auto_note_off;
    }
  }
}

function renderDnsProviders() {
  const sel = $("#dnsProvider");
  if (!sel || !dnsProviders.length) return;
  const prev = sel.value;
  sel.innerHTML = "";
  for (const p of dnsProviders) {
    const o = document.createElement("option");
    o.value = p.id;
    const ms = dnsBench && dnsBench[p.id];
    const ping = ms != null ? T.dns_ms(ms) : "";
    o.textContent = `${p.name} · ${p.primary} / ${p.secondary}${ping}`;
    sel.appendChild(o);
  }
  sel.value = prev || "cloudflare";
  renderDnsInfo();
}

function renderDnsInfo() {
  const p = dnsProviders.find((x) => x.id === $("#dnsProvider").value);
  const info = $("#dnsInfo");
  const desc = $("#dnsDesc");
  if (!p || !info) return;
  const ms = dnsBench && dnsBench[p.id];
  const ping = ms != null ? T.dns_ping(ms) : "";
  info.textContent = T.dns_info(p.note, ping, p.dohTemplate);
  if (desc) {
    desc.textContent = p.description || "";
    desc.classList.toggle("hidden", !p.description);
  }
}

async function runDnsBenchmark() {
  const btn = $("#btnDnsBench");
  const out = $("#dnsBench");
  if (!btn || !out) return;
  btnBusy(btn, true);
  out.classList.remove("hidden");
  out.textContent = T.dns_bench_running;
  try {
    const res = await invoke("dns_benchmark", {});
    dnsBench = {};
    for (const r of res || []) {
      if (r.avgMs != null) dnsBench[r.id] = r.avgMs;
    }
    // Таблица результатов, отсортированная по задержке.
    const rows = (res || [])
      .slice()
      .sort((a, b) => (a.avgMs ?? 1e9) - (b.avgMs ?? 1e9))
      .map((r) => {
        const name = dnsProviders.find((p) => p.id === r.id)?.name || r.id;
        const val = r.avgMs != null ? T.dns_ms_val(r.avgMs) : (r.error || T.dns_na);
        return `<div class="dns-bench-row"><span>${name}</span><span>${val}</span></div>`;
      })
      .join("");
    out.innerHTML = `<div class="dns-bench-note">${T.dns_bench_title}</div>${rows}`;
    renderDnsProviders();
  } catch (e) {
    out.textContent = T.dns_bench_fail(e);
  } finally {
    btnBusy(btn, false);
  }
}

async function loadDnsProviders() {
  try {
    dnsProviders = await invoke("dns_providers");
    renderDnsProviders();
  } catch (e) {
    const info = $("#dnsInfo");
    if (info) info.textContent = T.dns_load_fail(e);
  }
}

// ------------------------------------------------------------- misc

function tsText(unixts) {
  if (!unixts) return "";
  return new Date(unixts * 1000).toLocaleString("ru-RU", {
    day: "2-digit",
    month: "2-digit",
    year: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function btn(label, cls, onClick) {
  const b = document.createElement("button");
  b.className = "btn " + cls;
  b.textContent = label;
  if (typeof onClick === "function") b.addEventListener("click", onClick);
  return b;
}

// ------------------------------------------------------------- telegram

let tgState = { running: false };

async function refreshTg() {
  try {
    tgState = await invoke("tg_status");
  } catch (_) {
    tgState = { running: false };
  }
  renderTg();
  tgCheckBridge();
}

async function tgCheckBridge() {
  const el = $("#tgBridge");
  if (!el) return;
  el.textContent = T.tg_bridge_checking;
  el.className = "muted";
  try {
    const info = await invoke("tg_check_update");
    if (info.updateAvailable) {
      el.textContent = T.tg_bridge_update(info.upstreamVersion, info.localVersion);
      el.className = "muted warn";
    } else {
      el.textContent = T.tg_bridge_ok(info.localVersion);
      el.className = "muted";
    }
  } catch (e) {
    el.textContent = T.err_short(e);
    el.className = "muted err";
  }
}

function renderTg() {
  const badge = $("#tgBadge");
  const toggle = $("#btnTgToggle");
  const connect = $("#btnTgConnect");
  const hint = $("#tgHint");
  if (!badge) return;
  const on = !!tgState.running;
  badge.textContent = on ? T.tg_on : T.tg_off;
  badge.classList.toggle("ok", on);
  toggle.textContent = on ? T.btn_tg_off : T.btn_tg_on;
  toggle.classList.toggle("danger", on);
  connect.disabled = !on;
  if (tgState.error) {
    hint.textContent = T.err_short(tgState.error);
    hint.className = "muted err";
  } else if (on) {
    hint.textContent = T.tg_ready(tgState.port);
    hint.className = "muted";
  } else {
    hint.textContent = T.tg_hint_default;
    hint.className = "muted";
  }
  if (on && tgState.port && $("#tgPort")) $("#tgPort").value = tgState.port;
}

async function tgSavePrefs() {
  try {
    const cfg = B.settings || {};
    await invoke("set_settings", {
      settings: {
        ...cfg,
        tg_autostart: $("#tgAutostart")?.checked || false,
        tg_offer: $("#tgOffer") ? $("#tgOffer").checked : false,
        tg_port: $("#tgPort") ? Number($("#tgPort").value) || 1443 : (B && B.settings ? B.settings.tg_port : 1443) || 1443,
      },
    });
  } catch (e) {
    toast("err", String(e));
    refreshAll();
  }
}

async function tgToggle() {
  btnBusy($("#btnTgToggle"), true);
  try {
    if (tgState.running) {
      tgState = await invoke("tg_stop");
      toast("info", T.tg_stopped);
    } else {
      const port = Number($("#tgPort")?.value) || 1443;
      tgState = await invoke("tg_start", { port });
      toast("ok", T.tg_started);
    }
  } catch (e) {
    toast("err", String(e));
  } finally {
    btnBusy($("#btnTgToggle"), false);
    renderTg();
  }
}

/// Неблокирующее предложение Telegram-моста: Telegram запущен, VPN нет.
/// Галочку «предлагать» по умолчанию держим выключенной (см. Settings::tg_offer).
let tgOfferShown = false;
async function checkTgOffer() {
  if (tgOfferShown) return;
  const s = (B && B.settings) || {};
  if (!s.tg_offer) return;
  let offer = false;
  try { offer = await invoke("tg_offer"); } catch (_) { return; }
  if (!offer) return;
  tgOfferShown = true;
  const ok = await showConfirm({
    title: T.tg_title,
    okLabel: T.btn_tg_build,
    cancelLabel: T.btn_not_now,
    html: T.tg_offer_html,
  });
  if (!ok) return;
  try {
    const port = Number($("#tgPort")?.value) || (B && B.settings && B.settings.tg_port) || 1443;
    tgState = await invoke("tg_start", { port });
    renderTg();
    if (tgState.link) await invoke("open_url", { url: tgState.link });
    toast("ok", T.tg_started);
  } catch (e) {
    toast("err", String(e));
  }
}

async function tgConnect() {
  if (!tgState.link) {
    logWrite("warn", "telegram", T.tg_need_on, "W-UI-004");
    toast("warn", T.tg_need_on);
    return;
  }
  try {
    await invoke("open_url", { url: tgState.link });
    toast("info", T.tg_opening);
  } catch (e) {
    // Fallback: скопировать ссылку в буфер.
    try {
      await navigator.clipboard.writeText(tgState.link);
      toast("ok", T.tg_link_copied);
    } catch (_) {
      toast("err", String(e));
    }
  }
}

// ------------------------------------------------------------- wiring

// Страница «О программе»: RU-блок, разделитель, EN-блок. Версия — из бэкенда,
// содержимое кэшируем: за сессию оно не меняется.
async function renderAbout() {
  const el = $("#aboutBody");
  if (!el || el.dataset.ready) return;
  let ver = "";
  try {
    const info = await invoke("app_info");
    ver = (info && info.version) || "";
  } catch (_) {}
  el.innerHTML = T.about_ru_html(ver) + '<hr class="about-sep" />' + T.about_en_html(ver);
  // Ссылки на авторов открываем системным браузером: навигация внутри WebView запрещена.
  el.querySelectorAll("a[data-url]").forEach((a) =>
    a.addEventListener("click", (e) => {
      e.preventDefault();
      invoke("open_external", { target: a.dataset.url }).catch((err) => toast("err", String(err)));
    }),
  );
  el.dataset.ready = "1";
}

// --- Диагностика сервиса (сканер) ---
let scanKind = "site";
let scanReport = null;
let scanBound = false;
let scanPhaseStart = 0;
let scanPhaseBudget = 85000;
let scanBarCap = 0; // прогресс шкалы TTL (%), от событий шагов
let scanTimerId = null;

const SCAN_VERDICT = {
  NoEffect: "scan_v_no_effect",
  Collateral: "scan_v_collateral",
  Covered: "scan_v_covered",
  NotCovered: "scan_v_not_covered",
  Unrelated: "scan_v_unrelated",
};

// Общий таймер/шкала сканера и взаимная блокировка кнопок: «Начать проверку»,
// «Подобрать TTL» и «Метод блокировки» не должны идти одновременно (иначе
// теряется интервал и перезапускается движок под другой операцией).
function scanBusy(on) {
  for (const id of ["#btnScanRun", "#btnScanTtl", "#btnScanMethod"]) {
    const b = $(id);
    if (b) b.disabled = on;
  }
}
function startScanBar(label, budget) {
  if (scanTimerId) { clearInterval(scanTimerId); scanTimerId = null; }
  scanPhaseStart = Date.now();
  scanPhaseBudget = budget || 0;
  scanBarCap = 0;
  const ph = $("#scanPhase");
  if (ph) { ph.className = "scan-phase"; ph.textContent = label; }
  const f = $("#scanFill");
  if (f) f.style.width = "0%";
  $("#scanProgress").classList.remove("hidden");
  const tick = () => {
    const sec = Math.max(0, Math.floor((Date.now() - scanPhaseStart) / 1000));
    $("#scanTimer").textContent = `${Math.floor(sec / 60)}:${String(sec % 60).padStart(2, "0")}`;
    if (budget) $("#scanFill").style.width = Math.min(((Date.now() - scanPhaseStart) / budget) * 100, 100) + "%";
  };
  tick();
  scanTimerId = setInterval(tick, 250);
}
function stopScanBar() {
  if (scanTimerId) { clearInterval(scanTimerId); scanTimerId = null; }
  $("#scanProgress").classList.add("hidden");
}

function bindScanner() {
  $$("#scanTabs .tab").forEach((t) =>
    t.addEventListener("click", () => {
      $$("#scanTabs .tab").forEach((x) => x.classList.remove("active"));
      t.classList.add("active");
      scanKind = t.dataset.scanKind;
      const proc = scanKind === "process";
      $("#scanSiteField").classList.toggle("hidden", scanKind !== "site");
      $("#scanProcField").classList.toggle("hidden", !proc);
      $("#scanProcAddr").classList.toggle("hidden", !proc);
      $("#btnScanRefresh").classList.toggle("hidden", !proc);
      $("#btnScanBrowse").classList.toggle("hidden", !proc);
      $("#btnScanMethod").classList.toggle("hidden", proc);
      $("#btnScanTtl").classList.toggle("hidden", proc);
    }),
  );
  $("#btnScanRefresh").addEventListener("click", refreshScannerList);
  $("#btnScanBrowse").addEventListener("click", browseScanner);
  $("#btnScanMethod").addEventListener("click", runMethodDiagnosis);
  $("#btnScanTtl").addEventListener("click", runTtlTune);
  $("#btnScanRun").addEventListener("click", runScanner);
  $("#btnScanCancel").addEventListener("click", cancelScanner);
  $("#btnScanSave").addEventListener("click", saveScannerReport);
  $("#btnScanApply").addEventListener("click", applyScannerReport);
  listen("zgui:scan", (e) => {
    const p = e.payload || {};
    const ph = $("#scanPhase");
    if (p.phase === "ttl") {
      if (ph && p.msg) ph.textContent = p.msg;
      if (p.total) {
        scanBarCap = Math.max(scanBarCap, Math.round(((p.done || 0) / p.total) * 100));
        $("#scanFill").style.width = scanBarCap + "%";
      }
    } else {
      scanPhaseStart = Date.now();
      if (ph) {
        ph.className = "scan-phase" + (p.phase ? " phase-" + p.phase : "");
        if (p.msg) ph.textContent = p.msg;
      }
    }
    const pr = $("#scanProgress");
    if (pr) pr.classList.remove("hidden");
  });
}

async function renderScanner() {
  if (!scanBound) { bindScanner(); scanBound = true; }
  applyTexts($("#view-scanner"));
  try {
    const p = await invoke("scanner_page");
    const sel = $("#scanStrategy");
    const prev = sel.value;
    sel.innerHTML = "";
    for (const s of p.strategies) {
      const o = document.createElement("option");
      o.value = s.id;
      o.textContent = s.name;
      if (p.current === s.id || (!p.current && prev === s.id)) o.selected = true;
      sel.appendChild(o);
    }
    const procs = await invoke("scanner_processes");
    const plist = $("#scanProcList");
    const prevProc = plist.value;
    plist.innerHTML = "";
    for (const name of procs) {
      const o = document.createElement("option");
      o.value = name;
      o.textContent = name;
      plist.appendChild(o);
    }
    if (prevProc) plist.value = prevProc;
  } catch (e) { toast("err", String(e)); }
}

async function loadScanProcesses() {
  const procs = await invoke("scanner_processes");
  const plist = $("#scanProcList");
  const prevProc = plist.value;
  plist.innerHTML = "";
  for (const name of procs) {
    const o = document.createElement("option");
    o.value = name;
    o.textContent = name;
    plist.appendChild(o);
  }
  if (prevProc && procs.includes(prevProc)) plist.value = prevProc;
}

async function refreshScannerList() {
  const b = $("#btnScanRefresh");
  btnBusy(b, true);
  try { await loadScanProcesses(); toast("ok", T.scan_refreshed); }
  catch (e) { toast("err", String(e)); }
  finally { btnBusy(b, false); }
}

function scanProcName() {
  const sel = $("#scanProcList");
  return sel && sel.value ? sel.value : "";
}

function scanProcAddr() {
  return $("#scanTargetProc").value.trim();
}

async function browseScanner() {
  try {
    const p = await open({ multiple: false, filters: [{ name: "Программа", extensions: ["exe"] }] });
    if (!p) return;
    const name = String(p).split(/[\\/]/).pop().replace(/\.exe$/i, "");
    const sel = $("#scanProcList");
    if (![...sel.options].some((o) => o.value === name)) {
      const o = document.createElement("option");
      o.value = name;
      o.textContent = name + " (выбранный файл)";
      sel.appendChild(o);
    }
    sel.value = name;
    $("#scanTargetProc").value = "";
  } catch (e) { toast("err", String(e)); }
}

async function cancelScanner() {
  try {
    await invoke("scanner_cancel");
    toast("warn", T.scan_cancelling);
  } catch (e) { toast("err", String(e)); }
}

function renderMethod(m) {
  const el = $("#scanResult");
  el.textContent = "";
  const head = document.createElement("div");
  head.className = "scan-method";
  head.textContent = `${T.scan_method}: ${m.verdict}`;
  el.appendChild(head);
  const facts = document.createElement("div");
  facts.className = "muted";
  const tcp = { ok: "открыт", timeout: "таймаут", reset: "сброс", refused: "отказ", error: "ошибка" }[m.tcp] || m.tcp;
  facts.textContent = `IP: ${(m.ips || []).join(", ") || "—"} · TCP: ${tcp} · TLS: ${m.tls_ok ? "есть" : "нет"}`;
  el.appendChild(facts);
  const note = document.createElement("div");
  note.textContent = m.note;
  el.appendChild(note);
}

async function runMethodDiagnosis() {
  const target = $("#scanTargetSite").value.trim();
  if (!target) return toast("warn", T.scan_need_target);
  if ($("#btnScanRun").disabled) return; // уже идёт другая проверка
  const b = $("#btnScanMethod");
  btnBusy(b, true);
  scanBusy(true);
  try {
    const m = await invoke("scanner_method", { target });
    renderMethod(m);
  } catch (e) {
    toast("err", String(e));
  } finally {
    btnBusy(b, false);
    scanBusy(false);
  }
}

function renderTtl(r) {
  const el = $("#scanResult");
  el.textContent = "";
  const head = document.createElement("div");
  head.className = "scan-method";
  head.textContent = `${T.scan_ttl}: ${r.strategy}`;
  el.appendChild(head);
  for (const x of r.results || []) {
    const line = document.createElement("div");
    line.textContent = `TTL ${x.ttl}: ${x.ok ? `ok — ${x.ms} мс` : "не прошло"}`;
    el.appendChild(line);
  }
  const best = document.createElement("div");
  best.textContent = r.best != null ? `${T.scan_ttl_best}: ${r.best}` : T.scan_ttl_none;
  el.appendChild(best);
}

async function runTtlTune() {
  const target = $("#scanTargetSite").value.trim();
  if (!target) return toast("warn", T.scan_need_target);
  const sid = $("#scanStrategy").value;
  if (!sid) return toast("warn", T.scan_need_strategy);
  if ($("#btnScanRun").disabled) return; // уже идёт другая проверка
  const b = $("#btnScanTtl");
  btnBusy(b, true);
  scanBusy(true);
  startScanBar(T.scan_ttl, 0);
  try {
    const r = await invoke("scanner_ttl", { host: target, strategyId: sid });
    renderTtl(r);
  } catch (e) {
    toast("err", String(e));
  } finally {
    btnBusy(b, false);
    scanBusy(false);
    stopScanBar();
  }
}

async function runScanner() {
  const target = scanKind === "site" ? $("#scanTargetSite").value.trim() : (scanProcAddr() || scanProcName());
  if (!target) return toast("warn", scanKind === "site" ? T.scan_need_target : T.scan_need_proc);
  const html = scanKind === "process" ? T.scan_warn_proc : T.scan_warn;
  const go = await showConfirm({ title: T.scan_title, html, okLabel: T.scan_run, cancelLabel: T.btn_cancel });
  if (!go) return;
  if ($("#btnScanRun").disabled) return; // уже идёт другая проверка
  const b = $("#btnScanRun"); btnBusy(b, true);
  scanBusy(true);
  $("#btnScanCancel").classList.remove("hidden");
  $$(".nav-item").forEach((x) => { if (x.dataset.view !== "scanner") x.classList.add("disabled"); });
  $("#scanResult").textContent = T.scan_running;
  startScanBar(T.scan_searching, 85000);
  try {
    scanReport = await invoke("scanner_run", { kind: scanKind, target, strategyId: $("#scanStrategy").value, focus: $("#scanFocus").checked });
    renderScanReport(scanReport);
    $("#btnScanSave").disabled = false;
    $("#btnScanApply").disabled = !["Collateral", "NotCovered"].includes(scanReport.verdict);
  } catch (e) {
    toast("err", String(e));
    $("#scanResult").textContent = "";
  } finally {
    btnBusy(b, false);
    scanBusy(false);
    stopScanBar();
    $("#btnScanCancel").classList.add("hidden");
    $$(".nav-item").forEach((x) => x.classList.remove("disabled"));
  }
}

function renderScanReport(rep) {
  const el = $("#scanResult");
  el.textContent = "";
  const tg = document.createElement("div");
  tg.textContent = `${T.scan_target_short}: ${rep.target}`;
  el.appendChild(tg);
  const line = (label, m) => {
    const d = document.createElement("div");
    d.textContent = m.ms ? `${label}: ${m.detail} — ${m.ms} мс` : `${label}: ${m.detail}`;
    return d;
  };
  el.appendChild(line(T.scan_without, rep.without));
  el.appendChild(line(T.scan_with, rep.with));
  const v = document.createElement("div");
  v.innerHTML = `<b>${T.scan_verdict}:</b> ${T[SCAN_VERDICT[rep.verdict]] || rep.verdict}`;
  el.appendChild(v);
  if (rep.recommendation && rep.recommendation.note) {
    const r = document.createElement("div");
    r.textContent = `${T.scan_recommend}: ${rep.recommendation.note}`;
    el.appendChild(r);
  }
}

async function saveScannerReport() {
  if (!scanReport) return;
  const b = $("#btnScanSave"); btnBusy(b, true);
  try {
    const p = await invoke("scanner_report_save", { report: scanReport });
    toast("ok", T.scan_saved + " " + p);
  } catch (e) { toast("err", String(e)); }
  finally { btnBusy(b, false); }
}

async function applyScannerReport() {
  if (!scanReport) return;
  const b = $("#btnScanApply"); btnBusy(b, true);
  try {
    const msg = await invoke("scanner_apply", { report: scanReport });
    toast("ok", msg);
    $("#btnScanApply").disabled = true;
  } catch (e) { toast("err", String(e)); }
  finally { btnBusy(b, false); }
}

function bindStatic() {
  $$(".nav-item").forEach((n) =>
    n.addEventListener("click", () => {
      $$(".nav-item").forEach((x) => x.classList.remove("active"));
      n.classList.add("active");
      $$(".page").forEach((p) => p.classList.remove("active"));
      $("#view-" + n.dataset.view).classList.add("active");
      // Обновляем состояние при любом переходе: если событие zgui:op потерялось
      // (зависла/пропала метка «идёт операция…»), вкладка это вылечит.
      refreshAll();
      if (n.dataset.view === "updates") refreshAll();
      if (n.dataset.view === "telegram") refreshTg();
      if (n.dataset.view === "dns") loadDnsProviders();
      if (n.dataset.view === "appearance") applyTheme(currentTheme());
      if (n.dataset.view === "about") renderAbout();
      if (n.dataset.view === "scanner") renderScanner();
      if (n.dataset.view === "logs") openLogs();
    }),
  );

  $("#btnStop").addEventListener("click", async () => {
    const b = $("#btnStop");
    btnBusy(b, true);
    try {
      await invoke("stop_running");
      await refreshAll();
    } catch (e) {
      toast("err", String(e));
    } finally {
      btnBusy(b, false);
    }
  });

  $$("#themeGrid .theme-card").forEach((c) =>
    c.addEventListener("click", () => pickTheme(c.dataset.theme)),
  );

  if ($("#cfFxOff")) $("#cfFxOff").addEventListener("change", () => {
    const off = $("#cfFxOff").checked;
    document.body.classList.toggle("fx-off", off);
    try { localStorage.setItem("zgui.fx", off ? "off" : "on"); } catch (_) {}
  });

  if ($("#cfPatOff")) $("#cfPatOff").addEventListener("change", () => {
    const off = $("#cfPatOff").checked;
    document.body.classList.toggle("pat-off", off);
    try { localStorage.setItem("zgui.pat", off ? "off" : "on"); } catch (_) {}
  });

  if ($("#btnTgToggle")) $("#btnTgToggle").addEventListener("click", tgToggle);
  if ($("#btnTgConnect")) $("#btnTgConnect").addEventListener("click", tgConnect);
  if ($("#tgAutostart")) $("#tgAutostart").addEventListener("change", tgSavePrefs);
  if ($("#tgOffer")) $("#tgOffer").addEventListener("change", tgSavePrefs);
  if ($("#tgPort")) $("#tgPort").addEventListener("change", tgSavePrefs);

  if ($("#cmOk")) $("#cmOk").addEventListener("click", () => closeConfirm(true));

  if ($("#logLevels")) {
    $$("#logLevels .lvl-chip").forEach((c) =>
      c.addEventListener("click", () => {
        $$("#logLevels .lvl-chip").forEach((x) => x.classList.remove("active"));
        c.classList.add("active");
        logState.level = c.dataset.level;
        renderLog();
      }),
    );
    $("#logSearch").addEventListener("input", (e) => {
      logState.query = e.target.value;
      renderLog();
    });
    $("#btnLogOpenDir").addEventListener("click", () =>
      invoke("log_dir_open").catch((e) => toast("err", e.message)),
    );
    // --- Инструменты (журнал): кэш Discord, hosts, фейки ---
async function loadFakes() {
  const selD = $("#cfFakeDiscord");
  const selG = $("#cfFakeGame");
  if (!selD || !selG) return;
  try {
    const v = await invoke("fakes_view");
    const fill = (el, active) => {
      el.innerHTML = "";
      for (const f of v.files) {
        const o = document.createElement("option");
        o.value = f;
        o.textContent = f;
        if (active && f === active) o.selected = true;
        el.appendChild(o);
      }
      el.disabled = !v.files.length;
    };
    fill(selD, v.activeDiscord);
    fill(selG, v.activeGame);
  } catch (_) {}
}
loadFakes();

if ($("#btnDiscordCache"))
  $("#btnDiscordCache").addEventListener("click", async () => {
    const ok = await showConfirm({
      title: T.discord_cache_title,
      html: T.discord_cache_html,
      okLabel: T.btn_clear,
    });
    if (!ok) return;
    btnBusy($("#btnDiscordCache"), true);
    try {
      toast("ok", await invoke("discord_cache_clear"));
    } catch (e) {
      toast("err", e.message);
    } finally {
      btnBusy($("#btnDiscordCache"), false);
    }
  });
if ($("#btnHostsUpdate"))
  $("#btnHostsUpdate").addEventListener("click", async () => {
    btnBusy($("#btnHostsUpdate"), true);
    try {
      toast("ok", await invoke("hosts_update"));
      const p = ((B && B.dataDir) || "") + "\\catalog\\hosts-from-author.txt";
      invoke("open_path", { path: p }).catch(() => {});
      invoke("open_path", { path: "C:\\Windows\\System32\\drivers\\etc" }).catch(() => {});
    } catch (e) {
      toast("err", e.message);
    } finally {
      btnBusy($("#btnHostsUpdate"), false);
    }
  });
if ($("#btnFakeApply"))
  $("#btnFakeApply").addEventListener("click", async () => {
    btnBusy($("#btnFakeApply"), true);
    try {
      const msgs = [];
      msgs.push(await invoke("replace_fake", { kind: "discord", name: $("#cfFakeDiscord").value }));
      msgs.push(await invoke("replace_fake", { kind: "game", name: $("#cfFakeGame").value }));
      toast("ok", msgs.join("; "));
      loadFakes();
    } catch (e) {
      toast("err", e.message);
    } finally {
      btnBusy($("#btnFakeApply"), false);
    }
  });

$("#btnReportSave").addEventListener("click", async () => {
      btnBusy($("#btnReportSave"), true);
      try {
        const p = await invoke("report_save");
        toast("ok", T.report_saved(p));
      } catch (e) {
        toast("err", e.message);
      } finally {
        btnBusy($("#btnReportSave"), false);
      }
    });
    if ($("#btnReportIssue"))
      $("#btnReportIssue").addEventListener("click", async () => {
        // Сначала сохраняем отчёт (его можно приложить), затем открываем форму issue.
        let path = "";
        try {
          path = await invoke("report_save");
        } catch (_) {}
        const body = encodeURIComponent(T.issue_body(path));
        const url = `https://github.com/lECL1PS3l/zgui/issues/new?title=${encodeURIComponent(T.issue_title)}&body=${body}`;
        try {
          await invoke("open_external", { target: url });
          toast("ok", T.report_issue_opened);
        } catch (e) {
          toast("err", String(e));
        }
      });
  }
  if ($("#cmCancel")) $("#cmCancel").addEventListener("click", () => closeConfirm(false));
  if ($("#cmX")) $("#cmX").addEventListener("click", () => closeConfirm(false));

  if ($("#pmClose")) $("#pmClose").addEventListener("click", closeProfileModal);
  if ($("#pmClose2")) $("#pmClose2").addEventListener("click", closeProfileModal);

  $("#btnRunTest").addEventListener("click", () => runTest(false));
  $("#btnStopTest").addEventListener("click", async () => {
    btnBusy($("#btnStopTest"), true);
    try {
      await invoke("cancel_test");
      toast("info", T.test_stopping);
      await new Promise((r) => setTimeout(r, 2500));
      // Сохранившиеся частичные результаты живут в кэше: показываем их, а не
      // «не тестировалась» (жалоба «остановил — результаты исчезли»).
      testState = null;
      // Снимаем busy явно: у отменённого прогона финальное событие может
      // прийти позже/не прийти, а класс .busy блокирует клики по кнопке.
      btnBusy($("#btnRunTest"), false);
      await loadTestCache();
      renderTestProgress();
      await refreshAll();
    } catch (e) {
      toast("err", String(e));
    } finally {
      btnBusy($("#btnStopTest"), false);
    }
  });
  $("#testSelectAll").addEventListener("change", (e) => {
    const profs = visibleTestProfiles();
    testPicked = e.target.checked ? new Set(profs.map((p) => p.id)) : new Set();
    // Перерисовываем список: иначе галочки у отдельных стратегий остаются
    // в прежнем состоянии (снимаешь «выбрать все» — флаги «якобы» снимаются).
    renderTestCard();
  });
  $("#btnTestReport").addEventListener("click", async () => {
    // Результаты теста — отдельным файлом в журнал (для отправки/разбора).
    btnBusy($("#btnTestReport"), true);
    try {
      const p = await invoke("test_report_save");
      toast("ok", T.test_report_saved(p));
    } catch (e) {
      toast("err", T.test_report_fail(e));
    } finally {
      btnBusy($("#btnTestReport"), false);
    }
  });

  if ($("#btnTestFolder"))
    $("#btnTestFolder").addEventListener("click", async () => {
      try {
        await invoke("open_path", { path: ((B && B.dataDir) || "") + "\\logs" });
      } catch (e) {
        toast("err", String(e));
      }
    });

  $("#btnCheck").addEventListener("click", doCheck);
  $("#btnAppUpdCheck").addEventListener("click", checkAppUpdate);
  $("#btnAppUpdDownload").addEventListener("click", downloadAppUpdate);
  $("#btnAppUpdOpen").addEventListener("click", () => {
    const dir = (B && B.dataDir ? B.dataDir + "\\updates" : "");
    if (dir) invoke("open_path", { path: dir }).catch(() => {});
  });
  $("#btnApplyAll").addEventListener("click", () => doApply([]));
  $("#btnApplySel").addEventListener("click", () => {
    // Отключённые строки (уже «ок»/«skip-user») не отправляем: иначе повторное
    // «Применить выбранное» заново качает и перезаписывает уже применённое.
    const ids = $$("#updList input[type=checkbox]:checked")
      .filter((c) => !c.disabled)
      .map((c) => c.dataset.id);
    if (!ids.length) {
      logWrite("warn", "ui", T.nothing_selected, "W-UI-001");
      toast("warn", T.nothing_selected);
      return;
    }
    doApply(ids);
  });

  // Настройки применяются сразу при изменении — кнопки «Сохранить» больше нет.
  // Для числового поля ждём паузу в наборе, чтобы не сохранять на каждую цифру.
  const saveSoon = (delay) => {
    clearTimeout(saveSettingsTimer);
    saveSettingsTimer = setTimeout(() => {
      invoke("set_settings", { settings: collectSettings() }).catch((e) => toast("err", String(e)));
    }, delay);
  };
    for (const id of ["#cfGameFilter", "#cfIpset", "#cfFilterMode", "#cfAnticheat"]) {
      $(id).addEventListener("change", () => saveSoon(0));
    }
  $("#cfInterval").addEventListener("input", () => saveSoon(700));
  $("#cfInterval").addEventListener("change", () => saveSoon(0));

  // Карточка «Автозапуск»: выбор стратегии + галочка службы.
  // Смена стратегии при включённой службе переключает саму службу.
  $("#cfAutostart").addEventListener("change", async () => {
    const val = $("#cfAutostart").value;
    const svcInstalled = !!(B.service && B.service.installed);
    const sel = $("#cfAutostart");
    sel.disabled = true;
    try {
      if (svcInstalled) {
        if (val) {
          await invoke("install_service", { id: val });
          toast("ok", T.svc_switched);
        } else {
          // Сначала сохраняем «выключено», затем снимаем службу.
          const s = collectSettings();
          s.autostart_mode = "none";
          s.autostart_profile = null;
          await invoke("set_settings", { settings: s });
          await invoke("remove_service");
          toast("ok", T.auto_off);
        }
      } else {
        await invoke("set_settings", { settings: collectSettings() });
        toast("ok", val ? T.auto_saved : T.auto_off);
      }
    } catch (e) {
      // Возвращаем выпадающий список к фактическому значению из снапшота:
      // иначе он визуально остаётся на неудавшемся выборе.
      sel.value = (B && B.settings && B.settings.autostart_profile) || "";
      toast("err", String(e));
    }
    await refreshAll();
  });

  // Галочка «Включить службу»: автозапуск существует только службой.
  $("#cfSvcBoot").addEventListener("change", async () => {
    const box = $("#cfSvcBoot");
    const id = $("#cfAutostart").value;
    const want = box.checked;
    box.disabled = true;
    try {
      if (want) {
        if (!id) throw T.pick_profile_first;
        toast("info", T.svc_installing);
        await invoke("install_service", { id });
        toast("ok", T.svc_installed);
      } else {
        await invoke("remove_service");
        toast("ok", T.svc_removed);
      }
    } catch (e) {
      box.checked = !want;
      toast("err", String(e));
    }
    await refreshAll();
  });

  $("#dnsProvider").addEventListener("change", renderDnsInfo);
  if ($("#btnDnsBench")) $("#btnDnsBench").addEventListener("click", runDnsBenchmark);
  $("#btnApplyDns").addEventListener("click", async () => {
    const b = $("#btnApplyDns");
    btnBusy(b, true);
    try {
      const result = await invoke("apply_dns", {
        provider: $("#dnsProvider").value,
        adapter: $("#dnsAdapter").value.trim() || null,
      });
      toast("ok", result);
    } catch (e) {
      toast("err", T.err_short(e));
    }
    btnBusy(b, false);
  });
  $("#btnResetDns").addEventListener("click", async () => {
    const b = $("#btnResetDns");
    btnBusy(b, true);
    try {
      const result = await invoke("reset_dns", { adapter: $("#dnsAdapter").value.trim() || null });
      toast("ok", result);
    } catch (e) {
      toast("err", T.err_short(e));
    }
    btnBusy(b, false);
  });

  if ($("#btnNetReset")) $("#btnNetReset").addEventListener("click", netReset);
  if ($("#btnShowAdapters")) $("#btnShowAdapters").addEventListener("click", showAdapters);
  if ($("#tgOffer"))
    $("#tgOffer").addEventListener("change", () => {
      // Включили «предлагать» — разрешаем оффер снова в этой сессии.
      tgOfferShown = false;
      invoke("tg_offer_reset").catch(() => {});
    });
}

async function netReset() {
  const ok = await showConfirm({
    title: T.netreset_confirm_title,
    danger: true,
    okLabel: T.btn_net_start,
    cancelLabel: T.btn_cancel,
    // Блокировка как в тестах: 3 секунды на прочтение шагов.
    lockSec: 3,
    html: T.netreset_html,
  });
  if (!ok) return;
  const b = $("#btnNetReset");
  const out = $("#netResetOut");
  btnBusy(b, true);
  out.classList.remove("hidden");
  out.className = "muted";
  out.textContent = T.net_running;

  try {
    const r = await invoke("net_reset");
    const steps = (r.steps || []).map((s) => "• " + s).join("\n");
    out.className = "muted ok";
    out.textContent = steps + (r.rebootRequired ? T.net_reboot_note : "");
    toast("ok", T.net_done);
    if (r.rebootRequired) {
      // Перезагрузка — ТОЛЬКО по явному клику в окне. Никакой автоматики.
      const reboot = await showConfirm({
        title: T.reboot_title,
        danger: true,
        okLabel: T.reboot_ok,
        cancelLabel: T.reboot_later,
        html: T.reboot_html,
      });
      if (reboot) {
        toast("info", T.reboot_soon);
        await invoke("reboot_now").catch(() => {});
      }
    }
  } catch (e) {
    out.textContent = T.err_short(e);
    out.className = "muted err";
    toast("err", String(e));
  } finally {
    btnBusy(b, false);
  }
}

async function showAdapters() {
  const out = $("#netResetOut");
  out.classList.remove("hidden");
  out.textContent = T.adapters_searching;
  try {
    const list = await invoke("virtual_adapters");
    if (!list.length) {
      out.textContent = T.adapters_none;
    } else {
      out.textContent = "";
      const lead = document.createElement("div");
      lead.textContent = T.adapters_found_lead;
      const ul = document.createElement("ul");
      ul.className = "warn-list";
      for (const a of list) {
        const li = document.createElement("li");
        li.textContent = a;
        ul.appendChild(li);
      }
      out.append(lead, ul);
    }
    out.className = "muted";
    // Открываем «Сетевые подключения», где адаптеры видны по именам.
    try {
      await invoke("open_external", { target: "ncpa.cpl" });
    } catch (_) {
      toast("warn", T.adapters_open_fail);
    }
  } catch (e) {
    out.textContent = T.err_short(e);
    out.className = "muted err";
  }
}

async function wireEvents() {
  if ($("#btnTourStart")) $("#btnTourStart").addEventListener("click", () => startTour());
  await listen("zgui:toast", (ev) => toast((ev.payload || {}).kind || "info", (ev.payload || {}).text || ""));
  await listen("zgui:log", (ev) => logPush(ev.payload));
  await listen("zgui:status", async () => {
    refreshAll();
  });
  await listen("zgui:updates", async () => {
    clearUpdatesBusy();
    B = null;
    await refreshAll();
  });
  await listen("zgui:prog", (ev) => updateProgress(ev.payload));
  await listen("zgui:test", (ev) => {
    testState = ev.payload;
    renderTestProgress();
    renderTestResults();
    // Пока идёт тест — тулбар показывает «идёт тест», «Остановить» заблокирована.
    renderRunBar();
    if (testState && testState.done) {
      btnBusy($("#btnRunTest"), false);
      // Перечитываем tests.json: иначе счёт/«лучшая» на плитках остаются от
      // прошлого прогона до перезапуска программы.
      loadTestCache();
      renderTestCard();
      refreshAll();
    }
  });
  // Взаимная блокировка: долгая операция началась/закончилась — обновляем
  // индикатор и кнопки сразу, не дожидаясь следующего опроса bootstrap.
  await listen("zgui:op", async () => {
    await refreshAll();
  });
}

initTheme();
renderPatternGrid();
initFx();
applyTexts();
renderNavIcons();
bindStatic();

// Свёрнутое/невидимое окно не должно крутить анимацию фона впустую.
document.addEventListener("visibilitychange", () => {
  document.body.classList.toggle("fx-paused", document.hidden);
});
// Окно не в фокусе — фон не анимируем: бесконечный дрейф зря нагружал
// GPU-процесс WebView (в диспетчере задач видно десятки процентов).
// При возврате фокуса анимация продолжается с того же места.
window.addEventListener("blur", () => document.body.classList.add("fx-paused"));
window.addEventListener("focus", () => document.body.classList.toggle("fx-paused", document.hidden));

// ПКМ: штатное меню WebView2 (назад/обновить/печать/сохранить как) в программе не нужно.
// Оставляем его только там, где оно полезно: поля ввода и выделенный текст.
document.addEventListener("contextmenu", (e) => {
  const t = e.target;
  const field = t instanceof Element && t.closest("input, textarea, [contenteditable]");
  if (field || String(window.getSelection())) return;
  e.preventDefault();
});

// ------------------------------------------------------------- обучение (coach marks)
// Тур-оверлей поверх живого интерфейса: затемнение с «дыркой» на элементе и
// рукописная обводка (SVG-фильтр). Показывается один раз за установку.
const TOUR_NS = "http://www.w3.org/2000/svg";
let tourIdx = 0;
let tourActive = false;
let tourOffered = false;

function tourSteps() {
  return [
    { center: true, title: T.tour_welcome_title, text: T.tour_welcome_text },
    { view: "tests", nav: "tests", sel: "#testCard .card-head", title: T.tour_tabs_title, text: T.tour_tabs_text },
    {
      sel: '#testTabs .tab[data-filter="flowseal"]',
      title: T.tour_flow_title,
      text: T.tour_flow_text,
      onShow: () => {
        const t = document.querySelector('#testTabs .tab[data-filter="flowseal"]');
        if (t) t.click();
      },
    },
    { sel: "#btnRunTest", title: T.tour_run_title, text: T.tour_run_text },
    { view: "strategies", nav: "strategies", sel: "#profilesCard .card-head", title: T.tour_strat_title, text: T.tour_strat_text },
    { sel: "#profileList .profile .profile-actions .btn.primary", title: T.tour_launch_title, text: T.tour_launch_text },
    { sel: "#cfSvcBoot", title: T.tour_svc_title, text: T.tour_svc_text },
    { sel: "#btnStop", title: T.tour_stop_title, text: T.tour_stop_text },
  ];
}

function ensureTourLayer() {
  let l = $("#tour");
  if (!l) {
    l = document.createElement("div");
    l.id = "tour";
    l.className = "tour hidden";
    document.body.appendChild(l);
  }
  return l;
}

function tourFinish(markDone) {
  tourActive = false;
  const layer = $("#tour");
  if (layer) {
    layer.classList.add("hidden");
    layer.innerHTML = "";
  }
  window.removeEventListener("resize", tourRender);
  document.removeEventListener("keydown", tourKey);
  if (markDone) {
    invoke("tour_done_set").catch(() => {});
    if (B && B.settings) B.settings.tour_done = true;
  }
}

function tourKey(e) {
  if (e.key === "Escape") tourFinish(true);
}

function tourNext() {
  const steps = tourSteps();
  if (tourIdx >= steps.length - 1) return tourFinish(true);
  tourIdx += 1;
  tourRender();
}

function tourBack() {
  if (tourIdx > 0) {
    tourIdx -= 1;
    tourRender();
  }
}

function tourRender() {
  if (!tourActive) return;
  const steps = tourSteps();
  const step = steps[tourIdx];
  const layer = $("#tour");
  if (!layer || !step) return tourFinish(true);
  // Открываем вкладку, где лежит цель: явная `view` шага ИЛИ страница самого
  // элемента. Иначе при шаге «назад» цель оказывается на скрытой странице,
  // rect нулевой — и шаг молча уходит вперёд.
  let view = step.view;
  if (!view && !step.center) {
    const el0 = document.querySelector(step.sel);
    const page = el0 && el0.closest(".page");
    if (page && page.id.startsWith("view-")) view = page.id.slice(5);
  }
  if (view) {
    const nav = document.querySelector(`.nav-item[data-view="${view}"]`);
    if (nav && !nav.classList.contains("active")) nav.click();
  }
  if (step.onShow) {
    try { step.onShow(); } catch (_) {}
  }
  layer.innerHTML = "";
  layer.classList.remove("hidden");

  const W = window.innerWidth;
  const H = window.innerHeight;

  let rect = null;
  if (!step.center) {
    const el = document.querySelector(step.sel);
    if (el) {
      // Высокую цель подводим к верху окна (иначе «середина списка»), низкую — по центру.
      const r0 = el.getBoundingClientRect();
      el.scrollIntoView({ block: r0.height > H * 0.5 ? "start" : "center" });
      rect = el.getBoundingClientRect();
    }
    // Скрытый/нулевой элемент — шаг пропускаем (не вешаем подсказку в углу).
    if (!rect || rect.width < 2 || rect.height < 2) return tourNext();
  }
  // «Прожектор» и на пункте меню — когда шаг переводит на другую вкладку.
  let navRect = null;
  if (step.nav) {
    const navEl = document.querySelector(`.nav-item[data-view="${step.nav}"]`);
    if (navEl) navRect = navEl.getBoundingClientRect();
  }

  // Подсказка
  const box = document.createElement("div");
  box.className = "tour-box" + (step.center ? " center" : "");
  const ttl = document.createElement("b");
  ttl.textContent = step.title;
  const txt = document.createElement("p");
  txt.textContent = step.text;
  const nav = document.createElement("div");
  nav.className = "tour-actions";
  const dots = document.createElement("span");
  dots.className = "tour-dots";
  dots.textContent = `${tourIdx + 1} / ${steps.length}`;
  const mkBtn = (label, cls, fn) => {
    const b = document.createElement("button");
    b.className = "btn small " + cls;
    b.textContent = label;
    b.addEventListener("click", (e) => { e.stopPropagation(); fn(); });
    return b;
  };
  const back = mkBtn(T.tour_back, "ghost", tourBack);
  back.disabled = tourIdx === 0;
  const skip = mkBtn(T.tour_skip, "ghost", () => tourFinish(true));
  const next = mkBtn(tourIdx === steps.length - 1 ? T.tour_done_btn : T.tour_next, "primary", tourNext);
  nav.append(dots, back, skip, next);
  box.append(ttl, txt, nav);
  layer.appendChild(box);

  const boxW = Math.min(360, W - 24);
  box.style.width = boxW + "px";
  const bh = box.offsetHeight;
  let bx;
  let by;
  if (rect) {
    const belowY = rect.bottom + 14;
    by = belowY + bh < H - 10 ? belowY : Math.max(10, rect.top - bh - 14);
    bx = Math.min(Math.max(12, rect.left + rect.width / 2 - boxW / 2), W - boxW - 12);
  } else {
    bx = (W - boxW) / 2;
    by = (H - bh) / 2;
  }
  box.style.left = Math.round(bx) + "px";
  box.style.top = Math.round(by) + "px";

  // SVG: затемнение с «дыркой» + рукописная обводка и стрелка
  const svg = document.createElementNS(TOUR_NS, "svg");
  svg.setAttribute("class", "tour-svg");
  svg.setAttribute("viewBox", `0 0 ${W} ${H}`);
  svg.setAttribute("width", W);
  svg.setAttribute("height", H);
  const defs = document.createElementNS(TOUR_NS, "defs");

  const arrow = document.createElementNS(TOUR_NS, "marker");
  arrow.setAttribute("id", "tour-arrow");
  arrow.setAttribute("markerWidth", "10");
  arrow.setAttribute("markerHeight", "10");
  arrow.setAttribute("refX", "7");
  arrow.setAttribute("refY", "3");
  arrow.setAttribute("orient", "auto");
  const arrowHead = document.createElementNS(TOUR_NS, "path");
  arrowHead.setAttribute("d", "M0,0 L7,3 L0,6");
  arrowHead.style.fill = "none";
  arrowHead.style.stroke = "var(--amber)";
  arrowHead.setAttribute("stroke-width", "1.6");
  arrow.append(arrowHead);
  defs.appendChild(arrow);

  const maskId = "tour-mask";
  const mask = document.createElementNS(TOUR_NS, "mask");
  mask.setAttribute("id", maskId);
  const mWhite = document.createElementNS(TOUR_NS, "rect");
  mWhite.setAttribute("width", W);
  mWhite.setAttribute("height", H);
  mWhite.setAttribute("fill", "#fff");
  mask.appendChild(mWhite);
  const holes = [];
  if (rect) holes.push({ r: rect, pad: 8 });
  if (navRect) holes.push({ r: navRect, pad: 6 });
  for (const h of holes) {
    const hole = document.createElementNS(TOUR_NS, "rect");
    hole.setAttribute("x", h.r.left - h.pad);
    hole.setAttribute("y", h.r.top - h.pad);
    hole.setAttribute("width", h.r.width + h.pad * 2);
    hole.setAttribute("height", h.r.height + h.pad * 2);
    hole.setAttribute("rx", "10");
    hole.setAttribute("fill", "#000");
    mask.appendChild(hole);
  }
  defs.appendChild(mask);
  svg.appendChild(defs);

  const dim = document.createElementNS(TOUR_NS, "rect");
  dim.setAttribute("width", W);
  dim.setAttribute("height", H);
  dim.style.fill = "rgba(6,7,10,0.55)";
  dim.setAttribute("mask", `url(#${maskId})`);
  svg.appendChild(dim);

  for (const h of holes) {
    const outline = document.createElementNS(TOUR_NS, "rect");
    outline.setAttribute("x", h.r.left - h.pad);
    outline.setAttribute("y", h.r.top - h.pad);
    outline.setAttribute("width", h.r.width + h.pad * 2);
    outline.setAttribute("height", h.r.height + h.pad * 2);
    outline.setAttribute("rx", "12");
    outline.style.fill = "none";
    outline.style.stroke = "var(--amber)";
    outline.setAttribute("stroke-width", "2.5");
    outline.setAttribute("stroke-linecap", "round");
    svg.appendChild(outline);
  }

  if (rect) {
    const cx = bx + boxW / 2;
    // Стрелка — строго прямая: от ближней стороны подсказки к ближнему краю цели.
    const boxAbove = by + bh <= rect.top;
    const cy = boxAbove ? by + bh : by;
    const tx = Math.min(Math.max(cx, rect.left), rect.right);
    const ty = Math.min(Math.max(cy, rect.top), rect.bottom);
    const path = document.createElementNS(TOUR_NS, "path");
    path.setAttribute("d", `M${cx},${cy} L${tx},${ty}`);
    path.style.fill = "none";
    path.style.stroke = "var(--amber)";
    path.setAttribute("stroke-width", "2");
    path.setAttribute("stroke-linecap", "round");
    path.setAttribute("marker-end", "url(#tour-arrow)");
    svg.appendChild(path);
  }

  layer.insertBefore(svg, box);
}

function startTour() {
  tourIdx = 0;
  tourActive = true;
  ensureTourLayer();
  window.addEventListener("resize", tourRender);
  document.addEventListener("keydown", tourKey);
  const nav = document.querySelector('.nav-item[data-view="strategies"]');
  if (nav) nav.click();
  tourRender();
}

async function maybeOfferTour() {
  if (tourOffered) return;
  if (!B || !B.settings || B.settings.tour_done) return;
  tourOffered = true;
  const go = await showConfirm({
    title: T.tour_offer_title,
    html: T.tour_offer_text,
    okLabel: T.tour_offer_yes,
    cancelLabel: T.tour_offer_no,
  });
  if (go) {
    startTour();
  } else {
    invoke("tour_done_set").catch(() => {});
    if (B.settings) B.settings.tour_done = true;
  }
}

(async function init() {
  try {
    await wireEvents();
    // Сначала test_status: если после жёсткого закрытия остался фоновый раннер
    // теста (elevated), эта команда гасит его — иначе новый запуск движка
    // упирался бы в живой winws теста.
    let ts = null;
    try {
      ts = await invoke("test_status");
    } catch (_) {}
    await refreshAll();
    loadDnsProviders();
    setTimeout(maybeOfferTour, 800);
    loadTestCache();
    if (ts && ts.running) {
      testState = ts;
      renderTestCard();
      renderTestProgress();
    }
    // Права администратора запрашиваются при старте программы (один UAC) —
    // здесь ничего предлагать не нужно.
    invoke("app_info")
      .then((i) => {
        // В подвале сайдбара — только версия (просьба владельца).
        $("#appMeta").innerHTML = `<span class="ver">v${i.version}</span>`;
        const av = $("#appVer");
        if (av) av.textContent = "v" + i.version;
        const ap = $("#appUpdPath");
        if (ap && B && B.dataDir) ap.textContent = B.dataDir + "\\updates";
      })
      .catch(() => {});
    // Никаких таймеров: состояние окна обновляется событиями (zgui:status/
    // zgui:op/zgui:updates/zgui:test) и переходом по вкладкам.
  } catch (e) {
    console.error("init failed", e);
    toast("err", String(e));
  }
})();

(async function prefetchIgnore() {
  await invoke("current_status").catch(() => {});
})();
