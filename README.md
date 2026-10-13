# Z-GUI

![Version](https://img.shields.io/badge/version-1.3.0-success)
![License](https://img.shields.io/badge/license-MIT-blue)
![Privacy](https://img.shields.io/badge/privacy-no%20telemetry-brightgreen)
![Platform](https://img.shields.io/badge/platform-Windows%2010%2F11-0078D6?logo=windows&logoColor=white)
![Last commit](https://img.shields.io/github/last-commit/lECL1PS3l/zgui)

<div align="center">

[![Скачать Z-GUI 1.3.0](https://img.shields.io/badge/Скачать-Z--GUI%201.3.0-2ea44f?style=for-the-badge)](https://github.com/lECL1PS3l/zgui/releases/download/v1.3.0/Z-GUI.Stable.1.3.zip)

</div>

Портативная графическая оболочка для Windows к движкам локальной оптимизации трафика
([Flowseal / zapret-discord-youtube](https://github.com/Flowseal/zapret-discord-youtube),
[zapret2](https://github.com/bol-van/zapret2), [GoodbyeDPI](https://github.com/ValdikSS/GoodbyeDPI)) — в одном окне, без правки `.bat` и консоли.

> ⚠️ **Не связано с Flowseal, bol-van и ValdikSS.** Z-GUI — независимая оболочка; названия и товарные знаки движков принадлежат их владельцам.

## Что это

Z-GUI — портативная графическая оболочка для Windows к движкам локальной оптимизации трафика.
Программа, все три движка и стратегии лежат рядом в одной папке; первый запуск работает без
интернета. Обработка трафика идёт целиком на вашем компьютере; наружу программа обращается
только за обновлениями — подробности в разделе «Приватность».

## Скриншоты

| Основной экран |
|---|
| <img src="screens/gith_preview.png" width="800" alt="Главный экран Z-GUI: список стратегий по движкам, блок автозапуска и меню слева"> |

## Что умеет

| Возможность | Что делает |
|---|---|
| 🟢 Стратегии | Список профилей плитками, запуск/остановка, автозапуск службой |
| 🟢 Тест стратегий | По очереди проверяет стратегии по контрольным доменам и ранжирует их |
| 🟢 Обновления | Стратегии, списки и пресеты одной кнопкой, с бэкапом перед изменениями |
| 🟡 DNS | Готовые провайдеры в один клик, возврат к автоматическим серверам |
| 🟡 Telegram-прокси | Подключение одной кнопкой |
| 🟢 Журнал | Полный журнал действий и ошибок, отчёт одной кнопкой |
| 🟢 Обучение | Короткий интерактивный тур при первом запуске (повтор — в «Настройках») |

## Установка

1. Скачайте `Z-GUI.Stable.<версия>.zip` со [страницы релизов](https://github.com/lECL1PS3l/zgui/releases)
   и распакуйте в папку **без кириллицы и пробелов** (например `D:\Zapret`).
2. Запустите `zgui.exe`. Рядом уже есть папка `data/` — движки, стратегии и списки; первый запуск
   работает без интернета.
3. Нужен [WebView2 Runtime](https://go.microsoft.com/fwlink/p/?LinkId=2124703) — на актуальных
   Windows 10/11 уже установлен; если нет — см. FAQ.
4. Программа работает только от администратора. Заранее ничего включать не нужно: при первом
   запуске она один раз запросит права Windows и перезапустится.

**Проверка подлинности.** В релизе два файла — архив программы и набор стратегий. Их **SHA-256**:

```
Z-GUI.Stable.1.3.zip  DD091A87D13F27B196D65D70812417C58F5D949BCC95655D60D9FAA5EAEEDED8
presets.json          2964F5F891C7DD424FB7981526D486BE935F4DFBDEC9BD276DA58DD7C565DA26
```

Сверьте скачанный архив:

```powershell
Get-FileHash .\Z-GUI.Stable.1.3.zip -Algorithm SHA256
```

Качайте только с этой страницы: поисковики часто выводят на сборки с вредоносами.

## Приватность

- Никакой телеметрии и аналитики.
- Настройки хранятся только рядом с программой (`data/state.json`).
- Сеть используется только для GitHub (обновления, набор пресетов) и серверов Telegram,
  когда включён его мост. Движки программа не скачивает — они лежат рядом, в `data/`.
- Telegram-мост слушает только `127.0.0.1`.
- Права администратора нужны для системного драйвера и службы Windows.

## FAQ

<details>
<summary><b>Во время теста пропадает интернет — это нормально?</b></summary>

Да. Стратегия может кратковременно оборвать связь — вплоть до полного короткого обрыва. После
прогона программа возвращает прежнее состояние.
</details>

<details>
<summary><b>«Could not find the WebView2 Runtime»</b></summary>

Интерфейс рисует системный компонент WebView2. На LTSC, урезанных сборках и Windows Server его
может не быть. Установите один раз с <https://developer.microsoft.com/en-us/microsoft-edge/webview2>
и запустите программу заново.
</details>

<details>
<summary><b>SmartScreen предупреждает о файле</b></summary>

Сборка не подписана. Нажмите «Подробнее» → «Выполнить в любом случае» и сверьте SHA-256 из
раздела «Проверка подлинности».
</details>

<details>
<summary><b>Служба осталась после удаления программы / «WinDivert64.sys уже используется»</b></summary>

Служба `zapret` регистрируется в Windows, а не в папке программы. Перед удалением или переносом
папки снимите галочку «Включить службу». Если программа уже удалена — проще всего запустить
скачанную сборку заново и нажать «Восстановить интернет»: она сама остановит службу, снимет
зависшие драйверы и сбросит сеть. Если запустить уже нечем, выполните от администратора:

```powershell
sc.exe stop zapret
sc.exe delete zapret
Get-CimInstance Win32_SystemDriver | Where-Object { $_.Name -like 'WinDivert*' } |
    ForEach-Object { sc.exe stop $_.Name; sc.exe delete $_.Name }
```
</details>

<details>
<summary><b>Антивирус ругается на WinDivert</b></summary>

WinDivert — легитимный системный драйвер, который используется движком zapret; часть антивирусов
помечает его как risk-tool (`Not-a-virus:RiskTool.Multi.WinDivert`). Добавьте папку программы в
исключения либо отключите детект PUA.
</details>

<details>
<summary><b>Движок не помогает моему провайдеру</b></summary>

Всё зависит от провайдера. Если программа показывает, что дело в самом провайдере — локальная
обработка не поможет, нужен другой маршрут.
</details>

## Известные ограничения

- Только Windows 10 / 11, x64 (без 32-бит и ARM).
- Нужен WebView2 Runtime.
- Нужны права администратора (драйвер трафика и служба Windows).
- Обновления идут через GitHub; при заблокированном GitHub не обновятся.
- Поддерживаются три встроенных движка.

## Разработка

- Стек: Rust (Tauri 2) и фронтенд на vanilla JS + Vite.
- Сборка: `npm install`, затем `powershell -File scripts\build.ps1` (программа должна быть закрыта).
- Тесты: `cargo test --lib` в `src-tauri/` и `cargo test` в `src-tauri/crates/tg-ws-proxy-rs`.
- Самопроверка безопасности: `powershell -File scripts\security-check.ps1`.
- О проблемах — через [issues](https://github.com/lECL1PS3l/zgui/issues) или «Журнал → Сохранить отчёт».

## Лицензия

**MIT** — см. [LICENSE](LICENSE). Полные указания сторонних компонентов и лицензии —
в [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md); история изменений — в [CHANGELOG.md](CHANGELOG.md).

Благодарности: движки и стратегии — [Flowseal](https://github.com/Flowseal/zapret-discord-youtube),
[bol-van](https://github.com/bol-van/zapret2), [ValdikSS](https://github.com/ValdikSS/GoodbyeDPI);
WinDivert — [Basil](https://github.com/basil00/divert);
иконки — [Lucide](https://github.com/lucide-icons/lucide); узоры фона — [Hero Patterns](https://heropatterns.com/)
(«Topography», «Bubbles», «Tic-Tac-Toe»), Steve Schoger.

[↑ Наверх](#z-gui)
