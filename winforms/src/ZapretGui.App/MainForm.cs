using System.Drawing;
using System.Windows.Forms;

namespace ZapretGui.App
{
    // Заглушка окна: навигация, темы и bootstrap-поллинг появляются в Task 16.
    public class MainForm : Form
    {
        public MainForm()
        {
            Text = "Zapret GUI";
            ClientSize = new Size(1180, 780);
            MinimumSize = new Size(960, 640);
            StartPosition = FormStartPosition.CenterScreen;
        }
    }
}
