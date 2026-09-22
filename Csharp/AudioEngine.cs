using System;
using System.Collections.Generic;
using System.Linq;
using NAudio.CoreAudioApi;
using NAudio.CoreAudioApi.Interfaces;

namespace MicTray;

public sealed class MicSession
{
    public int ProcessId { get; init; }
    public string DisplayName { get; init; } = "";
    public bool IsMuted { get; init; }
    public AudioSessionState State { get; init; }
    public AudioSessionControl? Control { get; init; }
}

/// <summary>
/// 封装 WASAPI / Core Audio：全局设备静音、单应用会话枚举与静音、实时回调。
/// 借鉴 mic-overlay：全局静音优先作用于 Communications 端点；遍历所有 ACTIVE 采集端点
/// 枚举会话，覆盖“使用非默认麦克风”的程序。
/// </summary>
public sealed class AudioEngine : IDisposable
{
    private readonly MMDeviceEnumerator _enumerator = new();
    private MMDevice? _device;                       // 默认通信端点（全局静音目标）
    private List<MMDevice> _captureDevices = new();  // 所有 ACTIVE 采集端点（会话枚举覆盖范围）

    public event EventHandler? StateChanged;

    public AudioEngine() => RefreshDevice();

    private void RefreshDevice()
    {
        try
        {
            // 优先 Communications 端点（通话/直播实际用的麦），回退 Console。
            try { _device = _enumerator.GetDefaultAudioEndpoint(DataFlow.Capture, Role.Communications); }
            catch { _device = null; }
            if (_device == null)
                _device = _enumerator.GetDefaultAudioEndpoint(DataFlow.Capture, Role.Console);

            if (_device != null)
            {
                _device.AudioEndpointVolume.OnVolumeNotification += OnVolumeNotification;
                _device.AudioSessionManager.OnSessionCreated += OnSessionCreated;
            }
            // 枚举全部 ACTIVE 采集端点，覆盖“用非默认麦的 App”。
            _captureDevices = _enumerator.EnumerateAudioEndPoints(DataFlow.Capture, DeviceState.Active).ToList();
        }
        catch
        {
            _device = null;
            _captureDevices = new();
        }
    }

    private void OnVolumeNotification(AudioVolumeNotificationData _)
        => StateChanged?.Invoke(this, EventArgs.Empty);

    private void OnSessionCreated(object _, IAudioSessionControl _session)
        => StateChanged?.Invoke(this, EventArgs.Empty);

    public bool IsDeviceMuted => _device?.AudioEndpointVolume.Mute ?? false;

    public void SetDeviceMute(bool mute)
    {
        if (_device?.AudioEndpointVolume is { } ev) ev.Mute = mute;
        // 主动广播：程序自身改静音时 WASAPI 回调未必在本进程触发，
        // 不广播会导致只依赖 StateChanged 的悬浮窗不同步。WASAPI 回调若再触发一次也只是重复刷新，无害。
        StateChanged?.Invoke(this, EventArgs.Empty);
    }

    public void ToggleDeviceMute() => SetDeviceMute(!IsDeviceMuted);

    /// <summary>
    /// 汇总当前状态：是否静音 + 正在活跃用麦（未静音）的程序数。悬浮窗三态用。
    /// </summary>
    public (bool Muted, int UsingCount) GetSummary()
    {
        var muted = IsDeviceMuted;
        var usingCount = 0;
        foreach (var s in GetSessions())
            if (s.State == AudioSessionState.AudioSessionStateActive && !s.IsMuted) usingCount++;
        return (muted, usingCount);
    }

    public IReadOnlyList<MicSession> GetSessions()
    {
        var list = new List<MicSession>();
        foreach (var dev in _captureDevices)
        {
            try
            {
                var sm = dev.AudioSessionManager;
                try { sm.RefreshSessions(); } catch { }
                var sessions = sm.Sessions;
                for (var i = 0; i < sessions.Count; i++)
                {
                    var session = sessions[i];
                    try
                    {
                        var pid = (int)session.GetProcessID;
                        if (pid == 0) continue;                              // 系统音频服务会话，跳过
                        if (list.Exists(x => x.ProcessId == pid)) continue;  // 同一 App 跨端点去重
                        var sv = session.SimpleAudioVolume;
                        list.Add(new MicSession
                        {
                            ProcessId = pid,
                            DisplayName = ProcessResolver.GetFriendlyName(pid, session.DisplayName),
                            IsMuted = sv?.Mute ?? true,
                            State = session.State,
                            Control = session
                        });
                    }
                    catch
                    {
                        // 会话已失效（进程退出），跳过
                    }
                }
            }
            catch { }
        }
        return list;
    }

    public void SetSessionMute(int processId, bool mute)
    {
        foreach (var dev in _captureDevices)
        {
            try
            {
                var sm = dev.AudioSessionManager;
                var sessions = sm.Sessions;
                for (var i = 0; i < sessions.Count; i++)
                {
                    var session = sessions[i];
                    try
                    {
                        if ((int)session.GetProcessID != processId) continue;
                        var sv = session.SimpleAudioVolume;
                        if (sv != null) sv.Mute = mute;
                    }
                    catch { }
                }
            }
            catch { }
        }
        StateChanged?.Invoke(this, EventArgs.Empty);
    }

    public void Dispose()
    {
        try { if (_device?.AudioSessionManager is { } sm) sm.OnSessionCreated -= OnSessionCreated; }
        catch { /* 忽略卸载期 COM 异常 */ }
        try { _enumerator.Dispose(); } catch { }
        try { _device?.Dispose(); } catch { }
        foreach (var d in _captureDevices) { try { d.Dispose(); } catch { } }
    }
}
