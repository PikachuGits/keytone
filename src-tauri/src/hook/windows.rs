//! Windows 全局输入捕获：`WH_KEYBOARD_LL` + `WH_MOUSE_LL` 低级钩子。
//!
//! 两个要点：
//!
//! 1. 低级钩子的回调运行在**安装钩子的那个线程**的消息循环上，
//!    所以必须在同一线程里 `SetWindowsHookExW` 之后跑 `GetMessageW`。
//!    回调里只做转发，任何阻塞都会让系统把钩子踢掉。
//! 2. 回调是 `extern "system"`，不能捕获环境，只能通过全局静态变量拿 channel。
//!
//! 修饰键：`WH_KEYBOARD_LL` 报的可能是通用的 `VK_SHIFT` / `VK_CONTROL` / `VK_MENU`，
//! 左右不分。这里用扫描码 + 扩展位归一化成 `VK_LSHIFT` / `VK_RSHIFT` 这类专有码，
//! 这样 `keymap::key_name` 才能给出 LeftShift / RightShift 这种音色包认的名字。

use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use crossbeam_channel::Sender;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, SetWindowsHookExW, UnhookWindowsHookEx, KBDLLHOOKSTRUCT,
    MSLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN,
    WM_SYSKEYUP, WM_XBUTTONDOWN, WM_XBUTTONUP,
};

use super::{Device, InputEvent};
use crate::keymap;

static SENDER: OnceLock<Sender<InputEvent>> = OnceLock::new();

/// 通用的修饰键虚拟码。
const VK_SHIFT: u32 = 0x10;
const VK_CONTROL: u32 = 0x11;
const VK_MENU: u32 = 0x12;

/// `KBDLLHOOKSTRUCT.flags` 里的扩展位，用来区分左右 Ctrl / Alt。
const LLKHF_EXTENDED: u32 = 0x01;

pub fn backend_name() -> &'static str {
    "WH_KEYBOARD_LL"
}

/// Windows 的低级钩子不需要额外授权，装了就能用。
pub fn is_trusted() -> bool {
    true
}

pub fn open_settings() {}

pub fn start(tx: Sender<InputEvent>) -> Result<(), String> {
    let _ = SENDER.set(tx);

    std::thread::Builder::new()
        .name("kt-input-hook".into())
        .spawn(|| unsafe {
            let keyboard_hook =
                match SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), None, 0) {
                    Ok(h) => h,
                    Err(e) => {
                        eprintln!("[hook] 键盘钩子安装失败: {e}");
                        return;
                    }
                };

            let mouse_hook = match SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), None, 0) {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("[hook] 鼠标钩子安装失败: {e}");
                    let _ = UnhookWindowsHookEx(keyboard_hook);
                    return;
                }
            };

            // 消息循环必须一直跑，钩子才会持续收到事件。
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {}

            let _ = UnhookWindowsHookEx(mouse_hook);
            let _ = UnhookWindowsHookEx(keyboard_hook);
        })
        .map_err(|e| format!("无法启动钩子线程: {e}"))?;

    Ok(())
}

/// 把通用的修饰键虚拟码换成左右分明的专有码。
fn normalize_modifier(vk: u32, scan: u32, extended: bool) -> u32 {
    match vk {
        // Shift 不看扩展位，靠扫描码：0x36 是右侧。
        VK_SHIFT => {
            if scan == 0x36 {
                0xA1
            } else {
                0xA0
            }
        }
        VK_CONTROL => {
            if extended {
                0xA3
            } else {
                0xA2
            }
        }
        VK_MENU => {
            if extended {
                0xA5
            } else {
                0xA4
            }
        }
        other => other,
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        if let Some(tx) = SENDER.get() {
            let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            let wm = wparam.0 as u32;

            if wm == WM_KEYDOWN || wm == WM_KEYUP || wm == WM_SYSKEYDOWN || wm == WM_SYSKEYUP {
                let pressed = wm == WM_KEYDOWN || wm == WM_SYSKEYDOWN;
                let extended = kb.flags.0 & LLKHF_EXTENDED != 0;
                let vk = normalize_modifier(kb.vkCode, kb.scanCode, extended);

                let _ = tx.try_send(InputEvent {
                    device: Device::Keyboard,
                    code: vk,
                    name: keymap::key_name(vk),
                    pressed,
                    timestamp_ns: now_ns(),
                });
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        if let Some(tx) = SENDER.get() {
            let ms = &*(lparam.0 as *const MSLLHOOKSTRUCT);
            let wm = wparam.0 as u32;

            let button = match wm {
                WM_LBUTTONDOWN | WM_LBUTTONUP => Some((1u32, "left")),
                WM_RBUTTONDOWN | WM_RBUTTONUP => Some((2u32, "right")),
                WM_MBUTTONDOWN | WM_MBUTTONUP => Some((3u32, "middle")),
                // 侧键编号在 mouseData 的高 16 位：1 后退，2 前进。
                WM_XBUTTONDOWN | WM_XBUTTONUP => match (ms.mouseData >> 16) & 0xFFFF {
                    1 => Some((4u32, "back")),
                    2 => Some((5u32, "forward")),
                    _ => None,
                },
                _ => None,
            };

            if let Some((button_code, name)) = button {
                let pressed = matches!(
                    wm,
                    WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN
                );

                let _ = tx.try_send(InputEvent {
                    device: Device::Mouse,
                    code: button_code,
                    name,
                    pressed,
                    timestamp_ns: now_ns(),
                });
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}
