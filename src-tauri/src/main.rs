#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod bundled;
mod config;
mod hook;
mod keymap;
mod muyu;
mod profile;
mod speech;
mod synth;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, State, Theme, WebviewUrl, WebviewWindowBuilder, Wry};

use audio::AudioHandle;
use config::Settings;
use hook::Device;
use profile::ProfileMeta;
use speech::Lang;

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Serialize)]
struct Active {
    name: String,
    sources: usize,
    samples: usize,
}

pub struct AppState {
    audio: OnceLock<AudioHandle>,
    enabled: AtomicBool,
    hook_started: AtomicBool,
    data_dir: PathBuf,
    profiles_dir: PathBuf,
    profiles: Mutex<Vec<ProfileMeta>>,
    active_keyboard: Mutex<Option<Active>>,
    active_mouse: Mutex<Option<Active>>,
    volume: Mutex<f32>,
    voice_enabled: AtomicBool,
    voice_lang: Mutex<Lang>,
    update_repo: Mutex<String>,
    theme: Mutex<String>,
    tray_style: Mutex<String>,
    tray_toggle: Mutex<Option<CheckMenuItem<Wry>>>,
    /// 木鱼：库存、节拍、功德
    muyu: Arc<muyu::Muyu>,
    muyu_toggle: Mutex<Option<CheckMenuItem<Wry>>>,
}

impl AppState {
    fn audio(&self) -> Option<&AudioHandle> {
        self.audio.get()
    }

    fn slot(&self, device: Device) -> &Mutex<Option<Active>> {
        match device {
            Device::Keyboard => &self.active_keyboard,
            Device::Mouse => &self.active_mouse,
        }
    }

    /// 加载音色包并推给音频线程。
    fn activate(&self, meta: &ProfileMeta) -> Result<Active, String> {
        let (compiled, warnings) = profile::load(&meta.dir)?;
        for warning in &warnings {
            eprintln!("[profile] {warning}");
        }

        let active = Active {
            name: compiled.name.clone(),
            sources: compiled.source_count(),
            samples: compiled.sample_count(),
        };

        if let Some(audio) = self.audio() {
            audio.set_profile(meta.device, Arc::new(compiled));
        }
        *self.slot(meta.device).lock().unwrap() = Some(active.clone());

        eprintln!(
            "[profile] {} → {} ({} 音源 / {} 采样)",
            meta.device.as_str(),
            active.name,
            active.sources,
            active.samples
        );

        Ok(active)
    }

    fn persist(&self) {
        let settings = Settings {
            volume: *self.volume.lock().unwrap(),
            keyboard_profile: self.active_keyboard.lock().unwrap().as_ref().map(|a| a.name.clone()),
            mouse_profile: self.active_mouse.lock().unwrap().as_ref().map(|a| a.name.clone()),
            enabled: self.enabled.load(Ordering::Relaxed),
            voice_enabled: self.voice_enabled.load(Ordering::Relaxed),
            voice_lang: self.voice_lang.lock().unwrap().as_str().to_string(),
            update_repo: self.update_repo.lock().unwrap().clone(),
            theme: self.theme.lock().unwrap().clone(),
            tray_style: self.tray_style.lock().unwrap().clone(),
            muyu_enabled: self.muyu.enabled(),
            muyu_sound: self.muyu.sound(),
            muyu_volume: self.muyu.volume(),
            muyu_interval_ms: self.muyu.interval_ms(),
            muyu_window: self.muyu.window_visible(),
            muyu_scale: self.muyu.scale(),
            muyu_x: self.muyu.pos().map(|p| p.0),
            muyu_y: self.muyu.pos().map(|p| p.1),
        };
        config::save(&self.data_dir, &settings);
    }

    fn sync_tray(&self, enabled: bool) {
        if let Some(item) = self.tray_toggle.lock().unwrap().as_ref() {
            let _ = item.set_checked(enabled);
        }
    }

    fn sync_muyu_tray(&self, enabled: bool) {
        if let Some(item) = self.muyu_toggle.lock().unwrap().as_ref() {
            let _ = item.set_checked(enabled);
        }
    }
}


#[derive(Serialize)]
struct Status {
    platform: &'static str,
    hook_backend: &'static str,
    trusted: bool,
    audio_ready: bool,
    sample_rate: u32,
    enabled: bool,
    volume: f32,
    keyboard: Option<Active>,
    mouse: Option<Active>,
    voice_enabled: bool,
    voice_lang: &'static str,
    /// 当前平台是否提供语音播报。Windows / Linux 尚未实现，界面据此隐藏入口。
    voice_available: bool,
    update_repo: String,
    theme: String,
    tray_style: String,
    version: &'static str,
}

#[derive(Serialize)]
struct ProfileInfo {
    name: String,
    author: Option<String>,
    description: Option<String>,
    device: &'static str,
    active: bool,
}

#[derive(Serialize)]
struct VoiceState {
    enabled: bool,
    lang: &'static str,
}

#[derive(Serialize)]
struct UpdateInfo {
    current: &'static str,
    latest: Option<String>,
    has_update: bool,
    url: Option<String>,
    /// 当前平台对应安装包的下载地址（dmg / exe / msi）
    asset_url: Option<String>,
    asset_name: Option<String>,
    error: Option<String>,
}

#[tauri::command]
fn get_status(state: State<'_, AppState>) -> Status {
    Status {
        platform: std::env::consts::OS,
        hook_backend: hook::backend_name(),
        trusted: hook::is_trusted(),
        audio_ready: state.audio().is_some(),
        sample_rate: state.audio().map(|a| a.sample_rate()).unwrap_or(0),
        enabled: state.enabled.load(Ordering::Relaxed),
        volume: *state.volume.lock().unwrap(),
        keyboard: state.active_keyboard.lock().unwrap().clone(),
        mouse: state.active_mouse.lock().unwrap().clone(),
        voice_enabled: state.voice_enabled.load(Ordering::Relaxed),
        voice_lang: state.voice_lang.lock().unwrap().as_str(),
        voice_available: speech::available(),
        update_repo: state.update_repo.lock().unwrap().clone(),
        theme: state.theme.lock().unwrap().clone(),
        tray_style: state.tray_style.lock().unwrap().clone(),
        version: APP_VERSION,
    }
}

#[tauri::command]
fn list_profiles(state: State<'_, AppState>, device: String) -> Vec<ProfileInfo> {
    let Some(device) = Device::parse(&device) else {
        return Vec::new();
    };
    let active = state.slot(device).lock().unwrap().clone();

    state
        .profiles
        .lock()
        .unwrap()
        .iter()
        .filter(|meta| meta.device == device)
        .map(|meta| ProfileInfo {
            name: meta.name.clone(),
            author: meta.author.clone(),
            description: meta.description.clone(),
            device: meta.device.as_str(),
            active: active.as_ref().is_some_and(|a| a.name == meta.name),
        })
        .collect()
}

#[tauri::command]
fn select_profile(state: State<'_, AppState>, device: String, name: String) -> Result<Active, String> {
    let device = Device::parse(&device).ok_or("未知设备类型")?;

    let meta = state
        .profiles
        .lock()
        .unwrap()
        .iter()
        .find(|m| m.device == device && m.name == name)
        .cloned()
        .ok_or_else(|| format!("找不到音色包: {name}"))?;

    let active = state.activate(&meta)?;
    state.persist();
    Ok(active)
}

#[tauri::command]
fn set_volume(state: State<'_, AppState>, volume: f32) -> f32 {
    let volume = volume.clamp(0.0, 1.0);
    *state.volume.lock().unwrap() = volume;
    if let Some(audio) = state.audio() {
        audio.set_volume(volume);
    }
    state.persist();
    volume
}

#[tauri::command]
fn set_voice(
    state: State<'_, AppState>,
    enabled: Option<bool>,
    lang: Option<String>,
) -> VoiceState {
    if let Some(enabled) = enabled {
        state.voice_enabled.store(enabled, Ordering::SeqCst);
    }
    if let Some(lang) = lang.and_then(|l| Lang::parse(&l)) {
        *state.voice_lang.lock().unwrap() = lang;
    }
    state.persist();

    VoiceState {
        enabled: state.voice_enabled.load(Ordering::Relaxed),
        lang: state.voice_lang.lock().unwrap().as_str(),
    }
}

/// 试读：播一段示例，让用户确认语音效果和语言。
#[tauri::command]
fn speak_sample(app: tauri::AppHandle, lang: String) {
    if !speech::available() {
        return;
    }
    let lang = Lang::parse(&lang).unwrap_or(Lang::Zh);
    let text = match lang {
        Lang::Zh => "啊，波，次，左键，右键",
        Lang::En => "a, b, c, left, right",
    };
    let _ = app.run_on_main_thread(move || speech::speak(text, lang, 1.0));
}

/// 两种菜单栏图标，均由 assets/speed-mark.svg 渲染而来（36px = 18pt @2x）：
/// - `gold`：品牌金线稿，原样显示，颜色固定
/// - `mono`：同一几何的黑色图，只取 alpha，系统按菜单栏明暗自动着色
fn tray_icon(style: &str) -> Option<tauri::image::Image<'static>> {
    let bytes: &'static [u8] = if style == "mono" {
        include_bytes!("../icons/tray-mark.png")
    } else {
        include_bytes!("../icons/tray-mark-gold.png")
    };
    tauri::image::Image::from_bytes(bytes).ok()
}

fn apply_tray_style(app: &tauri::AppHandle, style: &str) {
    let Some(tray) = app.tray_by_id("main") else {
        return;
    };
    if let Some(icon) = tray_icon(style) {
        // 模板图只取 alpha，系统按菜单栏明暗自动着色；
        // 彩色图则原样显示，不跟随系统。
        let _ = tray.set_icon_with_as_template(Some(icon), style == "mono");
    }
}

/// 切换菜单栏图标样式：gold（金色线稿）/ mono（单色模板）。
#[tauri::command]
fn set_tray_style(app: tauri::AppHandle, state: State<'_, AppState>, style: String) -> String {
    // 旧版本存的是 "matrix"，一并当作 gold 处理，避免升级后样式丢失。
    let style = if style == "mono" { "mono" } else { "gold" }.to_string();
    *state.tray_style.lock().unwrap() = style.clone();
    state.persist();
    apply_tray_style(&app, &style);
    style
}

/// 切换主题。同时把原生窗口外观改掉，否则浅色主题下窗口边框还是深的。
#[tauri::command]
fn set_theme(app: tauri::AppHandle, state: State<'_, AppState>, theme: String) -> String {
    let theme = match theme.as_str() {
        "light" | "dark" => theme,
        _ => "auto".to_string(),
    };
    *state.theme.lock().unwrap() = theme.clone();
    state.persist();
    apply_window_theme(&app, &theme);
    theme
}

fn apply_window_theme(app: &tauri::AppHandle, theme: &str) {
    let native = match theme {
        "light" => Some(Theme::Light),
        "dark" => Some(Theme::Dark),
        _ => None,
    };
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_theme(native);
    }
}

#[tauri::command]
fn set_update_repo(state: State<'_, AppState>, repo: String) -> String {
    let repo = repo.trim().to_string();
    *state.update_repo.lock().unwrap() = repo.clone();
    state.persist();
    repo
}

/// 在文件管理器里打开一个目录。三个平台各有各的命令。
fn open_folder(dir: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    const CMD: &str = "explorer";
    #[cfg(target_os = "macos")]
    const CMD: &str = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    const CMD: &str = "xdg-open";

    std::process::Command::new(CMD)
        .arg(dir)
        .spawn()
        .map_err(|e| format!("打开目录失败: {e}"))?;
    Ok(())
}

/// 构造 curl 命令。
///
/// Windows 上必须带 `CREATE_NO_WINDOW`，否则每次检查更新都会闪一个黑框；
/// 另外把「curl 不存在」和「网络失败」区分开，不然用户只看到一句「网络请求失败」，
/// 没法判断是自己没网还是系统里没有 curl。
fn curl() -> std::process::Command {
    #[allow(unused_mut)]
    let mut cmd = std::process::Command::new("curl");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// 检查更新。用 curl 而不是引 reqwest —— 少一个重依赖，macOS / Windows 都自带。
#[tauri::command]
fn check_update(state: State<'_, AppState>) -> UpdateInfo {
    let repo = state.update_repo.lock().unwrap().clone();

    if repo.trim().is_empty() {
        return UpdateInfo {
            current: APP_VERSION,
            latest: None,
            has_update: false,
            url: None,
            asset_url: None,
            asset_name: None,
            error: Some("未配置更新源".into()),
        };
    }

    let api = format!("https://api.github.com/repos/{repo}/releases/latest");
    let output = curl()
        .args(["-sL", "--max-time", "8", "-H", "User-Agent: keytone"])
        .arg(&api)
        .output();

    let body = match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).to_string(),
        Ok(out) => {
            return UpdateInfo {
                current: APP_VERSION,
                latest: None,
                has_update: false,
                url: None,
                asset_url: None,
                asset_name: None,
                error: Some(format!(
                    "网络请求失败（curl 退出码 {:?}）",
                    out.status.code()
                )),
            }
        }
        Err(e) => {
            return UpdateInfo {
                current: APP_VERSION,
                latest: None,
                has_update: false,
                url: None,
                asset_url: None,
                asset_name: None,
                error: Some(format!("无法执行 curl：{e}")),
            }
        }
    };

    let json: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => {
            return UpdateInfo {
                current: APP_VERSION,
                latest: None,
                has_update: false,
                url: None,
                asset_url: None,
                asset_name: None,
                error: Some("返回内容无法解析".into()),
            }
        }
    };

    let tag = json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .map(|s| s.trim_start_matches('v').to_string());

    let Some(tag) = tag else {
        let msg = json
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("没有找到发布版本");
        return UpdateInfo {
            current: APP_VERSION,
            latest: None,
            has_update: false,
            url: None,
            asset_url: None,
            asset_name: None,
            error: Some(msg.to_string()),
        };
    };

    let url = json
        .get("html_url")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let (asset_url, asset_name) = pick_asset(&json);

    UpdateInfo {
        current: APP_VERSION,
        has_update: version_is_newer(&tag, APP_VERSION),
        latest: Some(tag),
        url,
        asset_url,
        asset_name,
        error: None,
    }
}

/// 从 release 的 assets 里挑当前平台能装的包。
///
/// 一个 release 会挂多个平台的产物，这里按后缀挑：macOS 认 `.dmg`，
/// Windows 优先 `-setup.exe`（NSIS）再退 `.msi`，Linux 认 AppImage / deb。
fn pick_asset(json: &serde_json::Value) -> (Option<String>, Option<String>) {
    let Some(assets) = json.get("assets").and_then(|v| v.as_array()) else {
        return (None, None);
    };

    let suffixes: &[&str] = if cfg!(target_os = "macos") {
        &[".dmg"]
    } else if cfg!(target_os = "windows") {
        &["-setup.exe", ".msi"]
    } else {
        &[".AppImage", ".deb"]
    };

    for suffix in suffixes {
        for asset in assets {
            let name = asset.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if !name.ends_with(suffix) {
                continue;
            }
            if let Some(url) = asset.get("browser_download_url").and_then(|v| v.as_str()) {
                return (Some(url.to_string()), Some(name.to_string()));
            }
        }
    }

    (None, None)
}

fn version_is_newer(latest: &str, current: &str) -> bool {
    let parse = |v: &str| -> Vec<u32> {
        v.split('.')
            .map(|p| {
                p.chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse()
                    .unwrap_or(0)
            })
            .collect()
    };

    let a = parse(latest);
    let b = parse(current);
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    false
}

/// 下载安装包到数据目录的 `updates/` 下，边下边把进度推给前端。
/// 已存在同名文件直接复用，避免重复下载。
#[tauri::command]
async fn download_update(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    url: String,
    name: String,
) -> Result<String, String> {
    if !url.starts_with("https://") {
        return Err("下载地址必须是 https".into());
    }
    // 文件名只放行安全字符，防止 ../ 穿越到目录外。
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err("安装包文件名非法".into());
    }

    let dir = state.data_dir.join("updates");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建下载目录失败: {e}"))?;
    let dest = dir.join(&name);

    // 上次下到一半就关了的话文件是不完整的，这里只认非空文件，
    // 但下载失败时会主动删掉，所以正常情况不会残留半截包。
    if dest.is_file() && std::fs::metadata(&dest).map(|m| m.len() > 0).unwrap_or(false) {
        return Ok(dest.to_string_lossy().to_string());
    }

    let total = remote_size(&url).unwrap_or(0);
    let dest_task = dest.clone();
    let app_task = app.clone();

    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let mut child = curl()
            .args(["-sL", "--fail", "--max-time", "600", "-H", "User-Agent: keytone", "-o"])
            .arg(&dest_task)
            .arg(&url)
            .spawn()
            .map_err(|e| format!("无法启动下载: {e}"))?;

        // curl 不会把进度写进 stdout，只能盯着文件大小轮询。
        loop {
            if let Ok(Some(status)) = child.try_wait() {
                if !status.success() {
                    let _ = std::fs::remove_file(&dest_task);
                    return Err("下载失败，请检查网络后重试".into());
                }
                break;
            }

            let downloaded = std::fs::metadata(&dest_task).map(|m| m.len()).unwrap_or(0);
            let _ = app_task.emit(
                "update-progress",
                serde_json::json!({ "downloaded": downloaded, "total": total }),
            );
            std::thread::sleep(std::time::Duration::from_millis(150));
        }

        let size = std::fs::metadata(&dest_task).map(|m| m.len()).unwrap_or(0);
        let _ = app_task.emit(
            "update-progress",
            serde_json::json!({ "downloaded": size, "total": size, "done": true }),
        );
        Ok(())
    })
    .await
    .map_err(|e| format!("下载任务异常: {e}"))??;

    Ok(dest.to_string_lossy().to_string())
}

/// 取远端文件大小，用来算下载百分比。拿不到就返回 None，界面显示已下载字节数。
fn remote_size(url: &str) -> Option<u64> {
    let out = curl()
        .args(["-sIL", "--max-time", "8", "-H", "User-Agent: keytone", url])
        .output()
        .ok()?;

    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let lower = line.to_ascii_lowercase();
            lower
                .strip_prefix("content-length:")
                .and_then(|v| v.trim().parse::<u64>().ok())
        })
        .last()
}

/// 安装已下载好的包，然后退出当前进程。
///
/// macOS 是「自己替换自己」：不能在自己还跑着的时候删掉自己的包，
/// 所以复制好新包之后写一个延迟脚本，退出进程由脚本完成替换并重新拉起。
#[tauri::command]
fn install_update(app: tauri::AppHandle, path: String) -> Result<(), String> {
    let file = std::path::PathBuf::from(&path);
    if !file.is_file() {
        return Err("安装包不存在，请重新下载".into());
    }

    #[cfg(target_os = "macos")]
    install_macos(&file)?;

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new(&file)
            .spawn()
            .map_err(|e| format!("无法启动安装程序: {e}"))?;
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    return Err("当前平台不支持自动安装，请手动下载".into());

    app.exit(0);
    #[allow(unreachable_code)]
    Ok(())
}

/// 本机是否装了指定名字的代码签名证书。
#[cfg(target_os = "macos")]
fn identity_available(name: &str) -> bool {
    std::process::Command::new("security")
        .args(["find-identity", "-v", "-p", "codesigning"])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).contains(name))
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn install_macos(dmg: &std::path::Path) -> Result<(), String> {    // 从当前可执行文件反推 .app 路径，这样开发时从 target 里跑也能正确替换。
    let exe = std::env::current_exe().map_err(|e| format!("无法定位当前程序: {e}"))?;
    let bundle = exe
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .filter(|p| p.extension().is_some_and(|e| e == "app"))
        .ok_or("当前不是以 .app 方式运行，无法自动替换")?;

    let mount = std::env::temp_dir().join("keytone-update-mnt");
    let _ = std::fs::remove_dir_all(&mount);
    std::fs::create_dir_all(&mount).map_err(|e| format!("创建挂载点失败: {e}"))?;

    let ok = std::process::Command::new("hdiutil")
        .args(["attach", "-nobrowse", "-readonly", "-mountpoint"])
        .arg(&mount)
        .arg(dmg)
        .status()
        .map_err(|e| format!("挂载安装包失败: {e}"))?
        .success();
    if !ok {
        let _ = std::fs::remove_dir_all(&mount);
        return Err("挂载安装包失败".into());
    }

    let src = mount.join("KeyTone.app");
    let stage_dir = std::env::temp_dir().join("keytone-update-stage");
    let _ = std::fs::remove_dir_all(&stage_dir);
    std::fs::create_dir_all(&stage_dir).map_err(|e| format!("创建暂存目录失败: {e}"))?;
    let staged = stage_dir.join("KeyTone.app");

    let copied = std::process::Command::new("ditto")
        .arg(&src)
        .arg(&staged)
        .status()
        .map_err(|e| format!("复制新版本失败: {e}"))?
        .success();

    let _ = std::process::Command::new("hdiutil")
        .arg("detach")
        .arg(&mount)
        .status();

    if !copied || !staged.is_dir() {
        return Err("从安装包里提取应用失败".into());
    }

    // CI 产出的包是 ad-hoc 签名，它的 designated requirement 绑在 cdhash 上，
    // 装上去之后辅助功能授权就失效了，而且每次更新都会失效一次。
    // 本机如果有自签名证书，就用同一张证书重签，requirement 继续绑证书，
    // 授权只需要勾一次，以后更新都不会掉。
    let identity = "KeyTone Local Signing";
    if identity_available(identity) {
        let resigned = std::process::Command::new("codesign")
            .args(["--force", "--deep", "--sign", identity, "--options", "runtime"])
            .arg(&staged)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !resigned {
            eprintln!("[update] 用本地证书重签失败，安装后可能需要重新授权辅助功能");
        }
    } else {
        eprintln!("[update] 本机没有签名证书，装上去的是 ad-hoc 包，辅助功能需要重新授权");
    }

    let script = std::env::temp_dir().join("keytone-apply-update.sh");
    let body = format!(
        "#!/bin/sh\n\
         # 等当前进程彻底退出，再替换包，否则会被占用。\n\
         sleep 2\n\
         rm -rf '{target}'\n\
         ditto '{src}' '{target}'\n\
         xattr -dr com.apple.quarantine '{target}' 2>/dev/null\n\
         open '{target}'\n\
         # 清理暂存目录和安装包：不清理的话临时目录里会留一份完整的应用副本，\n\
         # 既占空间，也可能被 Spotlight 之类当成第二个应用索引到。\n\
         rm -rf '{stage}'\n\
         rmdir '{mount}' 2>/dev/null\n\
         rm -f '{dmg}'\n\
         rm -f \"$0\"\n",
        target = bundle.display(),
        src = staged.display(),
        stage = stage_dir.display(),
        mount = mount.display(),
        dmg = dmg.display(),
    );
    std::fs::write(&script, body).map_err(|e| format!("写入更新脚本失败: {e}"))?;

    std::process::Command::new("/bin/sh")
        .arg(&script)
        .spawn()
        .map_err(|e| format!("启动更新脚本失败: {e}"))?;

    Ok(())
}

#[tauri::command]
fn open_url(url: String) {
    // 只放行 http(s)，避免被当成任意命令执行
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return;
    }

    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(&url).spawn();

    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", &url])
        .spawn();

    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
}

#[tauri::command]
fn open_accessibility_settings() {
    hook::open_settings();
}

#[tauri::command]
fn open_profiles_folder(state: State<'_, AppState>) {
    let _ = open_folder(&state.profiles_dir);
}

/// 试听当前音色包。直接走音频线程，不经过全局钩子，
/// 所以不需要辅助功能权限、也不会被「本应用在前台不发声」的逻辑挡掉。
#[tauri::command]
fn preview(state: State<'_, AppState>, device: String) -> Result<(), String> {
    let device = Device::parse(&device).ok_or_else(|| "未知设备类型".to_string())?;
    let audio = state
        .audio()
        .cloned()
        .ok_or_else(|| "audio-unavailable".to_string())?;

    if state.slot(device).lock().unwrap().is_none() {
        return Err("no-profile".into());
    }

    std::thread::spawn(move || play_preview(&audio, device));

    Ok(())
}

/// 试听序列：键盘打一小段，鼠标点四个按钮。
fn play_preview(audio: &AudioHandle, device: Device) {
    match device {
        Device::Keyboard => {
            for key in ["a", "s", "d", "f", "j", "k", "l", "space", "enter"] {
                audio.play(Device::Keyboard, key, true);
                std::thread::sleep(Duration::from_millis(105));
                audio.play(Device::Keyboard, key, false);
                std::thread::sleep(Duration::from_millis(55));
            }
        }
        Device::Mouse => {
            for button in ["left", "left", "right", "middle"] {
                audio.play(Device::Mouse, button, true);
                std::thread::sleep(Duration::from_millis(190));
            }
        }
    }
}

/// 启用 / 停用的唯一入口，界面和托盘菜单都走这里，保证状态一致。
///
/// 成功之后必须广播 `enabled-changed`：托盘菜单能改状态，界面也能改，
/// 不广播的话从托盘切完，窗口里的开关还停在旧值，两边就对不上了。
fn apply_enabled(app: &tauri::AppHandle, enabled: bool) -> Result<bool, String> {
    let result = apply_enabled_inner(app, enabled);
    if let Ok(current) = result {
        let _ = app.emit("enabled-changed", serde_json::json!({ "enabled": current }));
        return Ok(current);
    }
    result
}

fn apply_enabled_inner(app: &tauri::AppHandle, enabled: bool) -> Result<bool, String> {
    let state = app.state::<AppState>();

    if !enabled {
        state.enabled.store(false, Ordering::SeqCst);
        state.sync_tray(false);
        state.persist();
        return Ok(false);
    }

    if state.audio().is_none() {
        return Err("audio-unavailable".into());
    }
    // 键盘包是必需的；鼠标包没有就只响键盘。
    if state.active_keyboard.lock().unwrap().is_none() {
        return Err("no-profile".into());
    }
    if !hook::is_trusted() {
        return Err("missing-permission".into());
    }

    ensure_hook(app)?;

    state.enabled.store(true, Ordering::SeqCst);
    state.sync_tray(true);
    state.persist();
    Ok(true)
}

/// 启动全局输入钩子和消费线程。
///
/// 按键音和木鱼共用这一套：谁先开谁负责启动，后开的那位发现已经在跑就直接复用。
/// 所以钩子的生命周期不再等于「按键音开关」，而是「任一功能开着」。
fn ensure_hook(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    if state.hook_started.swap(true, Ordering::SeqCst) {
        return Ok(());
    }

    let (tx, rx) = crossbeam_channel::unbounded::<hook::InputEvent>();

    hook::start(tx).map_err(|e| {
        state.hook_started.store(false, Ordering::SeqCst);
        e
    })?;

    // 消费线程：播放音频 + 语音播报 + 木鱼计数 + 把事件推给前端。
    // 播放必须离开钩子回调线程，否则系统会因为回调超时把 tap 禁用。
    let app_handle = app.clone();
    std::thread::Builder::new()
        .name("kbs-consumer".into())
        .spawn(move || {
            let state = app_handle.state::<AppState>();
            let mut down: Vec<(Device, u32)> = Vec::new();
            let mut last_pending_emit = std::time::Instant::now();

            for ev in rx {
                // 长按会被系统连发，重复的按下事件直接丢弃。
                let id = (ev.device, ev.code);
                if ev.pressed {
                    if down.contains(&id) {
                        continue;
                    }
                    down.push(id);
                } else {
                    down.retain(|c| *c != id);
                }

                // 本应用在前台时不发声、也不记功德：用户点自己的界面、在自己的窗口里
                // 打字，不该被自己的音效吵到，也不该给自己刷功德。
                // 每次都重新查窗口 —— 关到托盘后窗口会被销毁，重开时是新句柄。
                let self_focused = app_handle
                    .get_webview_window("main")
                    .and_then(|w| w.is_focused().ok())
                    .unwrap_or(false);

                // 木鱼计数与按键音开关无关：只要木鱼开着，按一下就攒一笔。
                if ev.pressed && !self_focused && state.muyu.enabled() {
                    state.muyu.hit();
                    // 库存提示节流 120ms，别让每次击键都过一次 IPC
                    if last_pending_emit.elapsed() >= Duration::from_millis(120) {
                        last_pending_emit = std::time::Instant::now();
                        let _ = app_handle.emit(
                            "muyu-pending",
                            serde_json::json!({
                                "pending": state.muyu.pending(),
                                "today": state.muyu.today(),
                            }),
                        );
                    }
                }

                if !state.enabled.load(Ordering::Relaxed) {
                    continue;
                }

                let latency_ms = now_ns().saturating_sub(ev.timestamp_ns) as f64 / 1_000_000.0;

                // 事件照常推给前端，日志还能看。
                if !self_focused {
                    let key = ev.name.to_lowercase();

                    if let Some(audio) = state.audio() {
                        audio.play(ev.device, &key, ev.pressed);
                    }

                    // 语音只在按下时播，抬起再读一遍会翻倍。
                    if ev.pressed && state.voice_enabled.load(Ordering::Relaxed) {
                        let lang = *state.voice_lang.lock().unwrap();
                        if let Some(text) = speech::spoken_text(ev.device, &key, lang) {
                            let volume = *state.volume.lock().unwrap();
                            let _ = app_handle.run_on_main_thread(move || {
                                speech::speak(&text, lang, volume);
                            });
                        }
                    }
                }

                let _ = app_handle.emit(
                    "key-event",
                    serde_json::json!({
                        "device": ev.device.as_str(),
                        "code": ev.code,
                        "name": ev.name,
                        "pressed": ev.pressed,
                        "latency_ms": latency_ms.min(999.0),
                    }),
                );
            }
        })
        .map_err(|e| format!("无法启动消费线程: {e}"))?;

    Ok(())
}

#[tauri::command]
fn set_enabled(app: tauri::AppHandle, enabled: bool) -> Result<bool, String> {
    apply_enabled(&app, enabled)
}

/* ---------------- 木鱼 ---------------- */

/// 木鱼开关的唯一入口：菜单栏和界面都走这里，保证状态一致。
///
/// 失败时把内部状态和托盘勾选一起退回「关」—— 权限没给却显示已开启，
/// 用户会以为功能坏了。
///
/// 注意这里**不写盘**：失败往往发生在启动阶段（新包还没拿到辅助功能授权），
/// 此时内存里的 enabled 可能还是 false，一 persist 就会把用户「按键音开着」
/// 的偏好覆盖成关。设置文件保持原样，下次启动再试。
fn apply_muyu(app: &tauri::AppHandle, enabled: bool) -> Result<bool, String> {
    match apply_muyu_inner(app, enabled) {
        Ok(v) => Ok(v),
        Err(e) => {
            let state = app.state::<AppState>();
            state.muyu.set_enabled(false);
            state.sync_muyu_tray(false);
            Err(e)
        }
    }
}

fn apply_muyu_inner(app: &tauri::AppHandle, enabled: bool) -> Result<bool, String> {
    let state = app.state::<AppState>();

    if !enabled {
        state.muyu.set_enabled(false);
        state.muyu.save();
        let _ = sync_muyu_window(app);
        state.sync_muyu_tray(false);
        state.persist();
        let _ = app.emit("muyu-state", serde_json::json!({ "enabled": false }));
        return Ok(false);
    }

    // 木鱼和按键音共用同一个全局钩子，权限是硬门槛
    if !hook::is_trusted() {
        return Err("missing-permission".into());
    }

    ensure_hook(app)?;
    state.muyu.set_enabled(true);
    // 窗口建不出来不应该连带把开关也回滚 —— 计数和声音照样能用
    if let Err(e) = sync_muyu_window(app) {
        eprintln!("[muyu] 挂件窗口创建失败（计数不受影响）: {e}");
    }
    state.sync_muyu_tray(true);
    state.persist();
    let _ = app.emit("muyu-state", serde_json::json!({ "enabled": true }));
    Ok(true)
}

fn destroy_muyu_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("muyu") {
        let _ = window.destroy();
    }
}

/// 挂件尺寸：设计基准 240×300，整体按 scale 缩放。
fn muyu_size(scale: f32) -> (f64, f64) {
    let s = scale as f64;
    (240.0 * s, 320.0 * s)
}

/// 把挂件调成「桌面挂件」该有的窗口行为。
///
/// 三件事：
/// 1. `collectionBehavior` 补上 `CanJoinAllSpaces | Stationary | IgnoresCycle` ——
///    跟着用户切桌面、Mission Control 里不乱跑、不参与 Cmd+` 循环
/// 2. 提到 `NSStatusWindowLevel`（25）。Tauri 的 `always_on_top` 只是 floating level（3），
///    会被别的应用的浮动面板压住
/// 3. 关掉 `hidesOnDeactivate`，切到别的应用时不要自动隐藏
///
/// **关键**：这一堆必须在窗口 `show()` 之前设好。窗口一旦上屏，macOS 就把它绑在
/// 当时那个 Space 上了，之后再改 collectionBehavior 不一定会重新生效 ——
/// 表现就是「切个桌面挂件就不见了」。
///
/// 另外：别的应用进全屏时这个窗口仍然不显示。实测把 level 提到 1000（screenSaver 层级）
/// 也没用，macOS 不允许第三方窗口进入其他应用的全屏 Space。
#[cfg(target_os = "macos")]
fn tune_widget_window(window: &tauri::WebviewWindow) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    const CAN_JOIN_ALL_SPACES: usize = 1 << 0;
    const STATIONARY: usize = 1 << 4;
    const IGNORES_CYCLE: usize = 1 << 6;
    const FULLSCREEN_AUXILIARY: usize = 1 << 8;
    const STATUS_LEVEL: isize = 8; // NSModalPanelWindowLevel

    let Ok(ptr) = window.ns_window() else {
        return;
    };
    let obj = ptr as *mut AnyObject;
    unsafe {
        let behavior: usize = msg_send![obj, collectionBehavior];
        let want = behavior
            | CAN_JOIN_ALL_SPACES
            | STATIONARY
            | IGNORES_CYCLE
            | FULLSCREEN_AUXILIARY;
        let _: () = msg_send![obj, setCollectionBehavior: want];
        let _: () = msg_send![obj, setLevel: STATUS_LEVEL];
        let _: () = msg_send![obj, setHidesOnDeactivate: false];

        let after: usize = msg_send![obj, collectionBehavior];
        if after != want {
            eprintln!("[muyu] collectionBehavior 设置失败: {behavior} -> {after}");
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn tune_widget_window(_window: &tauri::WebviewWindow) {}

/// 强制把窗口放到「当前正在看的那个 Space」上。
///
/// tao 的 `show()` 走的是 `orderFront:` —— 应用不在前台时，窗口会落到
/// 应用自己的那个 Space，用户切了桌面就看不见。`orderFrontRegardless:`
/// 不看激活状态，直接上屏到活动 Space。
#[cfg(target_os = "macos")]
fn order_front_regardless(window: &tauri::WebviewWindow) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    let Ok(ptr) = window.ns_window() else {
        return;
    };
    let obj = ptr as *mut AnyObject;
    unsafe {
        let _: () = msg_send![obj, orderFrontRegardless];
    }
}

#[cfg(not(target_os = "macos"))]
fn order_front_regardless(_window: &tauri::WebviewWindow) {}

/// 把挂件摆到屏幕上：拖过就还原坐标（并夹回屏幕内），没拖过就贴右下角。
///
/// 用 work_area 而不是 size，否则会压到 Dock / 菜单栏上。
fn position_muyu_window(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
    w: f64,
    h: f64,
    pos: Option<(i32, i32)>,
) {
    let Ok(Some(monitor)) = app.primary_monitor() else {
        return;
    };

    let area = monitor.work_area();
    let sf = monitor.scale_factor();
    let margin = 24.0 * sf;
    let ax = area.position.x as f64;
    let ay = area.position.y as f64;
    let max_x = ax + area.size.width as f64 - w * sf - margin;
    let max_y = ay + area.size.height as f64 - h * sf - margin;

    let (x, y) = match pos {
        // 拖过：保留用户的位置，但缩放变大后不能跑出屏幕
        Some((px, py)) => (
            (px as f64).clamp(ax + margin, max_x.max(ax + margin)),
            (py as f64).clamp(ay + margin, max_y.max(ay + margin)),
        ),
        // 没拖过：贴右下角
        None => (max_x, max_y),
    };

    let _ = window.set_position(tauri::PhysicalPosition::new(x as i32, y as i32));
}

/// 按当前状态同步挂件：开着就建 / 显示，关了就销毁。
fn sync_muyu_window(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    if state.muyu.enabled() && state.muyu.window_visible() {
        ensure_muyu_window(app, state.muyu.scale(), state.muyu.pos())
    } else {
        destroy_muyu_window(app);
        Ok(())
    }
}

/// 挂件窗口：透明、置顶、点击穿透。
///
/// 尺寸和位置由调用方传进来，不在这里读 AppState —— `--muyu-window`
/// 调试模式没有 AppState，读了会直接 panic。
fn ensure_muyu_window(
    app: &tauri::AppHandle,
    scale: f32,
    pos: Option<(i32, i32)>,
) -> Result<(), String> {
    let (w, h) = muyu_size(scale);

    if let Some(window) = app.get_webview_window("muyu") {
        // 已经在了：尺寸可能刚被调过，同步一下再显示
        let _ = window.set_size(tauri::LogicalSize::new(w, h));
        tune_widget_window(&window);
        let _ = window.show();
        order_front_regardless(&window);
        return Ok(());
    }

    // 建出来之后立刻把窗口行为调好，再用 orderFrontRegardless 强制上屏。
    let window = WebviewWindowBuilder::new(app, "muyu", WebviewUrl::App("muyu.html".into()))
        .title("木鱼")
        .inner_size(w, h)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .build()
        .map_err(|e| format!("创建木鱼窗口失败: {e}"))?;

    // 点击穿透：挂件不能挡住下面的任何操作。
    let _ = window.set_ignore_cursor_events(true);

    // 跟随所有桌面（Space）+ 状态栏层级，必须在 show 之前
    let _ = window.set_visible_on_all_workspaces(true);
    tune_widget_window(&window);

    position_muyu_window(app, &window, w, h, pos);

    window.show().map_err(|e| format!("显示木鱼窗口失败: {e}"))?;
    order_front_regardless(&window);

    Ok(())
}

#[derive(Serialize)]
struct MuyuInfo {
    enabled: bool,
    sound: bool,
    volume: f32,
    interval_ms: u64,
    today: u64,
    total: u64,
    pending: u32,
    window_visible: bool,
    scale: f32,
    /// 是否被拖动过（拖过就不再自动贴右下角）
    has_pos: bool,
}

fn muyu_info(state: &AppState) -> MuyuInfo {
    MuyuInfo {
        enabled: state.muyu.enabled(),
        sound: state.muyu.sound(),
        volume: state.muyu.volume(),
        interval_ms: state.muyu.interval_ms(),
        today: state.muyu.today(),
        total: state.muyu.total(),
        pending: state.muyu.pending(),
        window_visible: state.muyu.window_visible(),
        scale: state.muyu.scale(),
        has_pos: state.muyu.pos().is_some(),
    }
}

#[tauri::command]
fn get_muyu(state: State<'_, AppState>) -> MuyuInfo {
    muyu_info(&state)
}

#[tauri::command]
fn set_muyu(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    enabled: Option<bool>,
    sound: Option<bool>,
    volume: Option<f32>,
    interval_ms: Option<u64>,
    window: Option<bool>,
    scale: Option<f32>,
) -> Result<MuyuInfo, String> {
    if let Some(on) = enabled {
        apply_muyu(&app, on)?;
    }
    if let Some(on) = sound {
        state.muyu.set_sound(on);
    }
    if let Some(v) = volume {
        state.muyu.set_volume(v);
    }
    if let Some(ms) = interval_ms {
        state.muyu.set_interval(ms);
    }
    if let Some(on) = window {
        state.muyu.set_window_visible(on);
        sync_muyu_window(&app)?;
    }
    if let Some(s) = scale {
        state.muyu.set_scale(s);
        // 窗口已经开着的话直接改尺寸，不用销毁重建
        if let Some(w) = app.get_webview_window("muyu") {
            let (width, height) = muyu_size(state.muyu.scale());
            let _ = w.set_size(tauri::LogicalSize::new(width, height));
            // 放大后可能顶出屏幕，重新摆一下
            position_muyu_window(&app, &w, width, height, state.muyu.pos());
        }
    }
    if sound.is_some()
        || volume.is_some()
        || interval_ms.is_some()
        || window.is_some()
        || scale.is_some()
    {
        state.persist();
    }
    Ok(muyu_info(&state))
}

/// 进入「调整位置」：临时关掉点击穿透，让用户能把挂件拖走。
///
/// 拖完自动恢复 —— 位置稳定 500ms 或者超时 15s。不这么做的话，
/// 用户忘了退出这个模式，挂件就会一直挡着鼠标点击。
#[tauri::command]
fn begin_muyu_drag(app: tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("muyu")
        .ok_or("木鱼挂件没有显示")?;

    window
        .set_ignore_cursor_events(false)
        .map_err(|e| e.to_string())?;
    let _ = window.emit("muyu-drag", serde_json::json!({ "on": true }));

    // 起始坐标必须先读出来。之前用 None 起步，第一轮轮询「和 None 不相等」
    // 就判定成"动过了"，500ms 后直接把穿透恢复，用户根本来不及按鼠标。
    let start_pos = window.outer_position().ok().map(|p| (p.x, p.y));

    let handle = app.clone();
    std::thread::spawn(move || {
        let started = std::time::Instant::now();
        let mut last = start_pos;
        let mut moved = false;
        let mut stable = std::time::Instant::now();

        loop {
            std::thread::sleep(Duration::from_millis(120));

            let Some(w) = handle.get_webview_window("muyu") else {
                return;
            };
            let pos = w.outer_position().ok().map(|p| (p.x, p.y));

            if let Some(p) = pos {
                if last != Some(p) {
                    moved = true;
                    stable = std::time::Instant::now();
                    last = Some(p);
                }
            }

            // 动过、并且停稳 600ms 才算拖完；给足 30 秒让用户慢慢拖
            if (moved && stable.elapsed() >= Duration::from_millis(600))
                || started.elapsed() >= Duration::from_secs(30)
            {
                finish_muyu_drag(&handle, last);
                return;
            }
        }
    });

    Ok(())
}

/// 结束「调整位置」：记下当前位置，恢复点击穿透。
#[tauri::command]
fn end_muyu_drag(app: tauri::AppHandle) {
    let pos = app
        .get_webview_window("muyu")
        .and_then(|w| w.outer_position().ok())
        .map(|p| (p.x, p.y));
    finish_muyu_drag(&app, pos);
}

fn finish_muyu_drag(app: &tauri::AppHandle, pos: Option<(i32, i32)>) {
    // --muyu-window 调试模式没有 AppState，这里不能直接 state()
    if let Some(state) = app.try_state::<AppState>() {
        if let Some(p) = pos {
            state.muyu.set_pos(Some(p));
            state.persist();
        }
    }
    if let Some(window) = app.get_webview_window("muyu") {
        let _ = window.set_ignore_cursor_events(true);
        let _ = window.emit("muyu-drag", serde_json::json!({ "on": false }));
    }
}

#[tauri::command]
fn reset_muyu_today(app: tauri::AppHandle, state: State<'_, AppState>) -> MuyuInfo {
    state.muyu.reset_today();
    state.muyu.save();

    // 挂件是独立窗口，不通知它的话只有设置页的数字归零，桌面还挂在旧值上
    if let Some(window) = app.get_webview_window("muyu") {
        let _ = window.emit("muyu-reset", serde_json::json!({ "today": 0 }));
    }

    muyu_info(&state)
}

/* ---------------- 木鱼素材（可替换） ---------------- */

/// 内置素材。用户把同名文件放进数据目录的 muyu/ 下就会覆盖它。
const FISH_PNG: &[u8] = include_bytes!("../../dist/muyu-fish.png");
const MALLET_PNG: &[u8] = include_bytes!("../../dist/muyu-mallet.png");

fn muyu_asset_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("muyu"))
}

/// 首次运行把默认素材写出来，用户直接替换同名文件即可。
fn ensure_muyu_assets(app: &tauri::AppHandle) {
    let Some(dir) = muyu_asset_dir(app) else {
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    for (name, bytes) in [("fish.png", FISH_PNG), ("mallet.png", MALLET_PNG)] {
        let path = dir.join(name);
        if !path.exists() {
            let _ = std::fs::write(&path, bytes);
        }
    }
}

fn mime_of(bytes: &[u8]) -> &'static str {
    match bytes {
        [0x89, 0x50, 0x4E, 0x47, ..] => "image/png",
        [0xFF, 0xD8, 0xFF, ..] => "image/jpeg",
        [0x47, 0x49, 0x46, 0x38, ..] => "image/gif",
        b if b.len() > 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" => "image/webp",
        _ => "application/octet-stream",
    }
}

/// 读用户素材；没有（或太大）就退回内置的。返回 (mime, bytes, 是否为用户替换)。
fn load_asset(
    app: &tauri::AppHandle,
    stem: &str,
    fallback: &'static [u8],
) -> (String, Vec<u8>, bool) {
    const MAX: usize = 8 * 1024 * 1024;

    if let Some(dir) = muyu_asset_dir(app) {
        for ext in ["png", "jpg", "jpeg", "webp", "gif"] {
            let path = dir.join(format!("{stem}.{ext}"));
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            if bytes.is_empty() || bytes.len() > MAX {
                eprintln!("[muyu] 素材 {} 无效或过大，忽略", path.display());
                continue;
            }
            let custom = bytes != fallback;
            return (mime_of(&bytes).to_string(), bytes, custom);
        }
    }
    ("image/png".to_string(), fallback.to_vec(), false)
}

#[derive(Serialize)]
struct MuyuAssets {
    /// data URL，前端直接塞给 <img src>
    fish: String,
    mallet: String,
    /// 素材目录，设置页「打开素材目录」用
    dir: String,
    fish_custom: bool,
    mallet_custom: bool,
}

#[tauri::command]
fn get_muyu_assets(app: tauri::AppHandle) -> MuyuAssets {
    use base64::Engine;
    let enc = base64::engine::general_purpose::STANDARD;

    let (fish_mime, fish_bytes, fish_custom) = load_asset(&app, "fish", FISH_PNG);
    let (mallet_mime, mallet_bytes, mallet_custom) = load_asset(&app, "mallet", MALLET_PNG);

    MuyuAssets {
        fish: format!("data:{fish_mime};base64,{}", enc.encode(&fish_bytes)),
        mallet: format!("data:{mallet_mime};base64,{}", enc.encode(&mallet_bytes)),
        dir: muyu_asset_dir(&app)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
        fish_custom,
        mallet_custom,
    }
}

#[tauri::command]
fn open_muyu_assets_folder(app: tauri::AppHandle) -> Result<(), String> {
    let dir = muyu_asset_dir(&app).ok_or("找不到数据目录")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建素材目录失败: {e}"))?;
    open_folder(&dir)
}

/// 用户换完图点「重新加载」：通知挂件重新取一次素材。
#[tauri::command]
fn reload_muyu_assets(app: tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("muyu") {
        let _ = window.emit("muyu-assets", serde_json::json!({}));
    }
}

/// 试听：直接敲一下，不记账。设置页的「试听」按钮走这里。
#[tauri::command]
fn preview_muyu(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let audio = state.audio().ok_or("audio-unavailable")?;
    audio.play_muyu(state.muyu.volume());

    if let Some(window) = app.get_webview_window("muyu") {
        let _ = window.emit(
            "muyu-hit",
            serde_json::json!({
                "delta": 1,
                "today": state.muyu.today(),
                "pending": state.muyu.pending(),
            }),
        );
    }
    Ok(())
}

#[tauri::command]
fn toggle_muyu(app: tauri::AppHandle) -> Result<bool, String> {
    let state = app.state::<AppState>();
    apply_muyu(&app, !state.muyu.enabled())
}

fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// 关到托盘后窗口是被销毁的，需要时重建。
fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }

    let builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("KeyTone")
        .inner_size(900.0, 660.0)
        .min_inner_size(780.0, 580.0);

    // Overlay 标题栏是 macOS 专有的（把红绿灯叠在内容上）。
    // 其他平台没有这个方法，直接调用会编译不过。
    #[cfg(target_os = "macos")]
    let builder = builder.title_bar_style(tauri::TitleBarStyle::Overlay);

    if let Err(e) = builder.build() {
        eprintln!("[window] 重建主窗口失败: {e}");
    }
}

/// 应用标识符，与 tauri.conf.json 的 identifier 保持一致。
const BUNDLE_ID: &str = "com.keytone.app";
/// 改名前的旧目录，用于一次性迁移用户已有的设置与音色包。
const LEGACY_BUNDLE_ID: &str = "pro.keyboard.sounds.spike";

fn app_support_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    if cfg!(target_os = "macos") {
        PathBuf::from(&home).join("Library/Application Support")
    } else if cfg!(target_os = "windows") {
        std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(&home).join("AppData/Roaming"))
    } else {
        std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(&home).join(".local/share"))
    }
}

/// 与 Tauri 的 app_data_dir 保持一致，供命令行模式（此时还没有 AppHandle）使用。
fn data_dir_guess() -> PathBuf {
    app_support_dir().join(BUNDLE_ID)
}

/// 从旧版本目录搬一次家。只在目标目录不存在时执行，
/// 否则会把用户后来改过的设置覆盖掉。
fn migrate_legacy_data(new_dir: &Path) {
    if new_dir.exists() {
        return;
    }
    let old_dir = app_support_dir().join(LEGACY_BUNDLE_ID);
    if !old_dir.is_dir() {
        return;
    }
    match copy_dir_all(&old_dir, new_dir) {
        Ok(()) => eprintln!(
            "[migrate] {} → {}",
            old_dir.display(),
            new_dir.display()
        ),
        Err(e) => eprintln!("[migrate] 迁移旧数据失败: {e}"),
    }
}

/// 递归复制目录（音色包目录层级很浅，不需要额外依赖）。
fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), to)?;
        }
    }
    Ok(())
}

/// 导入外部音色包，然后逐个真正加载一遍做校验。
fn import_profiles(src: &Path) -> Result<(), String> {
    if !src.is_dir() {
        return Err(format!("{} 不是目录", src.display()));
    }

    let dest = data_dir_guess().join("profiles");
    std::fs::create_dir_all(&dest).map_err(|e| format!("创建目录失败: {e}"))?;

    let mut imported = 0;
    let mut skipped = 0;

    for entry in std::fs::read_dir(src).map_err(|e| format!("{e}"))?.flatten() {
        let dir = entry.path();
        if !dir.join("profile.yaml").is_file() {
            continue;
        }
        let target = dest.join(entry.file_name());
        if target.exists() {
            skipped += 1;
            continue;
        }
        copy_dir_all(&dir, &target).map_err(|e| format!("复制 {} 失败: {e}", dir.display()))?;
        imported += 1;
    }

    println!("导入 {imported} 个，跳过已存在 {skipped} 个\n");
    validate_all(&dest);
    Ok(())
}

/// 把目录下所有音色包加载一遍，报告音源数 / 采样数 / 警告。
fn validate_all(profiles_dir: &Path) {
    let metas = profile::scan(profiles_dir);
    println!("共 {} 个音色包", metas.len());

    let (mut keyboard, mut mouse, mut failed) = (0, 0, 0);

    for meta in &metas {
        match profile::load(&meta.dir) {
            Ok((p, warnings)) => {
                match meta.device {
                    Device::Keyboard => keyboard += 1,
                    Device::Mouse => mouse += 1,
                }
                let flag = if warnings.is_empty() { " " } else { "!" };
                println!(
                    "{flag} {:<26} {:<9} {:>2} 音源 {:>3} 采样  峰值 {:.3} → 增益 {:.2}x",
                    meta.name,
                    meta.device.as_str(),
                    p.source_count(),
                    p.sample_count(),
                    p.peak,
                    p.gain
                );
                for w in warnings {
                    println!("    · {w}");
                }
            }
            Err(e) => {
                failed += 1;
                println!("x {:<26} 加载失败: {e}", meta.name);
            }
        }
    }

    println!("\n键盘 {keyboard} / 鼠标 {mouse} / 失败 {failed}");
}

/// 命令行试听：加载指定包（不给就用第一个真实包），播放试听序列。
fn preview_cli(device: Device, name: Option<String>) -> Result<(), String> {
    let profiles_dir = data_dir_guess().join("profiles");
    let metas = profile::scan(&profiles_dir);

    let meta = name
        .and_then(|n| metas.iter().find(|m| m.name == n).cloned())
        .or_else(|| {
            metas
                .iter()
                .find(|m| m.device == device && !m.name.starts_with("Demo"))
                .cloned()
        })
        .or_else(|| metas.iter().find(|m| m.device == device).cloned())
        .ok_or("没有可用的音色包")?;

    let (compiled, _) = profile::load(&meta.dir)?;
    println!(
        "{} → {} (峰值 {:.3} → 增益 {:.2}x)",
        device.as_str(),
        compiled.name,
        compiled.peak,
        compiled.gain
    );

    let audio = AudioHandle::spawn()?;
    audio.set_volume(1.0);
    audio.set_profile(device, Arc::new(compiled));
    std::thread::sleep(Duration::from_millis(60));

    play_preview(&audio, device);
    std::thread::sleep(Duration::from_millis(300));
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if let Some(pos) = args.iter().position(|a| a == "--import-profiles") {
        let Some(src) = args.get(pos + 1) else {
            eprintln!("用法: keytone --import-profiles <音色包目录>");
            std::process::exit(2);
        };
        if let Err(e) = import_profiles(Path::new(src)) {
            eprintln!("导入失败: {e}");
            std::process::exit(1);
        }
        return;
    }

    if let Some(pos) = args.iter().position(|a| a == "--preview") {
        let device = args
            .get(pos + 1)
            .and_then(|d| Device::parse(d))
            .unwrap_or(Device::Keyboard);
        let name = args.get(pos + 2).cloned();

        if let Err(e) = preview_cli(device, name) {
            eprintln!("试听失败: {e}");
            std::process::exit(1);
        }
        return;
    }

    if let Some(pos) = args.iter().position(|a| a == "--speak") {
        let lang = args
            .get(pos + 1)
            .and_then(|l| Lang::parse(l))
            .unwrap_or(Lang::Zh);
        let text = args.get(pos + 2).cloned().unwrap_or_else(|| "测试".into());

        if !speech::available() {
            eprintln!("当前平台未实现语音播报");
            std::process::exit(1);
        }

        speech::speak(&text, lang, 1.0);

        // 给 TTS 一点时间把音频吐出来。CFRunLoop 是 AVSpeechSynthesizer
        // 需要的运行环境，这里模拟一下主线程。只有 macOS 有这套。
        #[cfg(target_os = "macos")]
        unsafe {
            core_foundation::runloop::CFRunLoop::run_in_mode(
                core_foundation::runloop::kCFRunLoopDefaultMode,
                Duration::from_secs(2),
                false,
            );
        }

        println!("spoke: [{:?}] {text}", lang);
        return;
    }

    if args.iter().any(|a| a == "--check-profiles") {
        validate_all(&data_dir_guess().join("profiles"));
        return;
    }

    if args.iter().any(|a| a == "--selftest") {
        if let Err(e) = selftest() {
            eprintln!("selftest failed: {e}");
            std::process::exit(1);
        }
        return;
    }

    if args.iter().any(|a| a == "--muyu-selftest") {
        if let Err(e) = muyu_selftest() {
            eprintln!("木鱼自检失败: {e}");
            std::process::exit(1);
        }
        return;
    }

    // 只弹木鱼挂件：不装输入钩子、不建托盘。用来单独调窗口外观和动画。
    // 加 `--hits N` 会真的跑一遍「入队 → 节拍 → 出声 → 事件 → 动画」，
    // 不需要辅助功能权限，可以和正式版同时跑。
    // generate_context! 一个 crate 只能展开一次，所以先算出来给两条分支共用。
    let context = tauri::generate_context!();

    if args.iter().any(|a| a == "--muyu-window") {
        let hits: u32 = args
            .iter()
            .position(|a| a == "--hits")
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);

        let scale: f32 = args
            .iter()
            .position(|a| a == "--scale")
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(1.0);

        let drag_test = args.iter().any(|a| a == "--drag-test");

        tauri::Builder::default()
            .setup(move |app| {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
                #[cfg(target_os = "macos")]
                let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                ensure_muyu_window(app.handle(), scale, None)?;

                // 拖动链路自测：不需要辅助功能权限，用来验证
                // 指针事件 → setPosition → 窗口真的移动
                if drag_test {
                    let h = app.handle().clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(1500));
                        if let Some(w) = h.get_webview_window("muyu") {
                            let _ = w.set_ignore_cursor_events(false);
                            let _ = w.emit("muyu-drag", serde_json::json!({ "on": true }));
                            eprintln!("[drag] 已进入拖动模式");
                        }
                    });
                }

                if hits > 0 {
                    let dir = std::env::temp_dir().join("keytone-muyu-window");
                    let _ = std::fs::remove_dir_all(&dir);
                    let _ = std::fs::create_dir_all(&dir);

                    let state = Arc::new(muyu::Muyu::load(
                        &dir,
                        true,
                        true,
                        0.9,
                        muyu::DEFAULT_INTERVAL_MS,
                        true,
                        1.0,
                        None,
                    ));
                    let audio = AudioHandle::spawn().ok();
                    let handle = app.handle().clone();
                    let player = audio.clone();

                    muyu::spawn_metronome(state.clone(), move |delta, today, pending| {
                        if let Some(audio) = &player {
                            audio.play_muyu(0.9);
                        }
                        if let Some(window) = handle.get_webview_window("muyu") {
                            let _ = window.emit(
                                "muyu-hit",
                                serde_json::json!({
                                    "delta": delta,
                                    "today": today,
                                    "pending": pending,
                                }),
                            );
                        }
                    });

                    // 等窗口把事件监听起来，再模拟一串按键
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(900));
                        for _ in 0..hits {
                            state.hit();
                            std::thread::sleep(Duration::from_millis(90));
                        }
                    });
                }
                Ok(())
            })
            .run(context)
            .expect("error while running tauri application");
        return;
    }

    let app = tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            // 改名后首次启动：把旧目录的设置和音色包搬过来，用户无感升级。
            migrate_legacy_data(&data_dir);
            let profiles_dir = data_dir.join("profiles");

            if let Err(e) = synth::ensure_demo_profiles(&profiles_dir) {
                eprintln!("[synth] {e}");
            }

            // 木鱼素材：首次运行把默认图写到数据目录，用户替换同名文件即可
            ensure_muyu_assets(app.handle());

            // 常驻菜单栏的应用必须用 accessory 策略。
            // regular（有 Dock 图标）时，应用不在前台新建的窗口会被丢到
            // 「应用自己的那个 Space」上 —— 用户切个桌面挂件就看不见了。
            #[cfg(target_os = "macos")]
            let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // 补装内置的原版音色包（已存在的跳过）。
            match bundled::install_missing(&profiles_dir) {
                Ok(installed) if !installed.is_empty() => {
                    eprintln!("[bundled] 安装 {} 个内置音色包", installed.len());
                }
                Ok(_) => {}
                Err(e) => eprintln!("[bundled] {e}"),
            }

            let settings = config::load(&data_dir);
            let profiles = profile::scan(&profiles_dir);

            let muyu_state = Arc::new(muyu::Muyu::load(
                &data_dir,
                settings.muyu_enabled,
                settings.muyu_sound,
                settings.muyu_volume,
                settings.muyu_interval_ms,
                settings.muyu_window,
                settings.muyu_scale,
                settings.muyu_x.zip(settings.muyu_y),
            ));

            let state = AppState {
                audio: OnceLock::new(),
                enabled: AtomicBool::new(false),
                hook_started: AtomicBool::new(false),
                data_dir,
                profiles_dir,
                profiles: Mutex::new(profiles),
                active_keyboard: Mutex::new(None),
                active_mouse: Mutex::new(None),
                volume: Mutex::new(settings.volume),
                voice_enabled: AtomicBool::new(settings.voice_enabled),
                voice_lang: Mutex::new(Lang::parse(&settings.voice_lang).unwrap_or(Lang::Zh)),
                update_repo: Mutex::new(settings.update_repo.clone()),
                theme: Mutex::new(settings.theme.clone()),
                tray_style: Mutex::new(settings.tray_style.clone()),
                tray_toggle: Mutex::new(None),
                muyu: muyu_state.clone(),
                muyu_toggle: Mutex::new(None),
            };

            // 提前打开音频设备：冷启动建流有几十毫秒开销，不能放在第一次按键上。
            match AudioHandle::spawn() {
                Ok(handle) => {
                    handle.set_volume(settings.volume);
                    let _ = state.audio.set(handle);
                }
                Err(e) => eprintln!("[audio] 初始化失败: {e}"),
            }

            // 每个设备各恢复上次用的包，找不到就退到第一个。
            for (device, wanted) in [
                (Device::Keyboard, settings.keyboard_profile.clone()),
                (Device::Mouse, settings.mouse_profile.clone()),
            ] {
                let meta = wanted
                    .and_then(|name| {
                        state
                            .profiles
                            .lock()
                            .unwrap()
                            .iter()
                            .find(|m| m.device == device && m.name == name)
                            .cloned()
                    })
                    // 默认优先选真实采样包，合成包只当兜底。
                    .or_else(|| {
                        let profiles = state.profiles.lock().unwrap();
                        profiles
                            .iter()
                            .find(|m| m.device == device && !m.name.starts_with("Demo"))
                            .or_else(|| profiles.iter().find(|m| m.device == device))
                            .cloned()
                    });

                if let Some(meta) = meta {
                    if let Err(e) = state.activate(&meta) {
                        eprintln!("[profile] 加载 {} 失败: {e}", meta.name);
                    }
                }
            }

            // ---- 菜单栏托盘 ----
            let toggle_item =
                CheckMenuItem::with_id(app, "toggle", "启用按键音", true, false, None::<&str>)?;
            let muyu_item = CheckMenuItem::with_id(
                app,
                "muyu",
                "木鱼",
                true,
                settings.muyu_enabled,
                None::<&str>,
            )?;
            let show_item = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[
                    &toggle_item,
                    &muyu_item,
                    &PredefinedMenuItem::separator(app)?,
                    &show_item,
                    &quit_item,
                ],
            )?;

            *state.tray_toggle.lock().unwrap() = Some(toggle_item);
            *state.muyu_toggle.lock().unwrap() = Some(muyu_item);

            // 菜单栏图标：金色线稿原样显示，单色版走模板（系统自动反色）。
            let use_mono = settings.tray_style == "mono";
            let initial_icon = tray_icon(&settings.tray_style)
                .or_else(|| app.default_window_icon().cloned());

            if let Some(icon) = initial_icon {
                TrayIconBuilder::with_id("main")
                    .icon(icon)
                    .icon_as_template(use_mono)
                    .tooltip("KeyTone")
                    .menu(&menu)
                    .show_menu_on_left_click(true)
                    .on_menu_event(|app, event| match event.id.as_ref() {
                        "toggle" => {
                            let state = app.state::<AppState>();
                            let next = !state.enabled.load(Ordering::Relaxed);
                            if let Err(e) = apply_enabled(app, next) {
                                eprintln!("[tray] 切换失败: {e}");
                                // 失败时把勾选状态改回去，避免界面说谎。
                                state.sync_tray(!next);
                            }
                        }
                        "show" => show_main_window(app),
                        "muyu" => {
                            let state = app.state::<AppState>();
                            let next = !state.muyu.enabled();
                            if let Err(e) = apply_muyu(app, next) {
                                eprintln!("[tray] 木鱼切换失败: {e}");
                                // 勾选状态由 apply_muyu 内部退回，这里只负责告诉用户为什么。
                                // 没授权时把主窗口拉出来，界面上有「去授权」的入口。
                                if e == "missing-permission" {
                                    show_main_window(app);
                                }
                            }
                        }
                        "quit" => app.exit(0),
                        _ => {}
                    })
                    .build(app)?;
            }

            let was_enabled = settings.enabled;
            let was_muyu = settings.muyu_enabled;
            apply_window_theme(app.handle(), &settings.theme);
            app.manage(state);

            // 木鱼节拍线程：敲一下 → 出声 + 通知挂件。
            // 声音在这里发，动画通过事件交给前端，两边同一时刻触发。
            let tick_app = app.handle().clone();
            muyu::spawn_metronome(muyu_state.clone(), move |delta, today, pending| {
                let state = tick_app.state::<AppState>();
                if state.muyu.sound() {
                    if let Some(audio) = state.audio() {
                        audio.play_muyu(state.muyu.volume());
                    }
                }
                if let Some(window) = tick_app.get_webview_window("muyu") {
                    let _ = window.emit(
                        "muyu-hit",
                        serde_json::json!({
                            "delta": delta,
                            "today": today,
                            "pending": pending,
                        }),
                    );
                }
            });

            // 恢复上次的启用状态：用户既然开着，就不该每次启动都要再点一下。
            if was_enabled {
                if let Err(e) = apply_enabled(app.handle(), true) {
                    eprintln!("[state] 恢复启用状态失败: {e}");
                }
            }

            // 挂件窗口不能在 setup 里同步创建。
            // Windows 上 WebView2 在这个阶段建窗口会把主线程卡死 ——
            // 主窗口是 setup 之前就建好的所以还看得见，但事件循环永远起不来，
            // 结果就是界面所有按钮都没反应。挪到事件循环跑起来之后再建。
            if was_muyu {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(400));
                    let inner = handle.clone();
                    let _ = handle.run_on_main_thread(move || {
                        if let Err(e) = apply_muyu(&inner, true) {
                            eprintln!("[muyu] 恢复木鱼失败: {e}");
                        }
                    });
                });
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // 点关闭只是收进菜单栏，不退出进程。
                api.prevent_close();
                let _ = window.hide();

                // 真正把 WebView 销毁掉，内存才还得回去 —— 只是 hide 的话
                // WKWebView 会一直占着几十上百 MB。下次从托盘点开时重建。
                let window = window.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(150));
                    let _ = window.destroy();
                });
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            list_profiles,
            select_profile,
            set_volume,
            set_enabled,
            set_voice,
            speak_sample,
            set_theme,
            set_tray_style,
            set_update_repo,
            check_update,
            download_update,
            install_update,
            open_url,
            open_accessibility_settings,
            open_profiles_folder,
            preview,
            get_muyu,
            set_muyu,
            toggle_muyu,
            preview_muyu,
            reset_muyu_today,
            begin_muyu_drag,
            end_muyu_drag,
            get_muyu_assets,
            open_muyu_assets_folder,
            reload_muyu_assets
        ])
        .build(context)
        .expect("error while building tauri application");

    app.run(|app, event| match event {
        // 窗口全销毁时不要退出进程（托盘还在），只有显式 quit 才走。
        tauri::RunEvent::ExitRequested { api, code, .. } => {
            if code.is_none() {
                api.prevent_exit();
            }
        }
        // 退出前把功德落盘，别丢最后几秒
        tauri::RunEvent::Exit => {
            let state = app.state::<AppState>();
            state.muyu.flush();
        }
        _ => {}
    });
}

/// 自检：不启动界面、不需要辅助功能权限，只验证
/// 「生成音色包 → 解码 wav → 按规则播放」这条链路。
fn selftest() -> Result<(), String> {
    let profiles_dir = data_dir_guess().join("profiles");
    synth::ensure_demo_profiles(&profiles_dir)?;

    let metas = profile::scan(&profiles_dir);
    let audio = AudioHandle::spawn()?;
    println!("audio: {} Hz", audio.sample_rate());
    audio.set_volume(0.8);

    for device in [Device::Keyboard, Device::Mouse] {
        let Some(meta) = metas.iter().find(|m| m.device == device) else {
            println!("{}: 没有可用音色包，跳过", device.as_str());
            continue;
        };

        let (compiled, warnings) = profile::load(&meta.dir)?;
        for w in &warnings {
            eprintln!("  warn: {w}");
        }
        println!(
            "{}: {} | sources={} samples={}",
            device.as_str(),
            compiled.name,
            compiled.source_count(),
            compiled.sample_count()
        );
        audio.set_profile(device, Arc::new(compiled));
    }

    let keys = ["a", "s", "d", "f", "space", "enter", "backspace"];
    for key in keys {
        audio.play(Device::Keyboard, key, true);
        std::thread::sleep(Duration::from_millis(120));
        audio.play(Device::Keyboard, key, false);
        std::thread::sleep(Duration::from_millis(100));
    }

    std::thread::sleep(Duration::from_millis(200));
    for button in ["left", "right", "middle", "left"] {
        audio.play(Device::Mouse, button, true);
        std::thread::sleep(Duration::from_millis(260));
    }

    std::thread::sleep(Duration::from_millis(300));
    println!("selftest ok");
    Ok(())
}

/// 木鱼自检：不开界面、不需要辅助功能权限，只验证
/// 「输入进队列 → 固定节拍敲出 → 功德落盘」这条链路。
///
/// 用法：`keytone --muyu-selftest`
fn muyu_selftest() -> Result<(), String> {
    let dir = std::env::temp_dir().join("keytone-muyu-test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建测试目录失败: {e}"))?;

    let state = Arc::new(muyu::Muyu::load(
        &dir,
        true,
        true,
        0.9,
        muyu::DEFAULT_INTERVAL_MS,
        true,
        1.0,
        None,
    ));
    let audio = AudioHandle::spawn()?;
    audio.set_volume(1.0);

    let started = std::time::Instant::now();
    let player = audio.clone();
    muyu::spawn_metronome(state.clone(), move |delta, today, pending| {
        player.play_muyu(0.9);
        println!(
            "{:>5}ms  敲 {}  今日 {}  库存 {}",
            started.elapsed().as_millis(),
            delta,
            today,
            pending
        );
    });

    // 模拟一串不均匀的按键。输入时刻是随机的，敲击应该落在等距网格上。
    for gap in [0u64, 55, 40, 95, 30, 70, 45] {
        std::thread::sleep(Duration::from_millis(gap));
        state.hit();
    }
    println!("输入完成，库存 {}", state.pending());

    // 停下来 2s 才开始结算，再按 500ms 一下敲完，留足余量
    for _ in 0..120 {
        if state.pending() == 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    state.flush();

    let today = state.today();
    let left = state.pending();
    println!("结果：今日功德 {today}，剩余库存 {left}");
    if today != 7 || left != 0 {
        return Err(format!("期望敲 7 下且库存清空，实际 {today} 下、剩 {left}"));
    }

    let file = dir.join("muyu.json");
    let text = std::fs::read_to_string(&file).map_err(|e| format!("没有落盘: {e}"))?;
    println!("落盘 {}: {}", file.display(), text.replace('\n', " "));

    std::thread::sleep(Duration::from_millis(300));
    println!("muyu selftest ok");
    Ok(())
}
