---
title: Usage
description: The basic sorting workflow
---

## 1. Register destination folders

Open Settings (the button at the top right, or <kbd>Ctrl</kbd>+<kbd>,</kbd>) → **キー割り当て** (key assignments) and register folders with keys.

- **＋ 振り分け先を追加** adds one folder. A free key (1–9, 0, Ctrl+1 …) is assigned automatically.
- **フォルダ内のサブフォルダをまとめて追加** adds every subfolder of a parent folder at once.
- Click a key cell and press a key to change the assignment.

Optionally set a delete folder under **一般** (General). See [Safety](/lumiwake/en/reference/safety/).

## 2. Load a source folder

Choose the folder containing the images to sort and press **読み込んで開始** (<kbd>Enter</kbd>). You can include subfolders. Destination folders and the delete folder are excluded automatically.

Recently used folders (up to 10) are listed on the start screen; pick one by clicking or with <kbd>↑</kbd> <kbd>↓</kbd>. Remove an entry from the history with × (the folder itself is not touched).

## 3. Sort with keys

| Key (default) | Action |
| --- | --- |
| Your assigned keys | Move to that folder and go to the next image |
| <kbd>Space</kbd> | Skip (keep the file where it is) |
| <kbd>Delete</kbd> | Delete (move to the delete folder, or mark for deletion) |
| <kbd>←</kbd> / <kbd>→</kbd> | Previous / next image (processed images are skipped) |
| <kbd>Ctrl</kbd>+<kbd>Z</kbd> | Undo (unlimited) |
| <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>F</kbd> | Toggle sidebar / full-screen view |
| <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>P</kbd> | Show / hide folder paths in the destination list (also a button above the list) |
| <kbd>Esc</kbd> | Leave full-screen / close dialogs |
| <kbd>Ctrl</kbd>+<kbd>Q</kbd> | Quit |

The list on the right shows each destination's name and **full path**, so folders with the same name can be told apart.

## 4. When a file with the same name exists

| Key | Choice | Result |
| --- | --- | --- |
| <kbd>1</kbd> | Keep existing | The incoming image is marked for deletion |
| <kbd>2</kbd> | Overwrite | The existing file is marked for deletion (not removed yet) |
| <kbd>3</kbd> | Keep both | The incoming image is renamed like `name (2).jpg` |
| <kbd>4</kbd> | Skip | Don't move it this time |

## 5. Quit

If files are pending deletion, a list appears when you quit. Open their locations to check, then choose to delete permanently or quit without deleting.
