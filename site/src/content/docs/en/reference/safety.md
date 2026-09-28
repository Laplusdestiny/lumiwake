---
title: Safety
description: How Lumiwake avoids losing your images
---

Lumiwake's top priority is never losing your image files.

## The delete key never deletes

Deleting on an SMB share (e.g. a NAS) bypasses the OS recycle bin and is permanent. So the delete key doesn't remove files:

- **With a delete folder configured**, the file is moved there (renamed like `name (2).jpg` on a clash).
- **Without one**, the file stays where it is and is *marked* for deletion and hidden from the queue.

Files are permanently deleted **only when you quit**, after a confirmation by default (you can switch to deleting without asking). Only files marked during the current run are deleted; files that were already in the delete folder are left alone.

## Undo

Moves, deletions, skips and name-clash decisions can all be undone, any number of steps. History lives in memory only and is not kept across restarts. If the app crashes, the deletion marks are lost but the files remain.

## Moving across drives and NAS

A move to another drive or an SMB share can't be a simple rename, so Lumiwake:

1. copies to a temporary file in the destination folder and flushes it to disk,
2. checks that the copy has the same size as the original,
3. renames the temporary file to the final name,
4. deletes the original (if that fails, the copy is removed again).

Whatever step fails, only the original file remains — no missing files and no duplicates. An existing file at the destination is never silently overwritten.
