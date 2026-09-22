# micSentry

> 麦克风常驻托盘控制器 · 同一功能的三种独立实现（.NET / Rust / AutoHotkey）

micSentry 是一组 Windows 麦克风控制小工具，只专注解决一件事：**让你随时知道当前麦克风处于什么状态，并用一次点击或一个热键把它静音 / 恢复**。

仓库内提供三种**彼此独立、互不依赖**的实现，可按机器条件与需求任选其一使用或对照阅读。

## 实现对照

| | .NET 版（主力） | Rust 版 | AutoHotkey 版 |
| --- | --- | --- | --- |
| 目录 | [`Csharp/`](./Csharp) | [`Rust/`](./Rust) | [`Autohotkey/`](./Autohotkey) |
| 程序名 | `MicTray.exe` | `MicTrayRs.exe` | `micS.ahk` |
| 技术栈 | .NET 10 + WinForms + NAudio | Rust + Slint + windows-rs | AHK v2 内置 `Sound*` |
| 运行依赖 | 无（单文件自包含） | 无（MSVC 运行时，系统基本自带） | 需安装 AutoHotkey v2 |
| 构建前提 | .NET 10 SDK | Rust 工具链 + VS 生成工具 | 免构建，双击即跑 |
| 单应用静音 | ✅ | ❌（见下文说明） | ❌ |
| 选择受控麦克风 | 自动（遍历全部端点） | ✅ 菜单内显式选择并持久化 | ❌ 需手改脚本内设备序号 |
| 系统托盘图标 | ✅ 三态 | ✅ 三态 | ❌ 无托盘，只有小面板 |
| 全局热键 | ✅ | ✅ | ❌ |
| 桌面悬浮窗 | ✅ | ✅ | ❌ |
| 图标状态 | 空闲绿 / 使用中橙 / 已静音红 | 开麦绿 / 已静音红 / **未知灰** | 未静音绿 / 静音红 / 读不到灰 |
| 实时同步外部改键 | ✅（事件 + 2s 兜底） | ✅（600ms 轮询） | ✅（500ms 轮询） |

选择建议：要功能最全（含单应用静音）用 .NET 版；要单文件无运行时依赖用 Rust 版；不想装运行时、只想双击看状态用 AutoHotkey 版。

## 快速上手

### .NET 版

```powershell
cd Csharp
pwsh ./publish.ps1          # 产出 dist/MicTray.exe（单文件自包含）
```

双击 `dist/MicTray.exe`，托盘出现图标即可用。详见 [`Csharp/README.md`](./Csharp/README.md)。

### Rust 版

```powershell
cd Rust
.\build.cmd                 # 或双击；产出 target\release\MicTrayRs.exe
```

双击 `target\release\MicTrayRs.exe`。详见 [`Rust/README.md`](./Rust/README.md)。

### AutoHotkey 版

1. 安装 [AutoHotkey v2](https://www.autohotkey.com/)
2. 双击 `Autohotkey/soundcard.ahk` 查到自己麦克风的**设备序号**
3. 把 `Autohotkey/micS.ahk` 第 2 行的 `micName := 8` 改成该序号
4. 双击 `micS.ahk`，屏幕右下角出现面板

详见 [`Autohotkey/README.md`](./Autohotkey/README.md)。

## 目录结构

```
micSentry/
├── Csharp/          .NET 版：功能最全，含「单应用静音」
│   ├── *.cs         WinForms 托盘 + 悬浮窗 + WASAPI 会话控制
│   ├── publish.ps1  一键发布单文件自包含 exe
│   └── README.md
├── Rust/            Rust 版：Slint 自绘图标，无 WebView、无运行时依赖
│   ├── src/         音频后端 / 托盘 / 热键 / 图标光栅化 / 配置 / 日志
│   ├── ui/          main.slint（96px 无边框透明悬浮窗）
│   ├── build.cmd    一键构建（默认 MSVC）
│   └── README.md
├── Autohotkey/      AutoHotkey 版：轻量脚本，双击即跑
│   ├── micS.ahk     右下角两键静音面板
│   ├── soundcard.ahk 声音组件清单查看器（查设备序号用）
│   └── README.md
├── LICENSE          MIT
└── README.md        本文件
```

## 三版共守的设计约定

这些约定来自实际踩坑，三种实现都遵守：

1. **读 → 写 → 回读校验**：每次设置静音后立即回读真实值，不信任写入返回值，也不缓存"记忆态"。
2. **状态未知必须区别于"未静音"**：读不到设备时显示灰色 / 「无法读取」，绝不退化成绿色默认值 —— 否则界面会谎报一切正常。
3. **读取失败时拒绝盲写**：状态未知时直接放弃切换，而不是"猜一个反值"写下去。否则会退化成「只能静音、再也回不去」。
4. **持续同步外部变化**：键盘 Fn 静音键、Windows 音量面板、其他软件都能改变静音态，程序必须轮询或监听事件跟随，不能只在自身操作后刷新。
5. **GUI 程序不静默死亡**：桌面程序无控制台，任何 panic 都表现为"双击了没反应"，因此全链路落日志 + panic 钩子 + 自检模式。

## 已知限制

- **Rust 版暂不支持单应用静音**：`windows` crate 0.62 未导出 `IAudioSessionManager2` / `ISimpleAudioVolume`，逐应用会话控制无法实现。全局端点静音不受影响。需要该能力请用 .NET 版。
- **AutoHotkey 版以设备序号指定麦克风**：拔插 USB 麦克风或增删虚拟声卡后序号可能变化，需用 `soundcard.ahk` 重查。
- 三版控制的是 **WASAPI 端点静音**；若某机型把静音开关做在硬件或驱动层（部分笔记本阵列麦克风），端点 API 管不到，需要用 Rust 版的 `--diag` 往返测试确认。

## 许可证

[MIT](./LICENSE) © 2026 highwindmx
