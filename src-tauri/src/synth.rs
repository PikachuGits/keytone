//! 首次启动时生成内置音色包（键盘 + 鼠标各一个）。
//!
//! 目的不是「好听」，而是让整条链路可验证：**真的写出 wav 文件 → 真的从磁盘解码
//! → 真的按音色包规则播放**。顺带演示了「每个键位不同音色」这个原版没有的点。

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

const SAMPLE_RATE: u32 = 48_000;

/// 键盘分组：不同分组用不同的基频，听起来就像不同位置的键。
struct KeyGroup {
    id: &'static str,
    freq: f32,
    decay: f32,
    gain: f32,
}

const KEY_GROUPS: &[KeyGroup] = &[
    KeyGroup { id: "top", freq: 238.0, decay: 0.018, gain: 0.55 },
    KeyGroup { id: "home", freq: 196.0, decay: 0.021, gain: 0.60 },
    KeyGroup { id: "bottom", freq: 165.0, decay: 0.024, gain: 0.62 },
    KeyGroup { id: "space", freq: 118.0, decay: 0.045, gain: 0.70 },
    KeyGroup { id: "enter", freq: 142.0, decay: 0.038, gain: 0.68 },
    KeyGroup { id: "nav", freq: 180.0, decay: 0.026, gain: 0.58 },
];

/// 鼠标按键：比键盘短促得多，靠高频共振区分。
struct ButtonGroup {
    id: &'static str,
    freq: f32,
    decay: f32,
    duration: f32,
}

const BUTTON_GROUPS: &[ButtonGroup] = &[
    ButtonGroup { id: "left", freq: 1450.0, decay: 0.0060, duration: 0.030 },
    ButtonGroup { id: "right", freq: 1020.0, decay: 0.0085, duration: 0.036 },
    ButtonGroup { id: "middle", freq: 1780.0, decay: 0.0048, duration: 0.026 },
];

const TOP_KEYS: &[&str] = &[
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9",
    "Q", "W", "E", "R", "T", "Y", "U", "I", "O", "P",
    "[", "]", "-", "+", "\\", "`",
];
const HOME_KEYS: &[&str] = &["A", "S", "D", "F", "G", "H", "J", "K", "L", ";", "'"];
const BOTTOM_KEYS: &[&str] = &["Z", "X", "C", "V", "B", "N", "M", ",", ".", "/"];
const NAV_KEYS: &[&str] = &["Backspace", "Delete", "Tab", "Escape"];

/// 补齐内置音色包（键盘 / 鼠标各一个）。返回是否实际生成了新包。
///
/// 按包判断而不是「有任意包就跳过」——否则老版本只生成过键盘包的用户，
/// 升级后永远拿不到鼠标包。
pub fn ensure_demo_profiles(profiles_dir: &Path) -> Result<bool, String> {
    std::fs::create_dir_all(profiles_dir).map_err(|e| format!("创建音色包目录失败: {e}"))?;

    let mut created = false;

    let keyboard_dir = profiles_dir.join("demo-synth");
    if !keyboard_dir.join("profile.yaml").exists() {
        write_keyboard_profile(&keyboard_dir)?;
        created = true;
    }

    let mouse_dir = profiles_dir.join("demo-synth-mouse");
    if !mouse_dir.join("profile.yaml").exists() {
        write_mouse_profile(&mouse_dir)?;
        created = true;
    }

    Ok(created)
}

fn write_keyboard_profile(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("创建目录失败: {e}"))?;

    for group in KEY_GROUPS {
        // 3 个按下变体 + 2 个抬起变体，播放时随机挑，避免一听就是循环。
        for i in 0..3 {
            let samples = render_click(group.freq, group.decay, 0.55, 0x1000 + i * 977, 0.075, group.gain);
            write_wav(&dir.join(format!("{}_press{}.wav", group.id, i + 1)), &samples)?;
        }
        for i in 0..2 {
            let samples = render_click(
                group.freq * 0.72,
                group.decay * 0.55,
                0.35,
                0x9000 + i * 613,
                0.045,
                group.gain * 0.55,
            );
            write_wav(&dir.join(format!("{}_release{}.wav", group.id, i + 1)), &samples)?;
        }
    }

    std::fs::write(dir.join("profile.yaml"), keyboard_yaml())
        .map_err(|e| format!("写入 profile.yaml 失败: {e}"))?;

    Ok(())
}

fn write_mouse_profile(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("创建目录失败: {e}"))?;

    for group in BUTTON_GROUPS {
        for i in 0..3 {
            // 鼠标只做按下音：真实鼠标一次点击就是一个咔哒，抬起再来一下会很怪。
            let samples = render_click(
                group.freq,
                group.decay,
                0.72,
                0x4400 + i * 787,
                group.duration,
                0.62,
            );
            write_wav(&dir.join(format!("{}_press{}.wav", group.id, i + 1)), &samples)?;
        }
    }

    std::fs::write(dir.join("profile.yaml"), mouse_yaml())
        .map_err(|e| format!("写入 profile.yaml 失败: {e}"))?;

    Ok(())
}

fn keyboard_yaml() -> String {
    let mut yaml = String::new();

    yaml.push_str("profile:\n");
    yaml.push_str("  name: Demo Synth\n");
    yaml.push_str("  author: KeyTone\n");
    yaml.push_str("  description: 内置合成音色包（键盘）\n");
    yaml.push_str("  device: keyboard\n\n");

    yaml.push_str("sources:\n");
    for g in KEY_GROUPS {
        yaml.push_str(&format!("  - id: {}\n", g.id));
        yaml.push_str("    source:\n");
        yaml.push_str(&format!(
            "      press: [ {} ]\n",
            (1..=3)
                .map(|i| format!("{}_press{i}.wav", g.id))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        yaml.push_str(&format!(
            "      release: [ {} ]\n",
            (1..=2)
                .map(|i| format!("{}_release{i}.wav", g.id))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    yaml.push_str("\nkeys:\n");
    yaml.push_str("  default: [ home ]\n");
    yaml.push_str("  other:\n");
    for (id, keys) in [
        ("top", TOP_KEYS),
        ("home", HOME_KEYS),
        ("bottom", BOTTOM_KEYS),
        ("nav", NAV_KEYS),
    ] {
        yaml.push_str(&format!("    - sound: {id}\n"));
        yaml.push_str(&format!("      keys: [ {} ]\n", quote_all(keys)));
    }
    yaml.push_str("    - sound: space\n");
    yaml.push_str("      keys: [ Space ]\n");
    yaml.push_str("    - sound: enter\n");
    yaml.push_str("      keys: [ Enter ]\n");

    yaml
}

fn mouse_yaml() -> String {
    let mut yaml = String::new();

    yaml.push_str("profile:\n");
    yaml.push_str("  name: Demo Synth Mouse\n");
    yaml.push_str("  author: KeyTone\n");
    yaml.push_str("  description: 内置合成音色包（鼠标）\n");
    yaml.push_str("  device: mouse\n\n");

    yaml.push_str("sources:\n");
    for g in BUTTON_GROUPS {
        yaml.push_str(&format!("  - id: {}\n", g.id));
        yaml.push_str("    source:\n");
        yaml.push_str(&format!(
            "      press: [ {} ]\n",
            (1..=3)
                .map(|i| format!("{}_press{i}.wav", g.id))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    yaml.push_str("\nbuttons:\n");
    yaml.push_str("  default: left\n");
    yaml.push_str("  other:\n");
    yaml.push_str("    - sound: right\n");
    yaml.push_str("      buttons: [ right ]\n");
    yaml.push_str("    - sound: middle\n");
    yaml.push_str("      buttons: [ middle, back, forward ]\n");

    yaml
}

fn quote_all(keys: &[&str]) -> String {
    keys.iter()
        .map(|k| format!("\"{}\"", k.replace('\\', "\\\\").replace('"', "\\\"")))
        .collect::<Vec<_>>()
        .join(", ")
}

/// 合成一次咔哒声：噪声瞬态（撞击）+ 正弦体（腔体共振）。
fn render_click(
    freq: f32,
    decay: f32,
    noise_amount: f32,
    seed: u32,
    duration: f32,
    gain: f32,
) -> Vec<f32> {
    let frames = (SAMPLE_RATE as f32 * duration) as usize;
    let mut out = Vec::with_capacity(frames);
    let mut rng = seed | 1;

    for i in 0..frames {
        let t = i as f32 / SAMPLE_RATE as f32;

        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        let noise = (rng as f32 / u32::MAX as f32) * 2.0 - 1.0;

        let transient = (-t / 0.0035).exp();
        let body = (std::f32::consts::TAU * freq * t).sin() * (-t / decay).exp();

        let s = (noise * transient * noise_amount + body * (1.0 - noise_amount)) * gain;
        out.push(s.clamp(-1.0, 1.0));
    }

    out
}

fn write_wav(path: &Path, samples: &[f32]) -> Result<(), String> {
    let file = File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut w = BufWriter::new(file);

    let data_len = (samples.len() * 2) as u32;
    let write = |w: &mut BufWriter<File>| -> std::io::Result<()> {
        w.write_all(b"RIFF")?;
        w.write_all(&(36 + data_len).to_le_bytes())?;
        w.write_all(b"WAVE")?;
        w.write_all(b"fmt ")?;
        w.write_all(&16u32.to_le_bytes())?;
        w.write_all(&1u16.to_le_bytes())?; // PCM
        w.write_all(&1u16.to_le_bytes())?; // mono
        w.write_all(&SAMPLE_RATE.to_le_bytes())?;
        w.write_all(&(SAMPLE_RATE * 2).to_le_bytes())?;
        w.write_all(&2u16.to_le_bytes())?;
        w.write_all(&16u16.to_le_bytes())?;
        w.write_all(b"data")?;
        w.write_all(&data_len.to_le_bytes())?;
        for s in samples {
            w.write_all(&((s * 32767.0) as i16).to_le_bytes())?;
        }
        Ok(())
    };

    write(&mut w).map_err(|e| format!("{}: {e}", path.display()))?;
    w.flush().map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(())
}
