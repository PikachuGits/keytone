//! macOS 的朗读实现：`AVSpeechSynthesizer` 实时合成，不预生成音频文件。
//!
//! 为什么不预生成：`say` 命令每次启动要 0.8 秒，一百多个键位预生成要一分半。
//! 运行时合成零启动开销。
//!
//! 线程约束：`AVSpeechSynthesizer` 必须在有 run loop 的线程上用，
//! 所以只允许主线程调用（`main.rs` 里通过 `run_on_main_thread` 派发）。
//! 用 thread_local 存实例，避免给非 Send 的 Objective-C 对象加 unsafe impl。

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2_avf_audio::{
    AVSpeechBoundary, AVSpeechSynthesisVoice, AVSpeechSynthesizer, AVSpeechUtterance,
};
use objc2_foundation::NSString;

use super::Lang;

pub fn available() -> bool {
    true
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
            let zh =
                AVSpeechSynthesisVoice::voiceWithLanguage(Some(&NSString::from_str(Lang::Zh.locale())));
            let en =
                AVSpeechSynthesisVoice::voiceWithLanguage(Some(&NSString::from_str(Lang::En.locale())));
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
