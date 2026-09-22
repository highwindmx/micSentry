# micSentry-ahk

> 麦克风静音小面板（AutoHotkey v2 实现）· [micSentry](../README.md) 的脚本版（本版位于 `Autohotkey/` 目录）

用 AutoHotkey v2 写的两个轻量脚本：一个常驻屏幕右下角的麦克风静音面板，外加一个用来查设备序号的辅助工具。适合不想装 .NET / Rust 运行时、只想双击就跑的场景。

## 功能特性

- **常驻右下角小面板**：始终置顶、不抢焦点（`NoActivate`），点按钮不影响当前窗口输入
- **一屏两键**：`静音` / `解除静音`，点一下立即生效
- **500ms 轮询真实状态**：键盘 Fn 静音键、系统音量面板等外部改动后，面板文字与配色同步跟随
- **三态显示**：未静音（绿）/ 静音（红）/ 无法读取（灰）—— 读不到时如实显示"无法读取"，不退化成绿色
- **附带设备查询工具**：`soundcard.ahk` 列出全部声音设备与组件的音量、静音状态，用来确定 `micS.ahk` 该填哪个设备序号

## 系统要求

- Windows 10 / 11
- [AutoHotkey v2.0+](https://www.autohotkey.com/)（**v1 不兼容**，脚本用了 v2 的 `Gui` 对象与 `SoundGetMute` 等新语法）
- 无需管理员权限

## 使用

1. 安装 AutoHotkey v2
2. 双击 `soundcard.ahk`，在弹出的清单里找到你的麦克风，记下它的编号（`#` 列）
3. 编辑 `micS.ahk` 第 2 行，把 `micName := 8` 改成你的编号
4. 双击 `micS.ahk`，屏幕右下角出现面板

> `micName` 要填的是 `SoundGetName` 的**设备序号**，不是设备名字（中文设备名在脚本里当标识符容易踩编码坑）。

| 操作 | 效果 |
| --- | --- |
| 点击 `静音` 按钮 | 把 `micName` 指定设备设为静音 |
| 点击 `解除静音` 按钮 | 解除静音 |
| 关闭面板窗口 | 退出脚本 |

## 文件说明

| 文件 | 用途 |
| --- | --- |
| `micS.ahk` | 麦克风静音面板（主脚本） |
| `soundcard.ahk` | 声音组件清单查看器（辅助工具，用来查设备 / 组件序号） |

## 技术栈

- AutoHotkey v2，全程只用内置函数：`SoundGetMute` / `SoundSetMute` / `SoundGetName` / `SoundGetVolume`
- 无第三方库、无编译步骤、无运行时依赖

## 已知限制

- 设备以**序号**指定：拔插 USB 麦克风、新增/删除虚拟声卡后序号可能变化，需用 `soundcard.ahk` 重查
- 面板位置固定在屏幕右下角（`x A_ScreenWidth-320 y A_ScreenHeight-210`），可自行修改 `myGui.Show(...)`
- 相比 micSentry 的 [.NET 版](https://github.com/highwindmx/micSentry/tree/main/Csharp) / [Rust 版](https://github.com/highwindmx/micSentry/tree/main/Rust)：无系统托盘图标、无全局热键、无桌面悬浮窗、无单应用静音

## 许可证

[MIT](./LICENSE) © 2026 highwindmx
