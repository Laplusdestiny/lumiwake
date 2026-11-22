# CI（ubuntu-24.04）と同じ Ubuntu で、Tauri のビルドに要るライブラリを入れる
FROM mcr.microsoft.com/devcontainers/base:ubuntu-24.04

RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        # Tauri（WebKitGTK）。CI の「Linux の依存ライブラリ」と同じもの＋Tauri 公式の前提パッケージ
        build-essential pkg-config file curl wget libssl-dev libxdo-dev \
        libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev \
        # HEIC / AVIF（--features avif,heic）。HEIC のデコードには libde265 プラグインが必要
        libheif-dev libheif-plugin-libde265 libdav1d-dev \
        # UI 文言が日本語のため。ないと WebKitGTK のウィンドウで文字が化ける
        fonts-noto-cjk \
    && rm -rf /var/lib/apt/lists/*
