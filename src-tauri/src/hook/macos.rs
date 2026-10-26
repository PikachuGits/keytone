//! macOS 全局输入捕获：CGEventTap（ListenOnly）。
//!
//! 键盘和鼠标用同一个 tap，只是关注的事件类型不同。
//! 需要「辅助功能」权限。没授权时 CGEventTapCreate 直接返回 NULL。
//! 注意 ListenOnly 模式下回调的返回值会被系统忽略，我们永远返回 Keep。

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
use core_foundation::string::CFString;
use core_graphics::event::{
    CGEvent, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
    CallbackResult, EventField,
};
use crossbeam_channel::Sender;

use super::{Device, InputEvent};
use crate::keymap;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: core_foundation::dictionary::CFDictionaryRef) -> bool;
    static kAXTrustedCheckOptionPrompt: core_foundation::string::CFStringRef;
}

pub fn backend_name() -> &'static str {
    "CGEventTap"
}

pub fn is_trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// 触发系统授权弹窗。只调 `AXIsProcessTrusted` 是永远不会弹的，
/// 必须带 `kAXTrustedCheckOptionPrompt` 走 WithOptions 版本。
fn prompt_permission() -> bool {
    unsafe {
        let key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
        let dict = CFDictionary::from_CFType_pairs(&[(key, CFBoolean::true_value())]);
        AXIsProcessTrustedWithOptions(dict.as_concrete_TypeRef())
    }
}

pub fn open_settings() {
    // 先弹系统原生授权框；用户点「打开系统设置」后系统会自动跳到对应面板。
    if prompt_permission() {
        return;
    }
    let _ = Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn();
}

pub fn start(tx: Sender<InputEvent>) -> Result<(), String> {
    if !is_trusted() {
        return Err("missing-permission".into());
    }

    std::thread::Builder::new()
        .name("kbs-event-tap".into())
        .spawn(move || {
            let interest = vec![
                CGEventType::KeyDown,
                CGEventType::KeyUp,
                CGEventType::FlagsChanged,
                CGEventType::LeftMouseDown,
                CGEventType::LeftMouseUp,
                CGEventType::RightMouseDown,
                CGEventType::RightMouseUp,
                CGEventType::OtherMouseDown,
                CGEventType::OtherMouseUp,
            ];

            let tap = CGEventTap::new(
                CGEventTapLocation::Session,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::ListenOnly,
                interest,
                move |_proxy, etype, event| {
                    if let Some(ev) = translate(etype, event) {
                        // 回调里绝不能做重活，系统会因为超时把 tap 禁用。
                        let _ = tx.try_send(ev);
                    }
                    CallbackResult::Keep
                },
            );

            let tap = match tap {
                Ok(t) => t,
                Err(()) => {
                    eprintln!("[hook] CGEventTap 创建失败，检查辅助功能权限");
                    return;
                }
            };

            let source = match tap.mach_port().create_runloop_source(0) {
                Ok(s) => s,
                Err(()) => {
                    eprintln!("[hook] 无法创建 runloop source");
                    return;
                }
            };

            let run_loop = CFRunLoop::get_current();
            run_loop.add_source(&source, unsafe { kCFRunLoopCommonModes });

            tap.enable();
            // 阻塞在这里，直到进程退出。
            CFRunLoop::run_current();
        })
        .map_err(|e| format!("无法启动监听线程: {e}"))?;

    Ok(())
}

fn translate(etype: CGEventType, event: &CGEvent) -> Option<InputEvent> {
    let timestamp_ns = now_ns();

    match etype {
        CGEventType::KeyDown | CGEventType::KeyUp | CGEventType::FlagsChanged => {
            let code = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u32;

            let pressed = match etype {
                CGEventType::KeyDown => true,
                CGEventType::KeyUp => false,
                // 修饰键（Shift/Cmd/Option/Ctrl/CapsLock）走 FlagsChanged，
                // 必须靠 flag 位判断按下还是抬起，不能一律当 keyDown。
                _ => {
                    let bit = keymap::modifier_flag(code)?;
                    event.get_flags().contains(bit)
                }
            };

            Some(InputEvent {
                device: Device::Keyboard,
                code,
                name: keymap::key_name(code),
                pressed,
                timestamp_ns,
            })
        }

        CGEventType::LeftMouseDown
        | CGEventType::LeftMouseUp
        | CGEventType::RightMouseDown
        | CGEventType::RightMouseUp
        | CGEventType::OtherMouseDown
        | CGEventType::OtherMouseUp => {
            let pressed = matches!(
                etype,
                CGEventType::LeftMouseDown
                    | CGEventType::RightMouseDown
                    | CGEventType::OtherMouseDown
            );

            let code = match etype {
                CGEventType::LeftMouseDown | CGEventType::LeftMouseUp => 0,
                CGEventType::RightMouseDown | CGEventType::RightMouseUp => 1,
                // 中键 / 侧键都走 OtherMouse，靠 button number 区分
                _ => event.get_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER) as u32,
            };

            Some(InputEvent {
                device: Device::Mouse,
                code,
                name: button_name(code),
                pressed,
                timestamp_ns,
            })
        }

        _ => None,
    }
}

/// 按钮命名沿用通行写法：left / right / middle（小写）。
fn button_name(code: u32) -> &'static str {
    match code {
        0 => "left",
        1 => "right",
        2 => "middle",
        3 => "back",
        4 => "forward",
        _ => "button",
    }
}

fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}
