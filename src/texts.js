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
  slogan: "Локальная оптимизация сетевых пакетов",
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
  run_idle: "выключено",
  run_op: "идёт операция…",
  run_test: "идёт проверка стратегий",
  run_active: "winws активен",
  btn_stop: "Остановить",
  btn_start: "Запустить",

  // ------------------------------------------------------------- ошибки
  err_admin: "Не хватает прав администратора. Запустите программу заново и подтвердите запрос Windows",
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

  // ------------------------------------------------------------- общее
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
  btn_not_now: "Не сейчас",

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
  fs_note: "Движок лежит рядом с программой — папка data/engines/flowseal.",
  upd_cfg_title: "Обновление стратегий",
  upd_cfg_note: "Стратегии и списки, которые программа скачивает с GitHub.",
  btn_apply_available: "Применить доступные",
  btn_apply_selected: "Применить выбранное",
  eng_ready: "готов",
  eng_need_file: (exe) => `нужен файл ${exe}`,
  eng_not_installed: "не установлен",
  btn_open_folder: "Открыть папку",
  btn_change: "Сменить…",
  btn_pick_folder: "Выбрать папку…",
  engine_place_note: "Движок лежит рядом с программой (папка data)",
  tab_not_installed: "Не установлен. Положите движок в папку data/engines",
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

  // ------------------------------------------------------------- автозапуск
  card_autostart: "Автозапуск службой",
  label_start_on_login: "Стратегия для службы",
  label_run_as_service: "Включить службу",
  autostart_note:
    "Автозапуск работает только службой: оптимизация включается при загрузке ПК без программы. Программа запускается вручную.",
  auto_none: "— выберите стратегию —",
  auto_pick_first: "Сначала выберите стратегию",
  auto_chip_off: "выключен",
  auto_chip_stopped: "служба остановлена",
  auto_note_service: (name) =>
    `Служба запущена — оптимизация включится сама при загрузке ПК.${name ? ` Стратегия «${name}».` : ""}`,
  auto_note_stopped: (name) =>
    `Служба установлена, но остановлена. Включите галочку, чтобы запустить${name ? ` её для стратегии «${name}»` : ""}.`,
  auto_note_chosen: (name) =>
    `Служба выключена. Стратегия «${name}» выбрана — включите службу галочкой ниже`,
  auto_note_off: "Служба выключена — автозапуска нет",
  auto_off: "Автозапуск выключен",
  auto_saved: "Стратегия сохранена — включите службу галочкой",
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
  test_stopped_note: "Тест остановлен — проверены не все стратегии",
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
  test_started: "тест запущен",
  test_stopping: "останавливаю тест…",
  test_report_saved: (p) => `Результаты теста сохранены: ${p}`,
  test_report_fail: (e) => `Не удалось сохранить результаты: ${e}`,
  test_busy_other: "Идёт другая операция. Дождитесь завершения",
  nothing_selected: "Ничего не выбрано",
  vpn_warn: "Найден VPN: результаты теста будут недостоверными. Для реальной картины выключите VPN и повторите",

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
  settings_ipset_hint: "— как использовать список IP",
  opt_off: "выкл",
  opt_all: "все",
  opt_tcp: "только TCP",
  opt_udp: "только UDP",
  settings_hint:
    '<b>Game Filter</b> — добавляет порты игр в оптимизацию. Обычно не нужен: оставьте «выкл».<br>' +
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
    "<li><b>1.</b> Выключит службу zapret и VPN, завершит их процессы.</li>" +
    "<li><b>2.</b> Уберёт зависшие сетевые драйверы.</li>" +
    "<li><b>3.</b> Сбросит прокси, кэш DNS, Winsock и TCP/IP.</li>" +
    "<li><b>4.</b> Предложит перезагрузку отдельной кнопкой.</li></ul>" +
    '<p class="sub">Пароли Wi-Fi и настройки провайдера не тронет.</p>' +
    '<p class="sub">Сброс Winsock и TCP/IP заработает после перезагрузки.</p>',
  net_running: "Выполняю сброс сети…",
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

  // ------------------------------------------------------------- диагностика
  nav_scanner: "Диагностика",
  scan_title: "Диагностика сервиса",
  scan_intro: "Проверяет, мешает ли оптимизация конкретному сайту или программе, и предлагает готовое решение.",
  scan_warn:
    "VPN, прокси, туннели и другие сетевые оптимизации искажают результат — для чистой картины выключите их. " +
    "Проверка идёт под движок flowseal (winws). На время проверки оптимизация будет кратко выключена.",
  scan_warn_proc:
    "Проверим реальное соединение: сначала с выключенной оптимизацией, затем с включённой. " +
    "Когда попросит — создайте сессию в программе (для игры: лобби). Её придётся создать дважды. " +
    "VPN, прокси и туннели искажают результат.",
  scan_tab_site: "Сайт",
  scan_tab_process: "Программа",
  scan_target_site: "Домен или адрес сайта",
  scan_ph_site: "например store.steampowered.com",
  scan_target_process: "Программа",
  scan_or_addr: "или адрес (ip:порт)",
  scan_ph_process: "например 80.93.214.205:4531",
  scan_capture: "Поймать соединения",
  scan_browse: "Указать .exe",
  scan_refresh: "Обновить",
  scan_refreshed: "Список программ обновлён",
  scan_focus: "Вывести окно на передний план при каждой фазе теста",
  scan_strategy: "Стратегия для проверки",
  scan_run: "Начать проверку",
  scan_save: "Сохранить отчёт",
  scan_apply: "Применить рекомендацию",
  scan_cancel: "Отменить проверку",
  scan_cancelling: "Отменяю…",
  scan_searching: "Готовлю…",
  scan_need_target: "Укажите цель проверки",
  scan_need_proc: "Введите имя программы",
  scan_no_conns: "Соединения не найдены — запустите программу и повторите",
  scan_captured: "Поймано соединений:",
  scan_saved: "Отчёт сохранён:",
  scan_running: "Идёт проверка… если попросит — создайте сессию в программе. Это займёт несколько минут.",
  scan_without: "Без оптимизации",
  scan_with: "С оптимизацией",
  scan_yes: "есть ответ",
  scan_no: "нет ответа",
  scan_verdict: "Вердикт",
  scan_target_short: "Цель",
  scan_recommend: "Рекомендация",
  scan_v_no_effect: "оптимизация цели не мешает",
  scan_v_collateral: "оптимизация ломает незаблокированный сайт — добавьте его в исключения",
  scan_v_covered: "оптимизация нужна и работает",
  scan_v_not_covered: "оптимизация не покрывает соединение — добавьте подсеть и включите «Игровой фильтр»",
  scan_v_unrelated: "не похоже на проблему оптимизации",
  scan_capturing: "Ищу соединения… (до 20 с)",
  scan_need_addr: "Сначала «Поймать соединения» (когда программа в сети) или введите ip:порт",
  scan_guide_html:
    '<b>Как пользоваться — по шагам</b><br>' +
    '1. Выберите вкладку: <b>Сайт</b> — если тормозит сайт; <b>Программа</b> — если сбоит игра или приложение.<br>' +
    '2. Нажмите «Начать проверку». На это время оптимизация ненадолго выключается — так и надо.<br>' +
    '<b>Сайт:</b> впишите домен и дождитесь вердикта. Если «ломает» — нажмите «Применить»: домен добавят в исключения.<br>' +
    '<b>Программа:</b> выберите её из списка (кнопка «Обновить» — если только что запустили). Затем войдите в игру/приложение и, когда попросит, создайте сессию (лобби) заново — два раза. Если «не покрывает» — «Применить» добавит подсеть в обход.',
};
