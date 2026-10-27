//! 虚拟键码 → 规范键名。
//!
//! 命名沿用音色包生态里的通行写法（`backspace` / `enter` 这种小写名），
//! 这样现成的音色包（profile.yaml 里写的就是这套名字）可以原样导入，不需要改包。
//!
//! 各平台的键码表完全不同：macOS 用 Carbon 虚拟键码，Windows 用 VK_* 常量，
//! 所以这里按平台分别提供 `key_name`。

#[cfg(target_os = "macos")]
mod table {
    pub fn key_name(code: u32) -> &'static str {
        match code {
            0x33 => "Backspace",
            0x30 => "Tab",
            0x24 => "Enter",
            0x35 => "Escape",
            0x31 => "Space",
            0x74 => "PageUp",
            0x79 => "PageDown",
            0x77 => "End",
            0x73 => "Home",
            0x7B => "Left",
            0x7E => "Up",
            0x7C => "Right",
            0x7D => "Down",
            0x72 => "Insert",
            0x75 => "Delete",

            0x1D => "0", 0x12 => "1", 0x13 => "2", 0x14 => "3", 0x15 => "4",
            0x17 => "5", 0x16 => "6", 0x1A => "7", 0x1C => "8", 0x19 => "9",

            0x00 => "A", 0x0B => "B", 0x08 => "C", 0x02 => "D", 0x0E => "E",
            0x03 => "F", 0x05 => "G", 0x04 => "H", 0x22 => "I", 0x26 => "J",
            0x28 => "K", 0x25 => "L", 0x2E => "M", 0x2D => "N", 0x1F => "O",
            0x23 => "P", 0x0C => "Q", 0x0F => "R", 0x01 => "S", 0x11 => "T",
            0x20 => "U", 0x09 => "V", 0x0D => "W", 0x07 => "X", 0x10 => "Y",
            0x06 => "Z",

            0x37 => "LeftWin", 0x36 => "RightWin",

            0x52 => "Num0", 0x53 => "Num1", 0x54 => "Num2", 0x55 => "Num3",
            0x56 => "Num4", 0x57 => "Num5", 0x58 => "Num6", 0x59 => "Num7",
            0x5B => "Num8", 0x5C => "Num9",
            0x43 => "Num*", 0x45 => "Num+", 0x4E => "Num-", 0x41 => "Num.",
            0x4B => "Num/",

            0x7A => "F1", 0x78 => "F2", 0x63 => "F3", 0x76 => "F4", 0x60 => "F5",
            0x61 => "F6", 0x62 => "F7", 0x64 => "F8", 0x65 => "F9", 0x6D => "F10",
            0x67 => "F11", 0x6F => "F12", 0x69 => "F13", 0x6B => "F14", 0x71 => "F15",
            0x6A => "F16", 0x40 => "F17", 0x4F => "F18", 0x50 => "F19", 0x5A => "F20",

            0x47 => "NumLock",
            0x38 => "LeftShift",
            0x3C => "RightShift",
            0x3B => "LeftControl",
            0x3E => "RightControl",
            0x3A => "LeftAlt",
            0x3D => "RightAlt",
            0x32 => "`",
            0x21 => "[",
            0x1E => "]",
            0x2A => "\\",
            0x29 => ";",
            0x39 => "CapsLock",
            0x18 => "+",
            0x2B => ",",
            0x1B => "-",
            0x2F => ".",
            0x2C => "/",
            0x27 => "'",

            _ => "Unknown",
        }
    }
}

#[cfg(target_os = "windows")]
mod table {
    /// WH_KEYBOARD_LL 回调里的 `vkCode` 就是 VK_* 常量。
    /// 左右修饰键的区分在 hook 层用扫描码归一化成 0xA0..0xA5，这里直接按专有码查表。
    pub fn key_name(code: u32) -> &'static str {
        match code {
            0x08 => "Backspace",
            0x09 => "Tab",
            0x0D => "Enter",
            0x1B => "Escape",
            0x20 => "Space",
            0x21 => "PageUp",
            0x22 => "PageDown",
            0x23 => "End",
            0x24 => "Home",
            0x25 => "Left",
            0x26 => "Up",
            0x27 => "Right",
            0x28 => "Down",
            0x2D => "Insert",
            0x2E => "Delete",

            0x30 => "0", 0x31 => "1", 0x32 => "2", 0x33 => "3", 0x34 => "4",
            0x35 => "5", 0x36 => "6", 0x37 => "7", 0x38 => "8", 0x39 => "9",

            0x41 => "A", 0x42 => "B", 0x43 => "C", 0x44 => "D", 0x45 => "E",
            0x46 => "F", 0x47 => "G", 0x48 => "H", 0x49 => "I", 0x4A => "J",
            0x4B => "K", 0x4C => "L", 0x4D => "M", 0x4E => "N", 0x4F => "O",
            0x50 => "P", 0x51 => "Q", 0x52 => "R", 0x53 => "S", 0x54 => "T",
            0x55 => "U", 0x56 => "V", 0x57 => "W", 0x58 => "X", 0x59 => "Y",
            0x5A => "Z",

            0x5B => "LeftWin", 0x5C => "RightWin",

            0x60 => "Num0", 0x61 => "Num1", 0x62 => "Num2", 0x63 => "Num3",
            0x64 => "Num4", 0x65 => "Num5", 0x66 => "Num6", 0x67 => "Num7",
            0x68 => "Num8", 0x69 => "Num9",
            0x6A => "Num*", 0x6B => "Num+", 0x6D => "Num-", 0x6E => "Num.",
            0x6F => "Num/",

            0x70 => "F1", 0x71 => "F2", 0x72 => "F3", 0x73 => "F4", 0x74 => "F5",
            0x75 => "F6", 0x76 => "F7", 0x77 => "F8", 0x78 => "F9", 0x79 => "F10",
            0x7A => "F11", 0x7B => "F12", 0x7C => "F13", 0x7D => "F14", 0x7E => "F15",
            0x7F => "F16", 0x80 => "F17", 0x81 => "F18", 0x82 => "F19", 0x83 => "F20",
            0x84 => "F21", 0x85 => "F22", 0x86 => "F23", 0x87 => "F24",

            0x90 => "NumLock",
            0xA0 => "LeftShift",
            0xA1 => "RightShift",
            0xA2 => "LeftControl",
            0xA3 => "RightControl",
            0xA4 => "LeftAlt",
            0xA5 => "RightAlt",
            0x14 => "CapsLock",

            0xC0 => "`",
            0xDB => "[",
            0xDD => "]",
            0xDC => "\\",
            0xBA => ";",
            0xBB => "+",
            0xBC => ",",
            0xBD => "-",
            0xBE => ".",
            0xBF => "/",
            0xDE => "'",

            _ => "Unknown",
        }
    }
}

pub use table::key_name;

/// 修饰键的 flag 判定（仅 macOS 需要）。
///
/// macOS 的修饰键走 `FlagsChanged` 事件，没有独立的按下/抬起，
/// 只能读事件的 flag 位判断当前状态。Windows 的低级钩子有明确的
/// WM_KEYDOWN / WM_KEYUP，不需要这套。
#[cfg(target_os = "macos")]
pub fn modifier_flag(code: u32) -> Option<core_graphics::event::CGEventFlags> {
    use core_graphics::event::CGEventFlags as F;
    match code {
        0x38 | 0x3C => Some(F::CGEventFlagShift),
        0x3B | 0x3E => Some(F::CGEventFlagControl),
        0x3A | 0x3D => Some(F::CGEventFlagAlternate),
        0x37 | 0x36 => Some(F::CGEventFlagCommand),
        0x39 => Some(F::CGEventFlagAlphaShift),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_codes_fall_back() {
        assert_eq!(key_name(0xFFFF), "Unknown");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_common_keys_have_stable_names() {
        assert_eq!(key_name(0x00), "A");
        assert_eq!(key_name(0x31), "Space");
        assert_eq!(key_name(0x24), "Enter");
        assert_eq!(key_name(0x33), "Backspace");
        assert_eq!(key_name(0x37), "LeftWin");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_common_keys_have_stable_names() {
        assert_eq!(key_name(0x41), "A");
        assert_eq!(key_name(0x20), "Space");
        assert_eq!(key_name(0x0D), "Enter");
        assert_eq!(key_name(0x08), "Backspace");
        assert_eq!(key_name(0x5B), "LeftWin");
        assert_eq!(key_name(0xA0), "LeftShift");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn modifier_flags_match_the_right_bit() {
        assert!(modifier_flag(0x38).is_some());
        assert!(modifier_flag(0x37).is_some());
        assert!(modifier_flag(0x00).is_none());
    }
}
