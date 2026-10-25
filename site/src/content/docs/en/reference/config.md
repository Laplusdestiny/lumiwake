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
recent_sources = ["/home/me/Pictures/unsorted", "/mnt/nas/photos"]  # recent source folders, newest first, up to 10 (saved automatically)
include_subdirs = false
sort_order = "name_asc"                    # image order (see below)
delete_folder = "/home/me/Pictures/to-delete"  # omit to "mark for deletion" in place
on_exit = "confirm"                        # "confirm" / "delete" (without asking)
view_mode = "sidebar"                      # "sidebar" / "focus"
accent = "amber"                           # "amber" / "blue" / "green"
prefetch = 4                               # images to preload (0–16)
check_updates = true
show_paths = true                          # show folder paths (same-name folders always show them)

[keys]
skip = "Space"
delete = "Delete"
undo = "Ctrl+Z"
prev = "Left"
next = "Right"
toggle_view = "Ctrl+Shift+F"               # sidebar / full-screen
toggle_paths = "Ctrl+Shift+P"              # show / hide folder paths

[[targets]]
key = "1"
name = "Landscape"   # defaults to the folder name
path = "/home/me/Pictures/Sorted/Landscape"
```

### Image order (`sort_order`)

| Value | Order |
| --- | --- |
| `name_asc` / `name_desc` | File name, ascending (default) / descending. Numbers compare by value (`IMG_2` before `IMG_10`) |
| `modified_asc` / `modified_desc` | Modified time, oldest / newest first |
| `taken_asc` / `taken_desc` | EXIF capture time, oldest / newest first. Images without one go last, ordered by modified time |
| `size_asc` / `size_desc` | File size, smallest / largest first |

Ties are broken by file name. Changing the order in the settings screen while sorting re-orders the images not yet sorted (including skipped ones) right away and starts from the first of them.

If the file has a syntax error, Lumiwake starts with default settings and tells you so; the file is not overwritten.
