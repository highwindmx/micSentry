//! Windows WASAPI 后端（windows crate 直接调 COM）。
//!
//! # 为什么本文件要把 COM 关进专用线程
//!
//! winit（Slint 的窗口后端）在**主线程**创建窗口时会调用 `OleInitialize`，
//! 而 `OleInitialize` 要求当前线程是 **STA**。若主线程先被
//! `CoInitializeEx(None, COINIT_MULTITHREADED)` 设成 **MTA**，winit 的
//! `OleInitialize` 会返回 `RPC_E_CHANGED_MODE (0x80010106)` 并 **直接 panic**：
//!
//! ```text
//! panicked at winit/src/platform_impl/windows/window.rs:
//!   OleInitialize failed! Result was: `RPC_E_CHANGED_MODE`.
//! ```
//!
//! 在 `windows_subsystem = "windows"` 下，这个 panic 表现为**进程静默退出**（退出码 101，
//! 无控制台输出、也不产生 WER 崩溃记录），现象就是"双击了但什么反应都没有"。
//!
//! 因此本模块的设计是：**主线程绝不初始化 COM**。所有 WASAPI 调用都通过 channel
//! 投递到一个自建的、MTA 的工作线程上执行；主线程只做 `Send` 的消息收发。
//! 这样既避开了 apartment 冲突，也顺带消掉了跨线程复用 COM 接口指针的风险
//! （`WindowsMic` 因此天然是 `Send + Sync`，不再需要 `unsafe impl`）。
//!
//! # 为什么每次操作都"现场回读"而不是缓存状态
//!
//! 麦克风静音态会被**外部**改变（键盘 Fn 静音键、Windows 音量面板、其它软件）。
//! 一旦用缓存值做读-改-写，缓存与真实态就会分叉，用户会看到
//! "图标显示已静音、点一下却又变成静音"这类"回不去"的怪现象。
//! 所以：**读永远走真实端点，写之后立刻回读校验**。

use std::sync::mpsc::{self, Sender, SyncSender};
use std::time::Duration;

use windows::core::{HSTRING, PWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    eCapture, eCommunications, eConsole, eMultimedia, ERole, IMMDevice, IMMDeviceCollection,
    IMMDeviceEnumerator, MMDeviceEnumerator as CLSID_MMDEVICE_ENUMERATOR, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToStringAlloc};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_APARTMENTTHREADED, COINIT_MULTITHREADED, STGM_READ,
};

use crate::audio::{DeviceInfo, Snapshot};
use crate::log;

/// 统一用 `Res<T>` 表示"可能失败"的结果，错误文本里一定带 HRESULT，便于取证。
///
/// 注意：**不要** `use windows::core::*` 或 import `windows::core::Result` ——
/// 它是单参数类型别名，会遮蔽 std 的 `Result<T, E>` 并引发难以定位的类型错误。
type Res<T> = std::result::Result<T, String>;

// MMDeviceEnumerator 的 **CLSID**：{bcde0395-e52f-467c-8e3d-c4579291692e}
//
// 直接引用 windows-rs 的常量（`Win32::Media::Audio::MMDeviceEnumerator`），不要手写。
//
// 踩坑记录（真实事故，2026-09-22）：曾经手写成
// `GUID::from_u128(0xa95664d2_9614_4f35_a746_de8db63617e6)` —— 而
// `{A95664D2-9614-4F35-A746-DE8DB63617E6}` 是 **IMMDeviceEnumerator 的 IID**，
// 不是共存类（CoClass）的 CLSID。`CoCreateInstance` 只查 `HKCR\CLSID\`，
// 而该 GUID 只作为接口注册在 `HKCR\Interface\` 下 → 在**任何**机器上都会返回
// `0x80040154 REGDB_E_CLASSNOTREG（没有注册类）`，症状与"本机没有麦克风"
// 一模一样，极易误判成环境限制。
// 教训：CLSID 与 IID 是两回事；拿不准就用 crate 里的常量。

/// 投递给 COM 线程的请求。
enum Req {
    /// 读当前受控设备的完整状态（身份 + 静音态）
    Probe(SyncSender<Snapshot>),
    /// 设置静音（带回读校验），回执最终快照
    SetMuted { want: bool, ack: SyncSender<Snapshot> },
    /// 读-取反-写，回执最终快照
    Toggle(SyncSender<Snapshot>),
    /// 枚举活动录音设备
    Devices(SyncSender<Vec<DeviceInfo>>),
    /// 切换受控设备，回执选择后的快照
    Select { id: String, ack: SyncSender<Snapshot> },
}

/// 跨线程安全的 WASAPI 控制器：只持有一个发往 COM 线程的 channel。
pub struct WindowsMic {
    tx: Sender<Req>,
}

impl WindowsMic {
    /// 启动 COM 工作线程并等待其就绪。失败返回 `Err`（由 `audio::create()` 降级处理）。
    ///
    /// `selected_id`：显式选择的录音设备 id，空串 = 跟随系统默认。
    pub fn new(selected_id: String) -> Res<Self> {
        let (tx, rx) = mpsc::channel::<Req>();
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Res<()>>(1);

        std::thread::Builder::new()
            .name("wasapi-com".to_string())
            .spawn(move || com_thread(rx, ready_tx, selected_id))
            .map_err(|e| format!("spawn wasapi-com 线程失败: {e}"))?;

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self { tx }),
            Ok(Err(e)) => Err(e),
            Err(e) => Err(format!("wasapi-com 线程就绪前退出: {e}")),
        }
    }

    /// 向 COM 线程发一条请求并等回执。线程已死时返回 `None`（调用方负责降级）。
    fn round_trip<T>(&self, make: impl FnOnce(SyncSender<T>) -> Req) -> Option<T> {
        let (tx, rx) = mpsc::sync_channel::<T>(1);
        if let Err(e) = self.tx.send(make(tx)) {
            log::line(&format!("ERROR wasapi-com 线程不可用: {e}"));
            return None;
        }
        rx.recv().ok()
    }
}

impl super::MicControllerTrait for WindowsMic {
    fn snapshot(&self) -> Snapshot {
        self.round_trip(Req::Probe).unwrap_or_else(|| {
            Snapshot::unknown("wasapi-com 线程不可用（音频后端已失去响应）")
        })
    }

    fn set_muted(&self, muted: bool) -> Snapshot {
        self.round_trip(|ack| Req::SetMuted { want: muted, ack })
            .unwrap_or_else(|| {
                Snapshot::unknown("wasapi-com 线程不可用（音频后端已失去响应）")
            })
    }

    fn toggle_muted(&self) -> Snapshot {
        self.round_trip(Req::Toggle).unwrap_or_else(|| {
            Snapshot::unknown("wasapi-com 线程不可用（音频后端已失去响应）")
        })
    }

    fn devices(&self) -> Vec<DeviceInfo> {
        self.round_trip(Req::Devices).unwrap_or_default()
    }

    fn select_device(&self, id: &str) -> Snapshot {
        let id = id.to_string();
        self.round_trip(|ack| Req::Select { id, ack }).unwrap_or_else(|| {
            Snapshot::unknown("wasapi-com 线程不可用（音频后端已失去响应）")
        })
    }
}

/// COM 线程持有的会话状态。
struct ComCtx {
    enumerator: IMMDeviceEnumerator,
    /// 显式选择的设备 id；空串 = 跟随系统默认
    selected_id: String,
}

/// 一次成功读取得到的"活体"信息。
struct Live {
    vol: IAudioEndpointVolume,
    id: String,
    name: String,
    muted: bool,
}

impl ComCtx {
    fn mk(&self, id: &str, name: &str, muted: Option<bool>, error: Option<String>) -> Snapshot {
        Snapshot {
            device_id: id.to_string(),
            device_name: name.to_string(),
            muted,
            follows_default: self.selected_id.is_empty(),
            selected_id: self.selected_id.clone(),
            error,
        }
    }

    /// 解析出真正要控制的设备：显式选择的优先，否则系统默认。
    ///
    /// 默认角色依次尝试 eConsole（Windows 设置里的"默认设备"）→
    /// eCommunications（通信默认）→ eMultimedia，任一可用即返回。
    fn resolve(&self) -> Res<IMMDevice> {
        if !self.selected_id.is_empty() {
            let h = HSTRING::from(self.selected_id.as_str());
            match unsafe { self.enumerator.GetDevice(&h) } {
                Ok(d) => return Ok(d),
                Err(e) => log::line(&format!(
                    "WARN  已选设备当前不可用（{e}），本次回退系统默认"
                )),
            }
        }
        unsafe {
            self.enumerator
                .GetDefaultAudioEndpoint(eCapture, eConsole)
                .or_else(|_| self.enumerator.GetDefaultAudioEndpoint(eCapture, eCommunications))
                .or_else(|_| self.enumerator.GetDefaultAudioEndpoint(eCapture, eMultimedia))
                .map_err(|e| format!("GetDefaultAudioEndpoint(eCapture) 全部失败: {e}"))
        }
    }

    /// 设备身份（id + 名称），不碰静音。
    fn identify(&self) -> Res<(String, String)> {
        let dev = self.resolve()?;
        let id = unsafe { device_id(&dev)? };
        let name = unsafe { device_name(&dev) }.unwrap_or_else(|e| {
            log::line(&format!("WARN  设备名读取失败: {e}"));
            "(名称未知)".to_string()
        });
        Ok((id, name))
    }

    /// 设备身份 + `IAudioEndpointVolume` + 当前静音态。
    fn live(&self) -> Res<Live> {
        let dev = self.resolve()?;
        let id = unsafe { device_id(&dev)? };
        let name = unsafe { device_name(&dev) }.unwrap_or_else(|e| {
            log::line(&format!("WARN  设备名读取失败: {e}"));
            "(名称未知)".to_string()
        });
        let vol: IAudioEndpointVolume = unsafe {
            dev.Activate(CLSCTX_ALL, None)
                .map_err(|e| format!("IMMDevice::Activate(IAudioEndpointVolume) 失败: {e}"))?
        };
        let muted = unsafe {
            vol.GetMute()
                .map_err(|e| format!("IAudioEndpointVolume::GetMute 失败: {e}"))?
        }
        .as_bool();
        Ok(Live {
            vol,
            id,
            name,
            muted,
        })
    }

    /// 读快照。读取失败时 `muted = None`（状态未知），并尽量补上设备身份。
    fn probe(&self) -> Snapshot {
        match self.live() {
            Ok(l) => self.mk(&l.id, &l.name, Some(l.muted), None),
            Err(e) => {
                let (id, name) = match self.identify() {
                    Ok((id, name)) => (id, name),
                    Err(e2) => {
                        log::line(&format!("ERROR 设备解析失败: {e2}"));
                        (String::new(), "(未找到可用麦克风)".to_string())
                    }
                };
                self.mk(&id, &name, None, Some(e))
            }
        }
    }

    /// 设置静音 + 回读校验。
    ///
    /// 读取失败时**拒绝盲写**：否则会退化成"每次都写 true"，
    /// 用户感觉就是「静音之后再也回不去」。
    fn apply(&self, want: bool) -> Snapshot {
        let l = match self.live() {
            Ok(l) => l,
            Err(e) => {
                log::line(&format!(
                    "ERROR 读取当前状态失败，拒绝盲写（否则会退化成\"只能静音\"）: {e}"
                ));
                return self.probe();
            }
        };

        if l.muted == want {
            log::line(&format!("ACT   SetMute 跳过：目标 {want}，当前已是该状态"));
            return self.mk(&l.id, &l.name, Some(l.muted), None);
        }

        if let Err(e) = unsafe { l.vol.SetMute(want, std::ptr::null()) } {
            let msg = format!("IAudioEndpointVolume::SetMute({want}) 失败: {e}");
            log::line(&format!("ERROR {msg}"));
            return self.mk(&l.id, &l.name, Some(l.muted), Some(msg));
        }

        // 回读校验：最多 3 次、每次间隔 30ms，容忍驱动 / 硬件的落地延迟
        let mut actual: Option<bool> = None;
        let mut last_read_err: Option<String> = None;
        for attempt in 1..=3u32 {
            std::thread::sleep(Duration::from_millis(30));
            match unsafe { l.vol.GetMute() } {
                Ok(b) => {
                    actual = Some(b.as_bool());
                    if actual == Some(want) {
                        break;
                    }
                    if attempt == 3 {
                        log::line(&format!(
                            "WARN  SetMute({want}) 回读不一致（已重试 {attempt} 次）"
                        ));
                    }
                }
                Err(e) => {
                    last_read_err = Some(format!("SetMute({want}) 后回读失败: {e}"));
                    if attempt == 3 {
                        let msg = last_read_err.clone().unwrap_or_default();
                        log::line(&format!("ERROR {msg}"));
                    }
                }
            }
        }

        match (actual, last_read_err) {
            (Some(a), _) if a == want => {
                log::line(&format!("ACT   SetMute({want}) 生效（回读校验通过）"));
                self.mk(&l.id, &l.name, Some(a), None)
            }
            (Some(a), _) => {
                let msg = format!("SetMute({want}) 未生效：回读实际值 = {a}（可能被系统/驱动接管）");
                log::line(&format!("WARN  {msg}"));
                self.mk(&l.id, &l.name, Some(a), Some(msg))
            }
            (None, Some(e)) => self.mk(&l.id, &l.name, None, Some(e)),
            (None, None) => self.mk(
                &l.id,
                &l.name,
                None,
                Some("SetMute 后无法回读，状态未知".to_string()),
            ),
        }
    }

    /// 枚举活动录音设备，并标出哪只是系统默认 / 通信默认。
    fn list_devices(&self) -> Vec<DeviceInfo> {
        let mut out = Vec::new();
        let console_id = unsafe { default_device_id(&self.enumerator, eConsole) };
        let comms_id = unsafe { default_device_id(&self.enumerator, eCommunications) };

        let col: IMMDeviceCollection =
            match unsafe { self.enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE) } {
                Ok(c) => c,
                Err(e) => {
                    log::line(&format!("ERROR EnumAudioEndpoints(eCapture) 失败: {e}"));
                    return out;
                }
            };
        let n = match unsafe { col.GetCount() } {
            Ok(n) => n,
            Err(e) => {
                log::line(&format!("ERROR IMMDeviceCollection::GetCount 失败: {e}"));
                return out;
            }
        };

        for i in 0..n {
            let Ok(dev) = (unsafe { col.Item(i) }) else {
                continue;
            };
            let Ok(id) = (unsafe { device_id(&dev) }) else {
                continue;
            };
            let name = unsafe { device_name(&dev) }.unwrap_or_else(|_| "(名称未知)".to_string());
            out.push(DeviceInfo {
                is_default_console: console_id.as_deref() == Some(id.as_str()),
                is_default_comms: comms_id.as_deref() == Some(id.as_str()),
                name,
                id,
            });
        }
        out
    }
}

/// COM 工作线程主体：自己初始化 MTA，之后所有 WASAPI 调用都在这条线程上串行执行。
fn com_thread(rx: mpsc::Receiver<Req>, ready: SyncSender<Res<()>>, selected_id: String) {
    let enumerator = match create_enumerator() {
        Ok(e) => e,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };

    let mut ctx = ComCtx {
        enumerator,
        selected_id: selected_id.clone(),
    };

    let _ = ready.send(Ok(()));
    log::line(&format!(
        "OK    WASAPI COM 线程就绪（MTA）；显式选择 = {}",
        if selected_id.is_empty() {
            "<跟随系统默认>".to_string()
        } else {
            selected_id
        }
    ));
    // 启动时把设备清单写进日志，用户可直接核对"我控制的是哪只麦"
    for l in crate::audio::format_devices(&ctx.list_devices()) {
        log::line(&l);
    }

    while let Ok(req) = rx.recv() {
        match req {
            Req::Probe(ack) => {
                let _ = ack.send(ctx.probe());
            }
            Req::SetMuted { want, ack } => {
                let _ = ack.send(ctx.apply(want));
            }
            Req::Toggle(ack) => {
                let snap = ctx.probe();
                let out = match snap.muted {
                    Some(m) => ctx.apply(!m),
                    None => {
                        log::line("ERROR 状态未知（读取失败），拒绝盲写切换");
                        snap
                    }
                };
                let _ = ack.send(out);
            }
            Req::Devices(ack) => {
                let _ = ack.send(ctx.list_devices());
            }
            Req::Select { id, ack } => {
                ctx.selected_id = id;
                log::line(&format!(
                    "ACT   选择设备 -> {}",
                    if ctx.selected_id.is_empty() {
                        "<跟随系统默认>".to_string()
                    } else {
                        ctx.selected_id.clone()
                    }
                ));
                let _ = ack.send(ctx.probe());
            }
        }
    }
    log::line("WASAPI COM 线程退出");
}

/// 创建工作线程上的 `IMMDeviceEnumerator`。
///
/// 先按 **MTA**（`COINIT_MULTITHREADED`）试；失败则退回 **STA**
/// （`COINIT_APARTMENTTHREADED`）再试一次 —— 个别环境下 MTA 起不了外进程 COM。
///
/// 失败时把 HRESULT 一起带出来，并给出下一步指引。
/// `0x80040154 REGDB_E_CLASSNOTREG（没有注册类）` 的含义是
/// 「`HKCR\CLSID` 下查不到这个类」——先确认 CLSID 本身没写错（见上方踩坑记录），
/// 再看 `--diag` 的环境自检。
fn create_enumerator() -> Res<IMMDeviceEnumerator> {
    // 第一次：MTA
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    match unsafe { CoCreateInstance(&CLSID_MMDEVICE_ENUMERATOR, None, CLSCTX_ALL) } {
        Ok(e) => return Ok(e),
        Err(e_mta) => {
            log::line(&format!(
                "WARN  MTA 下创建 MMDeviceEnumerator 失败（{e_mta}），改用 STA 重试"
            ));
            // 换 apartment：必须先 CoUninitialize 才能重新 CoInitializeEx
            unsafe {
                CoUninitialize();
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            }
            match unsafe { CoCreateInstance(&CLSID_MMDEVICE_ENUMERATOR, None, CLSCTX_ALL) } {
                Ok(e) => {
                    log::line("OK    STA 下创建 MMDeviceEnumerator 成功");
                    Ok(e)
                }
                Err(e_sta) => {
                    // 错误文本保持精简（它会被写进快照、菜单和日志多处）；
                    // 排查指引单独作为 HINT 行输出，避免把长句塞进数据结构里。
                    let msg = format!(
                        "CoCreateInstance(MMDeviceEnumerator) 失败: {e_sta}（MTA 下为 {e_mta}）"
                    );
                    log::line(&format!("ERROR {msg}"));
                    log::line(
                        "HINT  0x80040154「没有注册类」= HKCR\\CLSID 下查不到 MMDeviceEnumerator。\
                         先确认 CLSID 是 {bcde0395-e52f-467c-8e3d-c4579291692e}（不是 \
                         IMMDeviceEnumerator 的 IID {a95664d2-...}），再跑 \
                         `MicTrayRs.exe --diag` 看环境自检与设备清单。",
                    );
                    Err(msg)
                }
            }
        }
    }
}

/// 取系统默认设备的 id（失败返回 `None`）。
unsafe fn default_device_id(en: &IMMDeviceEnumerator, role: ERole) -> Option<String> {
    let dev = unsafe { en.GetDefaultAudioEndpoint(eCapture, role) }.ok()?;
    unsafe { device_id(&dev) }.ok()
}

/// `IMMDevice::GetId()` 返回 CoTaskMem 分配的内存，取完必须 `CoTaskMemFree`。
unsafe fn device_id(dev: &IMMDevice) -> Res<String> {
    let p: PWSTR = unsafe { dev.GetId() }.map_err(|e| format!("IMMDevice::GetId 失败: {e}"))?;
    if p.is_null() {
        return Err("IMMDevice::GetId 返回空指针".to_string());
    }
    let s = unsafe { p.to_string() }.map_err(|e| format!("设备 id UTF-16 解码失败: {e}"))?;
    unsafe { CoTaskMemFree(Some(p.0 as *const core::ffi::c_void)) };
    Ok(s)
}

/// 取端点友好名（`PKEY_Device_FriendlyName`）。
/// `PROPVARIANT` 与 `PropVariantToStringAlloc` 返回的字符串都是**需要手动释放**的。
unsafe fn device_name(dev: &IMMDevice) -> Res<String> {
    let store = unsafe { dev.OpenPropertyStore(STGM_READ) }
        .map_err(|e| format!("IMMDevice::OpenPropertyStore 失败: {e}"))?;
    let mut pv = unsafe { store.GetValue(&PKEY_Device_FriendlyName) }
        .map_err(|e| format!("IPropertyStore::GetValue(FriendlyName) 失败: {e}"))?;

    let out = (|| -> Res<String> {
        let p: PWSTR = unsafe { PropVariantToStringAlloc(&pv) }
            .map_err(|e| format!("PropVariantToStringAlloc 失败: {e}"))?;
        let s = unsafe { p.to_string() }.map_err(|e| format!("设备名 UTF-16 解码失败: {e}"))?;
        unsafe { CoTaskMemFree(Some(p.0 as *const core::ffi::c_void)) };
        Ok(s)
    })();

    unsafe {
        let _ = PropVariantClear(&mut pv);
    }
    out
}
