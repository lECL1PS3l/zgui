//! Единый файл пользовательских текстов бэкенда (стиль «For Dummies»).
//!
//! Здесь всё, что видит пользователь: тосты, ошибки команд, баннер
//! предупреждений, шаги сброса сети, строки файлов результатов.
//! Технические строки (логи, маркеры протокола, имена файлов) остаются
//! в своих модулях.

// ------------------------------------------------------------------- баннер

pub const VPN_CONFLICT: &str = "Сначала выключите VPN — вместе с ней оптимизация не работает";

pub const EXTERNAL_WINWS: &str =
    "Рядом работает вторая оптимизация — они мешают друг другу. Нажмите «Остановить» и запустите стратегию заново";

pub fn author_bats(engine: &str) -> String {
    format!("В папке «{engine}» остались bat-файлы автора. Отключите их автозапуск, иначе они будут мешать")
}

// -------------------------------------------------------- диагностика (diag)

pub const BFE_OFF: &str = "Служба BFE выключена — без неё оптимизация не работает. Включите её в services.msc";

pub const TIMESTAMPS_OFF: &str =
    "Windows не ставит метки времени TCP — оптимизация может работать хуже. Запустите программу от администратора, и метки включатся сами";

pub fn conflicting_services(list: &str) -> String {
    format!("Оптимизации мешают службы: {list}. Отключите их в services.msc")
}

pub const ADGUARD_RUNNING: &str = "Adguard мешает Discord. Закройте Adguard, пока пользуетесь оптимизацией";

pub const CYRILLIC_PATH: &str =
    "В пути программы есть русские буквы — оптимизация может не работать. Перенесите программу в C:\\zapret";

pub const ONEDRIVE_PATH: &str =
    "Программа лежит в OneDrive — оптимизация может не работать. Перенесите её на локальный диск";

pub fn hosts_youtube(count: usize, path: &str) -> String {
    format!("В системном hosts {count} записей для YouTube — они мешают доступу. Уберите их из файла {path}")
}

// ------------------------------------------------------------------- движки

pub fn engine_root_set(engine: &str, path: &str) -> String {
    format!("Движок «{engine}» подключён: {path}")
}

pub fn engine_download_started(engine: &str) -> String {
    format!("Скачиваю движок «{engine}»…")
}

pub const FETCH_META: &str = "Смотрю последнюю версию…";

pub fn engine_installed_short(engine: &str) -> String {
    format!("Движок «{engine}» готов")
}

pub fn engine_installed(engine: &str, path: &str) -> String {
    format!("Движок «{engine}» установлен: {path}")
}

pub fn engine_integrity_failed(engine: &str) -> String {
    format!("Файл движка «{engine}» не совпал с эталоном целостности — установка отменена")
}

pub fn integrity_failed(name: &str) -> String {
    format!("Файл {name} не совпал с эталоном целостности — обновление отменено")
}

pub fn engine_install_failed(engine: &str) -> String {
    format!("Не удалось установить движок «{engine}»")
}

pub const NO_ZIP_IN_RELEASE: &str = "В релизе нет архива для скачивания. Попробуйте позже";

pub fn download_http_error(status: impl std::fmt::Display) -> String {
    format!("Сервер скачивания ответил ошибкой {status}")
}

pub fn downloading(name: &str, pct: i32) -> String {
    format!("Скачиваю {name} — {pct}%")
}

pub const UNPACKING: &str = "Распаковываю…";

pub fn unpack_failed(e: impl std::fmt::Display) -> String {
    format!("Не удалось распаковать архив: {e}")
}

pub fn exe_missing_in_archive(exe: &str) -> String {
    format!("В архиве нет файла {exe}")
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
    let hint = if admin { STRATEGY_DIED } else { STRATEGY_DIED_NO_ADMIN };
    if tail.is_empty() {
        format!("{hint} Движок не выдал ни строки")
    } else {
        format!("{hint} Последние строки движка:\n{tail}")
    }
}

// ------------------------------------------------------------------ служба

pub const VPN_PROCESS_NOTE: &str = "VPN или прокси мешает оптимизации — выгрузите";

pub const VPN_SERVICE_NOTE: &str = "Служба VPN запущена — выгрузите";

pub const FOREIGN_SERVICE_RUNNING: &str = "Служба zapret запущена другой программой";

pub const FOREIGN_SERVICE_INSTALLED: &str = "Служба zapret установлена другой программой";

pub const FOREIGN_ENGINE_PROCESS: &str = "winws запущен вне нашей программы";

pub const CONFLICTS_FOUND: &str = "Найдены программы, мешающие оптимизации";

pub fn service_install_failed_code(code: i32) -> String {
    format!("Служба не установилась (код {code})")
}

pub fn service_start_failed_code(code: i32) -> String {
    format!("Служба не запустилась (код {code})")
}

pub const ADMIN_REQUIRED: &str = "Нужны права администратора: запрос отклонён";

pub fn engine_too_large(max_mb: u64) -> String {
    format!("Файл движка больше {max_mb} МиБ — установка отменена")
}

pub fn service_remove_failed_code(code: i32) -> String {
    format!("Служба не удалилась (код {code})")
}

pub fn process_stop_failed(code: i32) -> String {
    format!("Процесс не закрылся (код {code}) — нужны права администратора")
}

pub const SERVICE_INSTALL_CONTEXT: &str = "Не удалось установить службу";

pub const SERVICE_REMOVE_CONTEXT: &str = "Не удалось удалить службу";

pub const SERVICE_SWITCH_CONTEXT: &str = "Не удалось переключить службу на эту стратегию";

pub const SERVICE_INSTALLED: &str = "Служба zapret установлена и запущена";

pub const SERVICE_STRATEGY_MISSING: &str =
    "Служба установлена, но её стратегия не записалась — выберите стратегию заново";

pub const SERVICE_REMOVED_FALLBACK: &str = "Служба удалена. Автозапуск теперь через программу";

pub const SERVICE_REMOVED: &str = "Служба zapret удалена";

// ---------------------------------------------------------------- конфликты

pub fn killed_list(list: &str) -> String {
    format!("Закрыто: {list}")
}

pub fn kill_left(list: &str) -> String {
    format!("Не удалось закрыть: {list}")
}

pub fn kill_failed(e: &str) -> String {
    format!("Не удалось закрыть: {e}")
}

pub const NO_CONFLICTS: &str = "Помех нет";

pub fn proxy_healed(proxy: &str) -> String {
    format!("Убрал нерабочий системный прокси {proxy}")
}

// ---------------------------------------------------------------- Telegram

pub const TG_PROXY_OFF_HINT: &str =
    "Осталось выключить прокси в Telegram: Настройки → Продвинутые → Тип подключения → «Отключить прокси»";

pub const TG_VPN_OFF: &str =
    "Заметил VPN — выключил прокси для Telegram. В Telegram отключите прокси: Настройки → Продвинутые → Тип подключения";

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

pub const CURL_MISSING: &str = "Тест не работает без curl.exe. Обновите Windows или установите curl";

pub const TEST_NO_DOMAINS: &str = "Нет сайтов для проверки";

pub const TEST_STARTING: &str = "Запускаю тест…";

pub const TEST_STARTING_UAC: &str = "Запускаю тест. Подтвердите права администратора один раз";

pub fn test_start_failed(e: &str) -> String {
    format!("Не удалось запустить тест: {e}")
}

pub fn testing_strategy(name: &str) -> String {
    format!("Проверяю «{name}»")
}

pub const TEST_DONE: &str = "Тест завершён";

pub const TEST_NOT_STARTED_UAC: &str =
    "Тест не запустился. Возможно, вы отклонили запрос прав администратора";

pub fn test_timed_out(secs: u64) -> String {
    format!("Тест прервался: ответа не было {secs} с. Запустите тест снова")
}

pub const TEST_NOT_STARTED: &str = "Тест не запустился. Подробности — в «Журнале»";

pub const TEST_STOPPED: &str = "Тест остановлен";

pub const TEST_REUSED: &str = "Использованы прежние результаты — повторять тест не нужно";

pub const WINWS_RUNNING: &str = "Оптимизация уже запущена в другом месте. Остановите её и повторите тест";

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
        "Zapret GUI — результаты теста стратегий\r\n\
         Время         : {time}\r\n\
         Проверено     : {count} стратегий\r\n\
         Папка данных  : {data}\r\n\
         ============================================================\r\n\
         {tests}"
    )
}

// ------------------------------------------------------- сброс сети (netreset)

pub const RESTORE_EXISTS: &str = "Точка восстановления уже есть — второй раз не нужно";

pub const RESTORE_CREATED: &str = "Точка восстановления создана";

pub const RESTORE_DISABLED: &str =
    "Не получилось создать точку восстановления. Возможно, отключена Защита системы";

pub fn restore_failed_code(code: i32) -> String {
    format!("Не получилось создать точку восстановления (код {code})")
}

pub fn restore_launch_failed(e: &str) -> String {
    format!("Не получилось создать точку восстановления: {e}")
}

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
    format!(
        "Свежий hosts скачан: {path}. Скопируйте его в системный файл hosts (нужны права администратора)"
    )
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

pub const CHECK_INTERRUPTED: &str = "Проверка прервалась — попробуйте ещё раз";

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

pub const ADMIN_DECLINED: &str = "Запрос прав отклонён. Программа продолжит работу без прав";

pub const SECOND_COPY: &str =
    "Программа уже запущена. Закройте лишнюю копию — иначе настройки будут конфликтовать";

pub const NO_ADMIN_START: &str = "Нет прав администратора. Запуск стратегий и тесты недоступны";

pub fn autostart_failed(e: &str) -> String {
    format!("Автозапуск не сработал: {e}")
}

pub const ENGINE_STOPPED: &str = "Движок остановился — смотрите «Журнал»";

// ---------------------------------------------------------------- watchdog

pub const WATCHDOG_OK: &str = "Оптимизация снова работает";

pub fn watchdog_failed(list: &str) -> String {
    format!("Оптимизация не отвечает: не открываются {list}")
}

// ------------------------------------- перевод системных ошибок (human.rs)

pub const HUMAN_EMPTY: &str = "Неизвестная ошибка. Подробности — в «Журнале»";

pub const HUMAN_ADMIN: &str =
    "Windows попросит права администратора для запуска оптимизации. Включите «Всегда запускать программу от администратора» в «Настройках»";

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
    "Не удалось запустить процесс. Возможно, вы отклонили запрос прав администратора";

pub const HUMAN_PANIC: &str = "Внутренняя ошибка программы. Подробности — в «Журнале»";

pub const HUMAN_FILE_MISSING: &str = "Файл не найден. Проверьте, что движок установлен";

pub fn human_unexpected(raw: &str) -> String {
    format!("Непредвиденная ошибка: {raw} (подробности в «Журнале»)")
}
