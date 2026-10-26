//! 音频引擎。
//!
//! 结构上的两个硬约束：
//!
//! 1. `cpal::Stream` 在 macOS 上是 `!Send`，音频设备必须独占一个线程。
//!    外部只能通过 channel 下指令，`AudioHandle` 就是这个跨线程句柄。
//! 2. 采样一次性解码进内存（`SamplesBuffer`），按键时只 clone 一份丢进 mixer。
//!    播放路径上没有解码、没有文件 IO。

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

use crossbeam_channel::{unbounded, Sender};
use rodio::buffer::SamplesBuffer;
use rodio::{ChannelCount, Decoder, DeviceSinkBuilder, MixerDeviceSink, SampleRate, Source};

use crate::hook::Device;

/// 一个音源：按下 / 抬起各有若干变体，播放时随机挑一个，避免机械重复感。
#[derive(Default)]
pub struct SourceAudio {
    pub press: Vec<SamplesBuffer>,
    pub release: Vec<SamplesBuffer>,
}

/// 加载完成、可直接播放的音色包。
pub struct CompiledProfile {
    pub name: String,
    /// 键名（小写）→ 音源 id 列表
    pub key_map: HashMap<String, Vec<String>>,
    /// 没有专门映射的键用这些
    pub default_sources: Vec<String>,
    pub sources: HashMap<String, SourceAudio>,
    /// 归一化前整个包的峰值电平（调试用）
    pub peak: f32,
    /// 归一化增益（调试用）
    pub gain: f32,
}

impl CompiledProfile {
    pub fn source_count(&self) -> usize {
        self.sources.len()
    }

    pub fn sample_count(&self) -> usize {
        self.sources
            .values()
            .map(|s| s.press.len() + s.release.len())
            .sum()
    }
}

/// 解码后的裸 PCM。还没定音量，等整个包解码完算出统一增益再转成 SamplesBuffer。
pub struct DecodedAudio {
    channels: ChannelCount,
    sample_rate: SampleRate,
    data: Vec<f32>,
}

impl DecodedAudio {
    pub fn peak(&self) -> f32 {
        self.data.iter().fold(0.0f32, |max, s| max.max(s.abs()))
    }

    /// 按增益缩放并转成可播放的 buffer。
    ///
    /// 音色包之间的录音电平差得很多（原版包里有的峰值只有 0.1），
    /// 统一归一化之后，切换包不会出现「这个特别小声、那个炸耳朵」。
    pub fn into_buffer(self, gain: f32) -> SamplesBuffer {
        let data = if (gain - 1.0).abs() < f32::EPSILON {
            self.data
        } else {
            self.data
                .iter()
                .map(|s| (s * gain).clamp(-1.0, 1.0))
                .collect()
        };

        SamplesBuffer::new(self.channels, self.sample_rate, data)
    }
}

enum Cmd {
    Play { device: Device, key: String, pressed: bool },
    Profile { device: Device, profile: Arc<CompiledProfile> },
    Volume(f32),
}

#[derive(Clone)]
pub struct AudioHandle {
    tx: Sender<Cmd>,
    sample_rate: u32,
}

impl AudioHandle {
    pub fn spawn() -> Result<Self, String> {
        let (cmd_tx, cmd_rx) = unbounded::<Cmd>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, String>>();

        thread::Builder::new()
            .name("kbs-audio".into())
            .spawn(move || match Engine::new() {
                Ok(mut engine) => {
                    let _ = ready_tx.send(Ok(engine.sample_rate));
                    while let Ok(cmd) = cmd_rx.recv() {
                        match cmd {
                            Cmd::Play { device, key, pressed } => engine.play(device, &key, pressed),
                            Cmd::Profile { device, profile } => match device {
                                Device::Keyboard => engine.keyboard = Some(profile),
                                Device::Mouse => engine.mouse = Some(profile),
                            },
                            Cmd::Volume(v) => engine.volume = v,
                        }
                    }
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            })
            .map_err(|e| format!("无法启动音频线程: {e}"))?;

        let sample_rate = ready_rx
            .recv()
            .map_err(|_| "音频线程未返回初始化结果".to_string())??;

        Ok(Self {
            tx: cmd_tx,
            sample_rate,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn play(&self, device: Device, key: &str, pressed: bool) {
        let _ = self.tx.send(Cmd::Play {
            device,
            key: key.to_string(),
            pressed,
        });
    }

    pub fn set_profile(&self, device: Device, profile: Arc<CompiledProfile>) {
        let _ = self.tx.send(Cmd::Profile { device, profile });
    }

    pub fn set_volume(&self, volume: f32) {
        let _ = self.tx.send(Cmd::Volume(volume.clamp(0.0, 1.0)));
    }
}

struct Engine {
    // 必须持有 sink，drop 掉声音就没了。
    sink: MixerDeviceSink,
    sample_rate: u32,
    keyboard: Option<Arc<CompiledProfile>>,
    mouse: Option<Arc<CompiledProfile>>,
    volume: f32,
    rng: u32,
}

impl Engine {
    fn new() -> Result<Self, String> {
        let mut sink = open_sink()?;
        sink.log_on_drop(false);

        let sample_rate = sink.config().sample_rate().get();

        Ok(Self {
            sink,
            sample_rate,
            keyboard: None,
            mouse: None,
            volume: 0.8,
            rng: 0x2545_F491,
        })
    }

    fn next_rand(&mut self) -> u32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        self.rng
    }

    fn play(&mut self, device: Device, key: &str, pressed: bool) {
        let slot = match device {
            Device::Keyboard => self.keyboard.clone(),
            Device::Mouse => self.mouse.clone(),
        };
        let Some(profile) = slot else {
            return;
        };

        let ids = profile
            .key_map
            .get(key)
            .filter(|v| !v.is_empty())
            .unwrap_or(&profile.default_sources);

        if ids.is_empty() {
            return;
        }

        let id = &ids[self.next_rand() as usize % ids.len()];
        let Some(source) = profile.sources.get(id) else {
            return;
        };

        let variants = if pressed {
            &source.press
        } else {
            &source.release
        };

        if variants.is_empty() {
            return;
        }

        let buffer = &variants[self.next_rand() as usize % variants.len()];
        // Mixer 会做混音，同时按下多个键不会互相打断。
        self.sink.mixer().add(buffer.clone().amplify(self.volume));
    }
}

/// rodio 默认把缓冲设成 sample_rate/20（48k 下是 2048 帧），
/// 但很多设备的合法区间要小得多（这台 Mac 是 15..=960），直接开流会失败。
/// 所以按「低延迟优先」逐个试，最后再让 rodio 自己挑配置兜底。
const BUFFER_CANDIDATES: [u32; 4] = [480, 960, 256, 240];

fn open_sink() -> Result<MixerDeviceSink, String> {
    let mut last_err = String::new();

    for frames in BUFFER_CANDIDATES {
        let attempt = DeviceSinkBuilder::from_default_device()
            .map_err(|e| format!("{e:?}"))
            .and_then(|builder| {
                builder
                    .with_buffer_size(rodio::cpal::BufferSize::Fixed(frames))
                    .open_stream()
                    .map_err(|e| format!("{e:?}"))
            });

        match attempt {
            Ok(sink) => return Ok(sink),
            Err(e) => last_err = format!("buffer={frames}: {e}"),
        }
    }

    DeviceSinkBuilder::open_default_sink()
        .map_err(|e| format!("{last_err} / 兜底也失败: {e:?}"))
}

/// 解码 mp3 / wav 到内存。一次解码，之后播放零 IO。
pub fn decode_file(path: &Path) -> Result<DecodedAudio, String> {
    // 优先用 gapless 解码：裁掉 mp3 编码器补的静音，声音更贴手。
    // 但极短的文件（比如原版某些包只有 1 帧的滚轮音）会被裁成 0 采样，
    // 这时退回不裁剪的完整解码。
    match decode_with(path, true) {
        Ok(buf) => Ok(buf),
        Err(first) => decode_with(path, false).map_err(|_| first),
    }
}

fn decode_with(path: &Path, gapless: bool) -> Result<DecodedAudio, String> {
    let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;

    let decoder = Decoder::builder()
        .with_data(BufReader::new(file))
        .with_gapless(gapless)
        .build()
        .map_err(|e| format!("{}: 解码失败 {e}", path.display()))?;

    let channels: ChannelCount = decoder.channels();
    let sample_rate: SampleRate = decoder.sample_rate();
    let data: Vec<f32> = decoder.collect();

    if data.is_empty() {
        return Err(format!(
            "{}: 解码后没有音频数据（gapless={gapless}）",
            path.display()
        ));
    }

    Ok(DecodedAudio {
        channels,
        sample_rate,
        data,
    })
}
