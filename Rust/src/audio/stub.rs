//! 非 Windows 平台的占位实现（编译通过、运行提示未实现）。
//! 以后 macOS / Linux 用各自原生音频 API 替换。
#![allow(dead_code)] // 仅在非 Windows 构建时被 create() 引用，Windows 下不构造

use super::{DeviceInfo, MicControllerTrait, Snapshot};

pub struct StubMic {
    /// 降级原因（会如实出现在 `--diag` 报告里，避免"看不出到底为什么不能用"）
    reason: String,
}

impl StubMic {
    pub fn new(reason: impl Into<String>) -> Self {
        StubMic {
            reason: reason.into(),
        }
    }
}

impl MicControllerTrait for StubMic {
    fn snapshot(&self) -> Snapshot {
        Snapshot::unknown(self.reason.clone())
    }

    fn set_muted(&self, _muted: bool) -> Snapshot {
        self.snapshot()
    }

    fn devices(&self) -> Vec<DeviceInfo> {
        Vec::new()
    }

    fn select_device(&self, _id: &str) -> Snapshot {
        self.snapshot()
    }
}
