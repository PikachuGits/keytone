//! macOS 虚拟键码 → 规范键名。
//!
//! 命名沿用音色包生态里的通行写法（`backspace` / `enter` 这种小写名），
//! 这样现成的音色包（profile.yaml 里写的就是这套名字）可以原样导入，不需要改包。

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

/// 修饰键的 flag 判定：FlagsChanged 事件没有按下/抬起，只能看 flag 位。
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
    fn common_keys_have_stable_names() {
        assert_eq!(key_name(0x00), "A");
        assert_eq!(key_name(0x31), "Space");
        assert_eq!(key_name(0x24), "Enter");
        assert_eq!(key_name(0x33), "Backspace");
        assert_eq!(key_name(0x37), "LeftWin");
        assert_eq!(key_name(0xFFFF), "Unknown");
    }

    #[test]
    fn modifier_flags_match_the_right_bit() {
        assert!(modifier_flag(0x38).is_some());
        assert!(modifier_flag(0x37).is_some());
        assert!(modifier_flag(0x00).is_none());
    }
}
