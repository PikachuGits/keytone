# 参与贡献

KeyTone 是 Tauri 2 + Rust 的桌面应用。改动请走 Pull Request，不要直接往 `main` 推。

## 流程

1. Fork 仓库，从 `main` 切分支
2. 本地改完、跑通检查
3. 提 PR 到 `main`，按模板填清楚做了什么、怎么验证的
4. 等 `CI / macOS` 和 `CI / Windows` 两个检查变绿
5. 维护者 review 通过后合并

`main` 已开启分支保护：必须走 PR、至少 1 个 approve、CI 双平台检查通过、所有 review 对话标记为已解决，
并且要求分支与 `main` 保持同步（落后时需要先 rebase 或 merge）。合并后源分支会自动删除。

分支命名建议：`fix/xxx`、`feat/xxx`、`docs/xxx`、`sound/xxx`。

## 本地检查

```bash
cd src-tauri
cargo fmt --check                          # 格式
cargo clippy --all-targets -- -D warnings  # 静态检查
cargo test                                 # 测试
cargo build                                # 构建
```

CI 实际只跑 `cargo test` 和 `cargo build`，但 fmt 和 clippy 请自己过一遍，别让 review 反复提同一件事。

## 代码约定

- 注释和提交信息用中文，代码标识符用英文
- 提交信息一句话说清意图，不要 `update`、`fix bug` 这种
- 新依赖要想清楚：这是常驻后台的小工具，二进制体积和内存都敏感

## 平台坑（重要）

Windows 上 wry 的 `send_user_message` 在主线程会同步执行 `handle_user_message`，
而 WebView2 创建 / 销毁窗口需要事件循环继续泵消息。如果主线程正卡在事件循环回调里
（托盘 `on_menu_event`、同步 IPC 命令），就会自锁 —— 事件循环停摆，界面所有按钮失灵。

所以：

- **不要在主线程同步创建或销毁 WebView2 窗口**，丢到 `std::thread::spawn` 的后台线程去
- `destroy()` 返回不等于窗口已从 manager 移除，要重建同名窗口得先等 `get_webview_window(label)` 变成 `None`
- 不要攥着 `Mutex` 调用会 `run_on_main_thread` 并阻塞等待的接口，先 `clone()` 句柄、放锁再调

## 音色包

新增音色包请附带来源和授权说明（`THIRD-PARTY-NOTICES.md`），不要提交版权不明的采样。
