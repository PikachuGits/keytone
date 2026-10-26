//! 设置持久化。存在 app_data_dir/settings.json。

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_volume")]
    pub volume: f32,
    #[serde(default)]
    pub keyboard_profile: Option<String>,
    #[serde(default)]
    pub mouse_profile: Option<String>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub voice_enabled: bool,
    #[serde(default = "default_lang")]
    pub voice_lang: String,
    /// 更新检查用的 GitHub 仓库（owner/repo），留空表示不检查。
    #[serde(default = "default_update_repo")]
    pub update_repo: String,
    /// 主题：auto / light / dark
    #[serde(default = "default_theme")]
    pub theme: String,
    /// 菜单栏图标：gold / mono
    #[serde(default = "default_tray")]
    pub tray_style: String,
}

fn default_volume() -> f32 {
    0.8
}

fn default_true() -> bool {
    true
}

fn default_lang() -> String {
    "zh".into()
}

fn default_theme() -> String {
    "auto".into()
}

fn default_tray() -> String {
    "mono".into()
}

/// 默认更新源。留空会让「检查更新」永远提示未配置，所以内置官方仓库。
fn default_update_repo() -> String {
    "PikachuGits/keytone".into()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            volume: default_volume(),
            keyboard_profile: None,
            mouse_profile: None,
            enabled: false,
            voice_enabled: default_true(),
            voice_lang: default_lang(),
            update_repo: default_update_repo(),
            theme: default_theme(),
            tray_style: default_tray(),
        }
    }
}

pub fn load(data_dir: &Path) -> Settings {
    let path = data_dir.join("settings.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(data_dir: &Path, settings: &Settings) {
    let _ = std::fs::create_dir_all(data_dir);
    let path = data_dir.join("settings.json");
    if let Ok(text) = serde_json::to_string_pretty(settings) {
        let _ = std::fs::write(path, text);
    }
}
