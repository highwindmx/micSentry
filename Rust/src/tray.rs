//! 系统托盘图标 + 右键菜单。
//!
//! muda 的 `MenuItem` / `CheckMenuItem` 内部持有 `Rc<RefCell<..>>`（因此都不是 `Send`），
//! 菜单动作不能直接挂闭包，而是通过**全局 `MenuEvent` + `MenuId`** 分发回 App。
//! 同理，勾选态不在事件处理器里就地改（跨线程处理器拿不到这些 item），
//! 而是**整体重建菜单** —— 这样勾选态永远与真实状态一致。

use tray_icon::menu::{
    CheckMenuItem, ContextMenu, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu,
};
use tray_icon::{TrayIcon, TrayIconBuilder};

use crate::audio::{MicController, Snapshot};
use crate::log;

/// 设备菜单项的 id 前缀：`dev:<设备id>`；`dev:`（空 id）表示"跟随系统默认"。
pub const DEV_PREFIX: &str = "dev:";

const ID_INFO: &str = "info_device";
const ID_TOGGLE: &str = "toggle_mute";
const ID_OVERLAY: &str = "toggle_overlay";
const ID_AUTOSTART: &str = "toggle_autostart";
const ID_RESCAN: &str = "dev_rescan";
const ID_QUIT: &str = "quit";

/// 托盘菜单动作回调（均为 `Send + Sync`，以适配全局 `MenuEvent` 处理器的线程约束）。
pub struct TrayCallbacks {
    pub toggle_mute: Box<dyn Fn() + Send + Sync>,
    pub toggle_overlay: Box<dyn Fn() + Send + Sync>,
    pub toggle_autostart: Box<dyn Fn() + Send + Sync>,
    /// 参数为设备 id（空串 = 跟随系统默认）
    pub select_device: Box<dyn Fn(&str) + Send + Sync>,
    pub rescan_devices: Box<dyn Fn() + Send + Sync>,
    pub quit: Box<dyn Fn() + Send + Sync>,
}

/// 按字符（而非字节）截断，避免把多字节 UTF-8 切坏。
fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// 托盘悬停提示（Windows 限制 128 字符，这里保持在 60 字符内）。
fn tooltip_text(snap: &Snapshot) -> String {
    format!(
        "MicTrayRs — {}｜{}",
        snap.state_text(),
        truncate(&snap.device_name, 40)
    )
}

/// 构建完整菜单。**每次都在最新状态上重建**，因此勾选项与设备列表永不过期。
fn build_menu(audio: &MicController) -> Option<Menu> {
    let snap = audio.snapshot();
    let menu = Menu::new();

    // 第一行是纯信息项（enabled=false）：明确告诉用户"现在控制的是哪只麦"
    let info = MenuItem::with_id(
        ID_INFO,
        format!(
            "当前麦克风：{}（{}）",
            truncate(&snap.device_name, 24),
            snap.state_text()
        ),
        false,
        None,
    );
    let toggle = MenuItem::with_id(ID_TOGGLE, "切换全局静音 (Ctrl+Alt+M)", true, None);
    let overlay = MenuItem::with_id(ID_OVERLAY, "显示/隐藏悬浮窗 (Ctrl+Alt+O)", true, None);

    // ---- 设备选择子菜单 ----
    // 注意参数顺序：with_id(id, text, enabled, checked, accelerator)
    let dev_menu = Submenu::new("选择麦克风", true);
    let follow = CheckMenuItem::with_id(
        DEV_PREFIX,
        "跟随系统默认",
        true,
        snap.follows_default,
        None,
    );
    let mut dev_items: Vec<CheckMenuItem> = Vec::new();
    for d in audio.devices() {
        dev_items.push(CheckMenuItem::with_id(
            format!("{DEV_PREFIX}{}", d.id),
            truncate(&d.label(), 48),
            true,
            !snap.follows_default && snap.selected_id == d.id,
            None,
        ));
    }
    let dev_sep = PredefinedMenuItem::separator();
    let rescan = MenuItem::with_id(ID_RESCAN, "重新扫描设备", true, None);

    let mut dev_refs: Vec<&dyn IsMenuItem> = vec![&follow];
    for it in &dev_items {
        dev_refs.push(it);
    }
    dev_refs.push(&dev_sep);
    dev_refs.push(&rescan);
    if let Err(e) = dev_menu.append_items(&dev_refs) {
        log::line(&format!("ERROR 设备子菜单 append_items 失败: {e}"));
    }

    let autostart = CheckMenuItem::with_id(
        ID_AUTOSTART,
        "开机自启",
        true,
        crate::config::is_autostart(),
        None,
    );
    let quit = MenuItem::with_id(ID_QUIT, "退出", true, None);
    let sep = PredefinedMenuItem::separator();

    if let Err(e) = menu.append_items(&[
        &info, &toggle, &overlay, &dev_menu, &sep, &autostart, &quit,
    ]) {
        log::line(&format!("ERROR menu.append_items 失败: {e}"));
        return None;
    }
    Some(menu)
}

/// 构建托盘图标与菜单，并安装全局 `MenuEvent` 处理器。
///
/// `state` / `px`：初始图标状态与**原生像素尺寸**（由 DPI 决定，见 `mic::tray_px`）。
/// 失败时返回 `None` 并写日志（不 panic —— GUI 程序 panic 等于无声退出）。
pub fn build_tray(
    audio: &MicController,
    cb: TrayCallbacks,
    state: crate::mic::State,
    px: u32,
) -> Option<TrayIcon> {
    let menu = build_menu(audio)?;
    let snap = audio.snapshot();

    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        // MenuId(pub String)，直接取内部字符串匹配
        let id = event.id().0.clone();
        if let Some(dev_id) = id.strip_prefix(DEV_PREFIX) {
            (cb.select_device)(dev_id);
            return;
        }
        match id.as_str() {
            ID_TOGGLE => (cb.toggle_mute)(),
            ID_OVERLAY => (cb.toggle_overlay)(),
            ID_AUTOSTART => (cb.toggle_autostart)(),
            ID_RESCAN => (cb.rescan_devices)(),
            ID_QUIT => (cb.quit)(),
            ID_INFO => {} // 信息项不可点，muda 一般不会派发
            other => log::line(&format!("WARN  未知菜单 id: {other}")),
        }
    }));

    let icon = match crate::mic::icon(state, px) {
        Some(i) => i,
        None => {
            log::line("ERROR 托盘图标生成失败，跳过托盘");
            return None;
        }
    };

    match TrayIconBuilder::new()
        .with_menu(Box::new(menu) as Box<dyn ContextMenu>)
        // 关闭"左键弹菜单"（默认开），让左键腾出给"切换静音"；菜单走右键。
        .with_menu_on_left_click(false)
        .with_tooltip(tooltip_text(&snap))
        .with_icon(icon)
        .build()
    {
        Ok(t) => Some(t),
        Err(e) => {
            log::line(&format!("ERROR TrayIconBuilder::build 失败: {e}"));
            None
        }
    }
}

/// 状态变化时刷新图标与悬停提示。
///
/// `state` 为**三态**（绿=开麦 / 红=已静音 / 灰=状态未知），
/// 未知态必须与"未静音"区别开，否则会出现"找不到麦克风却显示一切正常"的误导。
/// **刻意不重建菜单**：重建会打断用户正在操作的菜单。菜单只在设备选择 /
/// 重新扫描时重建（见 `rebuild_menu`）。
pub fn refresh_status(tray: &TrayIcon, snap: &Snapshot, state: crate::mic::State, px: u32) {
    if let Some(icon) = crate::mic::icon(state, px) {
        if let Err(e) = tray.set_icon(Some(icon)) {
            log::line(&format!("ERROR tray.set_icon 失败: {e}"));
        }
    }
    if let Err(e) = tray.set_tooltip(Some(tooltip_text(snap))) {
        log::line(&format!("ERROR tray.set_tooltip 失败: {e}"));
    }
}

/// 重建菜单（设备选择变化 / 重新扫描设备后调用）。
pub fn rebuild_menu(tray: &TrayIcon, audio: &MicController) {
    match build_menu(audio) {
        Some(m) => tray.set_menu(Some(Box::new(m) as Box<dyn ContextMenu>)),
        None => log::line("ERROR 菜单重建失败，保留旧菜单"),
    }
}
