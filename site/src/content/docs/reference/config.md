---
title: 設定ファイル
description: config.toml の項目一覧
---

設定は GUI の設定画面と TOML ファイルの両方で編集できます。GUI で保存すると TOML に書き戻されます（その際、ファイル内のコメントは保持されません）。TOML を直接編集した場合は、設定画面の **設定ファイル → ファイルから再読み込み** で反映できます。

<img class="shot" src="/lumiwake/settings-general.webp" width="1600" height="1038" loading="lazy" alt="設定の一般画面。削除フォルダの指定、終了時の削除の確認方法、表示モード、振り分け先リストのパス表示、アクセント色、サブフォルダの読み込み、並び順、先読みする枚数の項目が並ぶ" />

## 場所

| OS | パス |
| --- | --- |
| Windows | `%APPDATA%\io.github.laplusdestiny.lumiwake\config.toml` |
| Linux | `~/.config/io.github.laplusdestiny.lumiwake/config.toml` |

## 項目

```toml
[general]
source_dir = "/home/me/Pictures/未整理"  # 前回の仕分け元（自動で保存）
recent_sources = ["/home/me/Pictures/未整理", "/mnt/nas/写真"]  # 最近使った仕分け元（新しい順、10 件まで。自動で保存）
include_subdirs = false                  # サブフォルダも読み込む
sort_order = "name_asc"                  # 並び順（下記）
delete_folder = "/home/me/Pictures/削除候補"  # 省略すると「削除予定として記録」
on_exit = "confirm"                      # "confirm"（確認する） / "delete"（確認せずに削除）
view_mode = "sidebar"                    # "sidebar" / "focus"
accent = "amber"                         # "amber" / "blue" / "green"
prefetch = 4                             # 先読みする枚数（0〜16）
check_updates = true                     # 起動時にアップデートを確認
show_paths = true                        # 振り分け先リストにパスを表示（同名フォルダは常に表示）

[keys]
skip = "Space"
delete = "Delete"
undo = "Ctrl+Z"
prev = "Left"
next = "Right"
toggle_view = "Ctrl+Shift+F"             # サイドバー型 ⇔ 全画面型
toggle_paths = "Ctrl+Shift+P"            # パス表示の切り替え

[[targets]]
key = "1"
name = "風景"          # 省略するとフォルダ名
path = "/home/me/Pictures/整理済み/風景"
```

### 並び順（`sort_order`）

| 値 | 並び順 |
| --- | --- |
| `name_asc` / `name_desc` | ファイル名の昇順（既定）／降順。数字は数値として比べます（`IMG_2` → `IMG_10`） |
| `modified_asc` / `modified_desc` | 更新日時の古い順／新しい順 |
| `taken_asc` / `taken_desc` | EXIF の撮影日時の古い順／新しい順。撮影日時のない画像は後ろに更新日時の順で並べます |
| `size_asc` / `size_desc` | ファイルサイズの小さい順／大きい順 |

値が同じ画像はファイル名順になります。仕分け中に設定画面で変えると、まだ振り分けていない画像（保留を含む）をすぐ並べ直し、その先頭から表示します。

書式に誤りがあると、起動時に既定の設定で立ち上がり、画面にその旨が表示されます（ファイルは上書きされません）。
