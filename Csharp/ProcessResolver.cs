using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;

namespace MicTray;

/// <summary>
/// pid + 会话 DisplayName -> 友好显示名。
/// 处理两类噪音：
///   1) 形如 @%SystemRoot%\System32\AudioSrv.Dll,-202 的 MUI 资源字符串（系统音频服务）
///   2) PID 0 的系统会话（无法、也不该被单应用静音）
/// </summary>
public static class ProcessResolver
{
    [DllImport("shlwapi.dll", CharSet = CharSet.Unicode, ExactSpelling = true)]
    private static extern int SHLoadIndirectString(string pszSource, StringBuilder pszOutBuf, int cchOutBuf, IntPtr ppvReserved);

    public static string GetFriendlyName(int pid, string? displayName)
    {
        // 系统会话（音频服务本身）直接给固定友好名，不进菜单
        if (pid == 0)
            return "系统音频服务";

        var raw = (displayName ?? "").Trim();

        // 形如 @path,-id 或 @{pkg}?ms-resource://... 的资源串：先解码成本地化文本
        if (raw.StartsWith("@") && raw.Contains(",-", StringComparison.Ordinal))
        {
            var resolved = ResolveIndirect(raw);
            if (!string.IsNullOrWhiteSpace(resolved))
                raw = resolved.Trim();
        }

        // 解码后若是系统相关名，也归一成固定名
        if (raw.Contains("AudioSrv", StringComparison.OrdinalIgnoreCase) ||
            raw is "System" or "系统" or "System Sounds")
            return "系统音频服务";

        // 已是可读名（如 "Microsoft Edge" / "Teams"）就直接用
        if (!string.IsNullOrWhiteSpace(raw) && !raw.StartsWith("@", StringComparison.Ordinal))
            return raw;

        // 回退：进程名 / 窗口标题
        try
        {
            using var p = Process.GetProcessById(pid);
            if (!string.IsNullOrWhiteSpace(p.MainWindowTitle))
                return p.MainWindowTitle;
            return p.ProcessName;
        }
        catch
        {
            return $"PID {pid}";
        }
    }

    private static string? ResolveIndirect(string indirect)
    {
        var sb = new StringBuilder(512);
        var hr = SHLoadIndirectString(indirect, sb, sb.Capacity, IntPtr.Zero);
        return hr == 0 ? sb.ToString() : null;
    }
}
