using System;
using System.Drawing;
using System.Windows.Forms;

namespace ZapretGui.App.Controls
{
    /// <summary>Чип-метка как .chip в веб-версии: серая плашка с мелким текстом.</summary>
    public class Chip : Label
    {
        public Chip(string text)
        {
            Text = text;
            AutoSize = false;
            Height = 20;
            Font = new Font("Segoe UI", 8F);
            TextAlign = ContentAlignment.MiddleCenter;
            Padding = new Padding(6, 0, 6, 0);
        }

        protected override void OnPaint(PaintEventArgs e)
        {
            e.Graphics.Clear(Theme.P.Panel2);
            e.Graphics.DrawRectangle(new Pen(Theme.P.Line),
                0, 0, Width - 1, Height - 1);
            base.OnPaint(e);
        }

        /// <summary>Цветной вариант: ok/warn/err/best/score — как .chip.* в CSS.</summary>
        public void SetKind(string kind)
        {
            switch (kind)
            {
                case "ok": case "best": ForeColor = Theme.P.Green; break;
                case "warn": ForeColor = Theme.P.Amber; break;
                case "err": ForeColor = Theme.P.Red; break;
                case "score": ForeColor = Theme.P.Accent; break;
                default: ForeColor = Theme.P.Muted; break;
            }
        }

        protected override void OnSizeChanged(EventArgs e)
        {
            base.OnSizeChanged(e);
            var g = CreateGraphics();
            Width = (int)g.MeasureString(Text, Font).Width + 16;
            g.Dispose();
        }
    }
}