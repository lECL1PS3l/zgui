using System;
using System.Collections.Generic;
using System.Drawing;
using System.Windows.Forms;
using ZapretGui.App.Controls;
using ZapretGui.Core;
using ZapretGui.Core.Config;
using ZapretGui.Core.Watchdog;

namespace ZapretGui.App
{
    /// <summary>
    /// Главное окно: сайдбар с навигацией, верхняя панель состояния, контент.
    /// Порт index.html + main.js: 1180x780 (min 960x640), темы grey/dark/light,
    /// поллинг bootstrap раз в 4 c (main.js:2107).
    /// </summary>
    public class MainForm : Form
    {
        private const int PollMs = 4000;

        private readonly Panel _sidebar = new Panel();
        private readonly Panel _content = new Panel();
        private readonly Panel _topbar = new Panel();
        private readonly Label _runState = new Label();
        private readonly Label _wdState = new Label();
        private readonly Button _stop = new Button();
        private readonly Label _meta = new Label();
        private readonly Toast _toasts = new Toast();
        private readonly Timer _poll = new Timer { Interval = PollMs };
        private readonly Dictionary<string, Button> _nav = new Dictionary<string, Button>();
        private readonly Dictionary<string, Panel> _pages = new Dictionary<string, Panel>();

        /// <summary>Текущий снимок состояния, null до первого ответа.</summary>
        private Bootstrap _b;

        // Пункты навигации дословно из index.html:24-49 (порядок и подписи).
        private static readonly string[,] NavItems =
        {
            { "strategies", "Стратегии" },
            { "tests", "Тест стратегий" },
            { "updates", "Обновления" },
            { "telegram", "Telegram" },
            { "dns", "DNS" },
            { "settings", "Настройки" },
            { "appearance", "Внешний вид" },
            { "logs", "Журнал" },
        };

        public MainForm()
        {
            Text = "Zapret GUI";
            ClientSize = new Size(1180, 780);
            MinimumSize = new Size(960, 640);
            StartPosition = FormStartPosition.CenterScreen;
            BackColor = Theme.P.Bg;
            ForeColor = Theme.P.Text;
            Font = new Font("Segoe UI", 9F);

            BuildSidebar();
            BuildTopbar();
            BuildContent();

            _toasts.Parent = this;
            _toasts.Anchor = AnchorStyles.Bottom | AnchorStyles.Right;

            // Тема из настроек (main.js:239: до bootstrap — «grey», потом из state).
            Theme.Apply(this, "grey");

            Bus.Status += OnStatus;
            Bus.Toast += OnToast;
            Bus.Log += OnLog;
            Bus.WatchdogState += OnWatchdog;
            Bus.Updates += OnUpdates;
            FormClosed += (s, e) => _poll.Stop();

            _poll.Tick += (s, e) => Refresh2();
            Load += (s, e) =>
            {
                LayoutToasts();
                Refresh2();
                _poll.Start();
            };
        }

        private void BuildSidebar()
        {
            _sidebar.Dock = DockStyle.Left;
            _sidebar.Width = 232;
            _sidebar.MinimumSize = new Size(232, 0);
            _sidebar.Tag = "nav";
            _sidebar.Padding = new Padding(8, 12, 8, 12);
            Controls.Add(_sidebar);

            var brand = new Label
            {
                Text = "Zapret GUI",
                Font = new Font("Segoe UI", 11F, FontStyle.Bold),
                AutoSize = false,
                Height = 30,
                Dock = DockStyle.Top,
                Padding = new Padding(8, 0, 0, 0),
                Tag = "accent",
            };
            _sidebar.Controls.Add(brand);

            var sub = new Label
            {
                Text = "winws",
                AutoSize = false,
                Height = 20,
                Dock = DockStyle.Top,
                Padding = new Padding(8, 0, 0, 0),
                Tag = "muted",
                Font = new Font("Segoe UI", 8F),
            };
            _sidebar.Controls.Add(sub);

            _meta.Dock = DockStyle.Bottom;
            _meta.Height = 24;
            _meta.Padding = new Padding(8, 0, 0, 0);
            _meta.Tag = "muted";
            _meta.Font = new Font("Segoe UI", 8F);
            _sidebar.Controls.Add(_meta);

            // Навигация снизу вверх: Dock=Top добавляет в обратном порядке.
            for (int i = NavItems.GetLength(0) - 1; i >= 0; i--)
            {
                string view = NavItems[i, 0];
                var btn = new Button
                {
                    Text = NavItems[i, 1],
                    Dock = DockStyle.Top,
                    Height = 40,
                    TextAlign = ContentAlignment.MiddleLeft,
                    FlatStyle = FlatStyle.Flat,
                    Padding = new Padding(12, 0, 0, 0),
                    Tag = view == "strategies" ? "navActive" : "nav",
                    Cursor = Cursors.Hand,
                };
                string captured = view;
                btn.Click += (s, e) => Select2(captured);
                _sidebar.Controls.Add(btn);
                btn.BringToFront();
                _nav[view] = btn;
            }
        }

        private void BuildTopbar()
        {
            _topbar.Dock = DockStyle.Top;
            _topbar.Height = 52;
            _topbar.Tag = "nav";
            _topbar.Padding = new Padding(22, 10, 22, 10);
            Controls.Add(_topbar);
            _topbar.BringToFront();

            _runState.AutoSize = false;
            _runState.Width = 220;
            _runState.Height = 30;
            _runState.TextAlign = ContentAlignment.MiddleCenter;
            _runState.Text = "не запущено";
            _runState.Tag = "muted";
            _runState.Location = new Point(14, 11);
            _topbar.Controls.Add(_runState);

            _wdState.AutoSize = true;
            _wdState.Location = new Point(244, 19);
            _wdState.Tag = "muted";
            _topbar.Controls.Add(_wdState);

            _stop.Text = "Остановить";
            _stop.Width = 120;
            _stop.Height = 30;
            _stop.Anchor = AnchorStyles.Top | AnchorStyles.Right;
            _stop.Tag = "btn-danger";
            _stop.Enabled = false;
            _stop.Location = new Point(ClientSize.Width - 140, 11);
            _stop.Click += (s, e) =>
            {
                State st = AppHost.State;
                if (st == null) { return; }
                try
                {
                    Core.Runtime.ProfileRunner.StopAllOwn(st);
                    Bus.RaiseToast("ok", "обход остановлен");
                }
                catch (Exception ex)
                {
                    Bus.RaiseToast("err", ex.Message);
                }
                Refresh2();
            };
            _topbar.Controls.Add(_stop);
            _topbar.Resize += (s, e) => _stop.Left = _topbar.Width - 140;
        }

        private void BuildContent()
        {
            _content.Dock = DockStyle.Fill;
            _content.Padding = new Padding(22);
            _content.Tag = "nav";
            Controls.Add(_content);
            _content.BringToFront();

            for (int i = 0; i < NavItems.GetLength(0); i++)
            {
                string view = NavItems[i, 0];
                var page = new Panel
                {
                    Dock = DockStyle.Fill,
                    Name = "page-" + view,
                    Visible = view == "strategies",
                };
                _content.Controls.Add(page);
                _pages[view] = page;
            }
            BuildPlaceholders();
        }

        // Заглушки страниц: содержимое подключают задачи 17-23.
        private void BuildPlaceholders()
        {
            foreach (var kv in _pages)
            {
                var head = new Label
                {
                    Text = TitleOf(kv.Key),
                    Dock = DockStyle.Top,
                    Height = 34,
                    Font = new Font("Segoe UI", 13F, FontStyle.Bold),
                    Tag = "accent",
                };
                var hint = new Label
                {
                    Text = "Раздел появится в следующих задачах порта.",
                    Dock = DockStyle.Top,
                    Height = 24,
                    Tag = "muted",
                };
                kv.Value.Controls.Add(hint);
                kv.Value.Controls.Add(head);
            }
        }

        private static string TitleOf(string view)
        {
            for (int i = 0; i < NavItems.GetLength(0); i++)
            {
                if (NavItems[i, 0] == view) { return NavItems[i, 1]; }
            }
            return view;
        }

        private void Select2(string view)
        {
            foreach (var kv in _pages)
            {
                kv.Value.Visible = kv.Key == view;
            }
            foreach (var kv in _nav)
            {
                kv.Value.Tag = kv.Key == view ? "navActive" : "nav";
            }
            Theme.Apply(this, Theme.Current);

            // Побочные эффекты перехода по вкладкам (main.js:1586-1591).
            if (view == "appearance") { ApplySavedTheme(); }
        }

        private void LayoutToasts()
        {
            _toasts.Left = ClientSize.Width - _toasts.Width - 24;
            _toasts.Top = ClientSize.Height - _toasts.Height - 24;
        }

        /// <summary>Перечитывает снимок состояния (main.js:282 refreshAll).</summary>
        private void Refresh2()
        {
            try
            {
                Bootstrap b = AppHost.Snapshot();
                if (b == null) { return; }
                _b = b;
                ApplySavedTheme();
                RenderRunBar();
                _meta.Text = "v" + AppInfo.Version;
            }
            catch (Exception e)
            {
                OnToast("err", "bootstrap: " + e.Message);
            }
        }

        private void ApplySavedTheme()
        {
            string theme = _b != null && _b.Settings != null ? _b.Settings.Theme : null;
            if (!Theme.IsKnown(theme))
            {
                theme = Theme.Current;
            }
            Theme.Apply(this, theme);
            BackColor = Theme.P.Bg;
            _content.BackColor = Theme.P.Bg;
            _topbar.BackColor = Theme.P.Bg;
            _sidebar.BackColor = Theme.P.Bg;
            _runState.BackColor = Theme.P.Bg;
            _wdState.BackColor = Theme.P.Bg;
            _meta.BackColor = Theme.P.Bg;
            LayoutToasts();
        }

        /// <summary>Строка состояния владельца обхода (main.js:546-572).</summary>
        private void RenderRunBar()
        {
            string owner = _b != null && _b.Owner != null ? _b.Owner : "none";
            string text;
            Color color;
            switch (owner)
            {
                case "test":
                    text = "идёт тест стратегий";
                    color = Theme.P.Green;
                    break;
                case "app":
                    text = _b.Runtime != null ? NameOf(_b.Runtime.ProfileId) : "запущено";
                    color = Theme.P.Green;
                    break;
                case "service":
                    text = "служба: " + (_b.Service != null && _b.Service.Strategy != null
                        ? NameOf(_b.Service.Strategy) : "запущена");
                    color = Theme.P.Green;
                    break;
                case "external":
                    text = "winws запущен вне программы";
                    color = Theme.P.Green;
                    break;
                default:
                    text = "не запущено";
                    color = Theme.P.Muted;
                    break;
            }
            _runState.Text = text;
            _runState.ForeColor = color;
            _stop.Enabled = owner == "app" || owner == "service" || owner == "external";
        }

        private string NameOf(string id)
        {
            if (_b == null || _b.Profiles == null || string.IsNullOrEmpty(id)) { return id ?? ""; }
            Profile p = _b.Profiles.Find(x => x.Id == id);
            return p != null ? p.Name : id;
        }

        // ------------------------------------------------------------ события

        private void OnStatus()
        {
            if (IsDisposed) { return; }
            BeginInvoke(new Action(() =>
            {
                // Стратегию остановили/запустили — гасим индикатор watchdog сразу
                // (main.js:2038-2043, не ждём следующей минуты).
                _wdState.Text = "";
                _wdState.ForeColor = Theme.P.Muted;
                Refresh2();
            }));
        }

        private void OnWatchdog(WatchdogStatus s)
        {
            if (IsDisposed) { return; }
            BeginInvoke(new Action(() =>
            {
                if (s == null || !s.Active)
                {
                    _wdState.Text = "";
                    _wdState.ForeColor = Theme.P.Muted;
                    return;
                }
                _wdState.Text = s.Alarm ? "стратегия не отвечает" : "стратегия активна";
                _wdState.ForeColor = s.Alarm ? Theme.P.Red : Theme.P.Green;
            }));
        }

        private void OnUpdates()
        {
            if (IsDisposed) { return; }
            BeginInvoke(new Action(Refresh2));
        }

        private void OnLog(Core.Log.Entry e)
        {
            // Журнал показывается на странице «Журнал» (Task 23) — здесь только
            // гарантируем, что события не потеряются при закрытом окне.
        }

        private void OnToast(string kind, string text)
        {
            if (IsDisposed) { return; }
            BeginInvoke(new Action(() =>
            {
                _toasts.Show(kind, text);
                LayoutToasts();
            }));
        }
    }
}