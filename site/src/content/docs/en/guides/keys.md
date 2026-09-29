---
title: Key setup examples
description: Tips for assigning keys when you have many folders
---

Keys can be combined with `Ctrl` / `Shift` / `Alt`. In the TOML settings file:

## Ten folders with digits

```toml
[[targets]]
key = "1"
name = "Landscape"
path = "D:/Pictures/Sorted/Landscape"

[[targets]]
key = "2"
name = "People"
path = "D:/Pictures/Sorted/People"
# … up to "0"
```

## Group with modifiers

```toml
[[targets]]
key = "Shift+1"
name = "2025"
path = "/mnt/nas/photos/2025"

[[targets]]
key = "Shift+2"
name = "2026"
path = "/mnt/nas/photos/2026"
```

## Syntax

- Modifier order and case don't matter (`shift+ctrl+a` becomes `Ctrl+Shift+A`).
- Available keys: `A`–`Z`, `0`–`9`, `F1`–`F24`, `Space`, `Enter`, `Delete`, `Backspace`, `Left` / `Right` / `Up` / `Down`, `Num0`–`Num9` (numpad), `Minus`, `Comma`, and more.

## Keys to avoid

- Assigning the same key twice is an error and can't be saved.
- Keys used by the WebView or OS — `F5`, `Ctrl+R` (reload), `Ctrl+P` (print), `Ctrl+F` (find), `Alt+Left` (back) — show a warning and may not work.
- `Esc`, `Ctrl+,` (settings) and `Ctrl+Q` (quit) are used by Lumiwake.
