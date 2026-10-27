//! 没有语音合成实现的平台。
//!
//! 目前只有 macOS 接了系统 TTS。Windows 的 SAPI 和 Linux 的 espeak
//! 都需要额外的 COM / 外部进程对接，尚未实现，所以这里如实返回「不支持」，
//! 界面据此把语音设置隐藏掉，而不是留一个开关点了没反应。

use super::Lang;

/// 该平台是否提供语音播报。
pub fn available() -> bool {
    false
}

/// 空实现。调用方在 `available()` 为 false 时不应该走到这里。
pub fn speak(_text: &str, _lang: Lang, _volume: f32) {
    use std::sync::Once;

    static WARNED: Once = Once::new();
    WARNED.call_once(|| {
        eprintln!("[speech] 当前平台未实现语音播报，该功能已禁用");
    });
}
