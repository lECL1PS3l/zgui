# Third-Party Notices

Проект Z GUI распространяется под лицензией MIT (см. `LICENSE`).

Ниже перечислены сторонние проекты, код которых был использован (полностью или частично),
согласно их лицензиям. Все перечисленные проекты — MIT или совместимые с MIT.

Для вшитых ресурсов (`src-tauri/resources/`) SHA-256 зафиксированы в
`src-tauri/resources/checksums.sha256` и проверяются `scripts/security-check.ps1` — это
защита от подмены архивов чужеродным содержимым.

---

## Flowseal / zapret-discord-youtube (движок и конфиги)

- Источник: https://github.com/Flowseal/zapret-discord-youtube
- Лицензия: **MIT** (Copyright (c) 2016-2026 bol-van, Copyright (c) 2024-2026 Flowseal;
  полный текст — `LICENSE.txt` в архиве, а также `data/catalog/sources/flowseal/LICENSE.txt`)
- Что вшито: `resources/engine-flowseal.zip` (релиз движка winws), `resources/flowseal-main.zip`
  (снимок конфигов/списков/стратегий .bat).
- **WinDivert** (внутри архива движка: `bin/WinDivert.dll`, `bin/WinDivert64.sys`) —
  отдельный компонент под **LGPLv3** (Basil); распространяется без изменений как часть
  официального релиза движка.

---

## Lucide Icons

- Источник: https://github.com/lucide-icons/lucide (`packages/lucide-static`)
- Лицензия: **ISC** (разрешительная, совместима с MIT; требуется сохранение copyright)
- Copyright: Lucide Contributors
- Что взято: 10 иконок в `src/assets/icons/lucide-*.svg` (zap, flask-conical, download,
  send, shield-check, palette, settings, scroll-text, x, check). Файлы вендорены как есть,
  версия v1.47.0, внутри каждого SVG сохранён комментарий `@license`.
- Сборка не требует пакета `lucide-static` — файлы лежат в репозитории.

---

## tg-ws-proxy-rs

- Источник: https://github.com/AmantesNihilo/zapret-universal-interface (`crates/tg-ws-proxy-rs`)
- Лицензия: MIT
- Copyright: AmantesNihilo and contributors
- Что взято: библиотека Telegram-моста MTProto → WebSocket (crypto, faketls, splitter,
  pool, outbound, server, ws_client, stats, runtime, check).

---

## Zapret Control Center (reference)

- Источник: https://github.com/lolososka/zapret-discord-youtube (`gui/`)
- Лицензия: **GPL-3.0 — код НЕ использовался**, брались исключительно идеи
  (fingerprint стратегий, диагностика-чеклист). В этом файле не учитывается как
  источник кода.

---

## ZapretControl

- Источник: https://github.com/Virenbar/ZapretControl
- Лицензия: **MIT — код НЕ использовался**, брались исключительно идеи
  (трей, управление службами). Реализация собственная.

## Line

- Источник: https://github.com/Read1dno/Line
- Лицензия: **MIT — код НЕ использовался**, брались исключительно идеи
  (portable-сборка, управление конфигами). Реализация собственная.

## FreeConnect

- Источник: https://github.com/cold-hell/FreeConnect
- Лицензия: **MIT — код НЕ использовался**, брались исключительно идеи
  (watchdog, авто-проверки). Реализация собственная.

---

_Файл пополняется по мере переноса кода. Для каждого заимствования указывать точный
путь в исходном проекте и что именно перенесено._
