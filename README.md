# Lumiwake

キーボード操作で画像をすばやくフォルダへ振り分ける、軽量なデスクトップアプリです。

> 🚧 開発初期段階です（MVP の手動仕分けを実装済み）。仕様・機能は変更される可能性があります。

## 概要

仕分けたい画像を1枚ずつ表示し、あらかじめキーに割り当てたフォルダへキー1つで移動します。
数百枚程度の画像を、マウスに触れずにテンポよく整理することを目指しています。

## 主な機能

- 画像を1枚ずつ表示し、割り当てたキーで振り分け先フォルダへ移動
- Ctrl / Shift / Alt との組み合わせキーで多数のフォルダを割り当て
- スキップ（保留）と、複数回さかのぼれる Undo
- 削除キーは専用の削除待ちフォルダへ移動（未設定時はその場で「削除待ち」として記録）
- 移動先に同名ファイルがある場合は、両方を並べて比較してから処理を選択
- ローカルフォルダと SMB 共有の両方に対応
- 設定は GUI と TOML ファイルの両方で編集可能
- 振り分け先リストにフルパスを表示し、同名フォルダを区別

### 将来の拡張

- AI（CLIP ベース）による振り分け先候補の提示
- 確信度の高い画像をまとめて移動する一括確認モード

## 対応形式

JPEG / PNG / WebP / GIF / BMP / TIFF / AVIF / HEIC（RAW は対象外）

AVIF / HEIC はネイティブライブラリ（libdav1d / libheif）を使うため、Cargo の feature（`avif` / `heic`）で有効にします。配布版では Linux 版のみ対応で、Windows 版は現在未対応です（読み込み時にスキップして件数を表示します）。

## 対応 OS

- Windows
- Linux（デスクトップ環境あり）

## 技術スタック

- [Tauri](https://tauri.app/) + Rust
- 画像デコード: `image` クレート、HEIC は `libheif-rs`

## 開発

```sh
npm ci
npm run tauri dev      # 開発起動
npm test               # フロントのテスト
cd src-tauri && cargo test   # Rust のテスト
```

Linux では `libwebkit2gtk-4.1-dev` などが必要です（[Tauri の前提条件](https://v2.tauri.app/start/prerequisites/)）。詳しいコマンドは [CLAUDE.md](./CLAUDE.md) を参照してください。

## ドキュメント

- [説明ページ（使い方・インストール）](https://laplusdestiny.github.io/lumiwake/)（[ソース](./site/)）
- [要件定義書](./docs/requirements.md)
- [引き継ぎメモ](./docs/handoff-notes.md)
- [リリース手順](./docs/release.md)
- [UI モック](./docs/ui-mockups/)

## ライセンス

[MIT License](./LICENSE)
