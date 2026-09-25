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
function logWrite(level, scope, msg) {
  rawInvoke("log_write", { level, scope, msg: String(msg) }).catch(() => {});
  logPush({ ts: Date.now(), level, scope, msg: String(msg) });
}

/// Обёртка над вызовом бэкенда: понятный текст ошибки + запись в журнал.
async function invoke(cmd, args) {
  try {
    return await rawInvoke(cmd, args);
  } catch (e) {
    const raw = typeof e === "string" ? e : (e && e.message) || String(e);
    const human = humanError(raw);
    logWrite("err", "команда " + cmd, raw);
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
  await logPoll();
  renderLog();
  if (!logState.timer) logState.timer = setInterval(logPoll, 2000);
}

function stopLogPoll() {
  if (logState.timer) {
    clearInterval(logState.timer);
    logState.timer = null;
  }
}

let B = null; // bootstrap snapshot
let profileFilter = "all";
let lastUpdRender = "";
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
  // После смены темы кнопки блокируются на 5 секунд (тема применяется не мгновенно).
  if (Date.now() < themeLockUntil) return;
  applyTheme(theme);
  // Снапшот обновляем сразу: иначе следующий опрос bootstrap (раз в 4 с)
  // применял бы старую тему из ещё не сохранённого state — тема «отпрыгивала».
  if (B && B.settings) B.settings.theme = theme;
  themePicked = theme;
  themeLockUntil = Date.now() + 5000;
  $$("#themeGrid .theme-card").forEach((c) => lockBtnFill(c, 5));
  try {
    await invoke("set_theme", { theme });
  } catch (e) {
    toast("err", T.theme_err(e));
  }
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
    tgVpnGuard();
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
  renderWarnings();
  renderAdminBanner();
  renderProfiles();
  renderTestCard();
  renderAutostart();
  renderEngineUpd();
  renderSettings();
  const bt = B && B.updates && B.updates.lastCheck;
  $("#lastCheck").textContent = bt ? T.last_check(tsText(Number(bt))) : T.never_checked;
  renderUpdates($("#view-updates") === document.querySelector(".page.active") ? "force" : "lazy");
}

// ------------------------------------------------------------- conflicts

function fillConflictList(ul, items) {
  ul.innerHTML = "";
  // Группируем одинаковые имена (например, xray.exe ×25), чтобы список не раздувался.
  const groups = new Map();
  for (const p of items || []) {
    const g = groups.get(p.name) || { name: p.name, note: p.note, pids: [] };
    if (p.pid) g.pids.push(p.pid);
    groups.set(p.name, g);
  }
  for (const g of groups.values()) {
    const li = document.createElement("li");
    const nm = document.createElement("b");
    nm.textContent = g.name;
    li.appendChild(nm);
    if (g.pids.length > 1) {
      const cnt = document.createElement("span");
      cnt.className = "muted";
      cnt.textContent = T.conflict_procs(g.pids.length);
      li.appendChild(cnt);
    } else if (g.pids.length === 1) {
      const pid = document.createElement("span");
      pid.className = "muted";
      pid.textContent = ` (pid ${g.pids[0]})`;
      li.appendChild(pid);
    }
    if (g.note) {
      const nt = document.createElement("div");
      nt.className = "muted";
      nt.style.fontSize = "11px";
      nt.textContent = g.note;
      li.appendChild(nt);
    }
    ul.appendChild(li);
  }
}

function showConflict(report, opts = {}) {
  const modal = $("#conflictModal");
  fillConflictList($("#conflictList"), report.processes || []);
  const vpn = report.vpn || [];
  fillConflictList($("#conflictVpnList"), vpn);
  $("#conflictVpnWrap").classList.toggle("hidden", !vpn.length);
  $("#conflictText").classList.toggle("hidden", !(report.processes || []).length);
  $("#conflictList").classList.toggle("hidden", !(report.processes || []).length);
  $("#conflictTitle").textContent = opts.title || T.conflict_title;
  $("#conflictHint").textContent = opts.hint || T.conflict_hint;
  $("#btnKillConflicts").textContent = opts.killLabel || T.btn_kill_conflicts;
  // Без прав администратора taskkill получит «отказано в доступе» — предупреждаем
  // и предлагаем перезапуск от админа (иначе будет поток запросов UAC).
  const adminHint = $("#conflictAdminHint");
  if (adminHint) {
    if (B && !B.elevated) {
      adminHint.style.display = "";
      adminHint.textContent = T.conflict_admin_hint;
    } else {
      adminHint.style.display = "none";
      adminHint.textContent = "";
    }
  }
  conflictRetry = opts.onKilled || null;
  modal.classList.remove("hidden");
}

let conflictRetry = null;
// Согласие на выгрузку: после первого клика выгружаем без повторных вопросов.
let conflictConsent = false;
// Защита от наложения: пока одна выгрузка идёт, новые проверки не запускаем.
let conflictKilling = false;

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
    // 5 секунд на «прочитать риски»: кнопка заблокирована с заливкой-шкалой.
    if (lockSec > 0) lockBtnFill(ok, lockSec);
  });
}

function closeConfirm(val) {
  $("#confirmModal").classList.add("hidden");
  const r = cmResolve;
  cmResolve = null;
  if (r) r(val);
}

function hideConflict() {
  conflictRetry = null;
  $("#conflictModal").classList.add("hidden");
}

/// Выгружает конфликты (без диалога, т.к. согласие уже дано) и повторяет действие.
/// Итоговое уведомление со списком выгруженного показывает бэкенд (один раз),
/// поэтому здесь никаких тостов — только повтор действия при успехе.
async function autoKillAndProceed(retry) {
  if (conflictKilling) return;
  conflictKilling = true;
  try {
    try {
      await invoke("kill_conflicts");
    } catch (e) {
      toast("err", String(e));
      return;
    }
    // Дать процессам умереть, затем перепроверить (backend уже выгрузил списком).
    await new Promise((r) => setTimeout(r, 1500));
    const report = await invoke("conflict_check").catch(() => null);
    const still =
      report && ((report.processes || []).length || (report.vpn || []).length || report.foreignService);
    // Не смогли выгрузить — не запускаем и НЕ зацикливаемся: снимаем авто-согласие,
    // иначе событие zgui:conflict снова вызвало бы авто-выгрузку (вечный цикл powershell).
    if (still) {
      conflictConsent = false;
      return;
    }
    if (typeof retry === "function") retry();
  } finally {
    conflictKilling = false;
  }
}

async function checkConflicts(auto, onKilled) {
  try {
    const report = await invoke("conflict_check");
    if (report && ((report.processes || []).length || (report.vpn || []).length || report.foreignService)) {
      // Пользователь уже соглашался — не переспрашиваем, просто выгружаем.
      if (conflictConsent) {
        autoKillAndProceed(onKilled);
        return true;
      }
      if (auto && sessionStorage.getItem("conflictShown") === "1") return true;
      sessionStorage.setItem("conflictShown", "1");
      showConflict(report, { onKilled });
      return true;
    }
  } catch (_) {}
  return false;
}

// ------------------------------------------------------------- первый запуск: права

let adminOfferPending = false;

/// При первом входе предлагаем включить «Всегда запускать от администратора» и
/// перезапуститься: иначе каждое действие запрашивает права отдельно (5+ окон).
/// Модалку показываем не сразу — ждём, пока стартовый авто-прогон заполнит
/// каталог обновлений (иначе рестарт от админа убил бы проверку на середине).
/// Возвращает true, если предложение показано — проверку конфликтов откладываем.
async function maybeOfferAdmin() {
  if (!B || !B.settings || B.settings.admin_onboarded) return false;
  // Уже работаем от администратора — переспрашивать не нужно.
  if (B.elevated) {
    await invoke("mark_admin_onboarded").catch(() => {});
    return false;
  }
  const t0 = Date.now();
  // Ждём каталог недолго: модалка не должна подвешивать первый запуск.
  while (Date.now() - t0 < 3000) {
    const entries = (B && B.updates && B.updates.entries) || [];
    if (entries.length) break;
    await new Promise((r) => setTimeout(r, 400));
  }
  adminOfferPending = true;
  $("#adminModal").classList.remove("hidden");
  return true;
}

function hideAdminOffer() {
  adminOfferPending = false;
  $("#adminModal").classList.add("hidden");
}

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

/// Перезапуск от администратора — общий диалог для первого запуска, чекбокса
/// в «Настройках» и кнопки «Перезапустить от админа».
async function askRestartAdmin() {
  const ok = await showConfirm({
    title: T.admin_restart_title,
    okLabel: T.btn_restart,
    html: T.admin_restart_html,
  });
  if (!ok) return;
  try {
    await invoke("relaunch_as_admin");
  } catch (e) {
    toast("err", String(e));
  }
}

// ---- баннер «нужны права администратора» (не модалка: не мешает, но всегда доступен) ----
let adminBannerDismissed = false;

function renderAdminBanner() {
  const el = $("#adminBanner");
  if (!el) return;
  // Показываем, только если запущено без прав и «всегда от админа» не включено.
  const show = !!(B && !B.elevated && B.settings && !B.settings.always_admin);
  el.classList.toggle("hidden", !show || adminBannerDismissed);
}

function renderWarnings() {
  const el = $("#warnBanner");
  if (!el) return;
  const warn = (B && B.warnings) || [];
  if (!warn.length) {
    el.classList.add("hidden");
    el.innerHTML = "";
    return;
  }
  el.classList.remove("hidden");
  el.innerHTML = "";
  const t = document.createElement("div");
  t.className = "warn-title";
  t.textContent = T.warn_title;
  el.appendChild(t);
  const ul = document.createElement("ul");
  ul.className = "warn-list";
  for (const w of warn) {
    const li = document.createElement("li");
    li.textContent = w;
    ul.appendChild(li);
  }
  el.appendChild(ul);
}

function renderRunBar() {
  const owner = (B && B.owner) || "none";
  const rt = (B || {}).runtime || null;
  const st = $("#runState");
  const opRunning = !!(B && B.opRunning);
  const profiles = (B && B.profiles) || [];
  const nameOf = (id) => (profiles.find((p) => p.id === id) || {}).name || id || "";
  const svcStrategy = (B.service && B.service.strategy) || null;
  if (opRunning) {
    // Идёт долгая операция (обновления, DNS, служба, сброс сети, тест):
    // старт/стоп профилей запрещён — взаимная блокировка.
    st.className = "run-state busy";
    st.textContent = T.run_op;
  } else if (owner === "test") {
    // Идёт прогон тестов: winws управляется тестом — останавливать его тулбаром нельзя.
    st.className = "run-state running";
    st.textContent = T.run_test;
  } else if (owner === "app") {
    st.className = "run-state running";
    st.textContent = rt ? nameOf(rt.profileId) : T.run_started;
  } else if (owner === "service") {
    st.className = "run-state running";
    st.textContent = T.run_service(svcStrategy ? nameOf(svcStrategy) : T.service_running);
  } else if (owner === "external") {
    // winws нашего движка поднят вне программы (ручной .bat): показываем и
    // разрешаем остановить, иначе второй winws конфликтует с запущенным.
    st.className = "run-state running";
    st.textContent = T.run_external;
  } else {
    st.className = "run-state idle";
    st.textContent = T.run_idle;
  }
  $("#btnStop").disabled = opRunning || !["app", "service", "external"].includes(owner);
}

/// Индикатор watchdog в шапке: «стратегия активна / не отвечает». Активен только
/// когда запущена стратегия (watchdog проверяет YouTube/Discord раз в минуту).
function renderWatchdog(s) {
  const el = $("#wdState");
  if (!el) return;
  if (!s || !s.active) {
    el.textContent = "";
    el.className = "muted";
    return;
  }
  el.textContent = s.alarm ? T.wd_alarm : T.wd_ok;
  el.className = s.alarm ? "muted err" : "muted ok";
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
      acts.appendChild(btn(T.btn_update_engine, "ghost", () => doFetch(eng)));
      rootEl.appendChild(acts);
    } else {
      st.className = "state-chip warn";
      st.textContent = T.eng_not_installed;
      rootEl.innerHTML = "";
      const acts = document.createElement("div");
      acts.className = "root-actions";
       acts.appendChild(btn(T.btn_install_engine, "primary", () => doFetch(eng)));
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

function doFetch(eng) {
  btnBusy($("#engine-" + eng + " .btn.primary"), true);
  const label = engineLabel(eng);
  toast("info", T.engine_downloading(label));
  invoke("fetch_engine", { engine: eng, dest: null })
    .catch((e) => toast("err", String(e)))
    .finally(() => setTimeout(() => btnBusy($("#engine-" + eng + " .btn.primary"), false), 3000));
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
    sub.textContent = T.engine_from_release(e.repo || "");
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
      if (!isRun) {
        const has = await checkConflicts(false, doStart);
        if (has) return;
      }
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
  // «Лучшая» показывается ТОЛЬКО если она среди текущего вида (движка/фильтра):
  // иначе после прогона dpibreak висел бы best от flowseal — путаница.
  const bestId = (testState && testState.bestId) || (testCache && testCache.bestId) || null;
  const bestInView = bestId && tested.some((x) => x.r.id === bestId) ? bestId : null;
  const bestRes = bestInView ? byId.get(bestInView) : null;
  const runDone = (testState && testState.done) || false;
  if (runDone && bestRes) {
    bestBox.classList.remove("hidden");
    bestBox.innerHTML = "";
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
  } else if (runDone && tested.length) {
    // Прогон завершён, но успешной стратегии нет (или best не из этого движка).
    bestBox.classList.remove("hidden");
    bestBox.innerHTML = "";
    const t = document.createElement("div");
    t.className = "test-best-title muted";
    t.textContent =
      T.test_no_success
      + (testEngineFilter ? " " + T.test_no_success_hint + "." : "");
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
    toast("warn", T.nothing_selected);
    return;
  }
  const btn = $("#btnRunTest");
  // Взаимная блокировка: тест не запускается параллельно с другой операцией.
  if (B && B.opRunning) {
    toast("warn", T.test_busy_other);
    return;
  }
  if (!already) {
    const ok = await showConfirm({
      title: T.test_confirm_title,
      okLabel: T.btn_run,
      cancelLabel: T.btn_cancel,
      okKind: "warn",
      // 5 секунд на «прочитать риски»: кнопка заблокирована с заливкой-шкалой.
      lockSec: 5,
      html:
        T.test_confirm_html(useIds.length) +
        NET_HINT_HTML +
        T.test_admin_hint,
    });
    if (!ok) return;
  }
  btnBusy(btn, true);
  try {
    await invoke("test_strategies", { ids: useIds });
    toast("info", T.test_started);
  } catch (e) {
    const msg = String(e);
    if (msg.includes("VPN_RUNNING")) {
      const report = await invoke("vpn_check").catch(() => ({ vpn: [] }));
      showConflict(report, {
        title: T.vpn_test_title,
        hint: T.vpn_test_hint,
        killLabel: T.vpn_test_kill,
        onKilled: async () => {
          await new Promise((r) => setTimeout(r, 1500));
          runTest(true);
        },
      });
    } else {
      toast("err", msg);
    }
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

// Результат проверки обновления движка (для карточки Flowseal).
let engineUpdate = null;

function renderEngineUpd() {
  const el = $("#fsUpd");
  if (!el) return;
  const u = engineUpdate;
  if (!u) {
    el.textContent = "";
    el.className = "muted card-sub";
    return;
  }
  if (u.error) {
    el.textContent = T.engine_check_fail;
    el.className = "muted card-sub warn";
    return;
  }
  if (u.upToDate) {
    el.textContent = T.engine_uptodate(u.installed || "");
    el.className = "muted card-sub";
  } else {
    el.textContent = T.engine_update_available(u.latest || "", u.installed || "");
    el.className = "muted card-sub warn";
  }
}

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

// Одной кнопкой проверяем всё: конфиги, движок и Telegram-мост.
async function doCheck() {
  setUpdatesBusy(T.upd_checking, 180000);
  try {
    await invoke("check_updates");
    engineUpdate = await invoke("engine_check_update").catch(() => null);
    renderEngineUpd();
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
    // После установки движка сразу перепроверяем версию — иначе плашка
    // «доступно обновление» висела бы до следующей ручной проверки.
    if (ev && ev.phase === "done" && (ev.id || "").startsWith("fetch:")) {
      invoke("engine_check_update")
        .then((u) => { engineUpdate = u; renderEngineUpd(); })
        .catch(() => {});
    }
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
    game_filter_tcp: ($("#cfGameFilterTcp").value || "").trim() || "1024-65535",
    game_filter_udp: ($("#cfGameFilterUdp").value || "").trim() || "1024-65535",
    ipset_mode: $("#cfIpset").value,
    autostart_mode: $("#cfAutostart").value ? "profile" : "none",
    autostart_profile: $("#cfAutostart").value || null,
    always_admin: $("#cfAlwaysAdmin").checked,
    tg_autostart: $("#tgAutostart")?.checked || false,
    tg_offer: $("#tgOffer") ? $("#tgOffer").checked : (B && B.settings ? !!B.settings.tg_offer : false),
    tg_port: $("#tgPort")
      ? Number($("#tgPort").value) || 1443
      : (B && B.settings ? B.settings.tg_port || 1443 : 1443),
  };
}

function renderSettings() {
  const s = B.settings || {};
  // Не трогаем поля формы, если настройки не менялись: иначе опрос раз в 4 с
  // затирал то, что пользователь печатает прямо сейчас.
  const sig = JSON.stringify([s, (B.profiles || []).map((p) => p.id), B.service]);
  if (sig === sigSettings) { renderDnsProviders(); return; }
  sigSettings = sig;
  $("#cfInterval").value = s.update_interval_hours ?? 72;
  $("#cfGameFilter").value = s.game_filter || "off";
  if ($("#cfGameFilterTcp")) $("#cfGameFilterTcp").value = s.game_filter_tcp || "1024-65535";
  if ($("#cfGameFilterUdp")) $("#cfGameFilterUdp").value = s.game_filter_udp || "1024-65535";
  $("#cfIpset").value = s.ipset_mode || "loaded";
  const adm = $("#cfAlwaysAdmin");
  if (adm) adm.checked = !!s.always_admin;
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
  // схлопывался бы каждым опросом bootstrap раз в 4 с).
  const sig = JSON.stringify([
    s.boot_app,
    s.autostart_profile,
    B.service,
    (B.profiles || []).map((p) => [p.id, p.name]),
  ]);
  const svcInstalled = !!(B.service && B.service.installed);
  const bootOn = !!s.boot_app;
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
  // Профиль автозапуска можно менять и в режиме службы: служба переключится
  // (см. обработчик ниже). Блокировать выбор незачем.
  sel.disabled = false;
  sel.title = "";

  const chosen = sel.value;
  if (box) {
    box.checked = svcInstalled;
    box.disabled = !svcInstalled && !chosen;
    box.title = !svcInstalled && !chosen ? T.auto_pick_first : "";
  }

  const chip = $("#bootStateChip");
  if (chip) {
    chip.textContent = svcInstalled ? T.chip_service : bootOn ? T.auto_chip_on : T.auto_chip_off;
        chip.className = "chip" + (svcInstalled || bootOn ? " best" : " off");
  }

  const note = $("#bootStateNote");
  if (note) {
    const profile = (B.profiles || []).find((p) => p.id === s.autostart_profile);
    if (svcInstalled) {
      note.textContent = T.auto_note_service(profile ? profile.name : "");
    } else if (bootOn && profile) {
      note.textContent = T.auto_note_on(profile.name);
    } else if (bootOn && !profile) {
      note.textContent = T.auto_note_no_profile;
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

/// Стража VPN: если наш TG-мост включён, а обнаружен VPN/туннель — бэкенд гасит мост.
async function tgVpnGuard() {
  try {
    if (await invoke("tg_vpn_guard")) {
      tgState = await invoke("tg_status").catch(() => tgState);
      renderTg();
    }
  } catch (_) {}
}

/// Неблокирующее предложение Telegram-моста: Telegram запущен, VPN нет.
/// Приоритет: сначала вопрос о правах администратора (первый запуск/модалка),
/// только после него — TG-оффер. Галочку «предлагать» по умолчанию держим
/// выключенной (см. Settings::tg_offer).
let tgOfferShown = false;
async function checkTgOffer() {
  if (tgOfferShown) return;
  const s = (B && B.settings) || {};
  if (!s.tg_offer) return;
  if (adminOfferPending) return;
  // Первый запуск: пока админ-вопрос не разрешён, оффер не показываем и не
  // расходуем его одноразовый флаг в бэкенде.
  if (!B.elevated && !s.always_admin && !s.admin_onboarded) return;
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
      if (n.dataset.view === "logs") openLogs();
      else stopLogPoll();
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

  if ($("#btnTgToggle")) $("#btnTgToggle").addEventListener("click", tgToggle);
  if ($("#btnTgConnect")) $("#btnTgConnect").addEventListener("click", tgConnect);
  if ($("#tgAutostart")) $("#tgAutostart").addEventListener("change", tgSavePrefs);
  if ($("#tgOffer")) $("#tgOffer").addEventListener("change", tgSavePrefs);
  if ($("#tgPort")) $("#tgPort").addEventListener("change", tgSavePrefs);

  $("#btnKillConflicts").addEventListener("click", async () => {
    conflictConsent = true; // пользователь согласился — дальше без переспроса
    const retry = conflictRetry;
    btnBusy($("#btnKillConflicts"), true);
    hideConflict();
    await autoKillAndProceed(retry);
    btnBusy($("#btnKillConflicts"), false);
  });
  $("#btnConflictLater").addEventListener("click", hideConflict);
  $("#conflictClose").addEventListener("click", hideConflict);
  if ($("#btnConflictTaskmgr"))
    $("#btnConflictTaskmgr").addEventListener("click", () => {
      invoke("open_task_manager").catch((e) => toast("err", String(e)));
    });

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
      testState = null;
      renderTestCard();
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
  $("#btnApplyAll").addEventListener("click", () => doApply([]));
  $("#btnApplySel").addEventListener("click", () => {
    // Отключённые строки (уже «ок»/«skip-user») не отправляем: иначе повторное
    // «Применить выбранное» заново качает и перезаписывает уже применённое.
    const ids = $$("#updList input[type=checkbox]:checked")
      .filter((c) => !c.disabled)
      .map((c) => c.dataset.id);
    if (!ids.length) {
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
  for (const id of ["#cfGameFilter", "#cfIpset", "#cfGameFilterTcp", "#cfGameFilterUdp"]) {
    $(id).addEventListener("change", () => saveSoon(0));
  }
  $("#cfInterval").addEventListener("input", () => saveSoon(700));
  $("#cfInterval").addEventListener("change", () => saveSoon(0));

  // «Запускать при входе» (карточка «Автозапуск»): выбор профиля сам настраивает
  // запуск при входе, «— не запускать —» его снимает.
  $("#cfAutostart").addEventListener("change", async () => {
    const val = $("#cfAutostart").value;
    const svcInstalled = !!(B.service && B.service.installed);
    const sel = $("#cfAutostart");
    sel.disabled = true;
    try {
      if (svcInstalled) {
        // Служба — механизм обхода: смена профиля переключает саму службу.
        if (val) {
          await invoke("install_service", { id: val });
          toast("ok", T.svc_switched);
        } else {
          // Сначала сбрасываем автозапуск, иначе remove_service сохранит профиль
          // и переведёт обход на программный автозапуск — «выключено» не сработает.
          const s = collectSettings();
          s.autostart_mode = "none";
          s.autostart_profile = null;
          await invoke("set_settings", { settings: s });
          await invoke("remove_service");
          toast("ok", T.auto_off);
        }
      } else {
        // Профиль сохранён — задачу планировщика согласует бэкенд (sync_autostart).
        await invoke("set_settings", { settings: collectSettings() });
        toast("ok", val ? T.auto_on_login : T.auto_off);
      }
    } catch (e) {
      toast("err", String(e));
    }
    await refreshAll();
  });

  // «Запускать службой (без программы)» — альтернатива автозапуску через программу.
  // При включении сами снимаем автозапуск через программу (два механизма мешают).
  $("#cfSvcBoot").addEventListener("change", async () => {
    const box = $("#cfSvcBoot");
    const id = $("#cfAutostart").value;
    const want = box.checked;
    box.disabled = true;
    try {
      if (want) {
        if (!id) throw T.pick_profile_first;
        // Программный автозапуск снимется сам (бэкенд согласует механизмы).
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

  // Флажок «Всегда запускать от администратора» применяется сразу, без «Сохранить».
  // При включении предлагаем перезапуститься — иначе правила вступят только со
  // следующего запуска, а каждое действие всё ещё будет спрашивать права.
  $("#cfAlwaysAdmin").addEventListener("change", async () => {
    try {
      await invoke("set_settings", { settings: collectSettings() });
      toast("ok", $("#cfAlwaysAdmin").checked
        ? T.admin_on
        : T.admin_off);
      if ($("#cfAlwaysAdmin").checked) askRestartAdmin();
    } catch (e) {
      toast("err", String(e));
    }
  });

  $("#btnElevateNow").addEventListener("click", askRestartAdmin);
  if ($("#btnAdminRelaunch")) $("#btnAdminRelaunch").addEventListener("click", askRestartAdmin);
  if ($("#btnAdminDismiss"))
    $("#btnAdminDismiss").addEventListener("click", () => {
      adminBannerDismissed = true;
      renderAdminBanner();
    });

  // Первый запуск: предложение «Всегда запускать от администратора».
  // Таймер обратного отсчёта убран: кнопка доступна сразу (не заставляем ждать).
  $("#adminEnable").addEventListener("click", async () => {
    const btn = $("#adminEnable");
    btnBusy(btn, true);
    try {
      const always = $("#adminCheck").checked;
      await invoke("set_admin_prefs", { always });
      hideAdminOffer();
      if (always) {
        await askRestartAdmin();
      } else {
        checkConflicts(true);
        toast("ok", T.admin_later_ok);
      }
    } catch (e) {
      toast("err", String(e));
    } finally {
      btnBusy(btn, false);
    }
  });
  const adminDefer = async () => {
    hideAdminOffer();
    await invoke("mark_admin_onboarded").catch(() => {});
    checkConflicts(true);
  };
  $("#adminLater").addEventListener("click", adminDefer);
  $("#adminClose").addEventListener("click", adminDefer);

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
    // Блокировка как в тестах: 5 секунд на прочтение шагов.
    lockSec: 5,
    html: T.netreset_html,
  });
  if (!ok) return;
  const b = $("#btnNetReset");
  const out = $("#netResetOut");
  btnBusy(b, true);
  out.classList.remove("hidden");
  out.className = "muted";

  // Шаг 1: точка восстановления. Пока она не создана — сброс не запускаем.
  out.textContent = T.net_step1;
  let restoreOk = false;
  try {
    const msg = await invoke("net_create_restore_point");
    restoreOk = true;
    out.textContent = T.net_step1_done(msg);
  } catch (e) {
    out.className = "muted warn";
    out.textContent = T.net_restore_fail(e);
    const proceed = await showConfirm({
      title: T.net_continue_title,
      danger: true,
      okLabel: T.net_continue_ok,
      cancelLabel: T.btn_cancel,
      html: T.net_continue_html(escHtml(e)),
    });
    if (!proceed) {
      out.textContent += T.net_canceled;
      btnBusy(b, false);
      return;
    }
    out.className = "muted";
    out.textContent = T.net_running_no_restore;
  }

  // Шаг 2: сам сброс сети.
  try {
    const r = await invoke("net_reset");
    const steps = (r.steps || []).map((s) => "• " + s).join("\n");
    out.className = "muted ok";
    out.textContent =
      (restoreOk ? T.net_restore_created : "") +
      steps +
      (r.rebootRequired ? T.net_reboot_note : "");
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
  await listen("zgui:toast", (ev) => toast((ev.payload || {}).kind || "info", (ev.payload || {}).text || ""));
  await listen("zgui:log", (ev) => logPush(ev.payload));
  await listen("zgui:status", async () => {
    // Стратегию остановили/запустили — гасим старый индикатор watchdog сразу,
    // не ждём следующей минуты (иначе «активна» висит после выключения).
    renderWatchdog(null);
    refreshAll();
  });
  await listen("zgui:updates", async () => {
    clearUpdatesBusy();
    B = null;
    await refreshAll();
  });
  await listen("zgui:prog", (ev) => updateProgress(ev.payload));
  // ВАЖНО: по этому событию НЕ запускаем авто-выгрузку. Раньше здесь вызывался
  // checkConflicts → при активном согласии kill → снова событие → бесконечный
  // цикл запросов UAC/powershell. Итог выгрузки показывают тосты бэкенда.
  await listen("zgui:conflict", () => {});
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
  await listen("zgui:watchdog", (ev) => renderWatchdog(ev.payload));
  // Взаимная блокировка: долгая операция началась/закончилась — обновляем
  // индикатор и кнопки сразу, не дожидаясь следующего опроса bootstrap.
  await listen("zgui:op", async () => {
    await refreshAll();
  });
}

initTheme();
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

(async function init() {
  try {
    await wireEvents();
    // Сначала test_status: если после жёсткого закрытия остался фоновый раннер
    // теста (elevated), эта команда гасит его. Иначе ack_boot (автозапуск профиля
    // при входе) стартовал бы поверх живого движка теста и падал «winws уже запущен».
    let ts = null;
    try {
      ts = await invoke("test_status");
    } catch (_) {}
    try {
      await invoke("ack_boot");
    } catch (_) {}
    await refreshAll();
    loadDnsProviders();
    loadTestCache();
    if (ts && ts.running) {
      testState = ts;
      renderTestCard();
      renderTestProgress();
    }
    // Первый запуск: сначала предложение про права администратора (иначе 5+ запросов).
    // Если показали модалку — проверку конфликтов отложим до её закрытия.
    const adminShown = await maybeOfferAdmin();
    if (!adminShown) checkConflicts(true);
    invoke("app_info")
      .then((i) => {
        // В подвале сайдбара — только версия (просьба владельца).
        $("#appMeta").innerHTML = `<span class="ver">v${i.version}</span>`;
      })
      .catch(() => {});
    // Скрытое окно не опрашиваем: состояние всё равно перерисуется при возврате
    // (события zgui:status и переходы по вкладкам).
    setInterval(() => {
      if (!document.hidden) refreshAll();
    }, 4000);
  } catch (e) {
    console.error("init failed", e);
    toast("err", String(e));
  }
})();

(async function prefetchIgnore() {
  await invoke("current_status").catch(() => {});
})();
