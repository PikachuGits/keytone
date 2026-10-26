# KeyTone

> 打字即演奏 —— 一个常驻菜单栏的键盘 / 鼠标音效引擎。

把每一次敲击和点击都变成声音：机械轴的清脆、打字机的咔哒、老式薄膜键盘的闷响。
声音在内存里预解码，按下到出声的延迟控制在 10ms 级别，打字时不会有"音画不同步"的迟滞感。

![深色界面](docs/theme-dark.png)

| 深色 | 浅色 |
|---|---|
| ![深色](docs/theme-dark.png) | ![浅色](docs/theme-light.png) |

---

## 特性

**声音**

- 键盘与鼠标各自独立的音色包，可分别切换
- 每个键位可绑定专属音色；同一个键支持多个采样变体，播放时随机挑选，避免重复感
- 峰值归一化：不同音色包的录音电平差异最大能到 10 倍，加载时统一归一到接近满刻度，
  切包不会再出现"这个听不见、那个炸耳朵"
- 试听：选中音色包立即播放，也可以单独点「试听」按钮，全程不需要系统权限
- 内置 21 个音色包，编译进二进制，首次启动自动安装

**系统集成**

- 全局捕获键盘与鼠标（含修饰键、侧键），在任何应用里都生效
- 常驻菜单栏，点关闭只是收进托盘，不退出进程
- 关闭窗口时销毁 WebView 而不只是隐藏，常驻内存明显下降
- 深色 / 浅色 / 跟随系统三档主题，菜单栏图标支持单色模板与品牌金两种样式
- 键位语音播报（中 / 英），用系统 TTS 实时合成，不预生成音频文件
- 检查更新：读取 GitHub Releases 比对版本

**避免打扰**

- 本应用窗口在前台时不发声 —— 你点自己的界面、在自己的窗口里打字，不该被自己的音效吵到
- 语音只在按下时播报，抬起不重复；快速打字时打断上一句，保证读的是最新按下的键

---

## 技术栈

| 层 | 选型 |
|---|---|
| 应用框架 | Tauri 2 |
| 后端 | Rust（2021 edition） |
| 前端 | 单文件静态 HTML + 原生 CSS/JS，无框架、无构建步骤 |
| 音频 | rodio / cpal |
| 输入捕获 | macOS `CGEventTap` |
| 语音 | `AVSpeechSynthesizer` |

前端是 `dist/index.html` 一个纯静态页，编译期由 Tauri 内嵌进二进制。
**不需要 `npm install`，也不需要打包前端** —— 改完 HTML 直接 `cargo build` 即可。

---

## 快速开始

需要 Rust 1.77+ 和 macOS 13+。

```sh
git clone https://github.com/PikachuGits/keytone.git
cd keytone/src-tauri
cargo build --release
```

先单独验证音频链路（不需要任何系统权限，会依次播放一组按键声）：

```sh
./target/release/keytone --selftest
```

然后启动应用，点右上角开关启用。

### 首次使用要授权

macOS 不允许应用静默监听全局输入，必须手动开启辅助功能权限：

1. 启动应用，点开关
2. 系统弹出授权提示，前往「系统设置 → 隐私与安全性 → 辅助功能」
3. 勾选 KeyTone
4. 回到应用，再点一次开关

> **每次重新编译后授权都会失效** —— TCC 认的是二进制签名，重新编译签名就变了。
> 日常使用建议 `cargo build --release` 一次之后不要再重编，否则每次都要重新勾选。

### 授权了却一直显示「未授权」？

按顺序排查：

1. **确认 app 是被签名过的**。TCC 的辅助功能授权必须绑定一个稳定的代码签名身份，
   `.app` 包如果没有签名（只有内层可执行文件带链接器的 ad-hoc 签名），
   系统无法把授权和这个 app 关联起来，勾选后 `AXIsProcessTrusted()` 依然返回 false。

   ```sh
   codesign --verify --verbose=2 /Applications/KeyTone.app
   # 正常输出：valid on disk / satisfies its Designated Requirement
   ```

   如果报 `code object is not signed at all`，说明构建时没签名 —— 本仓库的
   `tauri.conf.json` 里 `bundle.macOS.signingIdentity` 设为 `"-"` 即可，CI 也会在
   发布前校验签名，未签名直接失败。

2. **删掉旧条目重新添加**。签名变了（比如重装、重新编译）之后，
   系统设置里的旧勾选会变成僵尸条目，指向一个已经不存在的签名。
   在「辅助功能」列表里选中 KeyTone，点「−」删掉，再点「＋」重新添加 `/Applications/KeyTone.app`。

3. **确认没有跑在 DMG 里**。直接从磁盘映像双击运行会触发 App Translocation，
   系统会把它挂到一个随机临时路径下，授权绑在那个路径上，下次启动就失效。
   必须先拖进 `/Applications` 再运行。

4. 授权后**切回应用会自动刷新**（应用每 1.5 秒复查一次），不需要重启。

---

## 命令行

```sh
keytone --selftest                      # 音频链路自检，不需要权限
keytone --check-profiles                # 校验所有音色包，打印峰值与归一化增益
keytone --preview keyboard [包名]       # 命令行试听键盘音色包
keytone --preview mouse    [包名]       # 命令行试听鼠标音色包
keytone --speak zh "左键"               # 验证语音播报
keytone --import-profiles <目录>        # 批量导入外部音色包并逐个校验
```

`--check-profiles` 的输出形如：

```
logitech-g915-tkl-brown    keyboard  10 音源  20 采样  峰值 0.129 → 增益 7.38x
mx-brown                   keyboard   8 音源  16 采样  峰值 0.301 → 增益 3.16x
```

---

## 音色包

存放位置：

```
macOS    ~/Library/Application Support/com.keytone.app/profiles/
Linux    ~/.local/share/com.keytone.app/profiles/
Windows  %APPDATA%\com.keytone.app\profiles\
```

界面里点「打开目录」可直接跳转。每个音色包是一个子目录，里面放 `profile.yaml` 和音频文件。

### 格式

采用 MechVibes 风格的 YAML 结构：

```yaml
profile:
  name: my-keyboard
  author: your-name
  description: 我的键盘

sources:
  - id: key1
    source:
      press: press_key1.wav
      release: release_key.wav

keys:
  default: [ key1 ]
  other:
    - sound: enter
      keys: [ enter ]
```

在此基础上扩展了一点：`press` / `release` 可以写成**列表**，表示同一个键的多个采样变体，
播放时随机挑一个，长时间打字不会听出明显的循环感：

```yaml
    source:
      press: [ press_key1.wav, press_key2.wav, press_key3.wav ]
```

支持 `.wav` / `.mp3` / `.flac` / `.ogg`。

### 鼠标音色包

把 `keys` 换成 `buttons`，按钮名是 `left` / `right` / `middle`：

```yaml
profile:
  name: my-mouse
  device: mouse

sources:
  - id: click
    source:
      press: click.wav
  - id: right_click
    source:
      press: right.wav

buttons:
  default: click
  other:
    - sound: right_click
      buttons: [ right ]
```

也兼容 MechVibes 的数字键写法（`'1'` 左键 / `'2'` 中键 / `'3'` 右键），两种都会正确映射。
`device` 字段可以省略 —— 有 `buttons` 没有 `keys` 会自动推断为鼠标包。

鼠标目前只播放按下音（一次点击一个咔哒），滚轮暂不支持。

---

## 目录结构

```
src-tauri/src/
├── main.rs        # 入口、IPC command、托盘、事件消费线程
├── audio.rs       # 音频线程（cpal 的 Stream 是 !Send，必须独占线程）
├── profile.rs     # 音色包解析、解码、峰值归一化
├── synth.rs       # 内置合成音色包生成
├── keymap.rs      # 虚拟键码 → 规范键名
├── config.rs      # 设置持久化
├── speech.rs      # 语音播报
├── bundled.rs     # 内置音色包安装
└── hook/
    ├── mod.rs     # 平台抽象
    ├── macos.rs   # CGEventTap + 辅助功能授权
    ├── windows.rs # WH_KEYBOARD_LL
    └── linux.rs   # evdev
```

数据目录里 `settings.json` 保存音量、启用的音色包、主题、菜单栏样式等。

---

## 实现笔记

开发过程中几个不太显然的点，记下来省得下次再踩：

1. **`cpal::Stream` 在 macOS 上是 `!Send`** —— 音频设备不能跨线程持有，
   必须独占一个线程，外部通过 channel 下指令。
2. **rodio 的默认缓冲可能开不了流** —— 它按 `sample_rate / 20` 算缓冲（48kHz 下是 2048 帧），
   而部分设备的合法区间只有几百帧。需要按 `[480, 960, 256, 240]` 逐个降级试，480 帧正好 10ms。
3. **`CGEventTap` 的修饰键走 `FlagsChanged`** —— 不能当 keyDown 处理，
   必须读 flag 位判断按下还是抬起，否则 Shift / Cmd 会一直被判为按下。
4. **`AXIsProcessTrusted()` 永远不会弹授权框** ——
   必须用 `AXIsProcessTrustedWithOptions` 配 `kAXTrustedCheckOptionPrompt`。
5. **菜单栏图标被系统强制缩放到 18pt 高** —— 提高 PNG 分辨率不会让图标变大，
   只会变糊。想让图形占满画框，只能收紧 SVG 的 `viewBox`。
6. **`generate_context!` 会读 `icons/icon.png`** —— 缺这个文件直接编译失败。

---

## 平台支持

| 平台 | 状态 |
|---|---|
| macOS | ✅ 完整可用，已实机验证 |
| Windows | ⚠️ 钩子代码已实现，未实机验证 |
| Linux | ⚠️ evdev 实现已写，未实机验证 |

目前音色包里的键名是按 macOS 虚拟键码表整理的，移植到其他平台需要补对应映射。

---

## 开发

```sh
cd src-tauri
cargo build        # 调试构建
cargo test         # 单元测试
cargo run          # 启动
```

### 打包与发布

打一个 `v` 开头的标签推上去，GitHub Actions 会自动构建 macOS 通用二进制
（Apple Silicon + Intel）并创建 Release：

```sh
git tag v0.1.0
git push origin v0.1.0
```

产物是 `KeyTone_<版本>_universal.dmg`，挂在 Release 页面下。

推送标签前记得先把 `src-tauri/tauri.conf.json` 和 `src-tauri/Cargo.toml` 里的
`version` 改掉——应用内的版本号来自 `Cargo.toml`。

### 检查更新

应用内「设置 → 更新」默认指向本仓库的 Releases，
点「检查更新」会读 `https://api.github.com/repos/PikachuGits/keytone/releases/latest`
并和本地版本逐段比对。

实现上直接用系统 `curl`，没有引入 HTTP 依赖。检查到新版本后显示「下载」按钮，
点了用浏览器打开发布页——**不会静默自动安装**，下载和覆盖由用户自己完成。

> 该功能读的是 **已发布** 的 Release，草稿状态不会返回。

---

## 许可

代码以 MIT 许可发布，详见 [LICENSE](LICENSE)。

内置音色包中的音频素材来自第三方开源项目，按 MIT 许可使用，
版权归属与许可全文见 [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)。
