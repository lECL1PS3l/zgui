// Единый файл всех текстов, которые видит пользователь.
// Стиль «For Dummies»: одна мысль — одна строка, без терминов и воды.

// Склонение числительных: 1 ошибка, 2 ошибки, 5 ошибок.
const plural = (n, one, few, many) => {
  const m10 = n % 10;
  const m100 = n % 100;
  if (m10 === 1 && m100 !== 11) return one;
  if (m10 >= 2 && m10 <= 4 && (m100 < 12 || m100 > 14)) return few;
  return many;
};

export const T = {
  // ------------------------------------------------------------- общее
  app_title: "Zapret GUI",
  slogan: "Локальная оптимизация сетевых пакетов для стабильной работы ваших приложений",
  logo_alt: "Логотип Zapret GUI",

  // ------------------------------------------------------------- навигация
  nav_strategies: "Стратегии",
  nav_tests: "Тест стратегий",
  nav_updates: "Обновления",
  nav_telegram: "Telegram",
  nav_dns: "DNS",
  nav_settings: "Настройки",
  nav_tools: "Инструменты",
  nav_logs: "Журнал",
  nav_appearance: "Внешний вид",
  nav_about: "О программе",

  // ------------------------------------------------------------- шапка и статусы
  run_idle: "не запущено",
  run_op: "идёт операция…",
  run_test: "идёт проверка стратегий",
  run_started: "запущено",
  run_service: (name) => `служба: ${name}`,
  service_running: "запущена",
  run_external: "оптимизация включена другой программой",
  wd_alarm: "стратегия не отвечает",
  wd_ok: "стратегия активна",
  btn_stop: "Остановить",
  btn_start: "Запустить",

  // ------------------------------------------------------------- ошибки
  err_admin: "Не хватает прав администратора. Включите их в «Настройках»",
  err_busy: "Файл занят другой программой. Закройте её и повторите",
  err_space: "На диске не хватает места",
  err_not_found: "Файл или папка не найдены. Возможно, движок ещё не установлен",
  err_net: "Нет связи с сервером. Проверьте интернет (или выключите VPN) и повторите",
  err_403: "Сервер отклонил запрос. Возможно, исчерпан лимит GitHub — повторите позже",
  err_404: "На сервере нет такого файла. Обновите программу",
  err_5xx: "Сервер временно недоступен. Попробуйте позже",
  err_invalid: "Недопустимое значение поля. Проверьте данные",
  err_exit: "Движок сразу завершился. Подробности в «Журнале»",
  err_launch: "Не удалось запустить. Возможно, отклонён запрос прав администратора",
  err_panic: "Внутренняя ошибка программы. Подробности в «Журнале»",
  err_unknown: "Неизвестная ошибка. Подробности в «Журнале»",
  err_unexpected: (s) => "Непредвиденная ошибка: " + s,
  err_short: (e) => `Не получилось: ${e}`,
  boot_err: (e) => `Не удалось обновить данные: ${e}`,
  theme_err: (e) => `Тема не сменилась: ${e}`,

  // ------------------------------------------------------------- журнал
  log_empty: "пока пусто",
  log_empty_search: "ничего не найдено",
  log_shown: (n, total) => `показано ${n} из ${total}`,
  log_loading: "загрузка…",
  logs_title: "Журнал",
  logs_note:
    "Здесь все действия программы и ошибки. Если что-то не работает — нажмите «Сохранить отчёт» и отправьте файл разработчику.",
  btn_report_save: "Сохранить отчёт",
  btn_report_issue: "Отправить отчёт",
  btn_log_dir: "Папка отчётов",
  log_search_placeholder: "Поиск по тексту…",
  log_lvl_all: "все",
  log_lvl_err: "ошибки",
  log_lvl_warn: "предупреждения",
  log_lvl_ok: "успешно",
  log_lvl_info: "инфо",
  report_saved: (p) => `Отчёт сохранён и открыт: ${p}`,
  issue_title: "Баг: ",
  issue_body: (path) =>
    "**Что случилось:**\n\n\n**Как воспроизвести:**\n\n\n" +
    (path ? `Отчёт сохранён рядом с журналом: ${path}\n` : ""),
  report_issue_opened: "Открыл форму отчёта в браузере. Приложите файл из папки отчётов",

  // ------------------------------------------------------------- конфликты
  conflict_title: "Найдены мешающие программы",
  conflict_text: "Эти программы могут мешать оптимизации:",
  conflict_vpn_text: "Сначала выключите VPN — вместе с ним оптимизация не работает",
  conflict_hint: "Закройте их кнопкой ниже или вручную.",
  conflict_procs: (n) => ` — ${n} ${plural(n, "процесс", "процесса", "процессов")}`,
  conflict_admin_hint:
    "Нет прав администратора — закрыть процессы не получится. Перезапустите программу от админа или закройте их в диспетчере задач.",
  btn_taskmgr: "Диспетчер задач",
  btn_kill_conflicts: "Закрыть процессы",
  btn_close: "Закрыть",
  about_title: "О программе",
  about_ru_html: (ver) =>
    `<p><b>Z GUI ${ver}</b> — оболочка для авторских движков оптимизации соединения.</p>` +
    `<p>Движки: <a href="#" data-url="https://github.com/Flowseal/zapret-discord-youtube">Flowseal zapret</a> (winws), ` +
    `<a href="#" data-url="https://github.com/bol-van/zapret2">bol-van zapret2</a> (winws2), ` +
    `<a href="#" data-url="https://github.com/ValdikSS/GoodbyeDPI">ValdikSS GoodbyeDPI</a>, ` +
    `<a href="#" data-url="https://github.com/dilluti0n/DPIBreak">dilluti0n DPIBreak</a>.</p>` +
    `<p>Спасибо авторам движков, <a href="#" data-url="https://github.com/lucide-icons/lucide">Lucide Icons</a>, ` +
    `<a href="#" data-url="https://heropatterns.com/">Hero Patterns</a> («Topography», Steve Schoger) и ` +
    `<a href="#" data-url="https://github.com/AmantesNihilo/zapret-universal-interface">tg-ws-proxy-rs</a> за работу и открытые лицензии.</p>` +
    `<p class="sub">Полный список лицензий и атрибуция — файл <b>THIRD_PARTY_NOTICES.md</b> в ` +
    `<a href="#" data-url="https://github.com/lECL1PS3l/zgui">репозитории</a> (на английском).</p>`,
  about_en_html: (ver) =>
    `<p><b>Z GUI ${ver}</b> — a portable shell for third-party connection-optimization engines.</p>` +
    `<p>Engines: <a href="#" data-url="https://github.com/Flowseal/zapret-discord-youtube">Flowseal zapret</a> (winws), ` +
    `<a href="#" data-url="https://github.com/bol-van/zapret2">bol-van zapret2</a> (winws2), ` +
    `<a href="#" data-url="https://github.com/ValdikSS/GoodbyeDPI">ValdikSS GoodbyeDPI</a>, ` +
    `<a href="#" data-url="https://github.com/dilluti0n/DPIBreak">dilluti0n DPIBreak</a>.</p>` +
    `<p>Credits: thanks to the engine authors, <a href="#" data-url="https://github.com/lucide-icons/lucide">Lucide Icons</a> (ISC), ` +
    `<a href="#" data-url="https://heropatterns.com/">Hero Patterns</a> “Topography” by Steve Schoger (CC BY 4.0), ` +
    `<a href="#" data-url="https://github.com/AmantesNihilo/zapret-universal-interface">tg-ws-proxy-rs</a> (MIT).</p>` +
    `<p class="sub">Full license texts and attribution: <b>THIRD_PARTY_NOTICES.md</b> in the ` +
    `<a href="#" data-url="https://github.com/lECL1PS3l/zgui">repository</a>.</p>`,

  // ------------------------------------------------------------- подтверждения
  cm_title: "Подтверждение",
  btn_continue: "Продолжить",
  btn_cancel: "Отмена",

  // ------------------------------------------------------------- права администратора
  admin_title: "Права администратора",
  admin_banner_text:
    "Программа запущена без прав администратора. Windows будет спрашивать разрешение при каждом действии.",
  btn_restart_admin: "Перезапустить от админа",
  btn_hide: "Скрыть",
  label_always_admin: "Всегда запускать от администратора (рекомендуется)",
  admin_restart_title: "Перезапуск от администратора",
  btn_restart: "Перезапустить",
  admin_restart_html:
    '<p>Программа перезапустится с правами администратора.</p><p class="sub">Окно закроется и откроется заново. Подтвердите запрос Windows один раз.</p>',
  admin_modal_text:
    "Чтобы включить оптимизацию, нужны права администратора. Без них Windows будет спрашивать разрешение при каждом действии — до 5 окон подряд.",
  admin_modal_note:
    "Программа один раз перезапустится. Дальше всё работает без запросов. Отключить можно в «Настройках».",
  btn_admin_enable: "Включить и перезапустить",
  btn_not_now: "Не сейчас",
  admin_on: "Программа будет запускаться от администратора",
  admin_off: "Запуск от администратора выключен",
  admin_later_ok: "Хорошо: при необходимости права спросим отдельно",

  // ------------------------------------------------------------- предупреждения
  warn_title: "Важно перед запуском",

  // ------------------------------------------------------------- стратегии
  card_profiles: "Стратегии",
  profiles_empty: "Стратегий пока нет. Проверьте обновления",
  chip_best: "лучшая",
  chip_builtin: "шаблон",
  chip_autostart: "автозапуск",
  chip_service: "служба",
  profile_args_title: "Параметры запуска",
  pm_title: "Стратегия",
  pm_engine: (name) => `движок ${name}`,
  pm_updated: (ts) => `обновлён ${ts}`,

  // ------------------------------------------------------------- обновления
  upd_check_title: "Проверка обновлений",
  btn_check: "Проверить",
  upd_check_note: "Стратегии, списки, Telegram-мост",
  label_every_hours: "каждые (ч)",
  upd_interval_note: "0 — отключить автопроверку",
  chip_none: "нет",
  fs_note: "Встроенная в программу оптимизация, обновляется вручную.",
  upd_cfg_title: "Обновление стратегий",
  upd_cfg_note: "Стратегии и списки, которые программа скачивает с GitHub.",
  btn_apply_available: "Применить доступные",
  btn_apply_selected: "Применить выбранное",
  eng_ready: "готов",
  eng_need_file: (exe) => `нужен файл ${exe}`,
  eng_not_installed: "не установлен",
  btn_open_folder: "Открыть папку",
  btn_change: "Сменить…",
  btn_update_engine: "Обновить движок",
  btn_install_engine: "Установить движок",
  btn_pick_folder: "Выбрать папку…",
  engine_downloading: (label) => `Скачиваю ${label}… это может занять пару минут`,
  engine_from_release: (repo) => `Скачается из последнего релиза на GitHub (${repo})`,
  tab_not_installed: "Не установлен. Откройте «Обновления» и нажмите «Установить движок»",
  filter_all: "Все",
  upd_checking: "Проверяю обновления…",
  upd_applying: "Применяю обновления…",
  upd_apply_started: "обновление запущено…",
  upd_apply_err: (e) => `Не удалось применить обновления: ${e}`,
  upd_empty: "Список пуст. Установите движок и нажмите «Проверить»",
  upd_loading: "Список загружается при старте…",
  upd_group_err: (n) => `${n} ${plural(n, "ошибка", "ошибки", "ошибок")}`,
  upd_group_avail: (n) => `${n} обновить`,
  st_ok: "актуально",
  st_avail: "обновить",
  st_new: "новый",
  st_modified: "изменён",
  st_err: "ошибка",
  st_skip: "пропущен",
  st_unknown: "—",
  last_check: (ts) => `последняя проверка: ${ts}`,
  never_checked: "ещё не проверялось",
  engine_check_fail: "Не удалось проверить обновление. Попробуйте позже",
  engine_uptodate: (ver) => `Всё актуально${ver ? ` (${ver})` : ""}.`,
  engine_update_available: (latest, installed) =>
    `Есть обновление${latest ? `: ${latest}` : ""}${installed ? ` (у вас ${installed})` : ""}. Нажмите «Обновить движок»`,

  // ------------------------------------------------------------- автозапуск
  card_autostart: "Автозапуск",
  label_start_on_login: "Запускать при входе в Windows",
  label_run_as_service: "Запускать службой (без программы)",
  autostart_note:
    "Оба способа включают оптимизацию сами. Выберите один: при входе в Windows или службой при загрузке ПК.",
  auto_none: "— не запускать —",
  auto_pick_first: "Сначала выберите стратегию",
  auto_chip_on: "включён",
  auto_chip_off: "выключен",
  auto_note_service: (name) =>
    `Оптимизацию включает служба — программа не нужна.${name ? ` Стратегия «${name}».` : ""}`,
  auto_note_on: (name) => `Оптимизация включится сама при входе в Windows: «${name}»`,
  auto_note_no_profile: "Автозапуск включён, но стратегия не выбрана. Выберите её",
  auto_note_chosen: (name) =>
    `При входе включится «${name}». Если не сработает — запустите программу от администратора`,
  auto_note_off: "Автозапуск выключен",
  auto_off: "Автозапуск выключен",
  auto_on_login: "Готово: оптимизация включится сама при входе в Windows",
  svc_switched: "Служба переключена на выбранную стратегию",
  svc_installing: "Ставлю службу — Windows спросит права администратора",
  svc_installed: "Готово: оптимизацию включает служба — программа не нужна",
  svc_removed: "Служба выключена",
  pick_profile_first: "Сначала выберите стратегию",

  // ------------------------------------------------------------- тест
  card_tests_title: 'Тест стратегий <span class="chip">autotest</span>',
  label_select_all: "выбрать все",
  btn_run_test: "Запустить тест",
  btn_stop_test: "Остановить тест",
  btn_test_report: "Сохранить результаты",
  btn_test_folder: "Папка тестов",
  test_hint:
    "По очереди проверит стратегии и покажет, какая лучше подходит для YouTube и Discord",
  test_no_engines: "Ни один движок не установлен. Установите его на вкладке «Обновления»",
  test_no_strategies: "У этого движка нет стратегий. Выберите «Все» или другой движок",
  test_best: (name) => `Лучшая стратегия: ${name}`,
  btn_apply_best: "Применить и запустить",
  test_best_applied: "Готово: автозапуск включён, стратегия работает",
  test_no_success: "Ни одна стратегия не подошла: YouTube и Discord не открылись",
  test_no_success_hint: "Попробуйте другой движок",
  test_untested: "не тестировалась",
  test_ok: "успешна",
  test_crit_fail: "YouTube и Discord не открылись",
  test_not_started: "не запустилась",
  test_bad_hosts: (list) => `Не ответили: ${list}`,
  test_all_hosts_ok: "Ответили все проверенные сайты",
  test_details: (score, max) => `Подробности по сайтам (${score}/${max})`,
  test_net_hint:
    '<p class="sub"><b>Это нормально:</b> во время теста интернет может кратко пропадать. После каждого шага связь возвращается. Просто не закрывайте программу.</p>',
  test_confirm_title: "Тест стратегий",
  btn_run: "Запустить",
  test_confirm_html: (n) =>
    `<p>Проверю <b>${n}</b> ${plural(n, "стратегию", "стратегии", "стратегий")} по очереди: какая лучше подойдёт для YouTube и Discord.</p>` +
    '<p class="sub">Главное — YouTube и Discord. Остальные сайты проверяются как получится.</p>',
  test_admin_hint:
    '<p class="sub">Windows один раз спросит права администратора — без них оптимизация не запустится.</p>',
  test_started: "тест запущен",
  test_stopping: "останавливаю тест…",
  test_report_saved: (p) => `Результаты теста сохранены: ${p}`,
  test_report_fail: (e) => `Не удалось сохранить результаты: ${e}`,
  test_busy_other: "Идёт другая операция. Дождитесь завершения",
  nothing_selected: "Ничего не выбрано",
  vpn_test_title: "VPN мешает тесту",
  vpn_test_hint: "Для теста нужно выключить VPN. Закрыть VPN и продолжить?",
  vpn_test_kill: "Закрыть VPN и продолжить",

  // ------------------------------------------------------------- Telegram
  tg_title: "Telegram-прокси",
  tg_off: "выключен",
  tg_on: "работает",
  tg_note:
    "Поможет Telegram работать без VPN. Включите прокси и нажмите «Подключить Telegram».",
  btn_tg_on: "Включить",
  btn_tg_off: "Выключить",
  btn_tg_connect: "Подключить Telegram",
  tg_hint_default:
    "После включения нажмите «Подключить Telegram». Настроится само.",
  tg_settings_title: "Настройки прокси",
  tg_offer_label: "Предлагать, когда запущен Telegram и нет VPN",
  tg_autostart_label: "Включать прокси при старте программы",
  tg_howto_title: "Как отключить прокси в Telegram",
  tg_howto_text:
    "Прокси включается сам, а выключается только в Telegram: Настройки → Продвинутые настройки → Тип подключения. Там же удалите лишнюю запись через ⋮.",
  tg_tips_alt: "Telegram: отключение и удаление прокси",
  tg_tips_caption:
    "1 — меню → Настройки; 2 — Продвинутые настройки; 3 — Тип подключения; 4 — «Отключить прокси»; лишнюю запись удалите через ⋮.",
  tg_bridge_checking: "Проверяю обновление моста…",
  tg_bridge_update: (up, local) =>
    `Есть новая версия моста: ${up || "новее"} (у вас ${local}). Обновите Z GUI`,
  tg_bridge_ok: (v) => `Мост актуален (версия ${v})`,
  tg_ready: (port) => `Готово. Порт ${port}. Нажмите «Подключить Telegram»`,
  tg_stopped: "Telegram-прокси выключен",
  tg_started: "Telegram-прокси включён",
  btn_tg_build: "Подключить",
  tg_offer_html:
    '<p>Telegram запущен, VPN не найден.</p><p class="sub">Включить прокси-мост? Telegram настроится сам, трафик пойдёт через программу.</p>',
  tg_need_on: "Сначала включите прокси",
  tg_opening: "Открываю Telegram — подтвердите подключение",
  tg_link_copied: "Ссылка скопирована",

  // ------------------------------------------------------------- настройки
  settings_title: 'Настройки <span class="chip">применяются сразу</span>',
  settings_gf_hint: "— порты для игр",
  settings_tcp_ports: "TCP-порты",
  settings_udp_ports: "UDP-порты",
  settings_gf_tcp_hint: "— диапазоны для TCP",
  settings_gf_udp_hint: "— диапазоны для UDP",
  settings_ipset_hint: "— как использовать список IP",
  opt_off: "выкл",
  opt_all: "все",
  settings_hint:
    '<b>Game Filter</b> — добавляет порты игр в оптимизацию. Обычно не нужен: оставьте «выкл». Диапазоны пишите через запятую, например 1024-1934,1936-65535.<br>' +
    '<b>ipsets</b> — как использовать список IP: <b>loaded</b> — применять список (рекомендуется), <b>any</b> — все IP, <b>none</b> — не использовать. Без необходимости не меняйте.',

  // ------------------------------------------------------------- сброс сети
  netreset_title: 'Восстановление интернета <span class="chip">если пропала сеть</span>',
  netreset_hint:
    "Выключит мешающие службы, уберёт зависшие драйверы, сбросит прокси и DNS. <b>Пароли Wi-Fi и настройки провайдера не тронет.</b> Потом нужна перезагрузка",
  btn_net_reset: "Восстановить интернет",
  btn_show_adapters: "Показать виртуальные адаптеры",
  netreset_confirm_title: "Восстановить интернет",
  btn_net_start: "Начать восстановление",
  netreset_html:
    "<p>Программа сделает по шагам:</p><ul>" +
    "<li><b>1.</b> Создаст точку восстановления Windows (1–2 минуты).</li>" +
    "<li><b>2.</b> Выключит службу zapret и VPN, завершит их процессы.</li>" +
    "<li><b>3.</b> Уберёт зависшие сетевые драйверы.</li>" +
    "<li><b>4.</b> Сбросит прокси, кэш DNS, Winsock и TCP/IP.</li>" +
    "<li><b>5.</b> Предложит перезагрузку отдельной кнопкой.</li></ul>" +
    '<p class="sub">Пароли Wi-Fi и настройки провайдера не тронет.</p>' +
    '<p class="sub">Сброс Winsock и TCP/IP заработает после перезагрузки.</p>',
  net_step1: "Шаг 1/2: создаю точку восстановления…",
  net_step1_done: (msg) => `Точка восстановления: ${msg}\nШаг 2/2: выполняю сброс сети…`,
  net_restore_fail: (e) => `Точка восстановления не создана: ${e}`,
  net_continue_title: "Продолжить без точки восстановления?",
  net_continue_ok: "Продолжить без точки",
  net_continue_html: (e) =>
    `<p>Не удалось создать точку восстановления:</p><p class="sub">${e}</p><p>Можно продолжить сброс без возможности отката.</p>`,
  net_canceled: "\nОперация отменена.",
  net_running_no_restore: "Выполняю сброс сети без точки восстановления…",
  net_restore_created: "Точка восстановления создана.\n",
  net_reboot_note: "\n\nГотово. Изменения заработают после перезагрузки.",
  net_done: "Сеть сброшена — нужна перезагрузка",
  reboot_title: "Нужна перезагрузка",
  reboot_ok: "Перезагрузить сейчас",
  reboot_later: "Позже, вручную",
  reboot_html:
    "<p>Сброс Winsock и TCP/IP заработает только после перезагрузки.</p><p>Можно перезагрузиться сейчас или позже вручную.</p>",
  reboot_soon: "Перезагрузка через 15 секунд…",
  adapters_searching: "Ищу виртуальные адаптеры…",
  adapters_none: "Виртуальных сетевых адаптеров не найдено",
  save_failed: "Настройки не сохраняются: файл данных занят другой программой или нет прав",

  adapters_found_lead:
    "Найдены виртуальные адаптеры. Удаляйте их вручную в Диспетчере устройств, только если из-за них проблемы:",
  adapters_open_fail:
    "Не удалось открыть «Сетевые подключения». Откройте вручную: Панель управления → Сеть и Интернет",

  // ------------------------------------------------------------- DNS
  dns_title: 'Защищённый DNS <span class="chip">IPv4 + DoH</span>',
  dns_note:
    "Включит защищённый DNS в Windows: адреса подставляются сами, обычный DNS не используется.",
  dns_provider_label: "Провайдер",
  dns_adapter_label: "Сетевой адаптер",
  dns_adapter_placeholder: "Автоматически: активный адаптер",
  btn_apply_dns: "Включить защищённый DNS",
  btn_reset_dns: "Вернуть стандартный DNS",
  btn_dns_bench: "Проверить скорость",
  dns_ms: (ms) => ` · ${ms} мс`,
  dns_ms_val: (ms) => `${ms} мс`,
  dns_ping: (ms) => ` · пинг ~${ms} мс`,
  dns_info: (note, ping, tpl) => `${note}${ping}. Зашифрованный адрес: ${tpl}. Обычный DNS выключен.`,
  dns_bench_running: "Считаю задержку (3 запроса на адрес)…",
  dns_na: "н/д",
  dns_bench_title: "Чем меньше, тем быстрее:",
  dns_bench_fail: (e) => `Не удалось замерить: ${e}`,
  dns_load_fail: (e) => `Не удалось загрузить список DNS: ${e}`,

  // ------------------------------------------------------------- инструменты
  tools_title: 'Инструменты <span class="chip">Автор — Flowseal</span>',
  btn_discord_cache: "Очистить кэш Discord",
  btn_hosts_update: "Обновить hosts",
  label_fake_discord: "Фейк: Discord UDP",
  label_fake_game: "Фейк: GameFilter UDP",
  btn_fake_apply: "Заменить фейки",
  tools_hint:
    "<b>Кэш Discord</b> — закроет Discord и почистит кэш, если он залипает.<br>" +
    "<b>hosts</b> — скачает свежий файл автора и откроет его. Заменить системный нужно вручную, от админа.<br>" +
    "<b>Фейки</b> — заменит служебные файлы движка файлами из bin\\*.bin.",
  discord_cache_title: "Очистить кэш Discord?",
  discord_cache_html: "Discord закроется, затем удалится кэш. Потом откройте его заново.",
  btn_clear: "Очистить",

  // ------------------------------------------------------------- внешний вид
  appearance_title: 'Внешний вид <span class="chip">тема</span>',
  appearance_note: "Тема применяется сразу и сохраняется между запусками.",
  theme_grey_name: "Графит",
  theme_grey_hint: "как в GitHub / Discord",
  theme_dark_name: "Космос",
  theme_light_name: "Светлая",
  theme_light_hint: "белая",
};
