using System;
using System.Collections.Generic;
using System.Drawing;
using System.Windows.Forms;

namespace ZapretGui.App.Controls
{
    /// <summary>
    /// Всплывающие уведомления в правом нижнем углу — аналог toast() в main.js:261
    /// (текст живёт 4.8 c, потом исчезает).
    /// </summary>
    public class Toast : Panel
    {
        private const int LifeMs = 4800;
        private readonly List<Label> _items = new List<Label>();
        private readonly Timer _timer;

        public Toast()
        {
            BackColor = Color.Transparent;
            Width = 420;
            Height = 0;
            _timer = new Timer { Interval = 1000 };
            _timer.Tick += (s, e) => Cleanup();
        }

        public void Show(string kind, string text)
        {
            if (string.IsNullOrEmpty(text)) { return; }

            var item = new Label
            {
                Text = text,
                AutoSize = false,
                Width = Width - 16,
                Height = 34,
                BackColor = Theme.P.Panel2,
                ForeColor = KindColor(kind),
                Font = new Font("Segoe UI", 9F),
                TextAlign = ContentAlignment.MiddleLeft,
                Padding = new Padding(10, 0, 10, 0),
                Tag = Environment.TickCount,
            };
            item.Top = Height;
            Controls.Add(item);
            _items.Add(item);
            Height = item.Bottom + 6;
            if (!_timer.Enabled) { _timer.Start(); }
        }

        private Color KindColor(string kind)
        {
            switch (kind)
            {
                case "ok": return Theme.P.Green;
                case "warn": return Theme.P.Amber;
                case "err": return Theme.P.Red;
                default: return Theme.P.Text;
            }
        }

        private void Cleanup()
        {
            int now = Environment.TickCount;
            var dead = new List<Label>();
            foreach (Label l in _items)
            {
                if (unchecked(now - (int)l.Tag) >= LifeMs) { dead.Add(l); }
            }
            if (dead.Count == 0) { return; }
            foreach (Label l in dead)
            {
                _items.Remove(l);
                Controls.Remove(l);
                l.Dispose();
            }
            int top = 0;
            foreach (Label l in _items)
            {
                l.Top = top;
                top = l.Bottom + 6;
            }
            Height = top;
            if (_items.Count == 0) { _timer.Stop(); }
        }
    }
}