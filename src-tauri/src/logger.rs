//! Встроенный журнал программы: кольцевой буфер в памяти + файл рядом с exe.
//!
//! Задача — чтобы тестер мог нажать «Сохранить отчёт» и отправить разработчику
//! понятную картину «что случилось»: сюда попадают старты/остановки движка,
//! результаты тестов, обновления и все ошибки команд (в т.ч. из UI).
//!
//! Файл: `<папка программы>/data/logs/zgui.log` (при росте > 1 МБ уезжает в
//! `zgui.1.log`). Буфер ограничен, чтобы долгая сессия не съела память.

use serde::Serialize;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// Сколько записей держим в памяти для окна «Журнал».
const CAP: usize = 1500;
/// Максимальная длина одной записи (сырой вывод движка бывает огромным).
const MAX_MSG: usize = 4000;
/// Порог ротации файла журнала.
const MAX_FILE: u64 = 1024 * 1024;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// Монотонный номер: UI забирает только новые записи (`after`).
    pub seq: u64,
    /// Время в миллисекундах от эпохи (UTC) — форматирует уже интерфейс.
    pub ts: u64,
    /// info | ok | warn | err
    pub level: String,
    pub scope: String,
    pub msg: String,
}

/// Колбэк доставки записи в UI. Логгер намеренно не знает про tauri: иначе
/// тестовый бинарник линкует tao/wry (comctl32 v6 без манифеста → exe не грузится).
type Sink = std::sync::Arc<dyn Fn(Entry) + Send + Sync>;

struct Inner {
    buf: VecDeque<Entry>,
    seq: u64,
    file: Option<PathBuf>,
    file_len: u64,
    sink: Option<Sink>,
}

static LOG: OnceLock<Mutex<Inner>> = OnceLock::new();

fn inner() -> &'static Mutex<Inner> {
    LOG.get_or_init(|| {
        Mutex::new(Inner {
            buf: VecDeque::new(),
            seq: 0,
            file: None,
            file_len: 0,
            sink: None,
        })
    })
}

fn lock() -> std::sync::MutexGuard<'static, Inner> {
    inner().lock().unwrap_or_else(|e| e.into_inner())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn norm_level(level: &str) -> &'static str {
    match level {
        "ok" => "ok",
        "warn" => "warn",
        "err" | "error" => "err",
        _ => "info",
    }
}

/// Вызывается один раз при старте: подключает файл и канал в UI.
pub fn init(data: &Path, sink: Sink) {
    let dir = data.join("logs");
    let _ = std::fs::create_dir_all(&dir);
    let file = dir.join("zgui.log");
    let len = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
    let mut g = lock();
    g.file = Some(file);
    g.file_len = len;
    g.sink = Some(sink);
}

/// Отключает доставку в UI (нужно при перезапуске окна в тестах/отладке).
#[allow(dead_code)]
pub fn set_sink(sink: Option<Sink>) {
    lock().sink = sink;
}

/// Главная точка входа: пишет в буфер, в файл и шлёт событие в интерфейс.
pub fn log(level: &str, scope: &str, msg: &str) {
    let level = norm_level(level);
    let msg = msg.trim();
    let msg = if msg.chars().count() > MAX_MSG {
        let cut: String = msg.chars().take(MAX_MSG).collect();
        format!("{}… [обрезано {} символов]", cut, msg.chars().count() - MAX_MSG)
    } else {
        msg.to_string()
    };

    let (entry, sink) = {
        let mut g = lock();
        g.seq += 1;
        let e = Entry {
            seq: g.seq,
            ts: now_ms(),
            level: level.into(),
            scope: scope.into(),
            msg,
        };
        g.buf.push_back(e.clone());
        while g.buf.len() > CAP {
            g.buf.pop_front();
        }
        write_file(&mut g, &e);
        // Колбэк копируем под блокировкой, вызываем — уже без неё: иначе UI-поток,
        // который сам пишет в журнал из обработчика, поймает дедлок.
        (e, g.sink.clone())
    };
    if let Some(sink) = sink {
        sink(entry);
    }
}

fn write_file(g: &mut Inner, e: &Entry) {
    use std::io::Write;
    let Some(path) = g.file.clone() else { return };
    let line = format!(
        "[{}] [{}] {}: {}\n",
        stamp(e.ts),
        e.level,
        e.scope,
        e.msg.replace('\n', "\n    ")
    );
    if g.file_len + line.len() as u64 > MAX_FILE {
        let old = path.with_file_name("zgui.1.log");
        let _ = std::fs::remove_file(&old);
        let _ = std::fs::rename(&path, &old);
        g.file_len = 0;
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        if f.write_all(line.as_bytes()).is_ok() {
            g.file_len += line.len() as u64;
        }
    }
}

/// Имя файла с отметкой времени (для отчёта) — без символов, запрещённых в путях.
pub fn now_stamp() -> String {
    stamp(now_ms()).replace(':', "-").replace(' ', "_").replace('.', "-")
}

/// Простое форматирование UTC без внешних крейтов: YYYY-MM-DD HH:MM:SS.mmm
pub fn stamp(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let millis = ms % 1000;
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        y,
        m,
        d,
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60,
        millis
    )
}

/// Дни от 1970-01-01 → календарная дата (алгоритм Говарда Хиннанта).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Записи новее `after` (0 — все, но не больше CAP).
pub fn entries(after: u64) -> Vec<Entry> {
    let g = lock();
    g.buf.iter().filter(|e| e.seq > after).cloned().collect()
}

/// Полный текст журнала — для отчёта и кнопки «Скопировать».
pub fn dump() -> String {
    let g = lock();
    g.buf
        .iter()
        .map(|e| format!("[{}] [{}] {}: {}", stamp(e.ts), e.level, e.scope, e.msg))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn clear() {
    let mut g = lock();
    g.buf.clear();
    if let Some(p) = g.file.clone() {
        let _ = std::fs::remove_file(&p);
        g.file_len = 0;
    }
}

/// Папка с файлами журнала (для кнопки «Открыть папку»).
pub fn dir() -> Option<PathBuf> {
    lock().file.as_ref().and_then(|p| p.parent().map(|d| d.to_path_buf()))
}

/// Пишем панику в журнал, не ломая штатный вывод.
pub fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log("err", "panic", &info.to_string());
        prev(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_formatting_is_utc_correct() {
        // 0 мс — начало эпохи.
        assert_eq!(stamp(0), "1970-01-01 00:00:00.000");
        // 2026-09-19 12:00:00 UTC = 1789819200000 мс.
        assert_eq!(stamp(1_789_819_200_000), "2026-09-19 12:00:00.000");
    }

    #[test]
    fn levels_are_normalized() {
        assert_eq!(norm_level("error"), "err");
        assert_eq!(norm_level("bogus"), "info");
    }

    #[test]
    fn buffer_keeps_newest_and_reports_delta() {
        clear();
        for i in 0..10 {
            log("info", "test", &format!("строка {i}"));
        }
        let all = entries(0);
        assert_eq!(all.len(), 10);
        let last_seq = all.last().unwrap().seq;
        log("info", "test", "ещё одна");
        let delta = entries(last_seq);
        assert_eq!(delta.len(), 1);
        assert_eq!(delta[0].msg, "ещё одна");
        clear();
    }

    #[test]
    fn long_messages_are_truncated() {
        clear();
        let huge = "x".repeat(MAX_MSG + 500);
        log("warn", "test", &huge);
        let e = &entries(0)[0];
        assert!(e.msg.contains("обрезано"));
        assert!(e.msg.chars().count() < MAX_MSG + 100);
        clear();
    }
}
