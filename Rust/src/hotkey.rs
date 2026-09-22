//! 全局热键：Ctrl+Alt+M 切换静音，Ctrl+Alt+O 切换悬浮窗。

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager};

use crate::log;

pub struct HotkeyCallbacks {
    pub toggle_mute: Box<dyn Fn() + Send + Sync>,
    pub toggle_overlay: Box<dyn Fn() + Send + Sync>,
}

/// 注册全局热键。
///
/// 返回的 `GlobalHotKeyManager` 必须在 `main` 作用域内保活（drop 即注销热键）。
/// 注册失败（例如组合键已被其它程序占用）只记日志、不 panic ——
/// GUI 子系统下 panic 会导致进程无声退出。
pub fn register(cb: HotkeyCallbacks) -> Option<GlobalHotKeyManager> {
    let manager = match GlobalHotKeyManager::new() {
        Ok(m) => m,
        Err(e) => {
            log::line(&format!("ERROR GlobalHotKeyManager::new failed: {e}"));
            return None;
        }
    };

    let hm = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyM);
    let ho = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyO);
    // 先取出 id（u32），再 register —— register 会消费 HotKey。
    let hm_id = hm.id();
    let ho_id = ho.id();

    let mut ok_count = 0;
    match manager.register(hm) {
        Ok(()) => ok_count += 1,
        Err(e) => log::line(&format!(
            "ERROR register Ctrl+Alt+M failed (组合键可能被占用): {e}"
        )),
    }
    match manager.register(ho) {
        Ok(()) => ok_count += 1,
        Err(e) => log::line(&format!("ERROR register Ctrl+Alt+O failed: {e}")),
    }
    log::line(&format!("OK    hotkeys registered: {ok_count}/2"));

    GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
        if event.id() == hm_id {
            (cb.toggle_mute)();
        } else if event.id() == ho_id {
            (cb.toggle_overlay)();
        }
    }));
    Some(manager)
}
