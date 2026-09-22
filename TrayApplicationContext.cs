using System;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
using System.Threading;
using System.Windows.Forms;
using Microsoft.Win32;
using NAudio.CoreAudioApi.Interfaces;

namespace MicTray;

public sealed class TrayApplicationContext : ApplicationContext
{
    private readonly NotifyIcon _tray;
    private readonly AudioEngine _audio;
    private readonly Hotkey _hotkey;
    private readonly Hotkey _hotkeyOverlay;
        private readonly OverlayForm _overlay;
        private readonly System.Windows.Forms.Timer _refreshTimer;
        private readonly Control _marshal; // 不可见封送控件：构造期即创建句柄，用于把后台线程刷新安全切回 UI 线程
    private Icon? _currentIcon;

    public TrayApplicationContext()
    {
        _audio = new AudioEngine();

        // 专用不可见控件，构造期即创建句柄，用于把后台(WASAPI/热键)线程的刷新请求安全封送回 UI 线程。
        // 不依赖悬浮窗句柄，避免“句柄未创建时调用 BeginInvoke”崩溃。
        _marshal = new Control();
        _marshal.CreateControl();

        _tray = new NotifyIcon
        {
            Visible = true,
            Text = "MicTray",
            Icon = MakeIcon(IconState.Idle)
        };
        _currentIcon = _tray.Icon;

        _tray.MouseClick += (_, e) =>
        {
            if (e.Button == MouseButtons.Left)
            {
                _audio.ToggleDeviceMute();
                Refresh();
            }
        };

        // WASAPI 回调可能在非 UI 线程触发，统一 BeginInvoke 回 UI 线程刷新（_overlay 句柄已在构造末 CreateControl）
        _audio.StateChanged += (_, _) => UiInvoke(Refresh);

        // 实时回调兜底：2s 定时器，防止个别事件丢失导致状态不同步
        _refreshTimer = new System.Windows.Forms.Timer { Interval = 2000 };
        _refreshTimer.Tick += (_, _) => Refresh();
        _refreshTimer.Start();

        _hotkey = new Hotkey();
        _hotkey.Register(Keys.M, Modifiers.Control | Modifiers.Alt, OnHotkeyToggleMute);

        _hotkeyOverlay = new Hotkey();
        _hotkeyOverlay.Register(Keys.O, Modifiers.Control | Modifiers.Alt, ToggleOverlay);

        // 预创建悬浮窗句柄（不显示），以便隐藏态也能收到状态刷新
        _overlay = new OverlayForm(_audio);
        _overlay.CreateControl();

        Refresh();
    }

    private void ToggleOverlay()
    {
        if (_overlay.Visible) _overlay.Hide();
        else { _overlay.Show(); _overlay.BringToFront(); }
    }

    private void OnHotkeyToggleMute()
    {
        try
        {
            _audio.ToggleDeviceMute();
            Refresh();
            if (_overlay.Visible) UiInvoke(() => _overlay.Invalidate());
        }
        catch
        {
            // 热键回调异常不应击垮 NativeWindow 的消息循环
        }
    }

    /// <summary>
    /// 把刷新请求封送到 UI 线程。优先用 _marshal（构造期已创建句柄、生命周期伴随进程），
    /// 其次回退悬浮窗句柄；若两者句柄都未就绪（极早期），丢弃本次刷新，2s 定时器会兜底补刷。
    /// </summary>
    private void UiInvoke(Action action)
    {
        if (_marshal != null && _marshal.IsHandleCreated)
            _marshal.BeginInvoke(action);
        else if (_overlay != null && _overlay.IsHandleCreated)
            _overlay.BeginInvoke(action);
    }

    private void Refresh()
    {
        try
        {
            var muted = _audio.IsDeviceMuted;
            var sessions = _audio.GetSessions();
            var usingCount = sessions.Count(s =>
                s.State == AudioSessionState.AudioSessionStateActive && !s.IsMuted);

            var state = muted ? IconState.Muted : (usingCount > 0 ? IconState.InUse : IconState.Idle);
            SetIcon(MakeIcon(state));

            _tray.Text = muted
                ? "麦克风已静音"
                : (usingCount > 0 ? $"麦克风使用中（{usingCount} 个程序）" : "麦克风空闲");

            var menu = _tray.ContextMenuStrip ??= new ContextMenuStrip();
            menu.Items.Clear();

            var toggle = new ToolStripMenuItem(muted ? "解除全局静音 (Ctrl+Alt+M)" : "全局静音 (Ctrl+Alt+M)")
            {
                Font = new Font(menu.Font, FontStyle.Bold)
            };
            toggle.Click += (_, _) => { _audio.ToggleDeviceMute(); Refresh(); };
            menu.Items.Add(toggle);

            menu.Items.Add(new ToolStripSeparator());
            menu.Items.Add(new ToolStripMenuItem($"正在使用麦克风的程序：{sessions.Count}") { Enabled = false });

            foreach (var s in sessions)
            {
                var label = $"{(s.IsMuted ? "[静音] " : "")}{s.DisplayName} (PID {s.ProcessId})";
                var item = new ToolStripMenuItem(label)
                {
                    Checked = s.IsMuted,
                    Enabled = s.ProcessId != 0
                };
                var pid = s.ProcessId;
                var isMuted = s.IsMuted;
                item.Click += (_, _) => { _audio.SetSessionMute(pid, !isMuted); Refresh(); };
                menu.Items.Add(item);
            }

            menu.Items.Add(new ToolStripSeparator());
            var ov = new ToolStripMenuItem(_overlay.Visible ? "隐藏悬浮窗 (Ctrl+Alt+O)" : "显示悬浮窗 (Ctrl+Alt+O)");
            ov.Click += (_, _) => ToggleOverlay();
            menu.Items.Add(ov);

            var auto = new ToolStripMenuItem("开机自启") { Checked = IsAutoStart() };
            auto.Click += (_, _) =>
            {
                SetAutoStart(!auto.Checked);
                Refresh();
            };
            menu.Items.Add(auto);

            var exit = new ToolStripMenuItem("退出");
            exit.Click += (_, _) => Exit();
            menu.Items.Add(exit);

            // 双保险：托盘状态变化后强制悬浮窗重绘（其 OnPaint 读实时状态）。Refresh 本身已在 UI 线程，直接调用即可。
            if (_overlay.Visible) _overlay.Invalidate();
        }
        catch
        {
            // 瞬态 COM 错误忽略，下一次刷新再修正
        }
    }

    private void SetIcon(Icon newIcon)
    {
        var old = _currentIcon;
        _tray.Icon = newIcon;
        _currentIcon = newIcon;
        if (old != null) DestroyIcon(old.Handle);
    }

    private const string RunKey = @"Software\Microsoft\Windows\CurrentVersion\Run";
    private const string AppName = "MicTray";

    private static bool IsAutoStart()
        => Registry.GetValue($"HKEY_CURRENT_USER\\{RunKey}", AppName, null) != null;

    private static void SetAutoStart(bool enable)
    {
        using var key = Registry.CurrentUser.OpenSubKey(RunKey, true)
                        ?? Registry.CurrentUser.CreateSubKey(RunKey);
        if (enable) key.SetValue(AppName, Application.ExecutablePath);
        else key.DeleteValue(AppName, false);
    }

    private void Exit()
    {
        _refreshTimer.Stop();
        _hotkey.Unregister();
        _hotkey.Dispose();
        _hotkeyOverlay.Unregister();
        _hotkeyOverlay.Dispose();
        _overlay.Dispose();
        _marshal.Dispose();
        _tray.Visible = false;
        _tray.Dispose();
        _currentIcon = null;
        Application.Exit();
    }

    [DllImport("user32.dll")]
    private static extern bool DestroyIcon(IntPtr hIcon);

    private enum IconState { Idle, InUse, Muted }

    private static Icon MakeIcon(IconState state)
    {
        const int S = 32;
        var color = state switch
        {
            IconState.Muted => Color.FromArgb(0xE8, 0x11, 0x23), // 红：静音
            IconState.InUse => Color.FromArgb(0xFF, 0x8C, 0x00), // 橙：有程序占用
            _ => Color.FromArgb(0x10, 0x7C, 0x10)                // 绿：空闲
        };

        using var bmp = new Bitmap(S, S, PixelFormat.Format32bppArgb);
        using (var g = Graphics.FromImage(bmp))
        {
            g.SmoothingMode = System.Drawing.Drawing2D.SmoothingMode.AntiAlias;
            g.Clear(Color.Transparent);
            MicGlyph.Draw(g, new Rectangle(0, 0, S, S), color, state == IconState.Muted);
        }

        // 预乘 alpha：GetHicon 生成的 HICON 需要预乘 alpha 才能正确合成（否则边缘/透明异常）。
        PremultiplyAlpha(bmp);
        IntPtr hIcon = bmp.GetHicon();
        // GetHicon 返回的 HICON 与底层位图共享像素，源 Bitmap 一旦 Dispose 图标会变空白。
        // 这里 Clone 出一份「独立拥有句柄」的 Icon（内部 CopyIcon 复制内存），再释放临时 hIcon，
        // 这样 bmp 释放后图标仍有效，由 SetIcon 在替换时销毁「上一个」图标句柄（零泄漏、无悬空）。
        Icon icon = (Icon)Icon.FromHandle(hIcon).Clone();
        DestroyIcon(hIcon);
        return icon;
    }

    /// <summary>
    /// 将 32 位 ARGB 位图原地转为预乘 alpha，供 GetHicon 生成正确的托盘图标。
    /// </summary>
    private static void PremultiplyAlpha(Bitmap bmp)
    {
        var rect = new Rectangle(0, 0, bmp.Width, bmp.Height);
        var data = bmp.LockBits(rect, ImageLockMode.ReadWrite, PixelFormat.Format32bppArgb);
        int bytes = Math.Abs(data.Stride) * bmp.Height;
        var buf = new byte[bytes];
        Marshal.Copy(data.Scan0, buf, 0, bytes);
        for (int i = 0; i < bytes; i += 4)
        {
            byte a = buf[i + 3];
            if (a == 0) continue;
            buf[i]     = (byte)(buf[i]     * a / 255); // B
            buf[i + 1] = (byte)(buf[i + 1] * a / 255); // G
            buf[i + 2] = (byte)(buf[i + 2] * a / 255); // R
        }
        Marshal.Copy(buf, 0, data.Scan0, bytes);
        bmp.UnlockBits(data);
    }
}
