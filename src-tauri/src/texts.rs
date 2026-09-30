//! Единый файл пользовательских текстов бэкенда (стиль «For Dummies»).
//!
//! Здесь всё, что видит пользователь: тосты, ошибки команд, шаги сброса сети,
//! строки файлов результатов. Технические строки (логи, маркеры протокола,
//! имена файлов) остаются в своих модулях.

// ------------------------------------------------------------------- движки

pub fn engine_root_set(engine: &str, path: &str) -> String {
    format!("Движок «{engine}» подключён: {path}")
}

pub const FOLDER_MISSING: &str = "Папка не найдена — выберите другую";

pub fn exe_missing_in_folder(exe: &str) -> String {
    format!("В этой папке нет {exe}. Выберите папку с движком")
}

pub fn unknown_engine(engine: &str) -> String {
    format!("Неизвестный движок: {engine}")
}

pub const BUSY_OTHER_OP: &str = "Идёт другая операция — дождитесь окончания";

pub const BUSY_DOWNLOAD: &str = "Уже идёт загрузка — дождитесь окончания";

pub fn engine_exe_missing(exe: &str) -> String {
    format!("В папке движка нет файла {exe}")
}

pub fn engine_root_missing(engine: &str) -> String {
    format!("Сначала установите движок «{engine}»")
}

// ------------------------------------------------------ стратегии: старт/стоп

pub const STOPPED_ONE: &str = "Оптимизация остановлена";

pub const ALL_STOPPED: &str = "Всё остановлено";

pub const STOP_FAILED: &str =
    "Движок не завершился. Попробуйте остановить ещё раз или перезапустите программу";

pub const PROFILE_NOT_FOUND: &str = "Стратегия не найдена. Обновите список и попробуйте снова";

pub const TEST_RUNNING: &str = "Идёт тест — дождитесь окончания";

pub fn strategy_started(name: &str) -> String {
    format!("Стратегия «{name}» запущена")
}

pub fn service_switched(name: &str) -> String {
    format!("Служба переключена на «{name}»")
}

pub const STRATEGY_DIED: &str = "Стратегия сразу завершилась. Подробности — в «Журнале».";

pub const STRATEGY_DIED_NO_ADMIN: &str =
    "Стратегия сразу завершилась. Похоже, нет прав администратора — включите «Всегда запускать программу от администратора» в «Настройках».";

pub fn strategy_died(admin: bool, tail: &str) -> String {
    let hint = if admin {
        STRATEGY_DIED
    } else {
        STRATEGY_DIED_NO_ADMIN
    };
    if tail.is_empty() {
        format!("{hint} Движок не выдал ни строки")
    } else {
        format!("{hint} Последние строки движка:\n{tail}")
    }
}

// ------------------------------------------------------------------ служба

pub const VPN_PROCESS_NOTE: &str = "VPN или прокси мешает оптимизации — выгрузите";

pub const VPN_SERVICE_NOTE: &str = "Служба VPN запущена — выгрузите";

pub fn service_install_failed_code(code: i32) -> String {
    format!("Служба не установилась (код {code})")
}

pub fn service_start_failed_code(code: i32) -> String {
    format!("Служба не запустилась (код {code})")
}

pub fn service_start_failed_detail(detail: &str) -> String {
    if detail.is_empty() {
        "Служба не запустилась — подробности в «Журнале»".to_string()
    } else {
        format!("Служба не запустилась: {detail}")
    }
}

pub const SERVICE_STARTED_THEN_DIED: &str =
    "Служба запустилась и сразу остановилась — обычно мешает уже работающий движок. Остановите стратегию и включите службу заново";

pub const ADMIN_REQUIRED: &str = "Нужны права администратора: запрос отклонён";

pub const NEED_ADMIN_DIALOG: &str = "Для работы программе нужны права администратора. Запустите её ещё раз и подтвердите запрос Windows — или нажмите правой кнопкой на zgui.exe → «Запуск от имени администратора»";

pub fn service_remove_failed_code(code: i32) -> String {
    format!("Служба не удалилась (код {code})")
}

pub fn process_stop_failed(code: i32) -> String {
    format!("Процесс не закрылся (код {code}) — попробуйте остановить ещё раз")
}

pub const SERVICE_INSTALL_CONTEXT: &str = "Не удалось установить службу";

pub const SERVICE_REMOVE_CONTEXT: &str = "Не удалось удалить службу";

pub const SERVICE_SWITCH_CONTEXT: &str = "Не удалось переключить службу на эту стратегию";

pub const SERVICE_INSTALLED: &str = "Служба zapret установлена и запущена";

pub const SERVICE_STRATEGY_MISSING: &str =
    "Служба установлена, но её стратегия не записалась — выберите стратегию заново";

pub const SERVICE_REMOVED: &str = "Служба zapret удалена";

pub fn proxy_healed(proxy: &str) -> String {
    format!("Убрал нерабочий системный прокси {proxy}")
}

// ---------------------------------------------------------------- Telegram

pub const TG_PROXY_OFF_HINT: &str =
    "Осталось выключить прокси в Telegram: Настройки → Продвинутые → Тип подключения → «Отключить прокси»";

pub const TG_ALREADY: &str = "Прокси для Telegram уже работает";

pub fn tg_port_busy(addr: &str, reason: &str) -> String {
    format!("Порт {addr} занят: {reason}")
}

pub const TG_STOPPED: &str = "Прокси для Telegram остановлен";

pub const TG_CRASHED: &str = "Прокси для Telegram неожиданно остановился";

pub const TG_TIMEOUT: &str = "Прокси для Telegram не запустился за 20 секунд";

pub const TG_CHECK_CLIENT: &str = "Не удалось выйти в интернет для проверки версии";

pub fn tg_check_failed(e: &str) -> String {
    format!("Не удалось узнать версию моста: {e}")
}

// -------------------------------------------------------------------- тест

pub const TEST_ALREADY: &str = "Тест уже идёт";

pub const TEST_NO_STRATEGIES: &str = "Нет стратегий для теста";

pub const CURL_MISSING: &str =
    "Тест не работает без curl.exe. Обновите Windows или установите curl";

pub const TEST_NO_DOMAINS: &str = "Нет сайтов для проверки";

pub const TEST_STARTING: &str = "Запускаю тест…";

pub fn test_start_failed(e: &str) -> String {
    format!("Не удалось запустить тест: {e}")
}

pub const TESTING: &str = "Проверяю";

pub const TEST_DONE: &str = "Тест завершён";

pub fn test_timed_out(secs: u64) -> String {
    format!("Тест прервался: ответа не было {secs} с. Запустите тест снова")
}

pub const TEST_NOT_STARTED: &str = "Тест не запустился. Подробности — в «Журнале»";

pub const TEST_STOPPED: &str = "Тест остановлен";

pub const TEST_REUSED: &str = "Использованы прежние результаты — повторять тест не нужно";

pub const WINWS_RUNNING: &str =
    "Оптимизация уже запущена в другом месте. Остановите её и повторите тест";

pub fn best_strategy(name: &str) -> String {
    format!("Лучшая стратегия: «{name}»")
}

pub const NO_BEST: &str = "Ни одна стратегия не открыла YouTube и Discord";

pub fn prev_strategy_failed(e: &str) -> String {
    format!("Не удалось вернуть прежнюю стратегию: {e}")
}

// --------------------------------------------------------- результаты теста

pub const GROUP_LABEL_YOUTUBE: &str = "YouTube";

pub const GROUP_LABEL_YOUTUBE_MUSIC: &str = "YouTube Music";

pub const GROUP_LABEL_DISCORD: &str = "Discord";

pub const GROUP_LABEL_MICROSOFT_XBOX: &str = "Microsoft / Xbox";

pub const GROUP_LABEL_GOOGLE: &str = "Google";

pub const GROUP_LABEL_CLOUDFLARE: &str = "Cloudflare";

pub const GROUP_LABEL_OTHER: &str = "Другие сайты";

pub const RESULTS_EMPTY: &str = "Результатов пока нет. Запустите «Тест стратегий»";

pub const BEST_NONE: &str = "не выбрана";

pub fn results_best_line(name: &str, score: u32, max: u32) -> String {
    format!("Лучшая стратегия: {name} ({score}/{max})")
}

pub fn result_not_started(reason: &str) -> String {
    format!("не запустилась: {reason}")
}

pub const RESULT_CRIT_OK: &str = "YouTube и Discord открываются";

pub const RESULT_CRIT_FAIL: &str = "YouTube и Discord НЕ открываются";

pub const RESULTS_FAILED_HEADER: &str = "Не открылись:";

pub fn test_report_body(time: &str, count: usize, data: &str, tests: &str) -> String {
    format!(
        "Z-GUI — результаты теста стратегий\r\n\
         Время         : {time}\r\n\
         Проверено     : {count} стратегий\r\n\
         Папка данных  : {data}\r\n\
         ============================================================\r\n\
         {tests}"
    )
}

// ------------------------------------------------------- сброс сети (netreset)

pub fn restore_launch_failed(e: &str) -> String {
    format!("Не получилось выполнить сброс: {e}")
}

/// Суффиксы статуса шага «Восстановления интернета»: скрипт сам решает,
/// выполнена ли команда шага (см. netreset.rs), UI показывает как есть.
pub const STEP_DONE_SUFFIX: &str = " — выполнено";
pub const STEP_FAILED_SUFFIX: &str = " — ошибка";

/// Обновление отменено: не удалось создать резервную копию файла.
pub const BACKUP_FAILED: &str = "Не удалось создать резервную копию — обновление отменено";

pub const STEP_SERVICE_REMOVED: &str = "Остановил и удалил службу zapret";

pub const STEP_VPN_SERVICES: &str = "Остановил службы VPN";

pub const STEP_VPN_PROCESSES: &str = "Закрыл программы VPN и лишние движки";

pub const STEP_DRIVERS: &str = "Убрал зависшие драйверы";

pub const STEP_PROXY: &str = "Убрал заглушку прокси";

pub const STEP_DNS: &str = "Очистил кэш DNS";

pub const STEP_NETWORK: &str = "Сбросил настройки сети";

pub const NET_RESET_FAILED: &str = "Не удалось восстановить сеть";

pub fn net_reset_failed_code(code: i32) -> String {
    format!("Не удалось восстановить сеть (код {code})")
}

// ------------------------------------------------------- инструменты (tools)

pub const APPDATA_MISSING: &str = "Windows не отдал папку APPDATA — запустите программу заново";

pub fn discord_cache_cleared(folders: usize, mb: u64) -> String {
    format!("Кэш Discord очищен: папок {folders}, освобождено ~{mb} МБ")
}

pub const FAKE_BAD_NAME: &str = "Неверное имя файла — выберите файл из списка";

pub fn fake_missing(path: &str) -> String {
    format!("Файл не найден: {path}")
}

pub fn fake_replace_failed(e: &str) -> String {
    format!("Не удалось заменить файл: {e}")
}

pub fn fake_replaced(name: &str) -> String {
    format!("Активный фейк заменён: {name}")
}

pub const HOSTS_EMPTY: &str = "Сервер прислал пустой hosts. Попробуйте позже";

pub const HOSTS_DOWNLOAD_CONTEXT: &str = "Не удалось скачать hosts";

pub fn hosts_http_error(status: u16) -> String {
    format!("Сервер ответил ошибкой {status}. Попробуйте позже")
}

pub fn hosts_download_failed(e: &str) -> String {
    format!("Не удалось скачать hosts: {e}")
}

pub fn hosts_up_to_date(path: &str) -> String {
    format!("hosts уже свежий: {path}")
}

pub fn hosts_downloaded(path: &str) -> String {
    format!("Свежий hosts скачан: {path}. Замените системный файл hosts вручную")
}

// ---------------------------------------------------------------- обновления

pub const UPDATES_CHECKED: &str = "Проверка обновлений завершена";

pub const UPDATES_CHECK_CONTEXT: &str = "Не удалось проверить обновления";

pub fn updates_applied(count: usize) -> String {
    format!("Обновлено записей: {count}")
}

pub fn updates_applied_partial(count: usize, errors: &str) -> String {
    format!("Обновлено записей: {count}. Не получилось: {errors}")
}

pub const UPDATES_APPLY_CONTEXT: &str = "Не удалось применить обновления";

pub fn presets_download_failed(e: &str) -> String {
    format!("Не удалось скачать пресеты: {e}")
}

pub const AUTOUPDATE_CHECKED: &str = "Автопроверка обновлений завершена";

pub const BEST_APPLIED: &str = "Лучшая стратегия запущена и добавлена в автозапуск";

pub fn apply_failed(e: &str) -> String {
    format!("Не удалось применить стратегию: {e}")
}

pub fn start_interrupted(e: &str) -> String {
    format!("Запуск прервался: {e}")
}

// --------------------------------------------------------------------- DNS

pub const DNS_NO_ANSWER: &str = "Нет ответа — сервер блокирует запросы";

pub const DNS_UNKNOWN_PROVIDER: &str = "Неизвестный DNS-сервер";

pub fn dns_apply_failed_code(code: i32) -> String {
    format!("Не удалось включить DNS (код {code})")
}

pub fn dns_applied(name: &str, primary: &str, secondary: &str) -> String {
    format!("DNS {name} включён: {primary} и {secondary}, с шифрованием")
}

pub fn dns_reset_failed_code(code: i32) -> String {
    format!("Не удалось сбросить DNS (код {code})")
}

pub const DNS_RESET_OK: &str = "DNS снова выдаётся автоматически";

pub const DNS_APPLY_FAILED: &str = "Не удалось применить DNS";

pub const DNS_RESET_FAILED: &str = "Не удалось вернуть стандартный DNS";

// ------------------------------------------------- журнал, отчёты и ссылки

pub fn unknown_theme(theme: &str) -> String {
    format!("Неизвестная тема: {theme}")
}

pub const ONLY_HTTP_LINKS: &str = "Разрешены только ссылки http(s) и ncpa.cpl";

pub const ONLY_TG_LINKS: &str = "Разрешены только ссылки tg://";

pub fn open_failed(code: isize) -> String {
    format!("Не удалось открыть (код {code})")
}

pub fn open_link_failed(code: isize) -> String {
    format!("Не удалось открыть ссылку (код {code})")
}

pub const REPORT_DIR_FAILED: &str = "Не удалось создать папку отчёта";

pub const REPORT_SAVE_FAILED: &str = "Не удалось сохранить отчёт";

pub const RESULTS_DIR_FAILED: &str = "Не удалось создать папку журнала";

pub const RESULTS_SAVE_FAILED: &str = "Не удалось сохранить результаты";

pub const LOG_NOT_READY: &str = "Журнал ещё не готов";

// ------------------------------------------------------------- автозапуск

pub const ENGINE_STOPPED: &str = "Движок остановился — смотрите «Журнал»";

// ------------------------------------- перевод системных ошибок (human.rs)

pub const HUMAN_EMPTY: &str = "Неизвестная ошибка. Подробности — в «Журнале»";

pub const HUMAN_ADMIN: &str =
    "Для операции не хватает прав администратора. Подробности — в «Журнале»";

pub const HUMAN_FILE_BUSY: &str = "Файл занят другой программой. Закройте её и повторите";

pub const HUMAN_NO_SPACE: &str = "На диске не хватает места. Освободите немного и повторите";

pub const HUMAN_NOT_FOUND: &str = "Файл или папка не найдены. Проверьте, что движок установлен";

pub const HUMAN_NETWORK: &str =
    "Нет связи с сервером. Проверьте интернет и выключите VPN, потом повторите";

pub const HUMAN_CERT: &str =
    "Не удалось проверить сертификат сайта. Проверьте дату и время на компьютере";

pub const HUMAN_HTTP_403: &str =
    "Сервер отклонил запрос (403). Возможно, исчерпан лимит обращений к GitHub — попробуйте позже";

pub const HUMAN_HTTP_404: &str = "На сервере нет такого файла (404). Обновите программу";

pub const HUMAN_HTTP_5XX: &str = "Сервер временно недоступен (ошибка 5xx). Попробуйте позже";

pub const HUMAN_BAD_VALUE: &str = "Недопустимое значение поля. Проверьте введённые числа";

pub const HUMAN_ENGINE_DIED: &str = "Движок сразу завершился. Подробности — в «Журнале»";

pub const HUMAN_LAUNCH: &str =
    "Не удалось запустить процесс. Подробности — в «Журнале»";

pub const HUMAN_PANIC: &str = "Внутренняя ошибка программы. Подробности — в «Журнале»";

pub const HUMAN_FILE_MISSING: &str = "Файл не найден. Проверьте, что движок установлен";

pub fn human_unexpected(raw: &str) -> String {
    format!("Непредвиденная ошибка: {raw} (подробности в «Журнале»)")
}

/// Тост античит-паузы: обнаружен античит — оптимизация приостановлена.
pub fn anticheat_paused(name: &str) -> String {
    format!("Обнаружен античит ({name}) — оптимизация приостановлена")
}
