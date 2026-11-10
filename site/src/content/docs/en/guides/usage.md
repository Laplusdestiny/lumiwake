---
title: Usage
description: The basic sorting workflow
---

## 1. Register destination folders

Open Settings (the button at the top right, or <kbd>Ctrl</kbd>+<kbd>,</kbd>) → **キー割り当て** (key assignments) and register folders with keys.

- **＋ 振り分け先を追加** adds one folder. A free key (1–9, 0, Ctrl+1 …) is assigned automatically.
- **フォルダ内のサブフォルダをまとめて追加** adds every subfolder of a parent folder at once.
- Click a key cell and press a key to change the assignment.

<img class="shot" src="/lumiwake/settings-keys.webp" width="1600" height="1038" loading="lazy" alt="The key assignment settings: a list of keys, display names and paths. Modifier keys such as Shift+8 can be used, and two folders both named “その他” (Other) are told apart by their paths" />

Optionally set a delete folder under **一般** (General). See [Safety](/lumiwake/en/reference/safety/).

## 2. Load a source folder

Choose the folder containing the images to sort and press **読み込んで開始** (<kbd>Enter</kbd>). You can include subfolders. Destination folders and the delete folder inside the source are excluded automatically, so images you already sorted don't show up again (if any destination folder was skipped, a notice tells you after loading). To re-sort images, or to check already-sorted images for look-alikes too, tick **振り分け先フォルダの画像も含める** (Include images in destination folders). The delete folder is never loaded. Images are shown in file-name order; to use modified time, capture time, or file size instead, change **一般 → 読み込み → 並び順** (General → Loading → Order).

<img class="shot shot--narrow" src="/lumiwake/start-screen.webp" width="1282" height="832" loading="lazy" alt="The start screen: the source folder field with a Browse button, a list of recently used folders, an “include subfolders” checkbox, and the Load and start button" />

Recently used folders (up to 10) are listed on the start screen; pick one by clicking or with <kbd>↑</kbd> <kbd>↓</kbd>. Remove an entry from the history with × (the folder itself is not touched).

## 3. Sort with keys

<img class="shot" src="/lumiwake/sort-in-progress.webp" width="1600" height="1038" loading="lazy" alt="Sorting in progress (image 5 of 24): the destination list shows how many images went to each folder, and processed thumbnails at the bottom are labeled with their destination, such as “→風景” (Landscape)" />

| Key (default) | Action |
| --- | --- |
| Your assigned keys | Move to that folder and go to the next image |
| <kbd>Space</kbd> | Skip (keep the file where it is) |
| <kbd>Delete</kbd> | Delete (move to the delete folder, or mark for deletion) |
| <kbd>←</kbd> / <kbd>→</kbd> | Previous / next image (processed images are skipped) |
| <kbd>Ctrl</kbd>+<kbd>Z</kbd> | Undo (unlimited) |
| <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>F</kbd> | Toggle sidebar / full-screen view |
| <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>P</kbd> | Show / hide folder paths in the destination list (also a button above the list) |
| <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>L</kbd> | Open / close the grid view (also a button at the top) |
| <kbd>Esc</kbd> | Leave full-screen / close dialogs |
| <kbd>Ctrl</kbd>+<kbd>Q</kbd> | Quit |

The list on the right shows each destination's name and **full path**, so folders with the same name can be told apart.

### View modes

Switch between the **sidebar** layout (preview plus destination list) and the **full-screen focus** layout (a large image with the destination keys along the bottom) with <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>F</kbd>.

<div class="shot-pair">
  <img class="shot" src="/lumiwake/sort-sidebar.webp" width="1600" height="1038" loading="lazy" alt="Sidebar layout: the image preview in the center and the destination list with keys and full paths on the right" />
  <img class="shot" src="/lumiwake/sort-fullscreen.webp" width="1600" height="1038" loading="lazy" alt="Full-screen focus layout: the image fills the window and the destination keys and names are lined up along the bottom" />
</div>


### Grid view

When the source folder holds many images and you are looking for one of them, press <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>L</kbd> (or the grid button at the top) to see all loaded images as thumbnails. Select an image and press <kbd>Enter</kbd> (or click it) to go back to the single-image view with that image, ready to sort.

| Key | Action |
| --- | --- |
| <kbd>←</kbd> <kbd>→</kbd> <kbd>↑</kbd> <kbd>↓</kbd> / <kbd>PageUp</kbd> <kbd>PageDown</kbd> / <kbd>Home</kbd> <kbd>End</kbd> | Select an image |
| <kbd>Enter</kbd> | Open the selected image |
| <kbd>Ctrl</kbd>+<kbd>F</kbd> | Filter by file name (press <kbd>Enter</kbd> or <kbd>↓</kbd> in the filter box to go back to the grid) |
| <kbd>Ctrl</kbd>+<kbd>=</kbd> / <kbd>Ctrl</kbd>+<kbd>-</kbd> | Larger / smaller thumbnails (4 steps; <kbd>Ctrl</kbd>+wheel also works) |
| <kbd>Ctrl</kbd>+<kbd>0</kbd> | Reset the thumbnail size |
| <kbd>Esc</kbd> | Go back |

- The slider at the top changes the thumbnail size freely, and the size is kept for next time
- Images you have already sorted are shown dimmed, labeled with their destination (such as “→風景”) or “削除予定” (marked for deletion). They cannot be opened; undo with <kbd>Ctrl</kbd>+<kbd>Z</kbd> to sort them again
- The sorting, delete and skip keys do nothing in the grid view

## 4. When a file with the same name exists

| Key | Choice | Result |
| --- | --- | --- |
| <kbd>1</kbd> | Keep existing | The incoming image is marked for deletion |
| <kbd>2</kbd> | Overwrite | The existing file is marked for deletion (not removed yet) |
| <kbd>3</kbd> | Keep both | The incoming image is renamed like `name (2).jpg` |
| <kbd>4</kbd> | Skip | Don't move it this time |

## 5. Quit

If files are pending deletion, a list appears when you quit. Open their locations to check, then choose to delete permanently or quit without deleting.
