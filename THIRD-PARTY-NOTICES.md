# 第三方素材说明

本项目源码以 MIT 许可发布（见 [LICENSE](LICENSE)）。

除源码外，仓库中随附了以下第三方素材，其版权归原作者所有，按各自的许可条款使用。

---

## 内置音色包音频素材

**位置**：`assets/profiles/`

**版权**：Copyright (c) 2025 Nathan Fiscaletti

**许可**：MIT License

以下音色包的录音采样与 `profile.yaml` 元数据来自
[keyboard-sounds/keyboardsounds-pro](https://github.com/keyboard-sounds/keyboardsounds-pro)，
按 MIT 许可随本项目一同分发：

```
键盘  alpaca              gateron-black-ink    gateron-red-ink
      holy-panda          ios                  logitech-g915-tkl-brown
      mx-black            mx-blue              mx-brown
      nk-cream            opera-gx             typewriter

鼠标  amazon-gaming-mouse  apple-magic-mouse    corsair-m65-elite
      g502x-wireless      logitech-g502-lightspeed
      logitech-m570-trackball                  razer-mamba-elite
```

原始许可全文亦随素材保留于
`assets/profiles/LICENSE-keyboardsounds-pro`。

本项目对上述素材的修改仅限于：在加载时做音量归一化、在 `profile.yaml` 中
补充多采样变体支持所需的字段。音频文件本身未作改动。

### MIT License

```
MIT License

Copyright (c) 2025 Nathan Fiscaletti

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

---

## 音色包格式

`profile.yaml` 采用 MechVibes 风格的 YAML 结构。本项目对该格式的实现为独立编写，
未包含任何 MechVibes 的源代码。

---

## 依赖库

全部运行时依赖（Tauri、rodio、cpal、serde、symphonia 等）均为 MIT / Apache-2.0 许可，
具体版本与许可见 `src-tauri/Cargo.lock`，各库的许可声明随 crate 一同分发。
