//! 启动诊断日志。
//!
//! 本程序是 `windows_subsystem = "windows"` 的 GUI 程序：**没有控制台**。
//! `main` 里任何 panic（展开到顶层 → 退出码 101，不产生 WER 事件）
//! 或提前 `return`，都不会有任何可见输出，表现为"双击了但什么都没发生"。
//!
//! 本模块把启动各阶段与 panic 写进日志文件，并支持 `--selftest` 自检：
//! 跑完整条初始化链后退出，用来在不看 UI 的情况下定位卡点。

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

static LOG: OnceLock<Mutex<Option<std::fs::File>>> = OnceLock::new();
static START: OnceLock<Instant> = OnceLock::new();

/// 找一个可写的日志位置：优先 exe 同目录，失败退到 `%LOCALAPPDATA%\MicTrayRs`。
fn open_log() -> Option<std::fs::File> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("MicTrayRs.log"));
        }
    }
    let dir = dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("MicTrayRs");
    let _ = std::fs::create_dir_all(&dir);
    candidates.push(dir.join("MicTrayRs.log"));

    for p in candidates {
        if let Ok(f) = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&p)
        {
            return Some(f);
        }
    }
    None
}

/// 初始化日志（每次启动覆盖写，保证只反映最近一次运行）。
pub fn init() {
    let _ = START.set(Instant::now());
    let _ = LOG.set(Mutex::new(open_log()));
    line(&format!(
        "=== MicTrayRs start  pid={} ===",
        std::process::id()
    ));
    match std::env::current_exe() {
        Ok(p) => line(&format!("exe : {}", p.display())),
        Err(e) => line(&format!("exe : <unknown: {e}>")),
    }
    line(&format!(
        "args: {:?}",
        std::env::args().collect::<Vec<String>>()
    ));
}

/// 追加一行日志（带自启动以来的毫秒偏移）。可在任意线程调用。
pub fn line(msg: &str) {
    let ms = START.get().map(|t| t.elapsed().as_millis()).unwrap_or(0);
    let text = format!("[+{ms:>6}ms] {msg}\n");
    if let Some(m) = LOG.get() {
        if let Ok(mut g) = m.lock() {
            if let Some(f) = g.as_mut() {
                let _ = f.write_all(text.as_bytes());
                let _ = f.flush();
            }
        }
    }
}

/// 把一段多行文本写进日志，并额外落一份到 exe 同目录的 `MicTrayRs-<name>.txt`。
///
/// 目的：诊断类输出（`--diag`）量比较大，用户直接打开这个文本文件比在日志里翻找方便。
pub fn dump(name: &str, text: &str) {
    for l in text.lines() {
        line(l);
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(dir) = exe.parent() else {
        return;
    };
    let p = dir.join(format!("MicTrayRs-{name}.txt"));
    match std::fs::write(&p, text) {
        Ok(()) => line(&format!("      诊断输出已写出: {}", p.display())),
        Err(e) => line(&format!("WARN  写 {} 失败: {e}", p.display())),
    }
}

/// 安装 panic 钩子：把 panic 位置与消息写进日志，同时保留默认 stderr 行为。
pub fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let loc = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown>".to_string());
        let msg = if let Some(s) = info.payload().downcast_ref::<&'static str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "<non-string panic payload>".to_string()
        };
        line(&format!("!!! PANIC at {loc} :: {msg}"));
        prev(info);
    }));
}
