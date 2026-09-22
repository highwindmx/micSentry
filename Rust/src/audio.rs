//! 麦克风控制后端。用 trait 隔离平台实现：Windows 走 WASAPI，其他平台用占位实现。
//! 以后新增 macOS / Linux 只需实现 `MicControllerTrait` 并加到 `create()`。

pub mod stub;
pub mod windows_impl;

/// 一个活动的录音设备（纯数据，可跨线程传递）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    /// 是否为系统「默认设备」（eConsole —— Windows 设置里显示为默认的那只）
    pub is_default_console: bool,
    /// 是否为「默认通信设备」（eCommunications —— Teams/Zoom 等通信用）
    pub is_default_comms: bool,
}

impl DeviceInfo {
    /// 菜单 / 日志里显示的一行文字。
    pub fn label(&self) -> String {
        let mut tags: Vec<&str> = Vec::new();
        if self.is_default_console {
            tags.push("默认");
        }
        if self.is_default_comms {
            tags.push("通信默认");
        }
        if tags.is_empty() {
            self.name.clone()
        } else {
            format!("{} [{}]", self.name, tags.join(" / "))
        }
    }
}

/// 把设备清单格式化成若干行（启动日志与 `--diag` 共用，便于用户核对设备）。
pub fn format_devices(devs: &[DeviceInfo]) -> Vec<String> {
    let mut out = Vec::with_capacity(devs.len() * 2 + 1);
    out.push(format!("      活动录音设备 {} 个:", devs.len()));
    for (i, d) in devs.iter().enumerate() {
        out.push(format!("        [{i}] {}", d.label()));
        out.push(format!("             id={}", d.id));
    }
    out
}

/// 一次操作后的设备状态快照。
///
/// # 关键约定
/// `muted == None` 表示**读取失败、状态未知**，绝不可以当成"未静音"。
/// 旧实现用 `unwrap_or(false)` 把读取失败静默当成 false，会退化成
/// "每次都写 true" → 用户感觉到的是「静音之后就再也回不去了」。
#[derive(Clone, Debug)]
pub struct Snapshot {
    /// 当前**实际受控**设备的 id（即使是跟随系统默认，这里也是解析后的真实设备 id）
    pub device_id: String,
    /// 当前实际受控设备的可读名称
    pub device_name: String,
    /// 真实静音态；`None` = 读取失败、状态未知
    pub muted: Option<bool>,
    /// 是否处于「跟随系统默认」模式
    pub follows_default: bool,
    /// 用户显式选择的设备 id（空串 = 跟随系统默认）
    pub selected_id: String,
    /// 最近一次失败原因（含 HRESULT），成功为 None
    pub error: Option<String>,
}

impl Snapshot {
    /// 完全拿不到设备信息时使用（例如后端线程已死 / 非 Windows 平台）。
    pub fn unknown(reason: impl Into<String>) -> Self {
        Snapshot {
            device_id: String::new(),
            device_name: "(未找到可用麦克风)".to_string(),
            muted: None,
            follows_default: true,
            selected_id: String::new(),
            error: Some(reason.into()),
        }
    }

    pub fn state_text(&self) -> &'static str {
        match self.muted {
            Some(true) => "已静音",
            Some(false) => "未静音",
            None => "状态未知",
        }
    }
}

/// 跨平台麦克风控制接口。
pub trait MicControllerTrait: Send + Sync {
    /// 读取当前受控设备的真实状态（含设备名 / id）。
    fn snapshot(&self) -> Snapshot;

    /// 设置静音，并在写入后**立即回读校验**，返回真实结果。
    fn set_muted(&self, muted: bool) -> Snapshot;

    /// 切换静音：读 → 取反 → 写 → 回读校验。
    ///
    /// 状态读取失败时**拒绝盲写**（直接返回原快照 + error），
    /// 否则就会重现"只能静音、回不去"的老毛病。
    fn toggle_muted(&self) -> Snapshot {
        let cur = self.snapshot();
        match cur.muted {
            Some(m) => self.set_muted(!m),
            None => cur,
        }
    }

    /// 枚举当前活动的录音设备。
    fn devices(&self) -> Vec<DeviceInfo>;

    /// 选择受控设备（按 id）；传空串 = 跟随系统默认。返回选择后的新状态。
    fn select_device(&self, id: &str) -> Snapshot;
}

pub type MicController = std::sync::Arc<dyn MicControllerTrait>;

/// 按当前平台创建后端实例。
/// WASAPI 初始化失败时降级到占位后端（记日志、不 panic），保证程序仍能启动。
///
/// `selected_device_id`：用户上次显式选择的设备 id，空串 = 跟随系统默认。
pub fn create(selected_device_id: String) -> MicController {
    #[cfg(target_os = "windows")]
    {
        match windows_impl::WindowsMic::new(selected_device_id) {
            Ok(m) => std::sync::Arc::new(m) as MicController,
            Err(e) => {
                crate::log::line(&format!(
                    "ERROR WASAPI init failed: {e}; 降级为占位后端（静音功能不可用）"
                ));
                crate::log::line(
                    "HINT  运行 `MicTrayRs.exe --diag` 可输出设备清单与环境自检，\
                     判断卡在 CLSID 注册 / MMDevApi.dll / Audiosrv 的哪一层",
                );
                std::sync::Arc::new(stub::StubMic::new(format!(
                    "WASAPI 后端初始化失败，已降级为占位后端：{e}"
                ))) as MicController
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = selected_device_id;
        std::sync::Arc::new(stub::StubMic::new(
            "非 Windows 平台：音频控制后端未实现（仅 Windows 提供 WASAPI 实现）",
        )) as MicController
    }
}
