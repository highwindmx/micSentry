# MicTray

> 麦克风常驻托盘控制器（GitHub 仓库名：`micSentry`）

MicTray 是一个常驻 Windows 系统托盘的小工具，实时显示麦克风状态，支持一键全局静音、单应用静音，以及桌面悬浮窗快捷操作。基于 **.NET 10 + Windows Forms + NAudio（WASAPI / Core Audio API）** 实现，无需安装、单文件即可运行。

## 特性

-   **三态托盘图标**：空闲（绿）/ 使用中（橙）/ 已静音（红），实时反映麦克风状态
-   **悬停提示**：鼠标悬停托盘显示当前状态，如「麦克风使用中（2 个程序）」
-   **一键全局静音**：左键点击托盘，或全局热键 `Ctrl + Alt + M`
-   **悬浮窗**：常驻桌面角落，可拖拽；左键点击切换静音，右键菜单隐藏 / 退出；全局热键 `Ctrl + Alt + O` 显隐
-   **单应用静音**：托盘右键菜单列出正在使用麦克风的程序（含进程名与 PID），可逐个静音 / 解除
-   **开机自启**：托盘菜单一键写入 / 移除 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` 启动项
-   **实时同步**：监听 WASAPI 音量与会话事件，状态变化即时刷新（另含 2s 兜底定时器防丢事件）

## 系统要求

-   Windows 10 / 11（x64）
-   自包含发布包已内置 .NET 10 运行时，无需单独安装

## 构建

需要 [.NET 10 SDK](https://dot.net)。

```bash
# 方式一：使用发布脚本，生成单文件自包含 exe -> dist/MicTray.exe
pwsh ./publish.ps1

# 方式二：等价 dotnet 命令
dotnet publish MicTray.csproj -c Release -r win-x64 --self-contained \
  -p:PublishSingleFile=true \
  -p:IncludeNativeLibrariesForSelfExtract=true \
  -p:EnableCompressionInSingleFile=true \
  -o dist
```

生成的 `dist/MicTray.exe` 为单文件自包含程序，可直接复制到任意 Windows x64 机器双击运行。

## 使用

| 操作 | 效果 |
| --- | --- |
| 左键点击托盘图标 | 切换全局麦克风静音 |
| Ctrl + Alt + M | 切换全局麦克风静音 |
| Ctrl + Alt + O | 显示 / 隐藏悬浮窗 |
| 拖拽悬浮窗 | 移动悬浮窗位置 |
| 左键点击悬浮窗 | 切换全局麦克风静音 |
| 右键悬浮窗 | 隐藏悬浮窗 / 退出 MicTray |
| 右键托盘图标 | 全局静音、应用列表、悬浮窗开关、开机自启、退出 |

## 技术栈

-   .NET 10 (`net10.0-windows`) + Windows Forms
-   [NAudio](https://github.com/naudio/NAudio) 2.2.1（WASAPI / Core Audio API；全局静音优先作用于 Communications 端点，并遍历所有 ACTIVE 采集端点覆盖非默认麦克风）
-   PerMonitorV2 DPI 感知，悬浮窗自绘圆角 + 手动 DPI 缩放保证高 DPI 下严格居中

## 许可证

[MIT](./LICENSE) © 2026 highwindmx

Powdered by WorkBuddy