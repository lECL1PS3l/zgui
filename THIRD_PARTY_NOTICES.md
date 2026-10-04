# Third-Party Notices

Z GUI itself is released under the **MIT License** (see `LICENSE`).
Third-party components and assets bundled with or used by this project remain
under their own licenses, listed below. **The MIT license applies to our code
only — not to the third-party works.**

Our warmest thanks to every author below for making their work available.
Without you this project would not exist.

---

## Flowseal / zapret-discord-youtube — engine and configurations

- Source: https://github.com/Flowseal/zapret-discord-youtube
- License: **MIT** (Copyright (c) 2016-2026 bol-van; Copyright (c) 2024-2026 Flowseal;
  full text: `LICENSE.txt` inside the archive and `data/catalog/sources/flowseal/LICENSE.txt`)
- Bundled: `resources/engine-flowseal.zip` (a single archive: the winws engine plus
  configurations, lists and `.bat` strategies; previously kept as two identical copies).
- **WinDivert** (inside the engine archive: `bin/WinDivert.dll`, `bin/WinDivert64.sys`) is a
  separate component under **LGPLv3** (Basil); it is redistributed unmodified as part of the
  official engine release.

## bol-van / zapret2 — engine

- Source: https://github.com/bol-van/zapret2
- License: **MIT** (Copyright (c) bol-van)
- Bundled: `release/engine-zapret2.zip` — our own build of `winws2.exe` with the official
  run-time files (`cygwin1.dll`, `WinDivert`, `lua/`, `files/`, `windivert.filter/`).

## ValdikSS / GoodbyeDPI — engine

- Source: https://github.com/ValdikSS/GoodbyeDPI
- License: **Apache License 2.0** (Copyright (c) ValdikSS)
- Downloaded by the application from the official releases (not bundled in this repository).

## dilluti0n / DPIBreak — engine

- Source: https://github.com/dilluti0n/DPIBreak
- License: **MIT** (see the upstream repository)
- Downloaded by the application from the official releases (not bundled in this repository).

## Lucide Icons

- Source: https://github.com/lucide-icons/lucide (`packages/lucide-static`)
- License: **ISC** (permissive, MIT-compatible; copyright notice required)
- Copyright: Lucide Contributors
- Vendored files: `src/assets/icons/lucide-*.svg` (14 files; version v1.47.0; each SVG keeps
  its `@license` comment). The build does not require the `lucide-static` package.

## Hero Patterns — background pattern "Topography"

- Work: **Hero Patterns**, pattern **"Topography"**
- Author: **Steve Schoger**
- Source: https://heropatterns.com/
- License: **Creative Commons Attribution 4.0 International (CC BY 4.0)** —
  https://creativecommons.org/licenses/by/4.0/
- Copyright (c) Steve Schoger
- Files used: `src/assets/topo.svg` (default) and `src/assets/patterns/*.svg`
  (extra patterns: bank-note, bubbles, circuit-board, hideout, plus, random-shapes,
  texture, tic-tac-toe, wiggle), all generated with the author's own generator on
  heropatterns.com.
- Changes made by this project: the pattern is applied as a CSS background layer, with
  opacity and a slow drift animation configured in `src/styles.css`; the pattern fill
  was recolored to white (`#fff`) to match the dark themes. No other modification.
- This attribution does not imply that Steve Schoger endorses this project.

## tg-ws-proxy-rs

- Source: https://github.com/AmantesNihilo/zapret-universal-interface (`crates/tg-ws-proxy-rs`)
- License: **MIT**
- Copyright: AmantesNihilo and contributors
- Used as: Telegram bridge library (bundled as `crates/tg-ws-proxy-rs`).

---

## Ideas and references (no code taken)

The following projects inspired features or provided reference behavior. **No source code
was copied**; implementations are our own.

- **Zapret Control Center** — https://github.com/lolososka/zapret-discord-youtube (`gui/`) —
  GPL-3.0; ideas only (strategy fingerprint, diagnostics checklist).
- **ZapretControl** — https://github.com/Virenbar/ZapretControl — MIT; ideas only
  (tray, service management).
- **Line** — https://github.com/Read1dno/Line — MIT; ideas only (portable build,
  configuration management).
- **FreeConnect** — https://github.com/cold-hell/FreeConnect — MIT; ideas only
  (watchdog, auto-checks).
- **ByeDPI Manager** (romanvht) and **CDPI UI** (Storik4pro) — UI/UX references.

---

_Keep this file up to date: for every third-party work, record the exact source,
what was taken, and its license. When in doubt, ask before adding._
