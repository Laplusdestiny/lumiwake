---
title: キー設定の例
description: 振り分け先が多いときのキー割り当ての工夫
---

キーは `Ctrl` / `Shift` / `Alt` と組み合わせられます。設定ファイル（TOML）では次のように書きます。

## 数字キーだけで 10 か所

```toml
[[targets]]
key = "1"
name = "風景"
path = "D:/Pictures/整理済み/風景"

[[targets]]
key = "2"
name = "人物"
path = "D:/Pictures/整理済み/人物"
# … "0" まで
```

## 修飾キーで分類を分ける

年ごとのフォルダを `Shift` 、よく使う分類を数字だけに、という分け方ができます。

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

同じ名前（例: `2026`）のフォルダが複数あっても、画面にはフルパスが出るので区別できます。

## 書き方

- 修飾キーの順序や大文字小文字は問いません（`shift+ctrl+a` は `Ctrl+Shift+A` になります）
- 使えるキー: `A`〜`Z`、`0`〜`9`、`F1`〜`F24`、`Space`、`Enter`、`Delete`、`Backspace`、`Left` / `Right` / `Up` / `Down`、`Num0`〜`Num9`（テンキー）、`Minus`、`Comma` など

## 使えない・注意が必要なキー

- 同じキーを 2 か所に割り当てるとエラーになり、保存できません
- `F5`、`Ctrl+R`（再読み込み）、`Ctrl+P`（印刷）、`Ctrl+F`（検索）、`Alt+←`（戻る）など、WebView や OS が使うキーは警告が出ます。効かない場合があります
- `Esc`、`Ctrl+,`（設定）、`Ctrl+Q`（終了）は Lumiwake が使います
