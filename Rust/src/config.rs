//! 配置持久化。
//!
//! - **显式选择的录音设备 id** → `%APPDATA%\MicTrayRs\config.ini`
//!   （`key=value` 纯文本，零依赖、可手工编辑，符合"配置不要硬编码在代码里"的偏好）
//! - **开机自启** → 写 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`

use std::path::PathBuf;

/// 运行期配置。
#[derive(Clone, Debug, Default)]
pub struct Config {
    /// 显式选择的录音设备 id；空串 = 跟随系统默认麦克风
    pub device_id: String,
}

fn config_path() -> PathBuf {
    let dir = dirs::config_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("MicTrayRs");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("config.ini")
}

/// 静默读取（不写日志，供内部复用）。
fn read_quiet() -> Config {
    let mut cfg = Config::default();
    let Ok(text) = std::fs::read_to_string(config_path()) else {
        return cfg;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        if k.trim() == "device_id" {
            cfg.device_id = v.trim().to_string();
        }
    }
    cfg
}

/// 读取配置并写启动日志。
pub fn load() -> Config {
    let path = config_path();
    let cfg = read_quiet();
    if path.exists() {
        crate::log::line(&format!(
            "CONF  已加载 {}（device_id={:?}）",
            path.display(),
            cfg.device_id
        ));
    } else {
        crate::log::line(&format!(
            "CONF  未找到配置 {}，使用默认值（跟随系统默认麦克风）",
            path.display()
        ));
    }
    cfg
}

/// 写入配置。
pub fn save(cfg: &Config) {
    let path = config_path();
    let text = format!(
        "# MicTrayRs 配置。可手工编辑，改完重启程序生效。\r\n# device_id：显式选定的录音设备 id；留空 = 跟随系统默认麦克风。\r\n# 设备 id 可在「托盘右键菜单 -> 选择麦克风」或 `MicTrayRs.exe --diag` 的输出里查看。\r\ndevice_id={}\r\n",
        cfg.device_id
    );
    match std::fs::write(&path, text) {
        Ok(()) => crate::log::line(&format!("CONF  已保存 {}", path.display())),
        Err(e) => crate::log::line(&format!("ERROR 保存配置失败 {}: {e}", path.display())),
    }
}

/// 只改 device_id 并落盘。
pub fn set_device_id(id: &str) {
    let mut cfg = read_quiet();
    if cfg.device_id == id {
        return;
    }
    cfg.device_id = id.to_string();
    save(&cfg);
}

#[cfg(target_os = "windows")]
mod autostart {
    use winreg::enums::*;
    use winreg::RegKey;

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const APP_NAME: &str = "MicTrayRs";

    pub fn is_autostart() -> bool {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        hkcu.open_subkey(RUN_KEY)
            .and_then(|k| k.get_value::<String, _>(APP_NAME))
            .is_ok()
    }

    pub fn set_autostart(enable: bool) -> Result<(), String> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = hkcu
            .open_subkey_with_flags(RUN_KEY, KEY_WRITE)
            .or_else(|_| hkcu.create_subkey(RUN_KEY).map(|(k, _)| k))
            .map_err(|e| format!("打开 Run 键失败: {e}"))?;
        if enable {
            let exe = std::env::current_exe()
                .map_err(|e| format!("取 exe 路径失败: {e}"))?
                .to_string_lossy()
                .to_string();
            key.set_value(APP_NAME, &exe)
                .map_err(|e| format!("写 Run 值失败: {e}"))
        } else {
            key.delete_value(APP_NAME)
                .or_else(|e| {
                    // 值本来就不存在也算成功
                    if e.kind() == std::io::ErrorKind::NotFound {
                        Ok(())
                    } else {
                        Err(e)
                    }
                })
                .map_err(|e| format!("删 Run 值失败: {e}"))
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod autostart {
    pub fn is_autostart() -> bool {
        false
    }
    pub fn set_autostart(_enable: bool) -> Result<(), String> {
        Err("当前平台未实现开机自启".to_string())
    }
}

pub use autostart::{is_autostart, set_autostart};
