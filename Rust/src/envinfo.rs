//! 运行环境自检：判定「读不到麦克风」到底卡在哪一层。
//!
//! # 一段被推翻的结论（务必保留这段，避免重犯）
//!
//! 2026-09-22 曾有一次误判：程序报 `CoCreateInstance(MMDeviceEnumerator)` →
//! `0x80040154 REGDB_E_CLASSNOTREG（没有注册类）`，当时"证据"有三条：
//!   1. `MMDevApi.dll` 明明存在于 System32；
//!   2. `HKCR\CLSID\{a95664d2-…}\InprocServer32` 读不到；
//!   3. Python `ctypes` 直接调 `ole32.CoCreateInstance` 也返回 `0x80040154`。
//! 于是结论写成「沙箱隔离了注册表，WASAPI 必然不可用，与本机无关」。
//!
//! **但这个结论是错的**：三条"独立证据"其实共用同一个错误前提 ——
//! `{A95664D2-9614-4F35-A746-DE8DB63617E6}` 是 **IMMDeviceEnumerator 的 IID**，
//! 不是 `MMDeviceEnumerator` 类的 **CLSID**（真正的 CLSID 是
//! `{BCDE0395-E52F-467C-8E3D-C4579291692E}`）。IID 只注册在 `HKCR\Interface\` 下，
//! 所以 `HKCR\CLSID\{a95664d2-…}` 在**任何**机器上都读不到，
//! `CoCreateInstance` 也必然失败。三条证据只是在互相印证同一个错误。
//!
//! **教训**：自检探针必须与被检代码**不共享假设**，否则它只会把 bug 复述一遍。
//! 现在的探针查的是真正的 CLSID（`HKCR\CLSID\{BCDE0395-…}`），
//! 与 `windows_impl.rs` 里引用的常量同源但独立取值。
//!
//! 结论只在 `--diag` 里输出，不影响正常运行。

/// 真正的 `MMDeviceEnumerator` CLSID，与 `windows_impl.rs` 引用的 crate 常量一致。
const MMDE_CLSID: &str = "{BCDE0395-E52F-467C-8E3D-C4579291692E}";

/// 环境自检结果。
pub struct Report {
    /// 是否检测到沙箱/宿主注入的标记变量。
    ///
    /// **仅作信息提示**：命中它只说明"进程由宿主（如 WorkBuddy）拉起"，
    /// 并**不能**据此推断 WASAPI 不可用 —— 这正是上面那次误判的根源。
    pub in_sandbox: bool,
    /// 命中的标记变量名（最多列出几个）
    pub markers: Vec<String>,
    /// `MMDevApi.dll` 是否存在
    pub mmdevapi_dll: Option<bool>,
    /// `HKCR\CLSID\{BCDE0395-…}`（真正的 CLSID）能否打开；`None` = 平台不支持该探测
    pub clsid_found: Option<bool>,
    /// CLSID 探测细节（类名 / InprocServer32 / 失败原因）
    pub clsid_detail: String,
}

/// 采集环境信息。
pub fn probe() -> Report {
    let markers = collect_markers();
    let (clsid_found, clsid_detail) = clsid_probe();
    Report {
        in_sandbox: !markers.is_empty(),
        markers,
        mmdevapi_dll: mmdevapi_dll_exists(),
        clsid_found,
        clsid_detail,
    }
}

/// 找沙箱/宿主标记。命中只表示"进程由宿主拉起"，不代表 COM 不可用。
fn collect_markers() -> Vec<String> {
    let needles = ["LSBOX", "CODEBUDDY_SAFE_DELETE", "GENIE_TRASH", "WORKBUDDY_STARTUP_PID"];
    let mut hits: Vec<String> = Vec::new();
    for (k, _) in std::env::vars() {
        let up = k.to_ascii_uppercase();
        if needles.iter().any(|n| up.contains(n)) {
            hits.push(k);
        }
    }
    hits.sort();
    hits.truncate(8);
    hits
}

fn mmdevapi_dll_exists() -> Option<bool> {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
    let p = std::path::Path::new(&root)
        .join("System32")
        .join("MMDevApi.dll");
    Some(p.exists())
}

/// 查 `HKCR\CLSID\{BCDE0395-…}`：键是否存在、类名是什么、InprocServer32 指向哪里。
#[cfg(target_os = "windows")]
fn clsid_probe() -> (Option<bool>, String) {
    use winreg::enums::HKEY_CLASSES_ROOT;
    use winreg::RegKey;

    let path = format!(r"CLSID\{MMDE_CLSID}");
    let key = match RegKey::predef(HKEY_CLASSES_ROOT).open_subkey(&path) {
        Ok(k) => k,
        Err(e) => return (Some(false), format!("打不开 {path}: {e}")),
    };

    let class_name = key
        .get_value::<String, _>("")
        .unwrap_or_else(|_| "(无默认值)".to_string());

    // InprocServer32 的默认值就是承载该 COM 类的 DLL 路径
    let dll = key
        .open_subkey("InprocServer32")
        .ok()
        .and_then(|k| k.get_value::<String, _>("").ok())
        .unwrap_or_else(|| "(读不到 InprocServer32)".to_string());

    (Some(true), format!("{class_name} → {dll}"))
}

#[cfg(not(target_os = "windows"))]
fn clsid_probe() -> (Option<bool>, String) {
    (None, "非 Windows 平台，跳过".to_string())
}

/// 把自检结果格式化成若干行（写进日志 / `--diag` 报告）。
///
/// `com_ok`：音频后端是否真的拿到了设备（用于给出结论）。
pub fn format(r: &Report, com_ok: bool) -> Vec<String> {
    let mut out = Vec::new();
    out.push("环境自检:".to_string());
    out.push(match r.mmdevapi_dll {
        Some(true) => "    MMDevApi.dll      : 存在（系统音频组件完整）".to_string(),
        Some(false) => "    MMDevApi.dll      : **缺失**（系统音频组件异常）".to_string(),
        None => "    MMDevApi.dll      : 未探测".to_string(),
    });
    out.push(match r.clsid_found {
        Some(true) => format!("    CLSID 已注册      : 是（{MMDE_CLSID}）"),
        Some(false) => format!("    CLSID 已注册      : **否**（{MMDE_CLSID}）"),
        None => "    CLSID 已注册      : 未探测".to_string(),
    });
    if !r.clsid_detail.is_empty() {
        out.push(format!("    注册详情          : {}", r.clsid_detail));
    }
    if r.in_sandbox {
        out.push(format!(
            "    宿主标记          : 命中 {} 个 —— {}（仅供参考，不代表 COM 不可用）",
            r.markers.len(),
            r.markers.join(", ")
        ));
    } else {
        out.push("    宿主标记          : 未命中".to_string());
    }

    out.push(format!("    结论              : {}", conclusion(r, com_ok)));
    out
}

fn conclusion(r: &Report, com_ok: bool) -> String {
    if com_ok {
        return "本机 WASAPI 链路正常，应以「活动录音设备」清单为准。".to_string();
    }
    if r.mmdevapi_dll == Some(false) {
        return "系统缺少 MMDevApi.dll，音频组件已损坏（可用 `sfc /scannow` 修复）。".to_string();
    }
    if r.clsid_found == Some(false) {
        return format!(
            "`HKCR\\CLSID\\{MMDE_CLSID}` 未注册 → MMDevApi 的 COM 注册信息缺失。\
             先跑 `sfc /scannow`；仍不行则检查 Windows Audio 服务（Audiosrv）与 MMDevApi 组件是否被安全软件拦截。"
        );
    }
    if r.clsid_found == Some(true) {
        return "CLSID 注册正常，但 COM 创建仍失败 → 通常是 Windows Audio 服务（Audiosrv）异常或被杀软拦截，\
                建议重启 Audiosrv（管理员 PowerShell：`Restart-Service Audiosrv`）后重试。"
            .to_string();
    }
    "无法完成环境判断（非 Windows 平台或探测失败）。".to_string()
}
