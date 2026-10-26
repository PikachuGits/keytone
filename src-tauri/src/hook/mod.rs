//! 全局输入捕获的平台抽象层。
//!
//! 三个平台各有一套原生实现，但对外暴露同一组接口：
//! - `backend_name()` 给 UI 显示当前用的是哪套机制
//! - `is_trusted()` 检查权限（macOS 辅助功能 / Linux input 组）
//! - `start(tx)` 启动监听线程，把事件推进 channel
//! - `open_settings()` 引导用户去授权

/// 输入设备类型。音色包按设备分开加载，键盘和鼠标各用一个 profile。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Device {
    Keyboard,
    Mouse,
}

impl Device {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Keyboard => "keyboard",
            Self::Mouse => "mouse",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "keyboard" => Some(Self::Keyboard),
            "mouse" => Some(Self::Mouse),
            _ => None,
        }
    }
}

/// 归一化后的输入事件。
///
/// 键盘的 `name` 是键名（`A` / `Space`），鼠标的 `name` 是按钮名（`left` / `right`）。
/// 音色包查表时统一转小写。
pub struct InputEvent {
    pub device: Device,
    /// 原始码：键盘是虚拟键码，鼠标是按钮号
    pub code: u32,
    pub name: &'static str,
    pub pressed: bool,
    /// 事件进入监听层的本地时间戳（纳秒），用于算后续处理延迟。
    pub timestamp_ns: u64,
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{backend_name, is_trusted, open_settings, start};

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::{backend_name, is_trusted, open_settings, start};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{backend_name, is_trusted, open_settings, start};
