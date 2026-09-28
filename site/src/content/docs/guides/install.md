---
title: インストール
description: Lumiwake のダウンロードとインストール
---

[GitHub Releases](https://github.com/laplusdestiny/lumiwake/releases/latest) から、お使いの OS 向けのファイルをダウンロードします。

## Windows

`Lumiwake_x.y.z_x64_ja-JP.msi`（または `_en-US.msi`）を実行してインストールします。

## Linux

### AppImage

```sh
chmod +x Lumiwake_x.y.z_amd64.AppImage
./Lumiwake_x.y.z_amd64.AppImage
```

### Debian / Ubuntu（.deb）

```sh
sudo apt install ./lumiwake_x.y.z_amd64.deb
```

Linux 版は Ubuntu 24.04 相当以降の環境を想定しています。

## 自動アップデート

起動時に新しいバージョンを確認し、見つかると画面上部にお知らせが出ます。「更新して再起動」を押すと更新されます。確認は設定の「一般 → アップデート」で止められます。

:::note
更新のための再起動では、削除予定の記録は消えます（ファイルは削除されずに残ります）。
:::
