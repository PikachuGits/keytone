//! 内置音色包：把 `assets/profiles/` 整个目录编译进二进制。
//!
//! 这样全新安装（或换台机器）第一次运行就能拿到全部原版音色包，
//! 不依赖仓库路径、也不需要额外打包配置。总共 2MB，代价可以接受。
//!
//! 已存在的包一律跳过，不会覆盖用户改过的内容。

use std::path::Path;

use include_dir::{include_dir, Dir};

static BUNDLED: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../assets/profiles");

/// 把内置包补进音色包目录，返回新装了哪些。
pub fn install_missing(profiles_dir: &Path) -> Result<Vec<String>, String> {
    let mut installed = Vec::new();

    for dir in BUNDLED.dirs() {
        // 只认真正带 profile.yaml 的目录
        if dir.get_file(dir.path().join("profile.yaml")).is_none() {
            continue;
        }

        let Some(name) = dir.path().file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        let target = profiles_dir.join(name);
        if target.join("profile.yaml").exists() {
            continue;
        }

        std::fs::create_dir_all(&target).map_err(|e| format!("创建 {name} 失败: {e}"))?;
        write_dir(dir, &target)?;
        installed.push(name.to_string());
    }

    installed.sort();
    Ok(installed)
}

fn write_dir(dir: &Dir<'_>, target: &Path) -> Result<(), String> {
    for file in dir.files() {
        let Some(name) = file.path().file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let path = target.join(name);
        std::fs::write(&path, file.contents()).map_err(|e| format!("{}: {e}", path.display()))?;
    }

    for sub in dir.dirs() {
        let Some(name) = sub.path().file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let path = target.join(name);
        std::fs::create_dir_all(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        write_dir(sub, &path)?;
    }

    Ok(())
}
