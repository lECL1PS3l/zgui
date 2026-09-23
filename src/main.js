import "./styles.css";
import { invoke as rawInvoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

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

const NAV_ICONS = {
  strategies: icoZap,
  tests: icoFlask,
  updates: icoDownload,
  telegram: icoSend,
  dns: icoShield,
  appearance: icoPalette,
  settings: icoSettings,
  logs: icoScroll,
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

// ------------------------------------------------------------- ошибки и журнал

// Технические ошибки расшифровываем на человеческий язык. Таблица повторяет
// логику src-tauri/src/human.rs — интерфейс и бэкенд говорят одинаково.
const ERROR_RULES = [
  [/os error 5|access is denied|administrator|admin_required/i,
    "нужны права администратора — включите их в «Настройках»"],
  [/os error 32|being used by another process/i,
    "файл занят другой программой — закройте её и повторите"],
  [/os error 112|not enough space/i, "на диске не хватает места"],
  [/os error 2|os error 3|cannot find/i,
    "файл или папка не найдены — возможно, движок ещё не установлен"],
  [/error sending request|error trying to connect|dns error|timed out|connection refused|connection reset|network is unreachable/i,
    "нет связи с сервером — проверьте интернет (или выключите VPN) и повторите"],
  [/http 403|\b403 forbidden\b/i,
    "сервер отклонил запрос (403) — возможно, исчерпан лимит обращений к GitHub, попробуйте позже"],
  [/http 404|\b404 not found\b/i, "на сервере нет такого файла (404) — обновите программу"],
  [/http 5\d\d/i, "сервер временно недоступен — попробуйте позже"],
  [/invalid args|expected u16|invalid type|invalid value/i,
    "недопустимое значение поля — проверьте введённые данные"],
  [/process exited immediately/i, "движок сразу завершился — подробности в «Журнале»"],
  [/launch_error/i, "не удалось запустить процесс — возможно, отклонён запрос прав администратора"],
  [/panic|panicked/i, "внутренняя ошибка программы — подробности в «Журнале»"],
];

const TECH_RE = /os error|error|failed|denied|http |invalid|panic|refused|timed out/i;
const CYR_RE = /[а-яё]/i;

function humanError(raw) {
  const s = String(raw == null ? "" : raw).trim();
  if (!s) return "неизвестная ошибка — подробности в «Журнале»";
  // Своё понятное сообщение (на русском и без технических маркеров) не портим.
  if (CYR_RE.test(s) && !TECH_RE.test(s)) return s;
  for (const [re, msg] of ERROR_RULES) if (re.test(s)) return msg;
  return "непредвиденная ошибка: " + (s.length > 220 ? s.slice(0, 220) + "…" : s);
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
    empty.textContent = logState.items.length ? "ничего не найдено" : "пока пусто";
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
  if (foot) foot.textContent = `показано ${items.length} из ${logState.items.length}`;
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
  ok: "актуально",
  avail: "обновить",
  new: "новый",
  modified: "изменён",
  err: "ошибка",
  "skip-user": "пропущен",
  unknown: "—",
};

// ------------------------------------------------------------- тема оформления
const THEMES = ["grey", "dark", "light"];

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
  const fromState = B && B.settings && B.settings.theme;
  if (THEMES.includes(fromState)) return fromState;
  try { return localStorage.getItem("zgui.theme") || "grey"; } catch (_) { return "grey"; }
}

async function pickTheme(theme) {
  applyTheme(theme);
  try {
    await invoke("set_theme", { theme });
  } catch (e) {
    toast("err", "тема: " + e);
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

function shortPath(p) {
  if (!p) return "—";
  return p;
}

// ------------------------------------------------------------- bootstrap

async function refreshAll(notify = false) {
  const refreshButton = $("#btnRefresh");
  btnBusy(refreshButton, true);
  try {
    B = await invoke("bootstrap");
    onBootstrap();
    if (notify) toast("ok", "статус обновлён");
  } catch (e) {
    toast("err", "bootstrap: " + e);
  } finally {
    btnBusy(refreshButton, false);
  }
}

function onBootstrap() {
  applyTheme(currentTheme());
  renderRunBar();
  ensureEngineCards();
  renderEngineTabs();
  renderEngines();
  renderWarnings();
  renderProfiles();
  renderTestCard();
  renderAutostart();
  renderEngineUpd();
  renderSettings();
  const bt = B && B.updates && B.updates.lastCheck;
  $("#lastCheck").textContent = bt ? "последняя проверка: " + tsText(Number(bt)) : "не проверялось";
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
      cnt.textContent = ` — ${g.pids.length} процессов`;
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
  $("#conflictTitle").textContent = opts.title || "Обнаружено конфликтующее ПО";
  $("#conflictHint").textContent = opts.hint || "Рекомендуем выгрузить эти процессы перед использованием.";
  $("#btnKillConflicts").textContent = opts.killLabel || "Выгрузить процессы";
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

function showConfirm({ title, html, okLabel = "Продолжить", cancelLabel = "Отмена", danger = false, delaySec = 0 }) {
  return new Promise((resolve) => {
    cmResolve = resolve;
    $("#cmTitle").textContent = title;
    $("#cmBody").innerHTML = html;
    const ok = $("#cmOk");
    ok.textContent = okLabel;
    ok.className = "btn " + (danger ? "danger" : "primary");
    $("#cmCancel").textContent = cancelLabel;
    $("#confirmModal").classList.remove("hidden");
    if (delaySec > 0) lockBtnWithCountdown(ok, delaySec);
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
/// Повторяет выгрузку, пока конфликты не уйдут (они могут подниматься не сразу).
async function autoKillAndProceed(retry, attempt = 0) {
  if (conflictKilling && attempt === 0) return;
  conflictKilling = true;
  try {
    if (attempt >= 5) {
      toast("warn", "не удалось выгрузить все конфликты — проверьте вручную");
      return;
    }
    try {
      await invoke("kill_conflicts");
    } catch (e) {
      toast("err", String(e));
    }
    // Дать процессам время умереть, затем перепроверить.
    await new Promise((r) => setTimeout(r, 2000));
    const report = await invoke("conflict_check").catch(() => null);
    const still =
      report && ((report.processes || []).length || (report.vpn || []).length || report.foreignService);
    if (still) {
      // await — чтобы флаг conflictKilling держался до конца цепочки.
      return await autoKillAndProceed(retry, attempt + 1);
    }
    toast("ok", "конфликтующие процессы выгружены");
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

/// Блокирует кнопку на `secs` секунд и показывает рядом маленький таймер.
/// Нужно, чтобы фоновый авто-прогон конфигов успел начаться до перезапуска.
function lockBtnWithCountdown(btn, secs) {
  if (btn.disabled) return;
  btn.disabled = true;
  const timer = document.createElement("span");
  timer.className = "countdown-timer";
  const update = () => {
    timer.textContent = `…${secs}с`;
    if (secs <= 0) {
      clearInterval(iv);
      timer.remove();
      btn.disabled = false;
    }
  };
  const iv = setInterval(() => {
    secs--;
    update();
  }, 1000);
  btn.after(timer);
  update();
}

/// Перезапуск от администратора — общий диалог для первого запуска, чекбокса
/// в «Настройках» и кнопки «Перезапустить от админа».
async function askRestartAdmin() {
  const ok = await showConfirm({
    title: "Перезапуск от администратора",
    okLabel: "Перезапустить",
    html: `<p>Программа будет перезапущена с правами администратора.</p>
           <p class="sub">Текущее окно закроется, откроется новое. Один раз подтвердите запрос Windows.</p>`,
  });
  if (!ok) return;
  try {
    await invoke("relaunch_as_admin");
  } catch (e) {
    toast("err", String(e));
  }
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
  t.textContent = "Важно перед запуском";
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
  const nameOf = (id) => (B.profiles.find((p) => p.id === id) || {}).name || id || "";
  const svcStrategy = (B.service && B.service.strategy) || null;
  if (opRunning) {
    // Идёт долгая операция (обновления, DNS, служба, сброс сети, тест):
    // старт/стоп профилей запрещён — взаимная блокировка.
    st.className = "run-state busy";
    st.textContent = "идёт операция…";
  } else if (owner === "test") {
    // Идёт прогон тестов: winws управляется тестом — останавливать его тулбаром нельзя.
    st.className = "run-state running";
    st.textContent = "идёт тест стратегий";
  } else if (owner === "app") {
    st.className = "run-state running";
    st.textContent = rt ? nameOf(rt.profileId) : "запущено";
  } else if (owner === "service") {
    st.className = "run-state running";
    st.textContent = "служба: " + (svcStrategy ? nameOf(svcStrategy) : "запущена");
  } else if (owner === "external") {
    // winws нашего движка поднят вне программы (ручной .bat): показываем и
    // разрешаем остановить, иначе второй winws конфликтует с запущенным.
    st.className = "run-state running";
    st.textContent = "winws запущен вне программы";
  } else {
    st.className = "run-state idle";
    st.textContent = "не запущено";
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
  el.textContent = s.alarm ? "стратегия не отвечает" : "стратегия активна";
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
      st.textContent = info.ready ? "готов" : "нужен файл " + (info.exe || "");
      rootEl.innerHTML = "";
      const pathEl = document.createElement("span");
      pathEl.className = "root-path";
      pathEl.textContent = shortPath(info.path);
      rootEl.appendChild(pathEl);

      const acts = document.createElement("div");
      acts.className = "root-actions";
      acts.appendChild(btn("Открыть папку", "ghost", () => invoke("open_path", { path: info.path })));
      acts.appendChild(btn("Сменить…", "ghost", async () => {
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
      acts.appendChild(btn("Обновить движок", "ghost", () => doFetch(eng)));
      rootEl.appendChild(acts);
    } else {
      st.className = "state-chip warn";
      st.textContent = "не установлен";
      rootEl.innerHTML = "";
      const acts = document.createElement("div");
      acts.className = "root-actions";
       acts.appendChild(btn("Установить движок", "primary", () => doFetch(eng)));
      acts.appendChild(btn("Выбрать папку…", "ghost", async () => {
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
  toast("info", `Скачиваю движок ${label}… это может занять пару минут`);
  invoke("fetch_engine", { engine: eng, dest: null })
    .catch((e) => toast("err", String(e)))
    .finally(() => setTimeout(() => btnBusy($("#engine-" + eng + " .btn.primary"), false), 3000));
}

// ------------------------------------------------------------- engine registry

function engineLabel(id) {
  const e = ((B && B.engines) || []).find((x) => x.id === id);
  return e ? e.label : id;
}

/// Карточки движков на вкладке «Обновления»: статичные для flowseal (HTML),
/// остальные создаём из bootstrap.engines (id, label, статус, кнопки).
let engineCardsBuilt = "";
function ensureEngineCards() {
  const wrap = document.querySelector(".engines");
  if (!wrap || !B.engines) return;
  const sig = B.engines.map((e) => e.id).join(",");
  if (sig === engineCardsBuilt) return;
  engineCardsBuilt = sig;
  for (const e of B.engines) {
    if (e.id === "flowseal" || $("#engine-" + e.id)) continue;
    const card = document.createElement("div");
    card.className = "card engine";
    card.id = "engine-" + e.id;
    const head = document.createElement("div");
    head.className = "card-head";
    const h = document.createElement("h3");
    h.textContent = e.label;
    const chip = document.createElement("span");
    chip.className = "chip";
    chip.textContent = e.exe || "";
    h.appendChild(chip);
    const st = document.createElement("span");
    st.className = "state-chip";
    st.id = "engState-" + e.id;
    head.appendChild(h);
    head.appendChild(st);
    card.appendChild(head);
    const sub = document.createElement("p");
    sub.className = "muted card-sub";
    sub.textContent = "Скачивается из последнего релиза " + (e.repo || "");
    card.appendChild(sub);
    const root = document.createElement("div");
    root.className = "root-row";
    root.id = "engRoot-" + e.id;
    card.appendChild(root);
    wrap.appendChild(card);
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

/// Табы движков на вкладке «Стратегии»: рендерятся из bootstrap.engines.
/// Пустой профиль-фильтр «all» + по движку; статус готовности виден прямо в табе.
let engineTabsBuilt = "";
function renderEngineTabs() {
  const wrap = $("#profileTabs");
  if (!wrap || !B.engines) return;
  const sig = B.engines.map((e) => `${e.id}:${e.ready ? 1 : 0}`).join(",");
  if (sig === engineTabsBuilt) return;
  engineTabsBuilt = sig;
  wrap.innerHTML = "";
  const mkTab = (filter, label, ready) => {
    const t = document.createElement("button");
    t.className = "tab" + (profileFilter === filter ? " active" : "");
    t.dataset.filter = filter;
    t.textContent = label;
    if (ready === false) {
      const dot = document.createElement("span");
      dot.className = "tab-dot";
      dot.title = "движок не установлен — нажмите «Скачать» на вкладке «Обновления»";
      t.appendChild(dot);
    }
    t.addEventListener("click", () => {
      $$("#profileTabs .tab").forEach((x) => x.classList.remove("active"));
      t.classList.add("active");
      profileFilter = filter;
      renderProfiles();
    });
    wrap.appendChild(t);
  };
  mkTab("all", "Все");
  for (const e of B.engines) {
    mkTab(e.id, e.label, e.ready);
  }
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
    list.innerHTML = '<div class="empty">Нет профилей. Синхронизируйте каталог или создайте свой.</div>';
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
    eng.textContent = p.engine === "flowseal" ? "winws" : engineLabel(p.engine);
    chips.appendChild(eng);
    if (bestId === p.id) {
      const b = document.createElement("span");
      b.className = "chip best";
      b.textContent = "лучшая";
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
      b.textContent = "шаблон";
      chips.appendChild(b);
    }
    if (autoProfile === p.id) {
      const b = document.createElement("span");
      b.className = "chip best";
      b.textContent = "автозапуск";
      chips.appendChild(b);
    }
    if (B.service && B.service.installed && B.service.strategy === p.id) {
      const b = document.createElement("span");
      b.className = "chip best";
      b.textContent = "служба";
      chips.appendChild(b);
    }
    tile.appendChild(chips);

    // Кнопки.
    const acts = document.createElement("div");
    acts.className = "profile-actions";
    const runBtn = btn(isRun ? "Остановить" : "Запустить", "primary", async () => {
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
    more.title = "Параметры стратегии";
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
  bits.push(p.engine === "flowseal" ? "движок winws" : "движок " + engineLabel(p.engine));
  if (p.updatedAt) bits.push("обновлён " + tsText(Number(p.updatedAt)));
  if (p.source) bits.push(p.source);
  $("#pmMeta").textContent = bits.join(" · ");
  $("#pmArgs").textContent = (p.args || []).join("\n");
  const authorProfile =
    p.builtin || (p.source && (p.source.startsWith("preset:") || p.source.toLowerCase().endsWith(".bat")));
  $("#pmDelete").classList.toggle("hidden", !!authorProfile);
  $("#profileModal").classList.remove("hidden");
}

function closeProfileModal() {
  pmProfile = null;
  $("#profileModal").classList.add("hidden");
}

// ------------------------------------------------------------- test

function flowsealProfiles() {
  // Все профили УСТАНОВЛЕННЫХ движков (матрица автоподбора).
  // B может быть null в момент перезагрузки (zgui:updates) — не роняем рендер.
  const engines = ((B && B.engines) || []).filter((e) => e.ready).map((e) => e.id);
  return ((B && B.profiles) || []).filter((p) => engines.includes(p.engine));
}

function renderTestCard() {
  const pick = $("#testPick");
  if (!pick) return;
  const profs = flowsealProfiles();
  if (!profs.length) {
    pick.innerHTML = '<div class="empty">Нет доступных движков — скачайте хотя бы один на вкладке «Обновления».</div>';
    $("#btnRunTest").disabled = true;
    return;
  }
  $("#btnRunTest").disabled = !!(testState && testState.running);
  if ($("#btnRunGeoblock")) $("#btnRunGeoblock").disabled = !!(testState && testState.running);
  if ($("#btnStopTest")) $("#btnStopTest").disabled = !(testState && testState.running);
  $("#testSelectAll").checked = testPicked === null || testPicked.size === profs.length;
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
      $("#testSelectAll").checked = testPicked.size === profs.length;
    });
    wrap.appendChild(cb);
    const nm = document.createElement("span");
    nm.className = "test-name";
    nm.textContent = p.name;
    wrap.appendChild(nm);
    if (p.id === best) {
      const b = document.createElement("span");
      b.className = "chip best";
      b.textContent = "лучшая";
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
  const results = (testState && testState.results && testState.results.length
    ? testState.results
    : (testCache && testCache.results) || []) || [];
  // Прогресс теста прилетает каждые ~0.7 с — перерисовываем список только при
  // реальном изменении набора результатов (иначе лишние тысячи DOM-узлов на кадр).
  const sig = JSON.stringify([
    results.length,
    (testState && testState.done) || false,
    (testState && testState.bestName) || "",
    results.length ? (results[results.length - 1].id || "") + ":" + results[results.length - 1].score : "",
    testState && testState.index,
  ]);
  if (sig === sigTestResults) return;
  sigTestResults = sig;
  if (!results.length) {
    box.innerHTML = "";
    bestBox.classList.add("hidden");
    return;
  }
  if (testState && testState.done && testState.bestName) {
    bestBox.classList.remove("hidden");
    bestBox.innerHTML = "";
    const t = document.createElement("div");
    t.className = "test-best-title";
    t.textContent = `Лучшая стратегия: ${testState.bestName}`;
    bestBox.appendChild(t);
    const b = btn("Применить: автозапуск + запустить сейчас", "primary small", async () => {
      btnBusy(b, true);
      try {
        await invoke("apply_best_strategy", { id: testState.bestId });
        toast("ok", "готово: автозапуск включён, стратегия запущена");
      } catch (e) {
        toast("err", String(e));
      }
      btnBusy(b, false);
      await refreshAll();
    });
    bestBox.appendChild(b);
  } else {
    bestBox.classList.add("hidden");
  }

  box.innerHTML = "";
  for (const r of results) {
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
    grp.textContent = engineLabel(r.engine) || r.engine;
    head.appendChild(grp);
    const sc = document.createElement("span");
    sc.className = "test-row-score";
    sc.textContent = r.started ? (r.criticalOk ? "успешна" : "критические домены не прошли") : "не запустилась";
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
      summary.textContent = `Подробности по доменам (${r.score}/${r.maxScore})`;
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

async function runTest(already, mode) {
  const geoblock = mode === "geoblock";
  const profs = flowsealProfiles();
  const useIds = testPicked === null ? profs.map((p) => p.id) : [...testPicked];
  if (!useIds.length) {
    toast("warn", "не выбрано ни одной стратегии");
    return;
  }
  const btn = geoblock ? $("#btnRunGeoblock") : $("#btnRunTest");
  // Взаимная блокировка: тест не запускается параллельно с другой операцией.
  if (B && B.opRunning) {
    toast("warn", "идёт другая операция — дождитесь завершения");
    return;
  }
  if (!already) {
    const ok = await showConfirm({
      title: geoblock ? "Геоблок-тест" : "Тест стратегий",
      okLabel: "Запустить",
      cancelLabel: "Отмена",
      html: geoblock
        ? `<p>Будет прогнан <b>весь онлайн-список geoblock</b> (без лимита) по выбранным стратегиям,
             с базовой пробой без Zapret.</p>
           <p class="sub">Это может занять продолжительное время. Windows запросит права администратора —
             они нужны, чтобы запускать обход (один раз).</p>`
        : `<p>Будет запущено <b>${useIds.length}</b> стратегий по очереди (все выбранные движки).</p>
           <p class="sub">Windows запросит права администратора — они нужны, чтобы запускать обход
             (один раз на весь тест).</p>`,
    });
    if (!ok) return;
  }
  btnBusy(btn, true);
  try {
    await invoke("test_strategies", { ids: useIds, mode: geoblock ? "geoblock" : "main" });
    toast("info", geoblock ? "геоблок-тест запущен" : "тест стратегий запущен");
  } catch (e) {
    const msg = String(e);
    if (msg.includes("VPN_RUNNING")) {
      const report = await invoke("vpn_check").catch(() => ({ vpn: [] }));
      showConflict(report, {
        title: "VPN мешает тесту",
        hint: "На время теста стратегий VPN нужно выгрузить. Выгрузить VPN и продолжить тест?",
        killLabel: "Выгрузить VPN и продолжить",
        onKilled: async () => {
          await new Promise((r) => setTimeout(r, 1500));
          runTest(true, mode);
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

// ------------------------------------------------------------- new profile

function showNewProfileCard(on) {
  $("#newProfileCard").classList.toggle("hidden", !on);
  if (on) {
    $("#npName").value = "";
    $("#npArgs").value = "";
    const sel = $("#npEngine");
    if (sel && B.engines) {
      sel.innerHTML = "";
      for (const e of B.engines) {
        const o = document.createElement("option");
        o.value = e.id;
        o.textContent = e.label + (e.ready ? "" : " (не установлен)");
        sel.appendChild(o);
      }
      // Открыт с активного фильтра движка — сразу подставляем его.
      if (profileFilter !== "all") sel.value = profileFilter;
    }
  }
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
    el.textContent = "Не удалось проверить обновление движка.";
    el.className = "muted card-sub warn";
    return;
  }
  if (u.upToDate) {
    el.textContent = `Движок актуален${u.installed ? " (" + u.installed + ")" : ""}.`;
    el.className = "muted card-sub";
  } else {
    el.textContent =
      "Доступно обновление движка" +
      (u.latest ? ": " + u.latest : "") +
      (u.installed ? " (у вас " + u.installed + ")" : "") +
      ". Нажмите «Обновить движок».";
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
  setUpdatesBusy("Проверяю обновления (конфиги, движок, Telegram)…", 180000);
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
  setUpdatesBusy("Применяю обновления…", 300000);
  try {
    await invoke("apply_updates", { ids });
    toast("info", "применение запущено…");
  } catch (e) {
    clearUpdatesBusy();
    toast("err", "применение: " + e);
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
      ? "Каталог пуст. Сначала установите движки, затем «Проверить обновления»."
      : "Каталог загружается автоматически при старте…";
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
    state.textContent = errors ? `${errors} ошибок` : available ? `${available} обновить` : "актуально";
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
      label.innerHTML = ``;
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
    update_interval_hours: Number($("#cfInterval").value) || 0,
    game_filter: $("#cfGameFilter").value,
    ipset_mode: $("#cfIpset").value,
    autostart_mode: $("#cfAutostart").value ? "profile" : "none",
    autostart_profile: $("#cfAutostart").value || null,
    always_admin: $("#cfAlwaysAdmin").checked,
    tg_autostart: $("#tgAutostart")?.checked || false,
    tg_port: Number($("#tgPort")?.value) || 1443,
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
  $("#cfIpset").value = s.ipset_mode || "loaded";
  const adm = $("#cfAlwaysAdmin");
  if (adm) adm.checked = !!s.always_admin;
  if ($("#tgAutostart")) $("#tgAutostart").checked = !!s.tg_autostart;
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
    sel.innerHTML = '<option value="">— не запускать —</option>';
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
    box.title = !svcInstalled && !chosen ? "Сначала выберите профиль" : "";
  }

  const chip = $("#bootStateChip");
  if (chip) {
    chip.textContent = svcInstalled ? "служба" : bootOn ? "включён" : "выключен";
    chip.className = "chip" + (svcInstalled || bootOn ? " best" : "");
  }

  const note = $("#bootStateNote");
  if (note) {
    const profile = (B.profiles || []).find((p) => p.id === s.autostart_profile);
    if (svcInstalled) {
      note.textContent =
        "Обход включается сам службой — программа для запуска не нужна." +
        (profile ? ` Профиль «${profile.name}».` : "");
    } else if (bootOn && profile) {
      note.textContent = `Автозапуск включён: при входе в Windows обход запустится сам — «${profile.name}».`;
    } else if (bootOn && !profile) {
      note.textContent = "Автозапуск включён, но профиль не выбран — выберите профиль.";
    } else if (profile) {
      note.textContent = `При входе будет запускаться «${profile.name}». Если не сработало — запустите программу от администратора.`;
    } else {
      note.textContent = "Автозапуск выключен.";
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
    const ping = ms != null ? ` · ${ms} мс` : "";
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
  const ping = ms != null ? ` · пинг ~${ms} мс` : "";
  info.textContent = `${p.note}${ping}. DoH: ${p.dohTemplate}; UDP fallback: выключен.`;
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
  out.textContent = "Тестирую пинг (медиана из 3 запросов на адрес)…";
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
        const val = r.avgMs != null ? `${r.avgMs} мс` : (r.error || "н/д");
        return `<div class="dns-bench-row"><span>${name}</span><span>${val}</span></div>`;
      })
      .join("");
    out.innerHTML = `<div class="dns-bench-note">Пинг DNS (меньше — быстрее):</div>${rows}`;
    renderDnsProviders();
  } catch (e) {
    out.textContent = "Не удалось замерить: " + e;
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
    if (info) info.textContent = "Не удалось загрузить список DNS: " + e;
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
  el.textContent = "Проверяю версию моста…";
  el.className = "muted";
  try {
    const info = await invoke("tg_check_update");
    if (info.updateAvailable) {
      el.textContent =
        "Доступно обновление моста: " +
        (info.upstreamVersion || "новее") +
        " (у вас " +
        info.localVersion +
        "). Обновите Z GUI.";
      el.className = "muted warn";
    } else {
      el.textContent = "Мост актуален (версия " + info.localVersion + ").";
      el.className = "muted";
    }
  } catch (e) {
    el.textContent = "";
  }
}

function renderTg() {
  const badge = $("#tgBadge");
  const toggle = $("#btnTgToggle");
  const connect = $("#btnTgConnect");
  const hint = $("#tgHint");
  if (!badge) return;
  const on = !!tgState.running;
  badge.textContent = on ? "работает" : "выключен";
  badge.classList.toggle("ok", on);
  toggle.textContent = on ? "Выключить" : "Включить";
  toggle.classList.toggle("danger", on);
  connect.disabled = !on;
  if (tgState.error) {
    hint.textContent = "Ошибка: " + tgState.error;
    hint.className = "muted err";
  } else if (on) {
    hint.textContent = `Готово. Порт ${tgState.port}. Нажмите «Подключить Telegram».`;
    hint.className = "muted";
  } else {
    hint.textContent =
      "После включения нажмите «Подключить Telegram» — мессенджер настроится автоматически.";
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
        tg_port: Number($("#tgPort")?.value) || 1443,
      },
    });
  } catch (_) {}
}

async function tgToggle() {
  btnBusy($("#btnTgToggle"), true);
  try {
    if (tgState.running) {
      tgState = await invoke("tg_stop");
      toast("info", "Telegram-прокси выключен");
    } else {
      const port = Number($("#tgPort")?.value) || 1443;
      tgState = await invoke("tg_start", { port });
      toast("ok", "Telegram-прокси включён");
    }
  } catch (e) {
    toast("err", String(e));
  } finally {
    btnBusy($("#btnTgToggle"), false);
    renderTg();
  }
}

async function tgConnect() {
  if (!tgState.link) {
    toast("warn", "Сначала включите прокси");
    return;
  }
  try {
    await invoke("open_url", { url: tgState.link });
    toast("info", "Открываю Telegram — подтвердите подключение");
  } catch (e) {
    // Fallback: скопировать ссылку в буфер.
    try {
      await navigator.clipboard.writeText(tgState.link);
      toast("ok", "Ссылка скопирована в буфер обмена");
    } catch (_) {
      toast("err", String(e));
    }
  }
}

// ------------------------------------------------------------- wiring

function bindStatic() {
  $$(".nav-item").forEach((n) =>
    n.addEventListener("click", () => {
      $$(".nav-item").forEach((x) => x.classList.remove("active"));
      n.classList.add("active");
      $$(".page").forEach((p) => p.classList.remove("active"));
      $("#view-" + n.dataset.view).classList.add("active");
      if (n.dataset.view === "updates") refreshAll();
      if (n.dataset.view === "telegram") refreshTg();
      if (n.dataset.view === "dns") loadDnsProviders();
      if (n.dataset.view === "appearance") applyTheme(currentTheme());
      if (n.dataset.view === "logs") openLogs();
      else stopLogPoll();
    }),
  );

  $("#btnStop").addEventListener("click", async () => {
    try {
      await invoke("stop_running");
      await refreshAll();
    } catch (e) {
      toast("err", String(e));
    }
  });

  $$("#themeGrid .theme-card").forEach((c) =>
    c.addEventListener("click", () => pickTheme(c.dataset.theme)),
  );

  if ($("#btnTgToggle")) $("#btnTgToggle").addEventListener("click", tgToggle);
  if ($("#btnTgConnect")) $("#btnTgConnect").addEventListener("click", tgConnect);
  if ($("#tgAutostart")) $("#tgAutostart").addEventListener("change", tgSavePrefs);
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
    $("#btnLogCopy").addEventListener("click", async () => {
      const text = logVisible()
        .map((e) => `[${logTime(e.ts)}] [${e.level}] ${e.scope}: ${e.msg}`)
        .join("\n");
      try {
        await navigator.clipboard.writeText(text || "журнал пуст");
        toast("ok", "журнал скопирован в буфер обмена");
      } catch (_) {
        toast("err", "не удалось скопировать журнал");
      }
    });
    $("#btnLogClear").addEventListener("click", async () => {
      const ok = await showConfirm({
        title: "Очистить журнал?",
        html: "Записи в окне и в файле <b>zgui.log</b> будут удалены.",
        okLabel: "Очистить",
        danger: true,
      });
      if (!ok) return;
      try {
        await invoke("log_clear");
      } catch (_) {}
      logState.items = [];
      logState.seq = 0;
      renderLog();
      toast("ok", "журнал очищен");
    });
    $("#btnLogOpenDir").addEventListener("click", () =>
      invoke("log_dir_open").catch((e) => toast("err", e.message)),
    );
    $("#btnReportSave").addEventListener("click", async () => {
      btnBusy($("#btnReportSave"), true);
      try {
        const p = await invoke("report_save");
        toast("ok", "отчёт сохранён и открыт: " + p);
      } catch (e) {
        toast("err", e.message);
      } finally {
        btnBusy($("#btnReportSave"), false);
      }
    });
  }
  if ($("#cmCancel")) $("#cmCancel").addEventListener("click", () => closeConfirm(false));
  if ($("#cmX")) $("#cmX").addEventListener("click", () => closeConfirm(false));

  if ($("#pmClose")) $("#pmClose").addEventListener("click", closeProfileModal);
  if ($("#pmClose2")) $("#pmClose2").addEventListener("click", closeProfileModal);
  if ($("#pmDelete"))
    $("#pmDelete").addEventListener("click", async () => {
      const p = pmProfile;
      if (!p) return;
      const ok = await showConfirm({
        title: "Удалить стратегию?",
        danger: true,
        okLabel: "Удалить",
        html: `<p>Удалить стратегию «<b>${p.name}</b>»?</p><p class="sub">Это действие нельзя отменить.</p>`,
      });
      if (!ok) return;
      try {
        await invoke("delete_profile", { id: p.id });
        await refreshAll();
        closeProfileModal();
      } catch (e) {
        toast("err", String(e));
      }
    });

  $("#btnNewProfile").addEventListener("click", () => showNewProfileCard(true));
  $("#btnCancelProfile").addEventListener("click", () => showNewProfileCard(false));

  $("#btnRunTest").addEventListener("click", () => runTest(false, "main"));
  $("#btnRunGeoblock").addEventListener("click", () => runTest(false, "geoblock"));
  $("#btnStopTest").addEventListener("click", async () => {
    btnBusy($("#btnStopTest"), true);
    try {
      await invoke("cancel_test");
      toast("info", "останавливаю тест…");
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
    const profs = flowsealProfiles();
    testPicked = e.target.checked ? new Set(profs.map((p) => p.id)) : new Set();
    renderTestCard();
  });

  $("#btnSaveProfile").addEventListener("click", async () => {
    const name = $("#npName").value.trim();
    const raw = $("#npArgs").value;
    const engine = ($("#npEngine") && $("#npEngine").value) || "flowseal";
    const args = raw.split("\n").map((x) => x.trim()).filter(Boolean);
    if (!name || !args.length) {
      toast("warn", "Укажите название и хотя бы один аргумент");
      return;
    }
    try {
      await invoke("save_profile", { id: null, name, engine, args });
      await refreshAll();
      showNewProfileCard(false);
      toast("ok", "Профиль сохранён");
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
      toast("warn", "Ничего не выбрано");
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
  for (const id of ["#cfGameFilter", "#cfIpset"]) {
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
          toast("ok", "служба переключена на выбранную стратегию");
        } else {
          // Сначала сбрасываем автозапуск, иначе remove_service сохранит профиль
          // и переведёт обход на программный автозапуск — «выключено» не сработает.
          const s = collectSettings();
          s.autostart_mode = "none";
          s.autostart_profile = null;
          await invoke("set_settings", { settings: s });
          await invoke("remove_service");
          toast("ok", "автозапуск выключен");
        }
      } else {
        // Профиль сохранён — задачу планировщика согласует бэкенд (sync_autostart).
        await invoke("set_settings", { settings: collectSettings() });
        toast("ok", val ? "готово: обход будет включаться сам при входе в Windows" : "автозапуск выключен");
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
        if (!id) throw "сначала выберите профиль";
        // Программный автозапуск снимется сам (бэкенд согласует механизмы).
        toast("info", "Ставлю службу — Windows запросит права администратора для её создания");
        await invoke("install_service", { id });
        toast("ok", "готово: обход будет включаться службой — программа не нужна");
      } else {
        await invoke("remove_service");
        toast("ok", "служба выключена");
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
        ? "GUI будет запускаться от администратора"
        : "запуск от администратора выключен");
      if ($("#cfAlwaysAdmin").checked) askRestartAdmin();
    } catch (e) {
      toast("err", String(e));
    }
  });

  $("#btnElevateNow").addEventListener("click", askRestartAdmin);

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
        toast("ok", "Хорошо — при необходимости права запросим отдельно");
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
      toast("err", "DNS: " + e);
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
      toast("err", "DNS: " + e);
    }
    btnBusy(b, false);
  });

  if ($("#btnNetReset")) $("#btnNetReset").addEventListener("click", netReset);
  if ($("#btnShowAdapters")) $("#btnShowAdapters").addEventListener("click", showAdapters);
}

async function netReset() {
  const ok = await showConfirm({
    title: "Восстановить интернет",
    danger: true,
    okLabel: "Начать восстановление",
    cancelLabel: "Отмена",
    html: `
      <p>Программа выполнит <b>по шагам</b>:</p>
      <ul>
        <li><b>1.</b> Создаст точку восстановления Windows (может занять 1–2 минуты).</li>
        <li><b>2.</b> Остановит службу zapret и VPN-службы (включая AmneziaVPN) и завершит их процессы.</li>
        <li><b>3.</b> Уберёт зависшие драйверы WinDivert.</li>
        <li><b>4.</b> Сбросит прокси (WinHTTP и системный), кэш DNS, Winsock и стек TCP/IP.</li>
        <li><b>5.</b> Предложит перезагрузку — <b>только отдельной кнопкой</b>, без автоматики.</li>
      </ul>
      <p class="sub">Пароли Wi-Fi, профили подключения и настройки провайдера <b>НЕ трогаются</b>.</p>
      <p class="sub">Изменения Winsock/TCP-IP вступят в силу только после перезагрузки.</p>
    `,
  });
  if (!ok) return;
  const b = $("#btnNetReset");
  const out = $("#netResetOut");
  btnBusy(b, true);
  out.classList.remove("hidden");
  out.className = "muted";

  // Шаг 1: точка восстановления. Пока она не создана — сброс не запускаем.
  out.textContent = "Шаг 1/2: создаю точку восстановления…";
  let restoreOk = false;
  try {
    const msg = await invoke("net_create_restore_point");
    restoreOk = true;
    out.textContent = "Точка восстановления: " + msg + "\nШаг 2/2: выполняю сброс сети…";
  } catch (e) {
    out.className = "muted warn";
    out.textContent = "Точка восстановления не создана: " + e;
    const proceed = await showConfirm({
      title: "Продолжить без точки восстановления?",
      danger: true,
      okLabel: "Продолжить без точки",
      cancelLabel: "Отмена",
      html: `<p>Не удалось создать точку восстановления:</p><p class="sub">${e}</p>
             <p>Можно продолжить сброс сети без возможности отката системы.</p>`,
    });
    if (!proceed) {
      out.textContent += "\nОперация отменена.";
      btnBusy(b, false);
      return;
    }
    out.className = "muted";
    out.textContent = "Выполняю сброс сети без точки восстановления…";
  }

  // Шаг 2: сам сброс сети.
  try {
    const r = await invoke("net_reset");
    const steps = (r.steps || []).map((s) => "• " + s).join("\n");
    out.className = "muted ok";
    out.textContent =
      (restoreOk ? "Точка восстановления создана.\n" : "") +
      steps +
      (r.rebootRequired ? "\n\nГотово. Изменения вступят в силу после перезагрузки." : "");
    toast("ok", "Сеть сброшена — нужна перезагрузка");
    if (r.rebootRequired) {
      // Перезагрузка — ТОЛЬКО по явному клику в окне. Никакой автоматики.
      const reboot = await showConfirm({
        title: "Нужна перезагрузка",
        danger: true,
        okLabel: "Перезагрузить сейчас",
        cancelLabel: "Позже, вручную",
        html: `
          <p>Сброс Winsock и TCP/IP вступает в силу <b>только после перезагрузки</b>.</p>
          <p>Можно перезагрузить сейчас или позже вручную — интернет заработает после неё.</p>
        `,
      });
      if (reboot) {
        toast("info", "Перезагрузка через 15 секунд…");
        await invoke("reboot_now").catch(() => {});
      }
    }
  } catch (e) {
    out.textContent = "Ошибка: " + e;
    out.className = "muted err";
    toast("err", String(e));
  } finally {
    btnBusy(b, false);
  }
}

async function showAdapters() {
  const out = $("#netResetOut");
  out.classList.remove("hidden");
  out.textContent = "Ищу виртуальные адаптеры…";
  try {
    const list = await invoke("virtual_adapters");
    if (!list.length) {
      out.textContent = "Виртуальных сетевых адаптеров не найдено.";
    } else {
      out.innerHTML =
        "Найдены виртуальные адаптеры (удалять только вручную в Диспетчере устройств, если из-за них проблемы):<br>" +
        list.map((a) => "• " + a).join("<br>");
    }
    out.className = "muted";
  } catch (e) {
    out.textContent = "Ошибка: " + e;
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
  await listen("zgui:conflict", () => {
    if (!conflictKilling) checkConflicts(false);
  });
  await listen("zgui:test", (ev) => {
    testState = ev.payload;
    renderTestProgress();
    renderTestResults();
    // Пока идёт тест — тулбар показывает «идёт тест», «Остановить» заблокирована.
    renderRunBar();
    if (testState && testState.done) {
      btnBusy($("#btnRunTest"), false);
      btnBusy($("#btnRunGeoblock"), false);
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
renderNavIcons();
bindStatic();

// Свёрнутое/невидимое окно не должно крутить анимацию фона впустую.
document.addEventListener("visibilitychange", () => {
  document.body.classList.toggle("fx-paused", document.hidden);
});

(async function init() {
  await wireEvents();
  try {
    await invoke("ack_boot");
  } catch (_) {}
  await refreshAll();
  loadDnsProviders();
  loadTestCache();
  try {
    const ts = await invoke("test_status");
    if (ts && ts.running) {
      testState = ts;
      renderTestCard();
      renderTestProgress();
    }
  } catch (_) {}
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
  setInterval(refreshAll, 4000);
})();

(async function prefetchIgnore() {
  await invoke("current_status").catch(() => {});
})();
