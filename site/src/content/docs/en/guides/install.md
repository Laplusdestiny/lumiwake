---
title: Install
description: Download and install Lumiwake
---

Download the file for your OS from [GitHub Releases](https://github.com/laplusdestiny/lumiwake/releases/latest).

## Windows

Run `Lumiwake_x.y.z_x64_ja-JP.msi` (or `_en-US.msi`).

## Linux

### AppImage

```sh
chmod +x Lumiwake_x.y.z_amd64.AppImage
./Lumiwake_x.y.z_amd64.AppImage
```

### Debian / Ubuntu (.deb)

```sh
sudo apt install ./lumiwake_x.y.z_amd64.deb
```

The Linux build targets Ubuntu 24.04 or newer equivalents.

## Automatic updates

Lumiwake checks for a new version at startup and shows a banner when one is available. Click “更新して再起動” (update and restart). You can turn the check off in Settings → General → Updates.

:::note
Restarting for an update discards the list of files pending deletion (the files themselves are kept).
:::
