#!/usr/bin/env bash
#
# 生成本机自签名「代码签名」证书，并导入登录钥匙串设为信任。
#
# 为什么需要：ad-hoc 签名（codesign --sign -）的授权绑定在 cdhash 上，
# 每次重新编译 cdhash 都变，macOS 的辅助功能授权就失效了。
# 换成证书签名后，授权绑定的是证书（designated requirement 里的
# certificate root = H"..."），重新编译不会影响，只需授权一次。
#
# 用法：
#   ./scripts/make-signing-cert.sh                 # 用默认名字
#   ./scripts/make-signing-cert.sh "My Cert Name"  # 自定义名字
#
set -euo pipefail

NAME="${1:-KeyTone Local Signing}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

if security find-identity -v -p codesigning 2>/dev/null | grep -qF "$NAME"; then
  echo "✓ 已存在签名身份：$NAME"
  exit 0
fi

echo "→ 生成自签名证书：$NAME"

# 必须带 extendedKeyUsage=codeSigning，否则 codesign 不认这个身份。
openssl req -x509 -newkey rsa:2048 -sha256 -days 3650 -nodes \
  -keyout "$WORK/key.pem" -out "$WORK/cert.pem" \
  -subj "/CN=$NAME/O=KeyTone" \
  -addext "basicConstraints=critical,CA:false" \
  -addext "keyUsage=critical,digitalSignature" \
  -addext "extendedKeyUsage=critical,codeSigning" 2>/dev/null

# macOS 的 security import 不认 OpenSSL 3 默认的 AES-256 PKCS12，
# 必须用 -legacy（3DES + SHA1），否则报 "MAC verification failed"。
openssl pkcs12 -export -legacy \
  -inkey "$WORK/key.pem" -in "$WORK/cert.pem" -out "$WORK/cert.p12" \
  -passout pass:keytone -name "$NAME" 2>/dev/null

echo "→ 导入登录钥匙串"
security import "$WORK/cert.p12" \
  -k "$HOME/Library/Keychains/login.keychain-db" \
  -P keytone -T /usr/bin/codesign -A

# 不设信任的话 find-identity -p codesigning 看不到这个身份。
echo "→ 设为受信任（可能弹出钥匙串密码框）"
security add-trusted-cert -r trustRoot \
  -k "$HOME/Library/Keychains/login.keychain-db" "$WORK/cert.pem"

echo
echo "✓ 完成。可用身份："
security find-identity -v -p codesigning | grep -F "$NAME"
echo
echo "现在用 ./scripts/build-macos.sh 打包，会自动使用这个证书签名。"
