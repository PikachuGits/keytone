//! 音色包（profile）加载。
//!
//! 采用 MechVibes 风格的 YAML 格式，现成的音色包可以直接用。
//!
//! ```yaml
//! profile:
//!   name: mx-brown
//!   author: kbsim
//!   device: keyboard
//! sources:
//!   - id: key1
//!     source:
//!       press: press_key1.mp3
//!       release: release_key.mp3
//! keys:
//!   default: [ key1, key2 ]
//!   other:
//!     - sound: enter
//!       keys: [ enter ]
//! ```
//!
//! 鼠标包用 `buttons`，但**实际存在两种写法**，两种都要认：
//!
//! ```yaml
//! # 写法 A：结构化（文档里的样子）
//! buttons:
//!   default: click
//!   other:
//!     - sound: right_click
//!       buttons: [ right ]
//!
//! # 写法 B：MechVibes 风格的数字映射（原版 7 个鼠标包全是这种）
//! buttons:
//!   '1': click_left      # 1 = 左键
//!   '2': click_middle    # 2 = 中键
//!   '3': click_right     # 3 = 右键
//!   default: click_left
//!   other: []
//! ```
//!
//! 原版把写法 B 里的数字键当未知字段忽略了，所以它的鼠标左/右/中键其实是同一个音。
//! 这里把它们正确映射到 left / middle / right，左右键能听出区别。
//!
//! 扩展：`press` / `release` / `default` 除了单个文件名，也可以写成列表，
//! 表示多个采样变体，播放时随机挑一个，避免机械重复感。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_yaml_ng::Value;

use crate::audio::{decode_file, CompiledProfile, DecodedAudio, SourceAudio};
use crate::hook::Device;

#[derive(Deserialize)]
struct RawProfile {
    profile: RawDetails,
    #[serde(default)]
    sources: Vec<RawSource>,
    #[serde(default)]
    keys: RawKeys,
    /// 两种写法差异太大，直接当 Value 手工解析，比拆成 enum 更省事。
    #[serde(default)]
    buttons: Option<Value>,
}

#[derive(Deserialize)]
struct RawDetails {
    name: String,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    device: Option<String>,
}

#[derive(Deserialize)]
struct RawSource {
    id: String,
    source: RawSourceSpec,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawSourceSpec {
    /// `source: press.mp3`（只有按下音）
    Single(String),
    /// `source: { press: ..., release: ... }`
    Config {
        #[serde(default)]
        press: Option<OneOrMany>,
        #[serde(default)]
        release: Option<OneOrMany>,
    },
}

#[derive(Deserialize, Clone)]
#[serde(untagged)]
enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    fn into_vec(self) -> Vec<String> {
        match self {
            Self::One(s) => vec![s],
            Self::Many(v) => v,
        }
    }
}

#[derive(Deserialize, Default)]
struct RawKeys {
    #[serde(default)]
    default: Vec<String>,
    #[serde(default)]
    other: Vec<RawKeyEntry>,
}

#[derive(Deserialize)]
struct RawKeyEntry {
    sound: OneOrMany,
    #[serde(default)]
    keys: Vec<String>,
}

/// 扫描结果：只读了元信息，没解码音频。
#[derive(Clone)]
pub struct ProfileMeta {
    pub dir: PathBuf,
    pub name: String,
    pub author: Option<String>,
    pub description: Option<String>,
    pub device: Device,
}

/// 扫描音色包目录，返回所有含 `profile.yaml` 的子目录。
pub fn scan(profiles_dir: &Path) -> Vec<ProfileMeta> {
    let Ok(entries) = std::fs::read_dir(profiles_dir) else {
        return Vec::new();
    };

    let mut found = Vec::new();

    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let yaml = dir.join("profile.yaml");
        let Ok(text) = std::fs::read_to_string(&yaml) else {
            continue;
        };
        let Ok(raw) = serde_yaml_ng::from_str::<RawProfile>(&text) else {
            continue;
        };

        found.push(ProfileMeta {
            dir,
            name: raw.profile.name,
            author: raw.profile.author,
            description: raw.profile.description,
            device: resolve_device(&raw.profile.device, &raw.buttons, &raw.keys),
        });
    }

    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

/// 没写 `device` 的老包，按「有没有 buttons 段」推断。
fn resolve_device(explicit: &Option<String>, buttons: &Option<Value>, keys: &RawKeys) -> Device {
    if let Some(device) = explicit.as_deref().and_then(Device::parse) {
        return device;
    }

    let has_buttons = matches!(buttons, Some(Value::Mapping(m)) if !m.is_empty());
    let has_keys = !keys.default.is_empty() || !keys.other.is_empty();

    if has_buttons && !has_keys {
        Device::Mouse
    } else {
        Device::Keyboard
    }
}

/// 归一化目标峰值。留一点余量避免削顶。
const TARGET_PEAK: f32 = 0.95;
/// 增益上限，防止把底噪一起放大到刺耳。
const MAX_GAIN: f32 = 16.0;

/// 还没定增益的中间态。
struct PendingSource {
    press: Vec<DecodedAudio>,
    release: Vec<DecodedAudio>,
}

/// 加载并解码整个音色包。返回的 warnings 用于在 UI 上提示坏文件。
pub fn load(dir: &Path) -> Result<(CompiledProfile, Vec<String>), String> {
    let yaml_path = dir.join("profile.yaml");
    let text = std::fs::read_to_string(&yaml_path)
        .map_err(|e| format!("读不到 {}: {e}", yaml_path.display()))?;
    let raw: RawProfile = serde_yaml_ng::from_str(&text)
        .map_err(|e| format!("{} 格式错误: {e}", yaml_path.display()))?;

    let mut warnings = Vec::new();
    let mut pending: HashMap<String, PendingSource> = HashMap::new();

    for src in raw.sources {
        let (press_files, release_files) = match src.source {
            RawSourceSpec::Single(press) => (vec![press], Vec::new()),
            RawSourceSpec::Config { press, release } => (
                press.map(OneOrMany::into_vec).unwrap_or_default(),
                release.map(OneOrMany::into_vec).unwrap_or_default(),
            ),
        };

        let press = decode_all(dir, &press_files, &mut warnings);
        let release = decode_all(dir, &release_files, &mut warnings);

        if press.is_empty() && release.is_empty() {
            warnings.push(format!("音源 {} 没有可用音频，已跳过", src.id));
            continue;
        }

        pending.insert(src.id, PendingSource { press, release });
    }

    // 整个包统一算一个增益：保住包内各键的相对强弱（空格就是该比字母键响），
    // 但消除不同包之间的录音电平差异。
    let peak = pending
        .values()
        .flat_map(|s| s.press.iter().chain(s.release.iter()))
        .fold(0.0f32, |max, audio| max.max(audio.peak()));

    let gain = if peak > 1e-6 {
        (TARGET_PEAK / peak).clamp(0.05, MAX_GAIN)
    } else {
        1.0
    };

    let sources: HashMap<String, SourceAudio> = pending
        .into_iter()
        .map(|(id, s)| {
            (
                id,
                SourceAudio {
                    press: s.press.into_iter().map(|a| a.into_buffer(gain)).collect(),
                    release: s.release.into_iter().map(|a| a.into_buffer(gain)).collect(),
                },
            )
        })
        .collect();

    // 键盘的 keys 和鼠标的 buttons 在运行时是同一张「名字 → 音源」表。
    let mut key_map: HashMap<String, Vec<String>> = HashMap::new();

    for entry in raw.keys.other {
        let ids = entry.sound.clone().into_vec();
        for key in entry.keys {
            key_map.insert(key.to_lowercase(), ids.clone());
        }
    }

    let mut default_sources = raw.keys.default;

    if let Some(buttons) = &raw.buttons {
        parse_buttons(buttons, &mut key_map, &mut default_sources, &mut warnings);
    }

    // 兜底：既没有按键映射也没有默认音源的键盘包（例如原版的 mx-blue），
    // 让所有键在全部音源里随机，总比一片静音好。
    if key_map.is_empty() && default_sources.is_empty() {
        let mut ids: Vec<String> = sources.keys().cloned().collect();
        ids.sort();
        if !ids.is_empty() {
            warnings.push(format!("这个包没有键位映射，已用全部 {} 个音源兜底", ids.len()));
            default_sources = ids;
        }
    }

    let profile = CompiledProfile {
        name: raw.profile.name,
        key_map,
        default_sources,
        sources,
        peak,
        gain,
    };

    Ok((profile, warnings))
}

/// 解析 `buttons` 段，同时兼容结构化写法和 MechVibes 数字映射写法。
fn parse_buttons(
    value: &Value,
    key_map: &mut HashMap<String, Vec<String>>,
    default_sources: &mut Vec<String>,
    warnings: &mut Vec<String>,
) {
    let Value::Mapping(map) = value else {
        warnings.push("buttons 段格式无法识别，已忽略".into());
        return;
    };

    for (key, val) in map {
        let Some(key) = key.as_str() else {
            continue;
        };

        match key {
            "default" => {
                if let Some(ids) = value_to_ids(val) {
                    default_sources.extend(ids);
                }
            }
            "other" => {
                let Value::Sequence(entries) = val else {
                    continue;
                };
                for entry in entries {
                    let Some(entry) = entry.as_mapping() else {
                        continue;
                    };
                    let sound = entry
                        .get("sound")
                        .and_then(value_to_ids)
                        .unwrap_or_default();
                    if sound.is_empty() {
                        continue;
                    }
                    let Some(Value::Sequence(names)) = entry.get("buttons") else {
                        continue;
                    };
                    for name in names {
                        if let Some(name) = name.as_str() {
                            key_map.insert(name.to_lowercase(), sound.clone());
                        }
                    }
                }
            }
            // 数字键：MechVibes 约定 1=左 2=中 3=右
            _ => {
                if let Some(button) = numeric_button_name(key) {
                    if let Some(ids) = value_to_ids(val) {
                        key_map.insert(button.to_string(), ids);
                    }
                }
            }
        }
    }
}

fn numeric_button_name(key: &str) -> Option<&'static str> {
    match key {
        "1" => Some("left"),
        "2" => Some("middle"),
        "3" => Some("right"),
        _ => None,
    }
}

/// YAML 值 → 音源 id 列表（支持单个字符串或字符串列表）。
fn value_to_ids(value: &Value) -> Option<Vec<String>> {
    match value {
        Value::String(s) => Some(vec![s.clone()]),
        Value::Sequence(seq) => Some(
            seq.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect(),
        ),
        _ => None,
    }
}

fn decode_all(dir: &Path, files: &[String], warnings: &mut Vec<String>) -> Vec<DecodedAudio> {
    files
        .iter()
        .filter_map(|name| {
            let path = if Path::new(name).is_absolute() {
                PathBuf::from(name)
            } else {
                dir.join(name)
            };
            match decode_file(&path) {
                Ok(buf) => Some(buf),
                Err(e) => {
                    warnings.push(e);
                    None
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_reference_format() {
        let yaml = r#"
profile:
  name: mx-brown
  author: kbsim
sources:
  - id: key1
    source:
      press: press_key1.mp3
      release: release_key.mp3
keys:
  default: [ key1 ]
  other:
    - sound: space
      keys: [ space ]
"#;
        let raw: RawProfile = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(raw.profile.name, "mx-brown");
        assert_eq!(raw.keys.default, vec!["key1"]);
        assert_eq!(raw.keys.other[0].keys, vec!["space"]);
        assert_eq!(
            resolve_device(&raw.profile.device, &raw.buttons, &raw.keys),
            Device::Keyboard
        );
    }

    #[test]
    fn parses_structured_buttons() {
        let yaml = r#"
profile:
  name: mouse-a
buttons:
  default: click
  other:
    - sound: right_click
      buttons: [ right ]
"#;
        let raw: RawProfile = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(
            resolve_device(&raw.profile.device, &raw.buttons, &raw.keys),
            Device::Mouse
        );

        let mut key_map = HashMap::new();
        let mut defaults = Vec::new();
        let mut warnings = Vec::new();
        parse_buttons(raw.buttons.as_ref().unwrap(), &mut key_map, &mut defaults, &mut warnings);

        assert_eq!(defaults, vec!["click"]);
        assert_eq!(key_map.get("right").unwrap(), &vec!["right_click".to_string()]);
    }

    #[test]
    fn parses_mechvibes_numeric_buttons() {
        // 原版 7 个鼠标包的真实写法
        let yaml = r#"
profile:
  name: logitech-g502
  device: mouse
buttons:
  '1': click_left
  '2': click_middle
  '3': click_right
  default: click_left
  other: []
"#;
        let raw: RawProfile = serde_yaml_ng::from_str(yaml).unwrap();
        let mut key_map = HashMap::new();
        let mut defaults = Vec::new();
        let mut warnings = Vec::new();
        parse_buttons(raw.buttons.as_ref().unwrap(), &mut key_map, &mut defaults, &mut warnings);

        assert_eq!(defaults, vec!["click_left"]);
        assert_eq!(key_map.get("left").unwrap(), &vec!["click_left".to_string()]);
        assert_eq!(key_map.get("middle").unwrap(), &vec!["click_middle".to_string()]);
        assert_eq!(key_map.get("right").unwrap(), &vec!["click_right".to_string()]);
    }

    #[test]
    fn accepts_variant_lists() {
        let yaml = r#"
profile:
  name: multi
sources:
  - id: k
    source:
      press: [a.wav, b.wav]
keys:
  default: [k]
"#;
        let raw: RawProfile = serde_yaml_ng::from_str(yaml).unwrap();
        let RawSourceSpec::Config { press: Some(p), .. } = &raw.sources[0].source else {
            panic!("expected config form");
        };
        assert_eq!(p.clone().into_vec().len(), 2);
    }
}
