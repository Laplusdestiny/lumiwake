#!/usr/bin/env bash
# コンテナを作ったあとに 1 回だけ走る。依存の取得と、CI と同じテストツールの用意
set -euo pipefail
cd "$(dirname "$0")/.."

# 名前付きボリュームは root の持ち物で作られるので、作業ユーザーに渡す
sudo chown "$(id -u):$(id -g)" node_modules site/node_modules src-tauri/target "$HOME/.claude"

# CI と同じカバレッジ計測（cargo llvm-cov nextest）用のツール。ビルド済みのものを取ってくる
bin_dir="${CARGO_HOME:-$HOME/.cargo}/bin"
case "$(uname -m)" in
  x86_64) arch=x86_64; nextest_platform=linux ;;
  aarch64) arch=aarch64; nextest_platform=linux-arm ;;
  *) echo "未対応の CPU: $(uname -m)" >&2; exit 1 ;;
esac
if ! command -v cargo-nextest >/dev/null; then
  curl -LsSf "https://get.nexte.st/latest/${nextest_platform}" | tar zxf - -C "$bin_dir"
fi
if ! command -v cargo-llvm-cov >/dev/null; then
  curl -LsSf "https://github.com/taiki-e/cargo-llvm-cov/releases/latest/download/cargo-llvm-cov-${arch}-unknown-linux-gnu.tar.gz" \
    | tar zxf - -C "$bin_dir"
fi

npm ci
(cd site && npm ci)

# generate_context! は frontendDist（../dist）の存在を前提にする（CI と同じ手当て）。
# npm run build をすれば本物で上書きされる
mkdir -p dist
[ -e dist/index.html ] || touch dist/index.html
