---
title: 設定ファイル
description: config.toml の項目一覧
---

設定は GUI の設定画面と TOML ファイルの両方で編集できます。GUI で保存すると TOML に書き戻されます（その際、ファイル内のコメントは保持されません）。TOML を直接編集した場合は、設定画面の **設定ファイル → ファイルから再読み込み** で反映できます。

## 場所

| OS | パス |
| --- | --- |
| Windows | `%APPDATA%\io.github.laplusdestiny.lumiwake\config.toml` |
| Linux | `~/.config/io.github.laplusdestiny.lumiwake/config.toml` |

## 項目

```toml
[general]
source_dir = "/home/me/Pictures/未整理"  # 前回の仕分け元（自動で保存）
include_subdirs = false                  # サブフォルダも読み込む
delete_folder = "/home/me/Pictures/削除候補"  # 省略すると「削除予定として記録」
on_exit = "confirm"                      # "confirm"（確認する） / "delete"（確認せずに削除）
view_mode = "sidebar"                    # "sidebar" / "focus"
accent = "amber"                         # "amber" / "blue" / "green"
prefetch = 4                             # 先読みする枚数（0〜16）
check_updates = true                     # 起動時にアップデートを確認

[keys]
skip = "Space"
delete = "Delete"
undo = "Ctrl+Z"
prev = "Left"
next = "Right"
toggle_view = "F"

[[targets]]
key = "1"
name = "風景"          # 省略するとフォルダ名
path = "/home/me/Pictures/整理済み/風景"
```

書式に誤りがあると、起動時に既定の設定で立ち上がり、画面にその旨が表示されます（ファイルは上書きされません）。
