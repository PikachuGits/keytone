# 木鱼功能设计（v0.3 提案 · 第三版）

> 一句话：键盘和鼠标只负责往队列里塞次数，木鱼按**固定节拍**把队列里的次数一下一下敲完。
> 输入时刻是随机的，敲击时刻是等距的，两者天然错开。

---

## 1. 体验定义

| 维度 | 设计 |
|---|---|
| 计数 | 每次键盘按下、每次鼠标按键按下，往队列里塞 1 下 |
| 出声时机 | **只要有库存就开始敲**，不等停顿、不做防抖 |
| 节奏 | **固定节拍**，一拍一下，与输入频率无关 |
| 延迟 | 延迟来自队列本身：这一下可能在你按完之后 0–1 拍才敲出来 |
| 视觉 | 每敲一下：木鱼挤压、木槌甩回、`+1` 上浮、今日功德 +1 |
| 声音 | 与敲击同步，与键盘音效同 mixer 叠加，互不打断 |

**为什么这样能"错开"**

键盘音是**随输入时刻**响的，一串不规则的脉冲；木鱼音是**固定网格**上的等距脉冲。
两套时间轴相位不同，绝大多数敲击都落在两次按键的间隙里，听感上就是两个独立的声部，
而不是每按一下跟着"咚"一声——后者才是真的会和键盘音糊在一起。

延迟不是刻意加的定时器，而是队列的自然结果：你打得比木鱼快，库存就涨；停下来，木鱼还在继续敲。
「累计数延迟更新」也是同一个道理——功德数字只在**敲击那一刻**才 +1，输入瞬间不动。

---

## 2. 数据流

```
输入钩子 ──► 队列 pending += 1（不出声、不加数字）
                  │
            固定节拍（170ms 一拍）
                  │  pending > 0 就敲
                  ▼
    strike() ─┬─► 木鱼音（同一个 mixer）
              ├─► muyu-hit 事件 → 挂件动画 +1
              └─► 今日功德 += 1 → muyu.json
```

---

## 3. 计数规则

**计入**

- 键盘 `pressed = true`（修饰键也算）
- 鼠标左 / 右 / 中 / 侧键的 `pressed = true`

**不计入**

- 键盘抬起事件（否则一次按键算两笔）
- 长按连发：消费线程里已有 `down` 表去重，沿用
- 滚轮、鼠标移动

**KeyTone 自身窗口在前台时不计**（与现有「本应用前台不发声」一致），提供开关可关掉这个限制。

**独立性**：木鱼计数不依赖键盘音效是否开启。只要「木鱼」或「音效」任一开启，钩子就要跑。
现在 `apply_enabled_inner` 硬性要求「键盘音色包已加载」，木鱼不需要音色包，这个条件要放宽。

---

## 4. 停顿后匀速结算（核心）

### 4.1 参数

| 参数 | 默认 | 说明 |
|---|---|---|
| `interval_ms` | 500 | 每敲一下的间隔，设置页 150–2000ms 可调 |
| `SETTLE_DELAY_MS` | 2000 | 从**第一笔待敲出现**算起的等待时间（常量） |

没有库存上限、没有合并、没有自适应节奏。

### 4.2 状态

```
pending: u32        库存（输入 +1，敲击 -1）
last_input: Instant 最后一次输入的时刻
settling: bool      是否已经在结算
next_tick: Instant  下一个敲击点
```

### 4.3 流程

```
收到输入：
    pending += 1
    last_input = now

结算线程：
    if pending == 0 or !enabled:  settling = false; 睡 120ms; continue

    // ① 等待：倒计时从第一笔库存出现算起，不是最后一次输入
    if !settling:
        if settle_at.elapsed() < 2000ms: 睡 60ms; continue
        settling = true; next_tick = now

    // ② 结算：匀速往下敲
    sleep_until(next_tick)
    pending -= 1
    today += 1
    play_sound(); emit("muyu-hit", { delta: 1, today, pending })
    next_tick += interval
    if next_tick < now: next_tick = now + interval   // 落后太多就重对齐，不补拍
```

**三条硬规则**

- **第一笔库存出现后 2 秒**开始结算 —— 不是「停手 2 秒」。边打边敲，新输入只往队列里续，
  不会把倒计时往后推
- **一下一笔，严格 1:1** —— `pending` 减 1、`today` 加 1，永远不合并成 `+2`、`+3`
- **匀速** —— 敲击点由 `next_tick += interval` 累加，不受输入时刻影响

### 4.4 为什么去掉「库存合并」

早期版本为了防积压，超过 12 笔就把多出来的并进当前一敲。结果同一段输入里
有的地方弹 `+1`、有的地方弹 `+3`，视觉上忽快忽慢，听感也不匀。
现在改成严格 1:1：库存攒多少就敲多少下，慢一点但节奏是干净的。

代价是一次长输入会敲很久（500ms 一下，60 笔就是 30 秒）。这是刻意的取舍 ——
它本来就不是「实时反馈」，而是一段输入结束后的回向。

### 4.5 线程模型

一条常驻 `kbs-muyu` 线程，三种状态：

| 状态 | 行为 | 唤醒频率 |
|---|---|---|
| 空闲（没开 / 库存空） | 睡 120ms，顺便 5s 落盘一次 | ~8 次/秒 |
| 等待（有库存但输入未停） | 睡 60ms 轮询 `idle_for()` | ~16 次/秒 |
| 结算中 | `sleep_until(next_tick)`，精确到毫秒 | 每 `interval` 一次 |

结算用「累加 + sleep 差值」而不是固定 `sleep(500ms)`，否则每次多几毫秒，
一分钟下来就飘到听得出偏差。

---

## 5. 音频方案（关键：不与键盘音打架）

现有引擎是单 `MixerDeviceSink` + `mixer().add()`，多音源天然叠加。
**木鱼走同一条路就不会打断键盘音**，不需要任何新机制。

要避开的两个坑：

1. **不要为木鱼新开输出流**——两条 stream 抢同一设备，系统重新协商采样率，容易把另一条切掉
2. **不要用独立 `Sink` 再 `append`**——`Sink` 是队列语义，同 Sink 内会排队，反而制造延迟

```rust
enum Cmd { /* ... */ Muyu { gain: f32 } }

struct Engine { /* ... */ muyu: Vec<SamplesBuffer> }

fn play_muyu(&mut self, gain: f32) {
    let buf = &self.muyu[self.next_rand() as usize % self.muyu.len()];
    self.sink.mixer().add(buf.clone().amplify(gain));   // 与键盘音同一个 mixer
}
```

**音源**：内置真实采样 `assets/muyu/muyu.wav`，编译进二进制（与音色包同一套 `include_dir` 机制）。
用户可覆盖：把 `muyu.wav` 放进 `~/Library/Application Support/com.keytone.app/`，启动时优先加载。

实测参数（已下载并验证）：

| 项 | 原始 mp3 | 裁切后 wav |
|---|---|---|
| 时长 | 287 ms | **167 ms** |
| 格式 | 44.1kHz / 立体声 / 128kbps | 44.1kHz / 立体声 / PCM 16bit |
| 峰值 | −0.9 dB | −0.9 dB（线性 0.901） |
| 均值 | — | −20.3 dB |
| 体积 | 5.3 KB | 29 KB |

**必须裁切**：原始文件前 30ms 是静音、后 105ms 是尾巴。
30ms 前导静音在 170ms 的节拍下等于每一敲都晚 30ms，105ms 的尾巴会让相邻两敲糊在一起。
裁切策略：`atrim 28ms → 195ms`，末尾 40ms 做淡出，保留 13ms 的自然衰减。

**备用方案**：若采样丢失，退回 `synth::render_muyu()` 合成音（f1 520/560/600Hz、泛音 ×2.72、τ 55ms、前 1.5ms 白噪声瞬态）。
只作为兜底，音色明显不如真实采样。

**音量**：木鱼独立音量 `muyu_volume`（默认 0.85），实际增益 = 主音量 × `muyu_volume`。
语音播报走 `AVSpeechSynthesizer`，是另一条系统通道，不冲突。

---

## 6. 桌面挂件窗口

### 6.1 窗口参数

| 项 | 值 |
|---|---|
| label | `muyu` |
| 尺寸 | 基准 240 × 320，整体可缩放 70%–150%（设置页滑块，窗口尺寸 = 基准 × scale） |
| 样式 | `transparent`、`decorations(false)`、`shadow(false)`、`alwaysOnTop`、`skipTaskbar`、`focus(false)`、`resizable(false)` |
| 穿透 | `set_ignore_cursor_events(true)`，不挡任何点击 |
| 位置 | 默认右下角，距工作区边缘 24px，按 `monitor.work_area` × `scale_factor` 换算 |
| 生命周期 | 开关打开才创建，关闭即 `destroy()`（省 30–50MB） |

macOS 注意：`tauri.conf.json` 需要 `app.macOSPrivateApi: true`，否则透明窗口会渲染成黑底。

### 6.2 素材

从一张黑底产品图里抠出来，按**色温**而不是亮度做遮罩：

- 背景是纯中性灰（R = B），木头是暖色（R − B ≈ 0.22–0.34）
- 所以 `alpha = clamp((R − B − 0.025) / 0.09)`，背景的灰色光晕天然被排除
- 按亮度抠的话，木鱼嘴里的暗槽会被一起抠掉；按色温抠能保住它
- 边缘像素反预乘还原真色，避免白底上出现黑边

| 文件 | 尺寸 | 说明 |
|---|---|---|
| `assets/muyu/src/muyu.png` | 320 × 206 | 木鱼，透明底 |
| `assets/muyu/src/mallet.png` | 311 × 49 | 木槌，透明底 |
| `dist/muyu-fish.png` / `dist/muyu-mallet.png` | 同上 | 挂件实际加载的副本 |

### 6.3 动画规范（已实现）

| 元素 | 动作 |
|---|---|
| 木鱼本体 | 命中 `scale(1.05, .94)` → 260ms `cubic-bezier(.2,.9,.3,1)` 回弹 |
| 木槌 | `rotate(-9deg)` 静止，18% 甩到 `-20deg`，34% 回 `-18deg`，340ms 一个来回 |
| 涟漪 | `scale(.7 → 1.9)`，`opacity .5 → 0`，420ms，单圈不叠 |
| `+1` 粒子 | 20px，从鱼身左上冒出，随机 `--x1`（−30…−4px）、`--y1`（−58…−76px），460ms 淡出 |
| 今日功德 | 标签 12px / 数字 32px，等宽，变化时 `scale(1.12) → 1`，200ms |
| 待敲提示 | `pending > 0` 显示「待敲 N」，淡入 120ms；归零清空 |
| 待机 | 4s 呼吸 `scale(1 → 1.015)`，纯 CSS，无 JS 定时器 |

**木槌的透视**：木鱼原图是斜上方俯视拍的，木槌是纯侧视。给木槌套了
`#mallet-arm`（`perspective`）+ `#mallet-tilt`（`rotateX`）两层，可以压扁到和木鱼同一个俯视平面。
当前参数里 `rotateX: 0`（用户调参后选择不压扁），两层结构保留着，改个数字就能开。

**支点与角度**：`transform-origin: 100% 50%`（手柄末端），槌头离支点约 113px，
所以**转 1° 槌头走约 2px**。当前静止 `-9°`、敲下 `-20°`（用户调参结果）。
角度上限很低 —— 试过 `-43°`，槌头直接埋进木鱼里看不见（两块都是深红木，颜色还几乎一样）。

### 6.4 连敲时的动画处理

170ms 一拍，挤压动画 260ms，会重叠。规则：

- 动画不排队：`remove class → 强制 reflow → add class` 直接重播，每一下都有反馈
- 粒子池 **6 个**节点循环复用（170ms 间隔 + 460ms 生命周期，峰值 3–4 个），超出回收最旧的
- 涟漪同一时刻只允许 1 圈，新的一下直接重置旧的那圈
- 数字变化用 `requestAnimationFrame` 合并，一批结算里最多触发一次跳动

### 6.5 底色适配

桌面壁纸不可控，所以计数区垫了一层 236×124 的径向暗角（中心 `rgba(22,13,9,.34)` 向外淡出），
文字另加三层阴影。实测在纯黑、中灰、浅米色三种底色下都读得清，且暗角在深色壁纸上几乎看不出来。

### 6.6 拖拽、缩放、显示隐藏

挂件是点击穿透的，所以它本身不能直接拖 —— 否则你在桌面上点任何东西都会被它吃掉。

**调整位置**：设置页点「调整位置」→ 后端把 `ignore_cursor_events` 关掉，前端显示
「拖动我 · 松手保存」，然后由前端自己算坐标：

```
pointerdown → 记下窗口位置和指针屏幕坐标
pointermove → target = 起始位置 + (screenX - 起始 screenX) * scaleFactor
              requestAnimationFrame 里 setPosition(target)
pointerup   → 通知后端收尾
```

**为什么不用 `data-tauri-drag-region`**：无边框窗口（`decorations: false`）的
`canBecomeKeyWindow` 返回 false，tao 没有覆写它，原生拖拽起不来。自己调 `setPosition`
不依赖窗口是不是 key window。

**踩过的坑**：收尾线程最初用 `let mut last = None` 起步，第一轮轮询
「当前坐标 != None」就判定成"动过了"，600ms 后直接把穿透恢复 —— 用户还没按下鼠标，
拖动模式已经退出。现在起始坐标必须先从窗口读出来。

**缩放**：`--ui-scale = window.innerWidth / 240`，舞台整体 `transform: scale()`，
内部坐标一个都不用改。窗口尺寸由 Rust 按 `240 × scale` 算，滑块 70%–150%。

**显示隐藏**：`muyu_window` 开关。关掉只是销毁窗口，计数照常走 —— 想安静地攒功德时用。

**跨 Space 可见**：这是最难缠的一个坑，根因不在窗口本身。

现象：切到另一个桌面，挂件就不见了。

排查过程：
- `collectionBehavior` 里 `CanJoinAllSpaces` 确实设上了（实测 `1 -> 337`）
- 但 `CGWindowListCopyWindowInfo` 里 `onscreen=false`，`isOnActiveSpace=false`
- 写了个最小 Swift 程序，同样的配置（各种 level）`onActiveSpace=true` —— 说明 API 没问题
- 把 FFI 全关掉、纯 Tauri 默认（level=5）也是 `onscreen=false`

**真正的原因**：应用的激活策略是 `.regular`（有 Dock 图标）。应用不在前台时，
新建的窗口会被丢到「应用自己的那个 Space」，而不是用户当前正在看的 Space。
菜单栏常驻应用必须用 `.accessory`：

```rust
app.set_activation_policy(tauri::ActivationPolicy::Accessory);
```

改完立刻 `onscreen=true`。副作用是应用不再出现在 Dock —— 对菜单栏工具来说这才是正常形态。

配合的另外两条：
- 窗口层级用 `NSModalPanelWindowLevel`（8），不是 `NSStatusWindowLevel`（25）
- `show()` 之后补一刀 `orderFrontRegardless:`，绕开 tao 的 `orderFront:`（后者看激活状态）

**全屏应用上也要看得见**：这是另一个坑，两道门槛缺一不可。

1. `set_visible_on_all_workspaces(true)` → `NSWindowCollectionBehaviorCanJoinAllSpaces`，
   解决「切个桌面挂件就消失」
2. 但全屏应用会新建一个 Space，窗口还是会被挡在后面。Tauri 的 `always_on_top`
   只是 floating level（3），而全屏窗口的层级比它高。必须再补：
   - `NSWindowCollectionBehaviorFullScreenAuxiliary`（`1 << 8`）
   - `setLevel(25)`（`NSStatusWindowLevel`）

tao 两个都没暴露，只能拿 `ns_window()` 的指针直接 `msg_send!`。加完之后
`CGWindowListCopyWindowInfo` 里 `layer=25`、`onscreen=true`，全屏视频上也能看见。

### 6.7 素材可替换

木鱼和木槌不是写死的：数据目录下的 `muyu/` 里放两张图，用户换同名文件就能用自己的。

- 首次运行把内置的两张 PNG 写出去（`fish.png` / `mallet.png`），用户打开目录就能看到要换什么
- 支持 png / jpg / webp / gif，按魔数判 MIME，单张上限 8MB
- 后端读成 data URL，通过 `get_muyu_assets` 交给前端塞进 `<img src>`
  —— 不走 `asset://` 协议，省掉 scope 配置，也不受路径限制
- 设置页有「打开素材目录」和「重新加载」；换完图点重新加载，挂件重新取一次，不用重启

布局按 240×320 的画布：木鱼 172px 宽、top 79，木槌 145px 宽、left 84 / top 18。
用户换了比例不同的图，位置不会自动适配 —— 这是已知限制。

### 6.8 调参页

`dist/muyu-tuner.html`：左边 iframe 实时预览挂件，右边滑杆调所有布局参数
（舞台尺寸、木鱼位置/大小、木槌位置/宽度/透视/挥动角度、涟漪、粒子、字号），
底部「复制参数」输出 JSON，粘回代码即可。

为了让调参页能实时改，挂件里几个数值抽成了 CSS 变量：
`--pop-x` / `--pop-y` / `--pop-size` / `--pop-rise`（粒子的起点、字号、上浮距离）。
其余（位置、角度）由调参页直接写内联样式覆盖。

**注意**：调参页是 iframe 嵌 `muyu.html`，必须走 http 打开（同源策略），
直接双击 file:// 会读不到 iframe 里的 DOM。

### 6.9 怎么关掉它

点击穿透意味着挂件本身不可交互。关闭入口：主窗口开关、菜单栏「显示木鱼」。
拖动位置用临时模式：设置里点「调整位置」→ 穿透关闭 8 秒，拖完自动恢复。

---

## 7. 持久化

`~/Library/Application Support/com.keytone.app/muyu.json`

```json
{ "date": "2026-10-28", "today": 328, "total": 12480 }
```

- 内存原子变量，输入只改内存，**不写盘**
- 后台每 5s 检查 dirty 标记，有变化才写；退出前强制 flush
- 写盘用 `tmp` + `rename` 原子替换
- 跨天：敲击时比较本地日期（新增 `chrono`），跨了就 `today = 0` 并推 `muyu-reset`
- 用 `Instant` 做节拍判定，不受系统时间调整影响

---

## 8. IPC 与命令

**事件**

- `muyu-hit` → 只发给 `muyu` 窗口：`{ delta, today, pending }`
  - 每敲一下一条；`delta` 通常是 1，库存溢出时可能是 2–3
- `muyu-pending` → 库存变化（节流 200ms），驱动「待敲 N」
- `muyu-reset` → 跨天清零
- `muyu-state` → 开关变化时广播给主窗口

**命令**

| 命令 | 说明 |
|---|---|
| `get_muyu()` | 全部配置 + 今日/累计功德 + 当前 pending |
| `set_muyu(patch)` | 局部更新，返回新状态 |
| `reset_muyu_today()` | 今日清零 |
| `preview_muyu()` | 直接敲一下 + 弹一次，给设置页「试听」 |
| `set_muyu_position(pos)` | 四角预设 / 自定义坐标 |

`get_status` 增加 `muyu_today`，概览页直接用。

---

## 9. 设置项

```rust
muyu_enabled:   bool   = false,  // 总开关
muyu_window:    bool   = true,   // 显示桌面挂件
muyu_sound:     bool   = true,   // 木鱼声
muyu_volume:    f32    = 0.85,
muyu_interval_ms: u64  = 500,    // 敲击间隔，150–2000
muyu_position:  String = "br",   // tl / tr / bl / br / custom
muyu_size:      String = "md",   // sm / md / lg
muyu_max_backlog: u32  = 12,
muyu_in_app:    bool   = false,  // 在 KeyTone 窗口内也计数
```

### 界面：侧栏新增「木鱼」页

1. **总开关** + 一句话解释（「敲键盘即敲木鱼，按固定节拍响」）
2. **桌面挂件**：显示开关 / 位置四角 / 大小 / 「调整位置」
3. **敲击**：频率滑块（150–2000ms，默认 500）+ 音量 + 试听
4. **今日功德**：大数字 + 累计 + 「重置今日」

概览页统计卡旁加一个「今日功德」。

---

## 10. 代码改动清单

| 文件 | 改动 | 规模 |
|---|---|---|
| `src-tauri/src/muyu.rs` | **新增**：队列、节拍线程、持久化 | ~200 行 |
| `assets/muyu/muyu.wav` | **已就位**：167ms 裁切采样 | — |
| `src-tauri/src/bundled.rs` | 增加 `include_bytes!("../../assets/muyu/muyu.wav")` | ~10 行 |
| `src-tauri/src/synth.rs` | 兜底 `render_muyu()`（采样缺失时用） | ~50 行 |
| `src-tauri/src/audio.rs` | `Cmd::Muyu`、`Engine.muyu`、`play_muyu`、启动加载 | ~40 行 |
| `src-tauri/src/config.rs` | 9 个新字段 + 默认值 | ~28 行 |
| `src-tauri/src/main.rs` | 消费线程挂钩、窗口创建/销毁、5 个命令、退出 flush | ~180 行 |
| `src-tauri/tauri.conf.json` | `macOSPrivateApi: true` | 1 行 |
| `src-tauri/Cargo.toml` | 新增 `chrono` | 1 行 |
| `dist/muyu.html` | **已完成**：挂件页面（PNG 素材 + 原生 CSS 动画 + 粒子池 + 浏览器预览模式） | 390 行 |
| `dist/index.html` | 木鱼页、概览卡片、导航项 | ~150 行 |
| `README.md` | 特性段补一条 | ~10 行 |

后端约 500 行，前端约 350 行。沿用现有「单文件静态页 + 原生 JS」，不引入框架。

---

## 11. 边界与风险

| 风险 | 处理 |
|---|---|
| 挂件挡住全屏应用 / 演示 | 菜单栏一项即可隐藏 |
| 节拍线程空转耗电 | 空闲 `park_timeout(interval)`，不做任何计算 |
| 库存无限增长 | `max_backlog = 12` 合并阀，账目仍精确 |
| 睡眠唤醒后炸一串 | `next_tick` 重对齐，不补拍 |
| 第二个 WebView 占内存 | 关闭开关即 `destroy()`，不是隐藏 |
| 点击穿透后无法拖动 | 临时「调整位置」模式，8 秒后恢复 |
| 多显示器 / 分辨率变化 | 监听 `scale_factor_changed`，按 `work_area` 重算 |
| 木鱼开、音效关时的权限提示 | 现有「需要辅助功能权限」文案要覆盖木鱼 |
| 跨天时库存未清 | 先敲完再清零，或直接把库存归入前一天 |

---

## 12. 分期

- **P0**：队列 + 固定节拍 + 挂件 + 采样音 + 菜单栏开关（已实现）
- **P1**：节拍三档、音量、位置、大小、试听、概览卡片（待做）
- **P2**：自定义 `muyu.wav`、拖动调整位置、全屏自动隐藏、减少动态偏好（待做）

---

## 13. 实现状态

**已完成**

| 模块 | 文件 | 说明 |
|---|---|---|
| 队列 + 节拍线程 + 落盘 | `src-tauri/src/muyu.rs` | `hit()` 入队；节拍线程 170ms 一拍；`muyu.json` 节流落盘 |
| 采样播放 | `src-tauri/src/audio.rs` | `Cmd::Muyu`，与键盘音共用同一个 mixer |
| 挂件窗口 | `src-tauri/src/main.rs` | `ensure_muyu_window`：透明 / 置顶 / 穿透 / 贴右下角 |
| 菜单栏开关 | `src-tauri/src/main.rs` | 托盘新增「木鱼」勾选项，独立于「启用按键音」 |
| 挂件页面 | `dist/muyu.html` | 敲击动画 + 粒子池 + Tauri/浏览器双模式 |
| 素材 | `assets/muyu/` | 采样 wav + 抠图 PNG |

**钩子与开关解耦**

原来 `apply_enabled_inner` 里内联启动钩子，只有按键音能启动。现在抽成 `ensure_hook`：

```
按键音开 ─┐
          ├─► ensure_hook()  ← 谁先开谁启动，后开的复用
木鱼开  ─┘
```

消费线程里木鱼计数放在「按键音开关」判断之前，所以只开木鱼、不开音效也能计数。

**自检**

```sh
keytone --muyu-selftest    # 输入 7 次 → 敲 7 下 → 落盘，实测节拍 170/172/170/169/174/166ms
keytone --muyu-window      # 只弹挂件，单独调外观
```

**尚未接入**

拖动调整位置、全屏自动隐藏、自定义 `muyu.wav` 覆盖还没做。

**升级注意：辅助功能授权会失效**

**已修复**。原先用 ad-hoc 签名（`signingIdentity: "-"`），指定要求是 `cdhash H"..."`，
换一次二进制就变一次，TCC 授权随之作废 —— 表现是升级后按键音和木鱼都"没反应"。

现在改用钥匙串里的自签名证书 `KeyTone Local Signing`：

```
designated => identifier "com.keytone.app" and certificate root = H"533f0e70..."
```

只跟证书有关，与代码版本无关，所以重打包不会再掉授权。
（切签名的那一次仍然要手动授权，因为旧的 cdhash 授权已经对不上了。）

`tccutil reset` 更彻底，但沙箱里跑不了，只能手动在辅助功能里删掉重加。

