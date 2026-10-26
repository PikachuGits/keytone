//! Windows 全局按键捕获：WH_KEYBOARD_LL 低级钩子。
//!
//! 注意：低级钩子的回调运行在安装钩子的那个线程的消息循环上，
//! 所以必须在同一线程里 SetWindowsHookExW + GetMessageW。
//! 回调里只做转发，任何阻塞都会让系统把钩子踢掉。
//!
//! ⚠️ 本文件未在 Windows 真机验证（开发机是 macOS），逻辑与 Go 版一致。

use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use crossbeam_channel::Sender;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, SetWindowsHookExW, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN,
    WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use super::{Device, InputEvent};

static SENDER: OnceLock<Sender<InputEvent>> = OnceLock::new();

pub fn backend_name() -> &'static str {
    "WH_KEYBOARD_LL"
}

pub fn is_trusted() -> bool {
    true
}

pub fn open_settings() {}

pub fn start(tx: Sender<InputEvent>) -> Result<(), String> {
    let _ = SENDER.set(tx);

    std::thread::Builder::new()
        .name("kbs-ll-hook".into())
        .spawn(|| unsafe {
            let hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0) {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("[hook] SetWindowsHookExW 失败: {e}");
                    return;
                }
            };

            let mut msg = MSG::default();
            // 消息循环必须一直跑，钩子才会持续收到事件。
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {}

            let _ = windows::Win32::UI::WindowsAndMessaging::UnhookWindowsHookEx(hook);
        })
        .map_err(|e| format!("无法启动钩子线程: {e}"))?;

    Ok(())
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        if let Some(tx) = SENDER.get() {
            let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            let pressed = matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
            let _ = tx.try_send(InputEvent {
                device: Device::Keyboard,
                code: kb.vkCode,
                name: "Key",
                pressed,
                timestamp_ns: now_ns(),
            });
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
