using System.Windows.Forms;

namespace ZapretGui.App.Controls
{
    /// <summary>Кнопка с признаком «занята» — аналог btnBusy() в main.js:269.</summary>
    public static class BtnBusy
    {
        public static void Set(Button btn, bool busy)
        {
            if (btn == null) { return; }
            btn.Enabled = !busy;
            btn.Tag = busy ? "btn-busy" : "btn";
            if (busy)
            {
                btn.BackColor = Theme.P.Panel2;
                btn.ForeColor = Theme.P.Muted;
            }
            else
            {
                btn.BackColor = Theme.P.Btn;
                btn.ForeColor = Theme.P.Text;
            }
        }
    }
}