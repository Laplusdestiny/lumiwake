---
title: Settings file
description: Reference for config.toml
---

Settings can be edited both in the GUI and in a TOML file. Saving in the GUI writes the TOML file (comments in the file are not preserved). After editing the file by hand, use **設定ファイル → ファイルから再読み込み** (reload from file).

## Location

| OS | Path |
| --- | --- |
| Windows | `%APPDATA%\io.github.laplusdestiny.lumiwake\config.toml` |
| Linux | `~/.config/io.github.laplusdestiny.lumiwake/config.toml` |

## Keys

```toml
[general]
source_dir = "/home/me/Pictures/unsorted"  # last source folder (saved automatically)
include_subdirs = false
delete_folder = "/home/me/Pictures/to-delete"  # omit to "mark for deletion" in place
on_exit = "confirm"                        # "confirm" / "delete" (without asking)
view_mode = "sidebar"                      # "sidebar" / "focus"
accent = "amber"                           # "amber" / "blue" / "green"
prefetch = 4                               # images to preload (0–16)
check_updates = true

[keys]
skip = "Space"
delete = "Delete"
undo = "Ctrl+Z"
prev = "Left"
next = "Right"
toggle_view = "F"

[[targets]]
key = "1"
name = "Landscape"   # defaults to the folder name
path = "/home/me/Pictures/Sorted/Landscape"
```

If the file has a syntax error, Lumiwake starts with default settings and tells you so; the file is not overwritten.
