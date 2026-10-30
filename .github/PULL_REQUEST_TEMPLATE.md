## 这个 PR 做了什么

<!-- 一句话说清目的。修 bug 就写现象和根因，加功能就写使用场景。 -->

## 类型

- [ ] 修 bug
- [ ] 新功能
- [ ] 音色包 / 素材
- [ ] 重构（不改变行为）
- [ ] 文档
- [ ] 构建 / CI

## 关联 issue

<!-- 例如 Closes #12。没有就写"无"。 -->

## 怎么验证的

<!-- 别人怎么确认这个改动是对的。贴步骤、贴截图、贴测试输出都行。 -->

## 自检

- [ ] `cd src-tauri && cargo fmt --check` 通过
- [ ] `cd src-tauri && cargo clippy --all-targets -- -D warnings` 无告警
- [ ] `cd src-tauri && cargo test` 通过
- [ ] 在 macOS 和 Windows 上至少各跑过一次（涉及平台相关代码时两边都要跑）
- [ ] 涉及窗口 / 托盘 / 线程的改动，已确认没有在主线程同步创建或销毁 WebView 窗口（见 CONTRIBUTING.md）

## 补充说明

<!-- 有 breaking change、需要重新生成素材、或者有已知遗留问题，写在这里。 -->
