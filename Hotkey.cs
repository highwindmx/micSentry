using System;
using System.Windows.Forms;

namespace MicTray;

[Flags]
public enum Modifiers
{
    Alt = 1,
    Control = 2,
    Shift = 4,
    Win = 8
}

/// <summary>
/// 注册全局热键（如 Ctrl+Alt+M 切换全局静音）。
/// 用一个隐藏的 NativeWindow 接收 WM_HOTKEY 消息。
/// </summary>
public sealed class Hotkey : IDisposable
{
    private const int WmHotkey = 0x0312;
    private const int Id = 1;

    private readonly MessageWindow _window;
    private Action? _action;

    public Hotkey() => _window = new MessageWindow(this);

    public void Register(Keys key, Modifiers modifiers, Action action)
    {
        _action = action;
        if (!RegisterHotKey(_window.Handle, Id, (uint)modifiers, (uint)key))
        {
            // 注册失败（如被其他程序占用）静默忽略
        }
    }

    public void Unregister() => UnregisterHotKey(_window.Handle, Id);

    public void Dispose() => Unregister();

    private sealed class MessageWindow : NativeWindow
    {
        private readonly Hotkey _owner;
        public MessageWindow(Hotkey owner) : base()
        {
            _owner = owner;
            CreateHandle(new CreateParams());
        }

        protected override void WndProc(ref Message m)
        {
            if (m.Msg == WmHotkey)
                _owner._action?.Invoke();
            base.WndProc(ref m);
        }
    }

    [System.Runtime.InteropServices.DllImport("user32.dll")]
    private static extern bool RegisterHotKey(IntPtr hWnd, int id, uint fsModifiers, uint vk);

    [System.Runtime.InteropServices.DllImport("user32.dll")]
    private static extern bool UnregisterHotKey(IntPtr hWnd, int id);
}
