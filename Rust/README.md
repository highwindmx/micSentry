# MicTrayRs

> 麦克风常驻托盘控制器（Rust 实现，隶属 [micSentry](../README.md) 仓库）

MicTrayRs 是 micSentry 的 Rust 实现：常驻系统托盘实时显示麦克风状态，一键全局静音，并附带一个无边框透明的桌面悬浮窗。用 **Rust + Slint + WASAPI** 写成，单文件 exe、无运行时依赖、无 WebView。

## 功能特性

- **三态托盘图标**：开麦（绿）/ 已静音（红）/ **状态未知（灰）**。灰态刻意区别于绿色 —— 读不到设备时如实承认"不知道"，不会谎报"一切正常"。
- **图标按 DPI 原生渲染**：100% → 16px、125% → 20px、150% → 24px 直接光栅化，而不是"渲染 32px 再让系统缩到 16px"（后者是二次重采样，小尺寸下明显发虚）。
- **一键全局静音**：左键点击托盘图标，或全局热键 `Ctrl + Alt + M`。
- **悬浮窗**：96px 无边框透明窗口、始终置顶，可拖拽移动；左键点击切换静音，右键隐藏，`Ctrl + Alt + O` 显隐。
- **实时同步外部改键**：600ms 轮询真实端点状态，键盘 Fn 静音键 / Windows 音量面板 / 其他软件改了静音，图标同步跟随。
- **显式选择受控麦克风**：托盘菜单「选择麦克风」子菜单列出全部活动录音设备（标注"默认"/"通信默认"），可锁定到指定设备或跟随系统默认；选择结果持久化到配置文件。
- **写后回读校验**：每次设置静音后立即回读真实值，杜绝「静音之后再也回不去」。
- **开机自启**：托盘菜单一键开关（写 `HKCU\...\CurrentVersion\Run`）。
- **可诊断**：`--diag` 输出设备清单、环境自检与静音读写往返测试；`--export-icons` 把三态图标渲染成 PNG 供肉眼验收。
- **崩溃可见**：GUI 程序无控制台，故全链路落日志 + panic 钩子，杜绝"双击了但什么反应都没有"。

## 系统要求

- Windows 10 / 11（x64）
- 无安装要求，复制 exe 即可运行
- MSVC 构建动态链接系统 CRT，绝大多数 Windows 10/11 已自带；若提示缺少 DLL，安装 [VC++ 2015–2022 运行库](https://aka.ms/vs/17/release/vc_redist.x64.exe)

## 构建

需要 [Rust 工具链](https://rustup.rs/)（`rustup`）；MSVC 目标另需 [Visual Studio 生成工具](https://visualstudio.microsoft.com/downloads/)（勾选「使用 C++ 的桌面开发」，提供链接器即可，无需完整 IDE）。

```powershell
# 方式一：一键脚本（默认 MSVC 主机目标，零额外下载）
.\build.cmd
# 等价于：
powershell -ExecutionPolicy Bypass -File .\build.ps1

# 方式二：MinGW-w64 目标（首次会自动下载工具链，约 150 MB）
powershell -ExecutionPolicy Bypass -File .\build.ps1 -Gnu

# 方式三：直接 cargo
cargo build --release
```

产物：`target\release\MicTrayRs.exe`（约 12 MB，GUI 子系统）。

> `SLINT_BACKEND` 环境变量可切换渲染后端；femtovg/OpenGL 不可用时用 `SLINT_BACKEND=winit-software` 走软件渲染兜底。

## 使用

| 操作 | 效果 |
| --- | --- |
| 左键点击托盘图标 | 切换全局静音 |
| 右键点击托盘图标 | 打开菜单 |
| `Ctrl + Alt + M` | 切换全局静音 |
| `Ctrl + Alt + O` | 显示 / 隐藏悬浮窗 |
| 拖拽悬浮窗 | 移动悬浮窗位置 |
| 左键点击悬浮窗 | 切换全局静音 |
| 右键点击悬浮窗 | 隐藏悬浮窗 |

托盘菜单结构：

```
当前麦克风：麦克风阵列 (2- Intel 智音技术)（未静音）   ← 纯信息行，不可点
切换全局静音 (Ctrl+Alt+M)
显示/隐藏悬浮窗 (Ctrl+Alt+O)
选择麦克风 ▸
    跟随系统默认                    ← 单选，勾选态实时反映当前模式
    麦克风阵列 (2- Intel 智音技术) [默认 / 通信默认]
    ToDesk Virtual Audio
    ───────────
    重新扫描设备
────────
开机自启                            ← 勾选态实时反映注册表状态
退出
```

## 命令行参数

| 参数 | 说明 |
| --- | --- |
| `--diag` | 输出设备清单、环境自检、静音「写→回读」往返测试；同时落一份 `MicTrayRs-diag.txt` 到 exe 同目录 |
| `--selftest` | 跑完整条初始化链后立即退出，不进入事件循环（用于定位启动期卡点） |
| `--selftest-ui` | 同上，但显示悬浮窗并跑 2.5s 事件循环，带看门狗确认事件循环确实在阻塞 |
| `--no-show` | 对照实验开关：配合 `--selftest-ui` 时跳过 `show()`，验证"未显示窗口时事件循环是否立即返回" |
| `--export-icons [目录]` | 把三态图标渲染成 PNG（三态 × 六尺寸 + 6 倍放大图），供改版前后肉眼对比 |

`--diag` 是排查"控制的到底是不是我想控制的那只麦"的首选手段：它会做一次静音写入并回读，明确告诉你这条控制链路在本机是否真能通。

## 文件位置

| 内容 | 路径 |
| --- | --- |
| 配置（显式选择的设备 id） | `%APPDATA%\MicTrayRs\config.ini` |
| 日志 | exe 同目录 `MicTrayRs.log`；该目录不可写时退到 `%LOCALAPPDATA%\MicTrayRs\MicTrayRs.log` |
| 诊断报告 | exe 同目录 `MicTrayRs-diag.txt` |

配置文件为纯文本 `key=value`，可手工编辑（改完重启生效）。`device_id` 留空表示跟随系统默认麦克风。

## 技术栈

- **Rust**（edition 2024）
- **[Slint](https://slint.dev/) 1.x** — 悬浮窗 UI；使用 winit + femtovg 后端，**不含 WebView**，另启用 `renderer-software` 作为 OpenGL 不可用时的兜底
- **[tray-icon](https://crates.io/crates/tray-icon) 0.25 / [muda](https://crates.io/crates/muda) 0.19** — 系统托盘与原生右键菜单
- **[global-hotkey](https://crates.io/crates/global-hotkey) 0.8** — 全局热键
- **[windows](https://crates.io/crates/windows) 0.62** — WASAPI（`IMMDeviceEnumerator` / `IAudioEndpointVolume`）端点静音控制
- **[winreg](https://crates.io/crates/winreg) 0.56** — 开机自启
- **图标为自绘**：SDF（有符号距离场）+ 解析抗锯齿的 CPU 光栅化，不依赖任何图标字体或图片资源

## 与 .NET 版的差异

| | .NET 版 | Rust 版 |
| --- | --- | --- |
| 单应用静音 | ✅ 列出占用麦克风的进程逐个静音 | ❌ 不支持 |
| 选择受控麦克风 | 自动覆盖全部活动端点 | 菜单内显式选择 + 持久化 |
| 图标状态机 | 空闲 / 使用中 / 已静音 三态 | 开麦 / 已静音 / 未知 三态 |

不支持单应用静音的原因：`windows` crate 0.62 未导出 `IAudioSessionManager2` / `IAudioSessionControl2` / `ISimpleAudioVolume`，逐应用会话枚举与控制无法实现。全局端点静音不受影响。

## 许可证

[MIT](../LICENSE) © 2026 highwindmx
