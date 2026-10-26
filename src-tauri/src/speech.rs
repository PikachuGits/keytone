//! 键位语音播报。
//!
//! 用系统 TTS（`AVSpeechSynthesizer`）实时朗读，不预生成音频文件 ——
//! `say` 命令每次启动要 0.8 秒，一百多个键位预生成要一分半，不可接受。
//!
//! 线程约束：`AVSpeechSynthesizer` 必须在有 run loop 的线程上用，
//! 所以这里只允许主线程调用（`main.rs` 里通过 `run_on_main_thread` 派发）。
//! 用 thread_local 存实例，避免给非 Send 的 Objective-C 对象加 unsafe impl。

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2_avf_audio::{
    AVSpeechBoundary, AVSpeechSynthesisVoice, AVSpeechSynthesizer, AVSpeechUtterance,
};
use objc2_foundation::NSString;

use crate::hook::Device;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    Zh,
    En,
}

impl Lang {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "zh" | "cn" | "chinese" => Some(Self::Zh),
            "en" | "english" => Some(Self::En),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Zh => "zh",
            Self::En => "en",
        }
    }

    fn locale(self) -> &'static str {
        match self {
            Self::Zh => "zh-CN",
            Self::En => "en-US",
        }
    }
}

struct Speaker {
    synth: Retained<AVSpeechSynthesizer>,
    zh: Option<Retained<AVSpeechSynthesisVoice>>,
    en: Option<Retained<AVSpeechSynthesisVoice>>,
}

impl Speaker {
    fn new() -> Option<Self> {
        unsafe {
            let synth = AVSpeechSynthesizer::new();
            let zh = AVSpeechSynthesisVoice::voiceWithLanguage(Some(&NSString::from_str(Lang::Zh.locale())));
            let en = AVSpeechSynthesisVoice::voiceWithLanguage(Some(&NSString::from_str(Lang::En.locale())));
            if zh.is_none() {
                eprintln!("[speech] 系统缺少 zh-CN 语音，中文播报会退回默认音色");
            }
            if en.is_none() {
                eprintln!("[speech] 系统缺少 en-US 语音，英文播报会退回默认音色");
            }
            Some(Self { synth, zh, en })
        }
    }

    fn speak(&self, text: &str, lang: Lang, volume: f32) {
        unsafe {
            // 打字快的时候朗读会排队，先把上一句掐掉，保证"最新的键"优先。
            self.synth.stopSpeakingAtBoundary(AVSpeechBoundary::Immediate);

            let utterance = AVSpeechUtterance::speechUtteranceWithString(&NSString::from_str(text));

            let voice = match lang {
                Lang::Zh => self.zh.as_ref(),
                Lang::En => self.en.as_ref(),
            };
            if let Some(voice) = voice {
                utterance.setVoice(Some(voice));
            }

            utterance.setRate(0.58);
            utterance.setVolume(volume.clamp(0.0, 1.0));
            self.synth.speakUtterance(&utterance);
        }
    }
}

thread_local! {
    static SPEAKER: RefCell<Option<Speaker>> = const { RefCell::new(None) };
}

/// 朗读一段文本。**只能在主线程调用。**
pub fn speak(text: &str, lang: Lang, volume: f32) {
    SPEAKER.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = Speaker::new();
        }
        if let Some(speaker) = slot.as_ref() {
            speaker.speak(text, lang, volume);
        }
    });
}

/// 键名 → 朗读文本。`key` 是内部小写名（`a` / `space` / `left`）。
pub fn spoken_text(device: Device, key: &str, lang: Lang) -> Option<String> {
    match device {
        Device::Mouse => mouse_text(key, lang),
        Device::Keyboard => keyboard_text(key, lang),
    }
}

fn mouse_text(key: &str, lang: Lang) -> Option<String> {
    let text = match (key, lang) {
        ("left", Lang::Zh) => "左键",
        ("right", Lang::Zh) => "右键",
        ("middle", Lang::Zh) => "中键",
        ("back", Lang::Zh) => "后退键",
        ("forward", Lang::Zh) => "前进键",
        ("left", Lang::En) => "left",
        ("right", Lang::En) => "right",
        ("middle", Lang::En) => "middle",
        ("back", Lang::En) => "back",
        ("forward", Lang::En) => "forward",
        _ => return None,
    };
    Some(text.to_string())
}

fn keyboard_text(key: &str, lang: Lang) -> Option<String> {
    // 单字母
    if key.len() == 1 {
        let ch = key.chars().next()?;
        if ch.is_ascii_lowercase() {
            return Some(match lang {
                // 中文按拼音读：A→啊 B→波 C→次 ……
                // 直接把拉丁字母丢给中文语音会被读成英文字母名，所以用同音字锁死读音。
                Lang::Zh => pinyin_of(ch).to_string(),
                Lang::En => ch.to_string(),
            });
        }
        if ch.is_ascii_digit() {
            return Some(ch.to_string());
        }
    }

    let text = match (key, lang) {
        ("space", Lang::Zh) => "空格",
        ("space", Lang::En) => "space",
        ("enter", Lang::Zh) => "回车",
        ("enter", Lang::En) => "enter",
        ("backspace", Lang::Zh) => "退格",
        ("backspace", Lang::En) => "backspace",
        ("delete", Lang::Zh) => "删除",
        ("delete", Lang::En) => "delete",
        ("tab", Lang::Zh) => "制表",
        ("tab", Lang::En) => "tab",
        ("escape", Lang::Zh) => "退出",
        ("escape", Lang::En) => "escape",
        ("capslock", Lang::Zh) => "大写锁定",
        ("capslock", Lang::En) => "caps lock",

        // 方向键：中文用「箭头」区分，避免和鼠标左右键混淆
        ("left", Lang::Zh) => "左箭头",
        ("left", Lang::En) => "left",
        ("right", Lang::Zh) => "右箭头",
        ("right", Lang::En) => "right",
        ("up", Lang::Zh) => "上箭头",
        ("up", Lang::En) => "up",
        ("down", Lang::Zh) => "下箭头",
        ("down", Lang::En) => "down",

        ("leftshift", Lang::Zh) | ("rightshift", Lang::Zh) => "上档键",
        ("leftshift", Lang::En) | ("rightshift", Lang::En) => "shift",
        ("leftcontrol", Lang::Zh) | ("rightcontrol", Lang::Zh) => "控制键",
        ("leftcontrol", Lang::En) | ("rightcontrol", Lang::En) => "control",
        ("leftalt", Lang::Zh) | ("rightalt", Lang::Zh) => "选项键",
        ("leftalt", Lang::En) | ("rightalt", Lang::En) => "option",
        ("leftwin", Lang::Zh) | ("rightwin", Lang::Zh) => "命令键",
        ("leftwin", Lang::En) | ("rightwin", Lang::En) => "command",

        _ => return None,
    };

    Some(text.to_string())
}

/// 字母的拼音读音，用同音汉字锁定，避免中文语音把它读成英文字母名。
fn pinyin_of(ch: char) -> &'static str {
    match ch {
        'a' => "啊",
        'b' => "波",
        'c' => "次",
        'd' => "的",
        'e' => "鹅",
        'f' => "佛",
        'g' => "哥",
        'h' => "喝",
        'i' => "衣",
        'j' => "鸡",
        'k' => "科",
        'l' => "勒",
        'm' => "摸",
        'n' => "讷",
        'o' => "哦",
        'p' => "坡",
        'q' => "七",
        'r' => "日",
        's' => "思",
        't' => "特",
        'u' => "乌",
        'v' => "微",
        'w' => "屋",
        'x' => "西",
        'y' => "呀",
        'z' => "资",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_use_pinyin_in_chinese() {
        assert_eq!(spoken_text(Device::Keyboard, "a", Lang::Zh).unwrap(), "啊");
        assert_eq!(spoken_text(Device::Keyboard, "b", Lang::Zh).unwrap(), "波");
        assert_eq!(spoken_text(Device::Keyboard, "z", Lang::Zh).unwrap(), "资");
    }

    #[test]
    fn letters_are_letters_in_english() {
        assert_eq!(spoken_text(Device::Keyboard, "a", Lang::En).unwrap(), "a");
        assert_eq!(spoken_text(Device::Keyboard, "z", Lang::En).unwrap(), "z");
    }

    #[test]
    fn mouse_buttons_differ_by_language() {
        assert_eq!(spoken_text(Device::Mouse, "left", Lang::Zh).unwrap(), "左键");
        assert_eq!(spoken_text(Device::Mouse, "left", Lang::En).unwrap(), "left");
        assert_eq!(spoken_text(Device::Mouse, "right", Lang::Zh).unwrap(), "右键");
    }

    #[test]
    fn arrows_are_distinguishable_from_mouse_in_chinese() {
        // 鼠标左键 vs 左方向键，中文必须能分清
        assert_ne!(
            spoken_text(Device::Mouse, "left", Lang::Zh).unwrap(),
            spoken_text(Device::Keyboard, "left", Lang::Zh).unwrap()
        );
    }

    #[test]
    fn unknown_keys_have_no_text() {
        assert!(spoken_text(Device::Keyboard, "f13", Lang::Zh).is_none());
    }
}
