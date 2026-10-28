//! 木鱼：把键盘 / 鼠标输入攒成库存，停下来之后匀速一下一下敲出来。
//!
//! 这里不碰 tauri，也不碰音频 —— 只负责三件事：
//!
//! 1. `hit()` 记一笔库存（输入线程调用）
//! 2. 一条常驻节拍线程：等到这段输入彻底停下来（`SETTLE_DELAY_MS`），
//!    再按 `interval_ms` 匀速往下敲，敲一下库存减一、功德加一
//! 3. 把「今日 / 累计功德」落盘
//!
//! 敲击之后要发声、要推动画，这些通过 `spawn_metronome` 的回调交给调用方。
//! 这样模块本身可以被单测，也不会和 AppState 互相纠缠。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// 内置采样。167ms，已裁掉首尾静音，见 docs/muyu-design.md。
pub const SAMPLE_WAV: &[u8] = include_bytes!("../../assets/muyu/muyu.wav");

/// 敲击间隔，默认半秒一次。设置页可以调。
pub const DEFAULT_INTERVAL_MS: u64 = 500;
pub const MIN_INTERVAL_MS: u64 = 150;
pub const MAX_INTERVAL_MS: u64 = 3000;

/// 输入停下来多久才开始结算。太短会在词组之间误触发，太长又显得没反应。
pub const SETTLE_DELAY_MS: u64 = 2000;

#[derive(Serialize, Deserialize, Default)]
struct Stored {
    #[serde(default)]
    date: String,
    #[serde(default)]
    today: u64,
    #[serde(default)]
    total: u64,
}

pub struct Muyu {
    enabled: AtomicBool,
    sound: AtomicBool,
    volume: Mutex<f32>,
    interval_ms: AtomicU64,
    pending: AtomicU32,
    /// 第一笔待敲出现的时刻。结算倒计时从这里起算，而不是最后一次输入。
    settle_at: Mutex<Option<Instant>>,
    today: AtomicU64,
    total: AtomicU64,
    /// 本地日期，跨天时把 today 清零
    date: Mutex<String>,
    dirty: AtomicBool,
    path: PathBuf,
    /// 桌面挂件是否显示（关掉只计数，不占屏幕）
    window_visible: AtomicBool,
    /// 挂件整体缩放，0.7–1.5
    scale: Mutex<f32>,
    /// 挂件位置（物理像素），None 表示贴右下角
    pos: Mutex<Option<(i32, i32)>>,
}

impl Muyu {
    pub fn load(
        data_dir: &Path,
        enabled: bool,
        sound: bool,
        volume: f32,
        interval_ms: u64,
        window_visible: bool,
        scale: f32,
        pos: Option<(i32, i32)>,
    ) -> Self {
        let path = data_dir.join("muyu.json");
        let stored = fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<Stored>(&t).ok())
            .unwrap_or_default();

        let today_date = local_date();
        // 隔天启动：昨天的 today 不算今天的
        let (date, today) = if stored.date == today_date {
            (stored.date, stored.today)
        } else {
            (today_date, 0)
        };

        Self {
            enabled: AtomicBool::new(enabled),
            sound: AtomicBool::new(sound),
            volume: Mutex::new(volume),
            interval_ms: AtomicU64::new(
                interval_ms.clamp(MIN_INTERVAL_MS, MAX_INTERVAL_MS),
            ),
            pending: AtomicU32::new(0),
            settle_at: Mutex::new(None),
            today: AtomicU64::new(today),
            total: AtomicU64::new(stored.total),
            date: Mutex::new(date),
            dirty: AtomicBool::new(false),
            path,
            window_visible: AtomicBool::new(window_visible),
            scale: Mutex::new(scale.clamp(0.7, 1.5)),
            pos: Mutex::new(pos),
        }
    }

    pub fn window_visible(&self) -> bool {
        self.window_visible.load(Ordering::Relaxed)
    }

    pub fn set_window_visible(&self, on: bool) {
        self.window_visible.store(on, Ordering::Relaxed);
    }

    pub fn scale(&self) -> f32 {
        *self.scale.lock().unwrap()
    }

    pub fn set_scale(&self, s: f32) {
        *self.scale.lock().unwrap() = s.clamp(0.7, 1.5);
    }

    pub fn pos(&self) -> Option<(i32, i32)> {
        *self.pos.lock().unwrap()
    }

    pub fn set_pos(&self, p: Option<(i32, i32)>) {
        *self.pos.lock().unwrap() = p;
    }

    /// 记一笔库存。关掉时不计数，避免下次打开炸出一串。
    ///
    /// 0 → 1 的那一下要记下时刻：结算倒计时从这里起算。
    pub fn hit(&self) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }
        let prev = self.pending.fetch_add(1, Ordering::AcqRel);
        if prev == 0 {
            *self.settle_at.lock().unwrap() = Some(Instant::now());
        }
    }

    /// 从第一笔待敲出现到现在过了多久。还没开始攒则返回 None。
    pub fn waiting_for(&self) -> Option<Duration> {
        self.settle_at.lock().unwrap().map(|t| t.elapsed())
    }

    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::SeqCst);
        if !on {
            // 关闭即清空库存：不然重新打开会把攒着的全敲一遍
            self.pending.store(0, Ordering::SeqCst);
            *self.settle_at.lock().unwrap() = None;
        }
    }

    pub fn set_sound(&self, on: bool) {
        self.sound.store(on, Ordering::Relaxed);
    }

    pub fn set_volume(&self, v: f32) {
        *self.volume.lock().unwrap() = v.clamp(0.0, 1.0);
    }

    pub fn set_interval(&self, ms: u64) {
        self.interval_ms
            .store(ms.clamp(MIN_INTERVAL_MS, MAX_INTERVAL_MS), Ordering::Relaxed);
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn sound(&self) -> bool {
        self.sound.load(Ordering::Relaxed)
    }

    pub fn volume(&self) -> f32 {
        *self.volume.lock().unwrap()
    }

    pub fn interval_ms(&self) -> u64 {
        self.interval_ms.load(Ordering::Relaxed)
    }

    pub fn pending(&self) -> u32 {
        self.pending.load(Ordering::Relaxed)
    }

    pub fn today(&self) -> u64 {
        self.today.load(Ordering::Relaxed)
    }

    pub fn total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    /// 今日清零（手动重置）。
    pub fn reset_today(&self) {
        self.today.store(0, Ordering::SeqCst);
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// 敲一下：库存减一、功德加一。跨天则先清零。
    ///
    /// 严格一次一下 —— 之前为了防积压做过「一次敲掉好几笔」的合并，
    /// 结果就是有的地方弹 +1、有的地方弹 +3，听感和动画都不匀速，去掉。
    fn strike(&self) -> (u32, u64) {
        self.roll_date();

        let pending = self.pending.load(Ordering::Acquire);
        if pending == 0 {
            return (0, self.today.load(Ordering::Relaxed));
        }

        self.pending.fetch_sub(1, Ordering::AcqRel);
        let today = self.today.fetch_add(1, Ordering::AcqRel) + 1;
        self.total.fetch_add(1, Ordering::AcqRel);
        self.dirty.store(true, Ordering::Relaxed);

        (1, today)
    }

    /// 跨天检查。整夜不关也没关系，下一次敲击时判定即可。
    fn roll_date(&self) {
        let now = local_date();
        let mut date = self.date.lock().unwrap();
        if *date != now {
            *date = now;
            self.today.store(0, Ordering::SeqCst);
            self.dirty.store(true, Ordering::Relaxed);
        }
    }

    /// 落盘。先写临时文件再 rename，避免掉电留下半截 JSON。
    pub fn save(&self) {
        if !self.dirty.swap(false, Ordering::AcqRel) {
            return;
        }

        let stored = Stored {
            date: self.date.lock().unwrap().clone(),
            today: self.today.load(Ordering::Relaxed),
            total: self.total.load(Ordering::Relaxed),
        };

        let Ok(text) = serde_json::to_string_pretty(&stored) else {
            return;
        };

        let tmp = self.path.with_extension("json.tmp");
        if fs::write(&tmp, text).is_err() {
            return;
        }
        // Windows 的 rename 不能覆盖已存在的文件，必须先删目标再改名，
        // 否则功德永远写不进去（第一次之后每次都失败）。
        if fs::rename(&tmp, &self.path).is_err() {
            let _ = fs::remove_file(&self.path);
            let _ = fs::rename(&tmp, &self.path);
        }
    }

    /// 退出前强制落盘，不受 dirty 标记限制。
    pub fn flush(&self) {
        self.dirty.store(true, Ordering::Relaxed);
        self.save();
    }
}

fn local_date() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// 启动结算线程。`on_strike(delta, today, pending)` 在每次敲击后调用，
/// 由调用方负责发声和推动画 —— 放在别的线程里做，节拍才不会被音频阻塞。
///
/// 两段式：
///
/// 1. **等待**：输入还在继续（`idle_for() < SETTLE_DELAY_MS`）就什么都不做，
///    只是攒库存。用户打完一段、停手两秒，才算「这一段结束」。
/// 2. **结算**：从这一刻起按 `interval_ms` 匀速往下敲，一下一笔，敲空为止。
///    结算期间又来了新输入，不打断、不清零，继续往下敲（新账直接进队列）。
pub fn spawn_metronome<F>(state: std::sync::Arc<Muyu>, mut on_strike: F) -> thread::JoinHandle<()>
where
    F: FnMut(u32, u64, u32) + Send + 'static,
{
    thread::Builder::new()
        .name("kbs-muyu".into())
        .spawn(move || {
            let mut next = Instant::now();
            let mut last_save = Instant::now();
            let mut settling = false;

            loop {
                // 没开 / 没库存：回到等待态
                if !state.enabled() || state.pending() == 0 {
                    settling = false;
                    *state.settle_at.lock().unwrap() = None;
                    next = Instant::now();
                    thread::sleep(Duration::from_millis(120));
                    if last_save.elapsed() >= Duration::from_secs(5) {
                        state.save();
                        last_save = Instant::now();
                    }
                    continue;
                }

                // 倒计时从「第一笔待敲出现」开始算，不是从最后一次输入。
                // 也就是说：一开始打字，2 秒后木鱼就开工，边打边敲，
                // 新输入只是往队列里续，不会把倒计时往后推。
                if !settling {
                    match state.waiting_for() {
                        Some(elapsed) if elapsed >= Duration::from_millis(SETTLE_DELAY_MS) => {
                            settling = true;
                            next = Instant::now();
                        }
                        _ => {
                            thread::sleep(Duration::from_millis(60));
                            continue;
                        }
                    }
                }

                // 精确等到下一个节拍点。用「累加 + sleep 差值」而不是固定 sleep，
                // 否则每次都会多出几毫秒，一分钟下来就飘了。
                let now = Instant::now();
                if next > now {
                    thread::sleep(next - now);
                }

                // 睡醒后重新确认：这一拍等的时间里可能被关掉或已清空
                if !state.enabled() || state.pending() == 0 {
                    settling = false;
                    *state.settle_at.lock().unwrap() = None;
                    next = Instant::now();
                    continue;
                }

                let (n, today) = state.strike();
                if n > 0 {
                    on_strike(n, today, state.pending());
                }

                next += Duration::from_millis(state.interval_ms());

                // 线程被系统挂起（睡眠唤醒）后不补拍，直接对齐到当前时刻，
                // 否则一睁眼会「咚咚咚咚」炸一串。
                if next < Instant::now() {
                    next = Instant::now() + Duration::from_millis(state.interval_ms());
                }

                if last_save.elapsed() >= Duration::from_secs(5) {
                    state.save();
                    last_save = Instant::now();
                }
            }
        })
        .expect("无法启动木鱼节拍线程")
}
