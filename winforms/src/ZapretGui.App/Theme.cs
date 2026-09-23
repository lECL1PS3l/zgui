using System.Drawing;
using System.Windows.Forms;

namespace ZapretGui.App
{
    /// <summary>
    /// Палитра темы: значения CSS-переменных из styles.css (:root/[data-theme]).
    /// WinForms не умеет прозрачность слоями, поэтому rgba сведены к близким
    /// плотным цветам — визуально как в веб-версии.
    /// </summary>
    public class Palette
    {
        public Color Bg;
        public Color Panel;
        public Color Panel2;
        public Color Line;
        public Color Text;
        public Color Muted;
        public Color Accent;
        public Color Green;
        public Color Amber;
        public Color Red;
        public Color Btn;
        public Color CodeBg;

        public static readonly string[] Themes = { "grey", "dark", "light" };

        public static Palette For(string theme)
        {
            switch (theme)
            {
                case "dark":
                    return new Palette
                    {
                        Bg = ColorTranslator.FromHtml("#050508"),
                        Panel = ColorTranslator.FromHtml("#10121a"),
                        Panel2 = ColorTranslator.FromHtml("#1a1e2a"),
                        Line = ColorTranslator.FromHtml("#242835"),
                        Text = ColorTranslator.FromHtml("#f2f5fa"),
                        Muted = ColorTranslator.FromHtml("#9ca3af"),
                        Accent = ColorTranslator.FromHtml("#3b82f6"),
                        Green = ColorTranslator.FromHtml("#4ade80"),
                        Amber = ColorTranslator.FromHtml("#f59e0b"),
                        Red = ColorTranslator.FromHtml("#f87171"),
                        Btn = ColorTranslator.FromHtml("#171a22"),
                        CodeBg = ColorTranslator.FromHtml("#05070c"),
                    };
                case "light":
                    return new Palette
                    {
                        Bg = ColorTranslator.FromHtml("#ffffff"),
                        Panel = ColorTranslator.FromHtml("#f6f8fa"),
                        Panel2 = ColorTranslator.FromHtml("#eaeef2"),
                        Line = ColorTranslator.FromHtml("#dfe3e8"),
                        Text = ColorTranslator.FromHtml("#1f2328"),
                        Muted = ColorTranslator.FromHtml("#656d76"),
                        Accent = ColorTranslator.FromHtml("#0969da"),
                        Green = ColorTranslator.FromHtml("#1a7f37"),
                        Amber = ColorTranslator.FromHtml("#9a6700"),
                        Red = ColorTranslator.FromHtml("#cf222e"),
                        Btn = ColorTranslator.FromHtml("#f2f4f6"),
                        CodeBg = ColorTranslator.FromHtml("#f0f2f5"),
                    };
                default:
                    return new Palette
                    {
                        Bg = ColorTranslator.FromHtml("#1e1f22"),
                        Panel = ColorTranslator.FromHtml("#2b2d31"),
                        Panel2 = ColorTranslator.FromHtml("#36393f"),
                        Line = ColorTranslator.FromHtml("#3a3d44"),
                        Text = ColorTranslator.FromHtml("#e6e8eb"),
                        Muted = ColorTranslator.FromHtml("#a3a7ad"),
                        Accent = ColorTranslator.FromHtml("#5865f2"),
                        Green = ColorTranslator.FromHtml("#4ade80"),
                        Amber = ColorTranslator.FromHtml("#fbbf24"),
                        Red = ColorTranslator.FromHtml("#f87171"),
                        Btn = ColorTranslator.FromHtml("#2f3136"),
                        CodeBg = ColorTranslator.FromHtml("#0f1012"),
                    };
            }
        }
    }

    /// <summary>
    /// Тема окна: применяется рекурсивно по роли контрола (Tag). Роли совпадают
    /// с классами веб-версии: card/panel, btn, nav, navActive, muted, accent и т.д.
    /// </summary>
    public static class Theme
    {
        public static string Current = "grey";
        public static Palette P = Palette.For("grey");

        public static void Apply(Control root, string theme)
        {
            Current = IsKnown(theme) ? theme : "grey";
            P = Palette.For(Current);
            Paint(root);
        }

        public static bool IsKnown(string theme)
        {
            if (string.IsNullOrEmpty(theme)) { return false; }
            foreach (string t in Palette.Themes)
            {
                if (t == theme) { return true; }
            }
            return false;
        }

        private static void Paint(Control c)
        {
            string role = c.Tag as string;
            switch (role)
            {
                case "card":
                    c.BackColor = P.Panel;
                    c.ForeColor = P.Text;
                    break;
                case "btn":
                case "btn-ghost":
                    c.BackColor = P.Btn;
                    c.ForeColor = P.Text;
                    break;
                case "btn-primary":
                    c.BackColor = P.Accent;
                    c.ForeColor = Color.White;
                    break;
                case "btn-danger":
                    c.BackColor = P.Btn;
                    c.ForeColor = P.Red;
                    break;
                case "nav":
                    c.BackColor = P.Bg;
                    c.ForeColor = P.Muted;
                    break;
                case "navActive":
                    c.BackColor = P.Panel2;
                    c.ForeColor = P.Accent;
                    break;
                case "muted":
                    c.ForeColor = P.Muted;
                    break;
                case "accent":
                    c.ForeColor = P.Accent;
                    break;
                case "ok":
                    c.ForeColor = P.Green;
                    break;
                case "warn":
                    c.ForeColor = P.Amber;
                    break;
                case "err":
                    c.ForeColor = P.Red;
                    break;
                case "bar":
                    c.BackColor = P.Panel2;
                    break;
                default:
                    c.ForeColor = P.Text;
                    break;
            }

            if (c is Button)
            {
                ((Button)c).FlatStyle = FlatStyle.Flat;
                ((Button)c).FlatAppearance.BorderColor = P.Line;
            }
            if (c is TextBox || c is ComboBox || c is NumericUpDown)
            {
                c.BackColor = P.Panel2;
                c.ForeColor = P.Text;
            }
            foreach (Control child in c.Controls)
            {
                Paint(child);
            }
        }
    }
}