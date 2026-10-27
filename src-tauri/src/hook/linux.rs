//! Linux 全局按键捕获：直接读 evdev 字符设备。
//!
//! 需要用户属于 `input` 组（`sudo usermod -aG input $USER`），
//! 或者给二进制加 CAP_DAC_READ_SEARCH。
//!
//! ⚠️ 本文件未在 Linux 真机验证（开发机是 macOS），逻辑与 Go 版一致。
//! Wayland 下焦点窗口检测没有统一 API，「应用规则」功能需要额外适配。

use std::time::{SystemTime, UNIX_EPOCH};

use crossbeam_channel::Sender;
// evdev 也有个 Device，和本模块的 Device 重名，必须改别名。
use evdev::{Device as EvdevDevice, EventType, Key};

use super::{Device, InputEvent};

pub fn backend_name() -> &'static str {
    "evdev"
}

/// 检查是否能读到任意输入设备。读不到通常意味着没加进 input 组。
pub fn is_trusted() -> bool {
    std::fs::read_dir("/dev/input")
        .map(|dir| {
            dir.flatten().any(|entry| {
                let path = entry.path();
                path.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("event"))
                    && std::fs::File::open(&path).is_ok()
            })
        })
        .unwrap_or(false)
}

pub fn open_settings() {}

pub fn start(tx: Sender<InputEvent>) -> Result<(), String> {
    let mut found = 0usize;

    for (_path, device) in evdev::enumerate() {
        let has_keys = device
            .supported_keys()
            .is_some_and(|keys| keys.contains(Key::KEY_A) && keys.contains(Key::KEY_ENTER));

        if !has_keys {
            continue;
        }

        found += 1;
        let tx = tx.clone();
        let mut device: EvdevDevice = device;

        std::thread::Builder::new()
            .name("kbs-evdev".into())
            .spawn(move || loop {
                match device.fetch_events() {
                    Ok(events) => {
                        for ev in events {
                            if ev.event_type() != EventType::KEY {
                                continue;
                            }
                            let code = ev.code();
                            let pressed = ev.value() == 1;
                            // value == 2 是长按连发，丢弃。
                            if ev.value() == 2 {
                                continue;
                            }
                            let _ = tx.try_send(InputEvent {
                                device: Device::Keyboard,
                                // evdev 的键码是 u16，InputEvent 统一用 u32。
                                code: code as u32,
                                name: key_name(code),
                                pressed,
                                timestamp_ns: now_ns(),
                            });
                        }
                    }
                    Err(e) => {
                        eprintln!("[hook] evdev 读取失败: {e}");
                        return;
                    }
                }
            })
            .map_err(|e| format!("无法启动 evdev 线程: {e}"))?;
    }

    if found == 0 {
        return Err("没有找到可读的键盘设备，请确认用户在 input 组内".into());
    }

    Ok(())
}

fn key_name(code: u16) -> &'static str {
    match Key::new(code) {
        Key::KEY_ESC => "Esc",
        Key::KEY_ENTER => "Return",
        Key::KEY_SPACE => "Space",
        Key::KEY_TAB => "Tab",
        Key::KEY_BACKSPACE => "Delete",
        Key::KEY_LEFTSHIFT | Key::KEY_RIGHTSHIFT => "Shift",
        Key::KEY_LEFTCTRL | Key::KEY_RIGHTCTRL => "Control",
        Key::KEY_LEFTALT | Key::KEY_RIGHTALT => "Option",
        Key::KEY_LEFTMETA | Key::KEY_RIGHTMETA => "Command",
        _ => "Key",
    }
}

fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}
