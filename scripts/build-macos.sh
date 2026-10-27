#!/usr/bin/env bash
#
# 构建 macOS 安装包。
#
# 本机存在自签名代码签名证书时自动用它签名，否则回退到 ad-hoc。
# 用证书签名后，辅助功能授权在重新编译之后依然有效 —— 授权绑定的是证书，
# 不是每次编译都会变的 cdhash。
#
# 首次使用先跑一次：./scripts/make-signing-cert.sh
#
# 用法：
#   ./scripts/build-macos.sh                    # 打包 app + dmg
#   ./scripts/build-macos.sh --bundles app      # 只打包 app（更快）
#
set -euo pipefail

IDENTITY="${APPLE_SIGNING_IDENTITY:-KeyTone Local Signing}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if security find-identity -v -p codesigning 2>/dev/null | grep -qF "$IDENTITY"; then
  echo "→ 使用证书签名：$IDENTITY"
  export APPLE_SIGNING_IDENTITY="$IDENTITY"
else
  echo "→ 没找到签名身份「$IDENTITY」，回退到 ad-hoc"
  echo "  注意：ad-hoc 签名下每次重新编译都要重新授权辅助功能。"
  echo "  想避免的话先运行 ./scripts/make-signing-cert.sh"
  export APPLE_SIGNING_IDENTITY="-"
fi

if [ $# -gt 0 ]; then
  npm run tauri build -- "$@"
else
  npm run tauri build
fi
