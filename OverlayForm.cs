using System;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Windows.Forms;

namespace MicTray;

/// <summary>
/// 全屏置顶悬浮窗：无边框、透明、圆角、可拖拽、点击切换全局静音。
/// 在窗口化全屏 / 无边框全屏游戏、直播软件（OBS 等）上正常置顶。
/// 真正的独占全屏（DX Exclusive）会接管主显示表面，普通窗口无法覆盖，需游戏内 overlay 注入。
/// 右键弹出菜单：隐藏悬浮窗 / 退出 MicTray。
/// </summary>
internal sealed class OverlayForm : Form
{
    private readonly AudioEngine _audio;
    private readonly ContextMenuStrip _menu;
    private bool _dragMoved;
    private Point _dragStart;

    public OverlayForm(AudioEngine audio)
    {
        _audio = audio;

        FormBorderStyle = FormBorderStyle.None;
        TopMost = true;
        ShowInTaskbar = false;
        BackColor = Color.Magenta;
        TransparencyKey = Color.Magenta;       // 洋红作为透明色键，圆角外透明
        Opacity = 0.85;                        // 整窗半透明 → 灰底透出后方画面，图标仍清晰
        StartPosition = FormStartPosition.Manual;

        // 高 DPI：禁用 WinForms 自动缩放，完全手动按 DeviceDpi 计算。
        AutoScaleMode = AutoScaleMode.None;
        AutoScaleDimensions = new SizeF(96f, 96f);
        DoubleBuffered = true;
        SetStyle(ControlStyles.UserPaint | ControlStyles.AllPaintingInWmPaint | ControlStyles.ResizeRedraw, true);

        // 右键菜单：隐藏悬浮窗（Ctrl+Alt+O 可再次唤出）/ 退出整个程序
        _menu = new ContextMenuStrip();
        var hide = new ToolStripMenuItem("隐藏悬浮窗");
        hide.Click += (_, _) => Hide();
        var exit = new ToolStripMenuItem("退出 MicTray");
        exit.Click += (_, _) => Application.Exit();
        _menu.Items.Add(hide);
        _menu.Items.Add(exit);
        ContextMenuStrip = _menu;
    }

    protected override void OnHandleCreated(EventArgs e)
    {
        base.OnHandleCreated(e);
        ApplyLayout();
    }

    protected override void OnShown(EventArgs e)
    {
        base.OnShown(e);
        ApplyLayout();
    }

    /// <summary>
    /// 在句柄创建/显示后统一设置尺寸、位置和圆角裁剪。
    /// 必须在此阶段读取 DeviceDpi，构造函数中的 DeviceDpi 可能还是 96。
    /// </summary>
    private void ApplyLayout()
    {
        if (!IsHandleCreated) return;

        float scale = DeviceDpi / 96f;
        int size = (int)(96 * scale);
        int corner = (int)(20 * scale);

        // 使用 ClientSize 对无边框窗体等价于 Size，显式设置更可靠。
        ClientSize = new Size(size, size);
        Region = MakeRoundedRegion(ClientSize.Width, ClientSize.Height, corner);

        var wa = Screen.FromHandle(Handle).WorkingArea;
        Location = new Point(wa.Right - Width - (int)(24 * scale), wa.Bottom - Height - (int)(24 * scale));
    }

    protected override void OnPaint(PaintEventArgs e)
    {
        var g = e.Graphics;
        g.ResetTransform();
        g.SmoothingMode = SmoothingMode.AntiAlias;
        g.PixelOffsetMode = PixelOffsetMode.HighQuality;

        // 半透明灰底圆角背景（配合整窗 Opacity）
        using (var bg = new SolidBrush(Color.FromArgb(80, 80, 80)))
        using (var path = MakeRoundedPath(new Rectangle(Point.Empty, ClientSize), Math.Min(ClientSize.Width, ClientSize.Height) * 20 / 96))
        {
            g.FillPath(bg, path);
        }

        var (muted, usingCount) = _audio.GetSummary();
        var color = muted ? Color.FromArgb(0xE8, 0x11, 0x23)
                    : usingCount > 0 ? Color.FromArgb(0xFF, 0x8C, 0x00)
                    : Color.FromArgb(0x10, 0x7C, 0x10);

        // 以窗口中心为圆心的正方形绘制区，严格水平+垂直居中
        const int pad = 14;
        int side = Math.Min(ClientSize.Width, ClientSize.Height) - pad * 2;
        int ox = (ClientSize.Width - side) / 2;
        int oy = (ClientSize.Height - side) / 2;
        MicGlyph.Draw(g, new Rectangle(ox, oy, side, side), color, muted);
    }

    protected override void OnMouseDown(MouseEventArgs e)
    {
        if (e.Button == MouseButtons.Left)
        {
            _dragStart = e.Location;
            _dragMoved = false;
        }
        base.OnMouseDown(e);
    }

    protected override void OnMouseMove(MouseEventArgs e)
    {
        if (e.Button == MouseButtons.Left)
        {
            if (!_dragMoved && (Math.Abs(e.X - _dragStart.X) > 4 || Math.Abs(e.Y - _dragStart.Y) > 4))
                _dragMoved = true;
            if (_dragMoved)
            {
                ReleaseCapture();
                SendMessage(Handle, 0xA1, 0x2, 0); // WM_NCLBUTTONDOWN, HTCAPTION
            }
        }
        base.OnMouseMove(e);
    }

    protected override void OnMouseUp(MouseEventArgs e)
    {
        // 没有拖动才算“点击”——切换静音
        if (e.Button == MouseButtons.Left && !_dragMoved)
        {
            _audio.ToggleDeviceMute();
            Invalidate();
        }
        base.OnMouseUp(e);
    }

    protected override void OnDoubleClick(EventArgs e)
    {
        // 双击在托盘/菜单里控制显隐；悬浮窗本体双击无操作，避免误触
        base.OnDoubleClick(e);
    }

    private static Region MakeRoundedRegion(int w, int h, int r)
    {
        return new Region(MakeRoundedPath(new Rectangle(0, 0, w, h), r));
    }

    private static GraphicsPath MakeRoundedPath(Rectangle rect, int r)
    {
        var path = new GraphicsPath();
        int d = r * 2;
        path.AddArc(rect.X, rect.Y, d, d, 180, 90);
        path.AddArc(rect.Right - d, rect.Y, d, d, 270, 90);
        path.AddArc(rect.Right - d, rect.Bottom - d, d, d, 0, 90);
        path.AddArc(rect.X, rect.Bottom - d, d, d, 90, 90);
        path.CloseFigure();
        return path;
    }

    [System.Runtime.InteropServices.DllImport("user32.dll")]
    private static extern bool ReleaseCapture();

    [System.Runtime.InteropServices.DllImport("user32.dll")]
    private static extern int SendMessage(IntPtr hWnd, int msg, int wParam, int lParam);
}
