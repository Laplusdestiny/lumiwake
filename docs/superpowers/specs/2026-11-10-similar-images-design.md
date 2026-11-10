# 類似画像の確認と削除（Issue #17）設計

2026-11-10

## 目的

仕分け元フォルダの中に似た画像があるとき、それを見つけて不要なほうを削除できるようにする。想定する例は次の 2 つ。

- SNS などから取得した画像が、実は取得済みだった（再圧縮・縮小・ノイズ・隅のマークの有無の違い）
- 写真を撮ったときに、ほぼ同じ構図のものが複数ある

アプリは候補を出して根拠を示すだけで、自動では削除しない。削除は既存の削除キーと同じ扱い（削除フォルダへ移動、または削除予定の記録）で、取り消しでき、完全削除はアプリ終了時の確認後だけである。

## 決定事項（ユーザー確認済み）

- 仕分け画面とは別の**専用の確認画面**で、フォルダ全体の類似画像をグループごとに並べて確認する
- グループは**中身の類似度**で作る。ファイル名の類似・サイズ一致・解像度一致は各グループの**根拠**として表示し、絞り込みに使う。バイト列まで同じファイルは「完全一致」として区別する
- 削除する画像は**手動で選ぶ**。1 枚ずつ削除するか、「これだけ残す」でグループの残りをまとめて削除予定にする。アプリは「解像度が最大」などの目印を出すだけで、あらかじめ選択はしない
- 中身の比較は **dHash（64 ビット）**。既定の閾値は類似度 90%、画面で 75% までゆるめられる
- 「これだけ残す」のキーは `Ctrl+Shift+K`（設定で変更可）
- 構図が大きく動いた撮り直しは範囲外（将来の Issue）

## 範囲外

次は今回は作らず、`enhancement` + `future` ラベルの Issue に登録する。

- 撮影時刻が近い画像をゆるい閾値でまとめる（構図が動いた撮り直しへの対応）
- 振り分け先フォルダにある画像との比較
- 切り抜き・回転した画像の検出
- ハッシュのファイル保存（再起動後の再利用）
- 仕分け画面（1 枚表示）での「似た画像あり」の印

## 中身の比較（dHash）

- 画像をデコードし、グレースケールの 9×8 に縮める。各行で隣り合う画素の大小を比べ、8×8＝64 ビットにする
- 2 枚の距離はハミング距離（違うビットの数、0〜64）。類似度は `1 − 距離/64` を百分率にして表示する
- 閾値は距離で持つ。既定 6（類似度 90% 以上）、範囲 0〜16（100%〜75%）
- 依存クレートは増やさない（`image` クレートの縮小と自前のビット演算）

合成画像 40 種類での距離の実測（設計時の確認、中央値／最大）:

| 変化 | 距離 |
| --- | --- |
| 半分に縮小＋JPEG 再圧縮 | 0 / 2 |
| ノイズ（強） | 0 / 3 |
| 隅のマーク（幅 30%） | 2 / 5 |
| 明るさ 15% 増し | 1 / 4 |
| 構図のずれ 1% | 2 / 7 |
| 構図のずれ 3% | 8 / 13 |
| 構図のずれ 6% | 16 / 22 |
| 別の画像 | 31（最小 22） |

実写では別の画像どうしがもう少し近くなることがあるため、既定値はテストで確かめ、利用者はスライダーで調整する。

## Rust

### 新モジュール `similar/`

ファイル操作はしない。計算と判定だけを持つ。

**`hash.rs`**

- `dhash(img: &DynamicImage) -> u64`
- `distance(a: u64, b: u64) -> u32`

**`names.rs`**

- `similar_names(a: &str, b: &str) -> bool`。拡張子を除いた名前（大文字小文字を区別しない）で、次のどちらかに当てはまれば true
  - 末尾追加: 短いほうが長いほうの先頭に一致し、残りが「複製を表す末尾」だけである。複製を表す末尾は、区切り（空白・`_`・`-`）と、`(1)` のような括弧つき番号、番号、`copy`、`コピー` の組み合わせ（例: `a` と `a (1)`、`a_copy`、`a - コピー`、`a-2`）
  - 連番: 末尾の数字列を除いた部分が同じで、数字の差が 1（例: `IMG_0012` と `IMG_0013`）
- 拡張子だけが違う同名（`a.jpg` と `a.png`）も true

**`group.rs`**

```rust
pub struct Fingerprint { pub hash: u64, pub width: u32, pub height: u32, pub size: u64 }
pub struct Candidate { pub index: usize, pub path: PathBuf, pub print: Fingerprint }

pub struct Group {
    pub members: Vec<Member>,      // セッションの並び順
    pub exact: bool,               // 全員のバイト列が同じ
    pub same_size: bool,           // 全員のファイルサイズが同じ
    pub same_resolution: bool,     // 全員の解像度が同じ
    pub similar_names: bool,       // 名前が似ている組が 1 つ以上ある
}
pub struct Member {
    pub index: usize,
    pub similarity: u8,            // グループ内で最も近い相手との類似度（%）
    pub largest_resolution: bool,  // 画素数が単独で最大
    pub largest_size: bool,        // ファイルサイズが単独で最大
}

pub fn group(candidates: &[Candidate], max_distance: u32, same_bytes: impl Fn(&Path, &Path) -> bool) -> Vec<Group>
```

- 距離が `max_distance` 以下の組をつなぎ、連結成分（2 枚以上）をグループにする。総当たり（数百枚で数万回のビット演算。1 万枚でも 5 千万回で問題ない）
- A と B、B と C が近ければ A と C が遠くても同じグループになる。メンバーの類似度は「最も近い相手」との値を出すので、鎖でつながっただけの画像は数字で分かる
- `exact` はサイズが全員同じときだけ `same_bytes` で確かめる（サイズが違えば読まない）。`same_bytes` はファイルを先頭から読み比べる
- 全員が同じ値のときは `largest_*` を付けない
- グループの並びは先頭メンバーの番号順

### 判定の実行（`state.rs` に `SimilarState`）

- `AppState` に `similar: Arc<SimilarScan>` を持つ。中身は「元の場所のパス → `Result<Fingerprint, ()>`」の表と、進行中の走査の `generation`・`done`・`total`・中止フラグ
- 走査の対象は、開始した時点のセッションで未処理・保留の画像のうち、表にまだないもの
- ワーカースレッド（`available_parallelism` を 1〜3 に丸めた数）が順にデコードし、ハッシュ・解像度・ファイルサイズを表に入れる。デコードは既存の `Registry::decode`
- 読めない・デコードできない画像は失敗として表に入れ、グループには含めない（件数を画面に出す）
- `start_session` で表を空にし、進行中の走査を中止する（ワーカーは 1 枚ごとに `generation` と中止フラグを見る）。古い走査の結果は表に入れない
- 並び順の変更（`reorder_open`）で画像の番号が変わっても、表のキーはパスなので使い続けられる

### ファイル操作（`fileops/session.rs`）

削除の整合性は引き続き `Session` だけが持つ。

- `DeletionReason::Similar` を追加
- `Session::delete_at(&mut self, idx: usize, reason: DeletionReason) -> Result<(), FileOpError>`
  - 同名ファイルの確認待ちがあれば `ConflictPending`
  - `idx` が範囲外、または未処理・保留でなければ新しいエラー `FileOpError::NotOpen`
  - 既存の `delete_item` を通す（削除フォルダへ移動／削除予定の記録、履歴に積む）。履歴のラベルは「削除（類似）」
  - `idx` が現在の画像なら次の未処理へ進む。そうでなければ現在位置は変えない
- 取り消しは既存の `undo` のまま（1 回で 1 枚。取り消した画像が現在の画像になる）

### コマンド（`commands.rs`）

| コマンド | 動作 |
| --- | --- |
| `start_similar_scan()` | 走査を始める（すでに進行中なら何もしない）。`SimilarStatus` を返す |
| `similar_status()` | `{ generation, done, total, running, failed }` |
| `similar_groups(max_distance)` | 表にある分だけでグループを作って返す（走査の途中でも呼べる）。`max_distance` は 0〜16 に丸める |
| `delete_similar(index)` | `delete_at(index, Similar)`。`ActionResult` を返す |
| `keep_only(keep, others)` | `others` を順に `delete_at`。`KeepOnlyResult { view, deleted, error }` を返す |

```ts
interface SimilarMember {
  index: number; name: string; width: number; height: number; size: number;
  similarity: number; largestResolution: boolean; largestSize: boolean; current: boolean;
}
interface SimilarGroup {
  members: SimilarMember[];
  exact: boolean; sameSize: boolean; sameResolution: boolean; similarNames: boolean;
}
interface SimilarGroups { generation: number; groups: SimilarGroup[] }
```

- `similar_groups` の対象は、その時点で未処理・保留の画像だけ。削除・移動した画像は次の呼び出しで外れ、1 枚になったグループは消える
- `keep_only` は、`keep` と `others` がすべて未処理・保留で重複がなく、`keep` が `others` に含まれないことを先に確かめる（1 つでも違えば何もせずエラー）。途中の 1 枚で失敗したらそこで止め、そこまでの枚数（`deleted`）と失敗の理由（`error`）を返す。すでに削除予定にした分はそのまま（取り消しで戻せる）
- 削除のあとは既存の操作と同じく先読みを更新する

### 設定（`config/`）

- `ActionKeys` に `toggle_similar`（既定 `Ctrl+Shift+D`）と `keep_only`（既定 `Ctrl+Shift+K`）
- 既存の設定ファイルに項目がなければ既定値を補う（`toggle_grid` 追加時と同じ）
- 重複チェックの対象に含める（「類似画像の確認」「これだけ残す」）
- 閾値は TOML に書かない（画面側で `localStorage` に保存。一覧のタイル幅と同じ扱い）

## 画面

### 画面の切り替え

- `Screen` に `"similar"` を追加。新規ファイル `src/views/similar.ts`
- 開く: 仕分け画面のトップバーのボタン（全画面モードでは右上のボタン）、または `toggle_similar` キー
- 閉じる: `Escape` または `toggle_similar` で仕分け画面に戻る
- 同名ファイルの確認中（`session.conflict`）は開かない
- 開いたら `start_similar_scan` を呼び、走査中は 300 ms ごとに `similar_status` を見て進捗を出し、`done` が進むたびに `similar_groups` を取り直す。走査が終わったらポーリングを止める
- 表示中にセッションが変わった（`generation` が変わった）ら閉じる

### 表示

- ヘッダー: 「類似画像」の見出し、進捗（`判定中 120 / 340`、終わったら `12 グループ・31 枚`）、読めなかった枚数、閾値スライダー（類似度 75〜100%、既定 90%）、絞り込み（すべて／完全一致／名前が似ている／解像度が同じ）、閉じるボタン
- 本体: グループを縦に並べる。グループの見出しに根拠のバッジ（完全一致・サイズが同じ・解像度が同じ・名前が似ている）
- グループ内は画像を横に並べる（多ければ折り返す）。タイルは一覧用サムネイル（`lumi://grid/{generation}/{index}`、長辺 480px）を幅 280px で表示し、ファイル名・解像度・ファイルサイズ・類似度を添える。「解像度が最大」「サイズが最大」「現在の画像」の印を付ける
- グループがなければ「似た画像は見つかりませんでした」（走査中は「判定中…」）
- 操作や閾値の変更でグループを取り直したあとは、同じ画像（なければ同じグループの近い画像、なければ近いグループ）を選択し直す

### キー操作

| キー | 動作 |
| --- | --- |
| ← → | グループ内で選択を移動 |
| ↑ ↓ | 前後のグループへ（同じ位置の画像、なければ末尾） |
| Home / End | 最初 / 最後のグループ |
| 設定の `delete` | 選択中の 1 枚を削除予定にする |
| 設定の `keep_only`（既定 `Ctrl+Shift+K`） | 選択中の 1 枚を残し、グループの残りを削除予定にする |
| 設定の `undo` | 直前の操作を取り消す |
| Enter | 選択中の画像を 1 枚表示で開く（`actions.jumpTo`） |
| Esc / `toggle_similar` | 仕分け画面に戻る |

- タイルのクリックで選択、ダブルクリックで開く。タイル上の「削除」「これだけ残す」ボタンでも同じ操作ができる
- 振り分けキー・スキップは受け付けない
- 操作の結果はトーストで伝える（「3 枚を削除予定にしました（取り消しで 1 枚ずつ戻せます）」など）

## エラー処理

- 走査の開始やグループの取得に失敗したらトーストを出す（開始に失敗したら仕分け画面のまま）
- 削除に失敗したら（削除フォルダがない、移動できないなど）既存の削除と同じ文言をトーストに出し、グループを取り直す
- 「これだけ残す」が途中で失敗したら「2 枚を削除予定にしましたが、途中で失敗しました: …」と出す
- 画像を読めなかった枚数はヘッダーに出す（どのファイルかは出さない）

## 終了時の確認

- 削除予定の一覧の理由に「類似画像」を追加する（`DeletionReason::Similar` → `"similar"`）

## テスト（先に書く）

ダミー画像は一時ディレクトリに生成する。実在の写真は使わない。

- Rust
  - `hash`: 同じ絵の縮小版・JPEG 再圧縮版・隅にマークを足した版が既定の閾値以内、別の絵が閾値より遠い
  - `names`: 末尾追加・連番・拡張子違いが true、無関係な名前や番号の差が 2 以上は false
  - `group`: 閾値の境界、鎖でつながる場合、根拠（完全一致はサイズが同じときだけ読み比べる）、目印、並び
  - `Session::delete_at`: 削除フォルダあり／なし、取り消し、現在の画像を消したときと別の画像を消したときの現在位置、処理済み・範囲外・確認待ちは拒否、移動に失敗したら状態を変えない
  - 走査: 全画像を処理して終わる、失敗した画像を数える、`start_session` で古い結果を捨てる
  - コマンド: `similar_groups` が処理済みを外す、`keep_only` の事前チェックと途中失敗、`delete_similar`
  - 設定: `toggle_similar`・`keep_only` の既定値の補完、重複の検出
- フロント（vitest）
  - `similar.test.ts`: 開いたときに走査を始め進捗を出す、グループと根拠の表示、矢印での移動、削除、これだけ残す、取り消し、閾値の変更と保存・復元、絞り込み、Enter で開く、Esc で戻る、失敗時のトースト
  - `main.test.ts`／`sorter.test.ts`: `toggle_similar` キーとボタンで開く、確認中は開かない
  - `settings.test.ts`: キー一覧に 2 つの項目が出る
  - `dialogs.test.ts`: 理由「類似画像」の表示

## ドキュメント

- 説明ページ（日英）の使い方・キー一覧・設定リファレンスに追記
- `docs/requirements.md` の拡張計画に追記、`docs/handoff-notes.md` の決定事項を更新
- `CLAUDE.md` のディレクトリ構成に `similar/` を追加
- バージョンを 0.5.0 に上げる
