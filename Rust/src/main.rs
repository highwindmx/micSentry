//! MicTrayRs —— 麦克风静音托盘 + 悬浮窗（Slint + tray-icon + global-hotkey + WASAPI）。
//!
//! # 启动期"无声死亡"的防护
//! 本程序是 `windows_subsystem = "windows"` 的 GUI 程序，没有控制台，
//! 启动期任何失败都是"双击了但什么反应都没有"。因此：
//!   1. 全流程写日志（`src/log.rs`），panic 也进日志；
//!   2. 初始化链上**不用 `unwrap()`/`?` 自杀**，失败就降级 + 记日志；
//!   3. Slint 必须**显式 `show()`** —— `run_event_loop()` 本身不会显示窗口。
//!
//! # 为什么要有轮询线程
//! 麦克风静音态会被**外部**改变（键盘 Fn 静音键、Windows 音量面板、其它软件）。
//! 如果 UI 只在自己的操作后更新，图标就会变成"记忆态"：外部改了它不知道，
//! 用户再点一下时程序却按**真实态**取反 —— 表现为"图标显示已静音，
//! 点一下还是静音"，即「静音之后怎么都回不去」。轮询线程专职消除这种分叉。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod config;
mod envinfo;
mod hotkey;
mod log;
mod mic;
mod png;
mod tray;

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use slint::{PhysicalPosition, WindowPosition};

use audio::{MicController, Snapshot};

slint::include_modules!();

struct App {
    audio: MicController,
    tray: Option<tray_icon::TrayIcon>,
    ui: MainWindow,
    /// 托盘图标的**原生像素尺寸**（16 逻辑像素 × DPI 缩放）
    tray_px: u32,
    /// 悬浮窗图标的像素尺寸（96 逻辑像素 × DPI 缩放）
    overlay_px: u32,
    overlay_visible: bool,
    drag_prev: Option<(f32, f32)>,
    dragged: bool,
    /// 托盘菜单需要重建（设备选择 / 设备变化后置位，由 `flush_menu` 落地）
    menu_dirty: bool,
    /// 最近一次已知快照
    snap: Snapshot,
}

impl App {
    /// 把一份新快照落到悬浮窗与托盘上（自己的操作、外部变化都走这里）。
    ///
    /// 图标状态由 `snap.muted` 三态推出：`Some(true)` 红 / `Some(false)` 绿 /
    /// `None`（读取失败、没找到设备）**灰** —— 灰态是"如实承认不知道"，
    /// 不能退回成绿色"未静音"，那会让用户以为一切正常。
    fn apply_snapshot(&mut self, snap: Snapshot) {
        let device_changed = self.snap.device_id != snap.device_id
            || self.snap.device_name != snap.device_name
            || self.snap.selected_id != snap.selected_id;

        let state = mic::State::from_muted(snap.muted);
        self.ui.set_muted(matches!(state, mic::State::Off));
        self.ui.set_mic_image(mic::image(state, self.overlay_px));
        if let Some(t) = &self.tray {
            tray::refresh_status(t, &snap, state, self.tray_px);
        }
        if device_changed {
            self.menu_dirty = true;
        }
        // 同一条错误只记一次：轮询线程每 600ms 取一次快照，不去重会把日志刷成噪声，
        // 反而掩盖真正的关键行。
        if let Some(e) = &snap.error {
            if self.snap.error.as_deref() != Some(e.as_str()) {
                log::line(&format!("WARN  音频: {e}"));
            }
        }
        self.snap = snap;
    }

    /// 若菜单已标脏则重建。调用点必须在**主线程**、且不在菜单事件派发过程中。
    fn flush_menu(&mut self) {
        if !self.menu_dirty {
            return;
        }
        self.menu_dirty = false;
        if let Some(t) = &self.tray {
            tray::rebuild_menu(t, &self.audio);
        }
    }

    fn toggle_mute(&mut self) {
        let snap = self.audio.toggle_muted();
        log::line(&format!(
            "ACT   切换静音 -> {}（{}）",
            snap.state_text(),
            snap.device_name
        ));
        self.apply_snapshot(snap);
    }

    fn toggle_overlay(&mut self) {
        self.overlay_visible = !self.overlay_visible;
        if self.overlay_visible {
            match self.ui.show() {
                Ok(()) => log::line("ACT   悬浮窗已显示"),
                Err(e) => log::line(&format!("ERROR 悬浮窗显示失败: {e}")),
            }
        } else {
            let _ = self.ui.hide();
            log::line("ACT   悬浮窗已隐藏");
        }
    }

    /// 选择受控麦克风（`id` 为空串 = 跟随系统默认）。
    fn select_device(&mut self, id: &str) {
        let snap = self.audio.select_device(id);
        config::set_device_id(id);
        log::line(&format!(
            "CONF  受控麦克风 -> {}（{}）",
            snap.device_name,
            if snap.follows_default {
                "跟随系统默认"
            } else {
                "已锁定到该设备"
            }
        ));
        self.apply_snapshot(snap);
    }

    fn quit(&self) {
        log::line("ACT   收到退出请求");
        if let Err(e) = slint::quit_event_loop() {
            log::line(&format!("ERROR quit_event_loop: {e}"));
        }
    }
}

/// 共享应用状态句柄。
///
/// 为什么要包一层：`tray_icon::TrayIcon` 内部含 `Rc<RefCell<..>>`（因此 `App` 既非 `Send`
/// 也非 `Sync`），而 `MenuEvent` / `TrayIconEvent` / `GlobalHotKeyEvent` 的事件处理器都要求
/// `Fn(..) + Send + Sync + 'static`。在 Windows 上这三类事件与 winit/slint 事件循环运行在
/// **同一主线程**，且轮询线程只往事件循环里投递闭包、从不亲自访问 `App`，
/// 故此处显式声明 `Send + Sync` 是安全的。
struct SharedApp(Arc<Mutex<App>>);

// SAFETY: 见上；所有对 `App` 的访问都发生在主线程（事件循环线程）上，由 Mutex 串行化。
unsafe impl Send for SharedApp {}
unsafe impl Sync for SharedApp {}

impl SharedApp {
    /// 取锁；若因 panic 中毒则恢复内部数据（宁可继续跑也不要二次 panic）。
    fn lock(&self) -> MutexGuard<'_, App> {
        match self.0.lock() {
            Ok(g) => g,
            Err(poisoned) => {
                log::line("WARN  mutex 中毒，恢复内部状态后继续");
                poisoned.into_inner()
            }
        }
    }
}

impl Clone for SharedApp {
    fn clone(&self) -> Self {
        SharedApp(self.0.clone())
    }
}

/// 把"重建托盘菜单"推迟到当前 Win32 消息派发结束之后执行。
///
/// 直接在菜单事件处理器里 `set_menu()` 有替换"正在派发的菜单"的风险，故统一走事件循环队列。
fn defer_menu_rebuild(app: SharedApp) {
    if slint::invoke_from_event_loop(move || {
        app.lock().flush_menu();
    })
    .is_err()
    {
        log::line("WARN  菜单重建被跳过（事件循环未运行）");
    }
}

/// 后台轮询线程：外部（Fn 静音键 / Windows 音量面板 / 其它软件）改变静音态后同步到 UI。
///
/// 关键点：**不直接锁 `App`**（那会跨线程触碰 `TrayIcon` 内部的 `Rc`），
/// 而是把更新投回主线程执行。这样既拿到了实时状态，又守住了
/// "`App` 只在主线程被访问"这条前提。线程挂掉也不会卡住 UI。
fn spawn_poller(audio: MicController, app: SharedApp) {
    const POLL_MS: u64 = 600;
    let result = std::thread::Builder::new()
        .name("mic-poll".to_string())
        .spawn(move || {
            let mut last: Option<Snapshot> = None;
            loop {
                std::thread::sleep(Duration::from_millis(POLL_MS));
                let snap = audio.snapshot();
                let changed = match &last {
                    None => true,
                    Some(p) => {
                        p.muted != snap.muted
                            || p.device_id != snap.device_id
                            || p.selected_id != snap.selected_id
                    }
                };
                if !changed {
                    continue;
                }
                if let Some(p) = &last {
                    if p.muted != snap.muted {
                        log::line(&format!(
                            "POLL  外部改变静音态 -> {}（{}）",
                            snap.state_text(),
                            snap.device_name
                        ));
                    }
                }
                last = Some(snap.clone());
                let a = app.clone();
                // 事件循环未启动（--selftest）或已退出时返回 Err，忽略即可
                let _ = slint::invoke_from_event_loop(move || {
                    let mut g = a.lock();
                    g.apply_snapshot(snap);
                    g.flush_menu();
                });
            }
        });
    match result {
        Ok(_) => log::line(&format!(
            "OK    状态轮询线程已启动（每 {POLL_MS}ms 对齐真实端点状态）"
        )),
        Err(e) => log::line(&format!("WARN  状态轮询线程启动失败: {e}")),
    }
}

fn err_suffix(s: &Snapshot) -> String {
    match &s.error {
        Some(e) => format!("   [{e}]"),
        None => String::new(),
    }
}

/// `--diag`：把设备清单与"写 → 回读"往返自检写进日志，并额外落一份纯文本报告。
///
/// 用它就能确定两件事：程序到底在控制哪只麦、这条控制链路在本机是否真能通。
fn run_diag(audio: &MicController) {
    let mut out = String::new();
    out.push_str("MicTrayRs 音频诊断\r\n========================================\r\n");

    let snap = audio.snapshot();
    out.push_str(&format!("受控设备 : {}\r\n", snap.device_name));
    out.push_str(&format!(
        "设备 id  : {}\r\n",
        if snap.device_id.is_empty() {
            "(未知)"
        } else {
            &snap.device_id
        }
    ));
    out.push_str(&format!(
        "选择模式 : {}\r\n",
        if snap.follows_default {
            "跟随系统默认"
        } else {
            "显式选择"
        }
    ));
    out.push_str(&format!("当前状态 : {}\r\n", snap.state_text()));
    if let Some(e) = &snap.error {
        out.push_str(&format!("读取错误 : {e}\r\n"));
    }

    // 环境自检：定位"读不到麦克风"卡在哪一层（CLSID 注册 / MMDevApi.dll / Audiosrv）。
    // 注意：这里的探针查的是**真正的 CLSID** —— 早期版本查的是接口 IID，
    // 与被检代码共享同一个错误假设，只会把 bug 复述一遍，见 envinfo.rs 顶部注释。
    let com_ok = snap.muted.is_some() && !snap.device_id.is_empty();
    out.push_str("\r\n");
    for l in envinfo::format(&envinfo::probe(), com_ok) {
        out.push_str(&format!("{l}\r\n"));
    }

    out.push_str("\r\n活动录音设备:\r\n");
    let devs = audio.devices();
    if devs.is_empty() {
        out.push_str("  （枚举不到任何录音设备。\r\n");
        out.push_str("    按上方环境自检的结论行处理：CLSID 未注册 → `sfc /scannow`；\r\n");
        out.push_str("    CLSID 正常 → 重启 Audiosrv；也请先在系统「声音设置 → 输入」里确认确有可用设备）\r\n");
    }
    for (i, d) in devs.iter().enumerate() {
        out.push_str(&format!("  [{i}] {}\r\n", d.label()));
        out.push_str(&format!("       id={}\r\n", d.id));
    }

    out.push_str("\r\n往返测试 : ");
    match snap.muted {
        None => out.push_str("跳过（当前状态读取失败，不做冒险写入）\r\n"),
        Some(orig) => {
            out.push_str(&format!(
                "起始状态 = {}\r\n",
                if orig { "已静音" } else { "未静音" }
            ));
            let a = audio.set_muted(!orig);
            out.push_str(&format!(
                "  SetMute({}) -> 回读 = {}{}\r\n",
                !orig,
                a.state_text(),
                err_suffix(&a)
            ));
            let b = audio.set_muted(orig);
            out.push_str(&format!(
                "  SetMute({}) -> 回读 = {}{}   （已恢复原状态）\r\n",
                orig,
                b.state_text(),
                err_suffix(&b)
            ));
            if a.muted == Some(!orig) && b.muted == Some(orig) {
                out.push_str("结论     : 端点静音「读-写-回读」链路正常。\r\n");
                out.push_str("           若此时 Windows 侧仍看不到变化，说明控制的不是你在看的那只麦，\r\n");
                out.push_str("           或该机型把静音开关放在硬件/驱动层（端点静音 API 管不到）。\r\n");
            } else {
                out.push_str("结论     : 端点静音读写异常，详见上方错误行（含 HRESULT）。\r\n");
            }
        }
    }
    out.push_str("========================================\r\n");

    log::dump("diag", &out);
}

/// exe 所在目录（拿不到就退到临时目录）。
fn exe_dir() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(std::env::temp_dir)
}

/// `--export-icons [输出目录]`：把三种状态 × 多个尺寸的图标渲染成 PNG。
///
/// 为什么要有这个模式：图标的最终裁判是眼睛。GUI 在无桌面会话里没法验证，
/// 但"形状对不对、16px 下边缘糊不糊、深/浅色任务栏上够不够看清"完全可以在
/// PNG 上肉眼验收 —— 这是排查"图标难看"最快的手段，也便于改版前后对比。
fn export_icons(dir: &std::path::Path) {
    const SIZES: [u32; 6] = [16, 20, 24, 32, 48, 96];
    let states = [mic::State::On, mic::State::Off, mic::State::Unknown];
    let bgs: [(u8, u8, u8); 2] = [(0x1F, 0x1F, 0x1F), (0xF3, 0xF3, 0xF3)];
    let pad: u32 = 8;

    // ---- 图 1：三态 × 六尺寸，1:1，深色任务栏 / 浅色任务栏各一组 ----
    let row_h = 96 + pad;
    let sheet_w = pad + SIZES.iter().map(|s| s + pad).sum::<u32>();
    let mut sheet = png::Canvas::new(sheet_w, row_h * 6, bgs[0]);
    let mut row = 0u32;
    for bg in bgs {
        for st in states {
            let y0 = row * row_h;
            sheet.fill_rect(0, y0 as i32, sheet_w, row_h, bg);
            let mut x = pad as i32;
            for s in SIZES {
                let rgba = mic::render(st, s);
                sheet.blit(&rgba, s, s, x, y0 as i32 + ((96 - s) / 2) as i32);
                x += (s + pad) as i32;
            }
            row += 1;
        }
    }

    // ---- 图 2：小尺寸放大 6 倍（最近邻），中灰底，专门看抗锯齿质量 ----
    let zoom: u32 = 6;
    let zsizes: [u32; 4] = [16, 20, 24, 32];
    let cell_h = 32 * zoom;
    let zrow_h = cell_h + pad;
    let zw = pad + zsizes.iter().map(|s| s * zoom + pad).sum::<u32>();
    let mut zoomed = png::Canvas::new(zw, pad + zrow_h * 3, (0x60, 0x60, 0x60));
    for (i, st) in states.iter().enumerate() {
        let y0 = pad + (i as u32) * zrow_h;
        let mut x = pad as i32;
        for s in zsizes {
            let rgba = mic::render(*st, s);
            zoomed.blit_zoom(&rgba, s, s, x, (y0 + cell_h - s * zoom) as i32, zoom);
            x += (s * zoom + pad) as i32;
        }
    }

    for (name, canvas) in [("icons-preview.png", sheet), ("icons-zoom.png", zoomed)] {
        let p = dir.join(name);
        match std::fs::write(&p, canvas.to_png()) {
            Ok(()) => log::line(&format!("ICON  已写出 {}", p.display())),
            Err(e) => log::line(&format!("ERROR 写 {} 失败: {e}", p.display())),
        }
    }
    log::line(&format!(
        "ICON  三态配色：绿 RGB{:?} / 红 RGB{:?} / 灰 RGB{:?}；尺寸 {:?}",
        mic::State::On.rgb(),
        mic::State::Off.rgb(),
        mic::State::Unknown.rgb(),
        SIZES
    ));
}

fn main() {
    log::init();
    log::install_panic_hook();

    let args: Vec<String> = std::env::args().collect();
    let has = |k: &str| args.iter().any(|a| a == k);
    let diag = has("--diag");
    let selftest_ui = has("--selftest-ui");
    let selftest = selftest_ui || has("--selftest");
    // 对照实验开关：跳过 show()，用来验证"未显示窗口时事件循环是否立即返回"
    let no_show = has("--no-show");
    // `--export-icons [目录]` / `--export-icons=<目录>`：把图标渲染成 PNG 供肉眼验收
    let export_dir = args
        .iter()
        .position(|a| a == "--export-icons" || a.starts_with("--export-icons="))
        .map(|i| {
            if let Some(v) = args[i].strip_prefix("--export-icons=") {
                std::path::PathBuf::from(v)
            } else {
                args.get(i + 1)
                    .filter(|v| !v.starts_with("--"))
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(exe_dir)
            }
        });

    // ---- 0) 图标导出模式：不启动 GUI，只把渲染结果落盘 ----
    if let Some(dir) = export_dir {
        export_icons(&dir);
        log::line("=== ICONS EXPORTED ===");
        return;
    }

    // ---- 1) 配置（含"上次选定的麦克风"）----
    let cfg = config::load();

    // ---- 2) 创建窗口（Slint 平台/渲染器初始化，最可能的失败点之一）----
    let ui = match MainWindow::new() {
        Ok(u) => {
            log::line("OK    MainWindow::new（slint 平台就绪）");
            u
        }
        Err(e) => {
            log::line(&format!("FATAL MainWindow::new 失败: {e}"));
            log::line("HINT  可尝试软件渲染：设环境变量 SLINT_BACKEND=winit-software");
            return;
        }
    };

    // ---- 2.5) 图标尺寸跟随 DPI ----
    // 托盘图标按**原生**像素渲染（100% → 16px，125% → 20px，150% → 24px）。
    // 旧版固定渲染 32px 再让系统缩到 16px，等于二次重采样，小尺寸下明显发虚。
    let sf = ui.window().scale_factor();
    let tray_px = mic::tray_px(sf);
    let overlay_px = mic::overlay_px(sf);
    log::line(&format!(
        "OK    DPI 缩放 {sf:.2} -> 托盘图标 {tray_px}px｜悬浮窗图标 {overlay_px}px"
    ));

    // ---- 3) 音频后端（WASAPI / COM）----
    let audio = audio::create(cfg.device_id.clone());
    let snap = audio.snapshot();
    log::line(&format!(
        "OK    音频后端就绪：受控设备 = {}｜{}｜{}",
        snap.device_name,
        snap.state_text(),
        if snap.follows_default {
            "跟随系统默认"
        } else {
            "显式选择"
        }
    ));
    if let Some(e) = &snap.error {
        log::line(&format!("WARN  音频: {e}"));
    }

    let app = SharedApp(Arc::new(Mutex::new(App {
        audio,
        tray: None,
        ui: ui.clone_strong(),
        tray_px,
        overlay_px,
        overlay_visible: true, // 启动即显示悬浮窗（Ctrl+Alt+O / 托盘菜单可隐藏）
        drag_prev: None,
        dragged: false,
        menu_dirty: false,
        snap,
    })));

    // ---- 4) 初始图标（把渲染问题提前暴露）----
    {
        let g = app.lock();
        let state = mic::State::from_muted(g.snap.muted);
        g.ui.set_muted(matches!(state, mic::State::Off));
        g.ui.set_mic_image(mic::image(state, g.overlay_px));
    }
    log::line(&format!(
        "OK    图标已渲染（悬浮窗 {overlay_px}px / 托盘 {tray_px}px，三态：绿=开麦·红=已静音·灰=未知）"
    ));

    // ---- 5) 悬浮窗交互回调 ----
    // 左键点击 = 切换静音（拖动后不误触，由 dragged 标记保护）
    let app_t = app.clone();
    ui.on_toggle(move || {
        let mut g = app_t.lock();
        if !g.dragged {
            g.toggle_mute();
        }
        g.dragged = false;
    });

    // 右键 = 隐藏悬浮窗
    let app_h = app.clone();
    ui.on_hide_overlay(move || {
        let mut g = app_h.lock();
        if g.overlay_visible {
            g.toggle_overlay();
        }
    });

    // 指针按下 = 重置拖拽基准
    let app_ds = app.clone();
    ui.on_drag_start(move || {
        let mut g = app_ds.lock();
        g.drag_prev = None;
        g.dragged = false;
    });

    // 拖拽 = 按位移差增量移动窗口（dx/dy 为逻辑像素，乘 scale_factor 转物理像素）。
    // 注：Slint 中 `length` 类型的回调参数在 Rust 侧映射为 f32（sp::Coord）。
    let app_dm = app.clone();
    ui.on_drag_move(move |dx: f32, dy: f32| {
        let mut g = app_dm.lock();
        if g.drag_prev.is_some() {
            let sf = g.ui.window().scale_factor();
            let p = g.ui.window().position();
            let np = PhysicalPosition {
                x: p.x + (dx * sf) as i32,
                y: p.y + (dy * sf) as i32,
            };
            g.ui.window().set_position(WindowPosition::Physical(np));
            g.dragged = true;
        }
        g.drag_prev = Some((dx, dy));
    });

    // ---- 6) 托盘图标 + 菜单 ----
    let audio_handle = app.lock().audio.clone();
    let initial_state = mic::State::from_muted(app.lock().snap.muted);
    let app_tray = app.clone();
    let tray = tray::build_tray(
        &audio_handle,
        tray::TrayCallbacks {
            toggle_mute: Box::new({
                let a = app_tray.clone();
                move || a.lock().toggle_mute()
            }),
            toggle_overlay: Box::new({
                let a = app_tray.clone();
                move || a.lock().toggle_overlay()
            }),
            toggle_autostart: Box::new(move || {
                let next = !config::is_autostart();
                match config::set_autostart(next) {
                    Ok(()) => log::line(&format!("ACT   开机自启 -> {next}")),
                    Err(e) => log::line(&format!("ERROR 开机自启设置失败: {e}")),
                }
            }),
            select_device: Box::new({
                let a = app_tray.clone();
                move |id: &str| {
                    {
                        let mut g = a.lock();
                        g.select_device(id);
                    }
                    defer_menu_rebuild(a.clone());
                }
            }),
            rescan_devices: Box::new({
                let a = app_tray.clone();
                move || {
                    let devs = a.lock().audio.devices();
                    log::line(&format!("ACT   重新扫描设备：发现 {} 个录音设备", devs.len()));
                    for l in audio::format_devices(&devs) {
                        log::line(&l);
                    }
                    defer_menu_rebuild(a.clone());
                }
            }),
            quit: Box::new({
                let a = app_tray.clone();
                move || a.lock().quit()
            }),
        },
        initial_state,
        tray_px,
    );
    match tray {
        Some(t) => {
            app.lock().tray = Some(t);
            log::line("OK    托盘图标已创建");
        }
        None => log::line("WARN  托盘图标创建失败（继续无托盘运行）"),
    }

    // ---- 7) 全局热键（manager 必须在 main 作用域内保活）----
    let app_hk = app.clone();
    let _hotkey_mgr = hotkey::register(hotkey::HotkeyCallbacks {
        toggle_mute: Box::new({
            let a = app_hk.clone();
            move || a.lock().toggle_mute()
        }),
        toggle_overlay: Box::new({
            let a = app_hk.clone();
            move || a.lock().toggle_overlay()
        }),
    });

    // ---- 8) 托盘左键 = 切换全局静音（build_tray 已关闭"左键弹菜单"）----
    let app_l = app.clone();
    tray_icon::TrayIconEvent::set_event_handler(Some(move |e| {
        if let tray_icon::TrayIconEvent::Click {
            button: tray_icon::MouseButton::Left,
            button_state: tray_icon::MouseButtonState::Up,
            ..
        } = e
        {
            app_l.lock().toggle_mute();
        }
    }));

    // ---- 9) 状态轮询（外部改静音后同步 UI）----
    spawn_poller(app.lock().audio.clone(), app.clone());

    // ---- 10) 诊断模式：跑完即退，输出在日志 + MicTrayRs-diag.txt ----
    if diag {
        let audio = app.lock().audio.clone();
        run_diag(&audio);
        log::line("=== DIAG DONE ===");
        return;
    }

    // ---- 11) 自检模式：跑完整条初始化链后退出，用于定位"运行了没反应" ----
    if selftest {
        log::line("SELFTEST: 初始化链已完成");
        if selftest_ui {
            if no_show {
                log::line("SELFTEST: [对照] 不调用 ui.show()");
            } else {
                log::line("SELFTEST: 显示悬浮窗并跑事件循环 2.5s");
                if let Err(e) = ui.show() {
                    log::line(&format!("ERROR ui.show() 失败: {e}"));
                }
            }
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_millis(2500));
                log::line("WATCHDOG: 2.5s 仍存活 -> 事件循环确实在阻塞");
                let _ = slint::quit_event_loop();
            });
            match slint::run_event_loop() {
                Ok(()) => log::line("SELFTEST: 事件循环正常返回 Ok"),
                Err(e) => log::line(&format!("SELFTEST: 事件循环返回 Err: {e}")),
            }
        }
        log::line("=== SELFTEST OK ===");
        return;
    }

    // ---- 12) 正式运行 ----
    // 关键：Slint 的 `slint::run_event_loop()` **不会**显示窗口，
    // 只有 `ComponentHandle::run()` 才等价于 show() + run_event_loop()。
    match ui.show() {
        Ok(()) => log::line("OK    悬浮窗已显示"),
        Err(e) => log::line(&format!("ERROR ui.show() 失败: {e}")),
    }

    log::line("OK    进入事件循环");
    if let Err(e) = slint::run_event_loop() {
        log::line(&format!("FATAL 事件循环错误: {e}"));
    }
    log::line("=== 事件循环已退出，进程结束 ===");
}
