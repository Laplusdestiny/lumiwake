# 類似画像の確認と削除（Issue #17） Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 仕分け元フォルダ内の似た画像をグループで示し、手動で（1 枚ずつ、または「これだけ残す」で）削除予定にできる確認画面を追加する。

**Architecture:** Rust に計算だけを持つ新モジュール `similar/`（dHash・ファイル名判定・グループ化・バックグラウンド走査）を足し、削除は `fileops::Session` の新メソッド `delete_at` を通す。画面は新しい `src/views/similar.ts` で、走査の進捗をポーリングしながらグループを表示する。

**Tech Stack:** Rust（Tauri 2、`image` 0.25。依存クレートは増やさない）、TypeScript + Vite + vitest。

**Spec:** `docs/superpowers/specs/2026-11-10-similar-images-design.md`

## Global Constraints

- ユーザーの画像を失わせない。削除は必ず `Session` の既存の経路（削除フォルダへ移動／削除予定の記録）を通し、取り消せること。完全削除は終了時の確認後だけ
- アプリは自動で削除しない・あらかじめ選択しない
- テストを先に書く。テストは一時ディレクトリのダミー画像だけを使う
- 依存クレートを増やさない
- 閾値は距離で持つ: 既定 6、範囲 0〜16。類似度の表示は `round((1 − 距離/64) × 100)`%
- キーの既定値: `toggle_similar = "Ctrl+Shift+D"`、`keep_only = "Ctrl+Shift+K"`
- UI 文言は日本語。コミットは既存の形式（`Feat:` / `Test:` / `Docs:` / `Chore:` ＋日本語）
- 各タスクの最後に `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test`（Rust を触った場合）、`npm run typecheck`、`npm test`（フロントを触った場合）を通す

## Review Focus

仕様が暗に求めるが、放っておくとテストされない条件。各行のテストは担当タスクに入れてある。

1. **走査中に仕分け元を切り替える** — 古い走査の結果が新しいフォルダの表に混ざらず、進捗も新しい走査のものになる（Task 5）
2. **画面が持つ番号が古い** — 設定で並び順を変えると画像の番号が変わる。古い `generation` の番号で `delete_similar` / `keep_only` を呼んでも、別の画像を削除せずエラーになる（Task 6）
3. **デコーダが panic する・ファイルが途中で消える** — その 1 枚を失敗に数えて走査は最後まで進み、`running` が true のまま残らない。完全一致の読み比べ中に読めなければ「完全一致ではない」とする（Task 5、Task 3）
4. **「これだけ残す」の連打・キーリピート** — 2 回目は何も削除しない。リピートは無視し、操作は `actions.enqueue` で 1 つずつ処理する（Task 7）
5. **単色の画像・極小の画像** — ハッシュが全部 0 になっても panic せず、互いに類似として 1 グループになる（Task 1）

---

### Task 1: dHash（`similar/hash.rs`）

**Files:**
- Create: `src-tauri/src/similar/mod.rs`, `src-tauri/src/similar/hash.rs`
- Modify: `src-tauri/src/lib.rs`（`mod similar;` を追加）

**Interfaces:**
- Produces: `similar::hash::dhash(img: &image::DynamicImage) -> u64`、`similar::hash::distance(a: u64, b: u64) -> u32`、定数 `similar::DEFAULT_MAX_DISTANCE: u32 = 6`、`similar::MAX_DISTANCE_LIMIT: u32 = 16`、`similar::similarity_percent(distance: u32) -> u8`

- [ ] **Step 1: 失敗するテストを書く**（`hash.rs` の `mod tests`）

テスト用の絵は `fn scene(seed: u64, w: u32, h: u32) -> RgbImage`（簡単な線形合同法の乱数で 6×4 の色の格子を作り、`resize` の `CatmullRom` で拡大して、塗りつぶした矩形を 12 個重ねる）で作る。

```rust
#[test] fn identical_images_have_distance_zero()            // distance(dhash(a), dhash(a)) == 0
#[test] fn resized_and_recompressed_copies_stay_within_default()
// 10 種類の seed で、800×600 の絵を 400×300 に縮めて JPEG 品質 70 で書いて読み直したものとの距離が DEFAULT_MAX_DISTANCE 以下
#[test] fn a_corner_mark_stays_within_default()
// 右下に幅 20%・高さ 8% の白い矩形を足した版との距離が DEFAULT_MAX_DISTANCE 以下
#[test] fn different_images_are_far_apart()
// seed の違う 10 組の距離がすべて MAX_DISTANCE_LIMIT より大きい
#[test] fn flat_and_tiny_images_do_not_panic()
// 1×1 の画像と単色 100×100 の画像のハッシュはどちらも 0（距離 0）
#[test] fn similarity_percent_rounds()  // 0→100, 6→91, 16→75, 64→0
```

注: 類似度 90% は距離 6.4 にあたるので、距離 6 は 91% と表示される。画面の文言は「類似度 91% 以上」のように計算結果をそのまま出す（設計書の「90%」は目安）。

- [ ] **Step 2: 失敗を確認** — `cd src-tauri && cargo test similar::hash`（コンパイルエラー）
- [ ] **Step 3: 実装** — `img.to_luma8()` を `image::imageops::resize(.., 9, 8, FilterType::Triangle)` で縮め、各行で `左 < 右` のビットを立てる
- [ ] **Step 4: 通ることを確認** — `cargo test similar::hash`。合成画像で閾値に収まらないテストがあれば、縮小フィルタを `Lanczos3` に変えて再確認する（既定の閾値は変えない。変える必要があるなら止めて報告する）
- [ ] **Step 5: コミット** — `Feat: 画像の中身を比べる dHash を追加する`

### Task 2: ファイル名の類似判定（`similar/names.rs`）

**Files:**
- Create: `src-tauri/src/similar/names.rs`
- Modify: `src-tauri/src/similar/mod.rs`

**Interfaces:**
- Produces: `similar::names::similar_names(a: &str, b: &str) -> bool`（引数は拡張子つきのファイル名）

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[test] fn appended_suffixes_match() {
    for (a, b) in [("a.jpg", "a (1).jpg"), ("a.jpg", "a_copy.jpg"), ("a.jpg", "a - コピー.jpg"),
                   ("a.jpg", "a-2.jpg"), ("photo.png", "photo copy 2.png"), ("A.JPG", "a (1).jpg"),
                   ("a.jpg", "a - コピー (2).jpg")] { assert!(similar_names(a, b)); assert!(similar_names(b, a)); }
}
#[test] fn sequential_numbers_match()   // IMG_0012/IMG_0013、shot9/shot10、12/13
#[test] fn same_stem_with_another_extension_matches()  // a.jpg / a.png
#[test] fn unrelated_names_do_not_match() {
    // ("a.jpg","b.jpg") ("IMG_0012.jpg","IMG_0014.jpg") ("cat.jpg","category.jpg")
    // ("a.jpg","ab.jpg") ("IMG_0012.jpg","DSC_0013.jpg") ("a.jpg","a.jpg" は同じ名前なので true)
}
```

- [ ] **Step 2: 失敗を確認** — `cargo test similar::names`
- [ ] **Step 3: 実装** — 拡張子を除いて小文字にする。末尾追加は、長いほうの残りを「区切り（空白・`_`・`-`）、`(数字)`、数字、`copy`、`コピー`」のトークンだけで読み切れるか（少なくとも 1 文字は区切りか括弧で始まる）で判定する。連番は末尾の数字列を `u64` にして差が 1。正規表現クレートは使わない
- [ ] **Step 4: 通ることを確認**
- [ ] **Step 5: コミット** — `Feat: ファイル名が似ているかの判定を追加する`

### Task 3: グループ化（`similar/group.rs`）

**Files:**
- Create: `src-tauri/src/similar/group.rs`
- Modify: `src-tauri/src/similar/mod.rs`

**Interfaces:**
- Consumes: Task 1 の `distance`・`similarity_percent`、Task 2 の `similar_names`
- Produces:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fingerprint { pub hash: u64, pub width: u32, pub height: u32, pub size: u64 }
pub struct Candidate { pub index: usize, pub path: PathBuf, pub print: Fingerprint }
pub struct Group { pub members: Vec<Member>, pub exact: bool, pub same_size: bool, pub same_resolution: bool, pub similar_names: bool }
pub struct Member { pub index: usize, pub similarity: u8, pub largest_resolution: bool, pub largest_size: bool }
pub fn group(candidates: &[Candidate], max_distance: u32, same_bytes: impl Fn(&Path, &Path) -> bool) -> Vec<Group>
pub fn same_bytes(a: &Path, b: &Path) -> bool   // 読めなければ false
```

- [ ] **Step 1: 失敗するテストを書く** — ハッシュは手で作る（`0`、`0b111`、`u64::MAX` など）

```rust
#[test] fn groups_pairs_within_the_threshold_only()   // 距離 6 は max 6 で同じグループ、max 5 では別。1 枚だけのものは出ない
#[test] fn chains_join_into_one_group_and_similarity_is_to_the_nearest()
// A-B 距離 3、B-C 距離 3、A-C 距離 6、max 4 → 1 グループ。全員の similarity は similarity_percent(3)
#[test] fn groups_and_members_follow_session_order()  // index の昇順。グループは先頭メンバーの index 順
#[test] fn evidence_flags()
// サイズが全員同じ → same_size。解像度が全員同じ → same_resolution。名前が似た組が 1 つあれば similar_names
#[test] fn exact_compares_bytes_only_when_sizes_match()
// same_bytes に呼ばれた回数を数えるクロージャを渡す。サイズが違うグループでは 0 回で exact == false。
// サイズが同じで same_bytes が true なら exact == true、false なら exact == false
#[test] fn largest_marks_only_a_single_maximum()      // 単独で最大のときだけ。全員同じ・最大が 2 枚なら誰にも付けない
#[test] fn same_bytes_reads_files()                   // 一時ファイル: 同じ内容 true、違う内容 false、片方がない false
```

- [ ] **Step 2: 失敗を確認** — `cargo test similar::group`
- [ ] **Step 3: 実装** — 総当たりで距離を求めて Union-Find でつなぐ。`exact` は先頭メンバーと残り全員を `same_bytes` で比べる。`same_bytes` は両方を `BufReader` で 64 KiB ずつ読み比べる
- [ ] **Step 4: 通ることを確認**
- [ ] **Step 5: コミット** — `Feat: 似た画像をグループにまとめ、根拠を付ける`

### Task 4: 番号を指定した削除（`fileops`）と理由「類似画像」

**Files:**
- Modify: `src-tauri/src/fileops/session.rs`、`src/api.ts`（`DeletionReason` に `"similar"`）、`src/views/dialogs.ts`（`REASON` に `similar: "類似画像の確認で削除"`）
- Test: `session.rs` の `mod tests`、`src/views/dialogs.test.ts`

**Interfaces:**
- Produces: `DeletionReason::Similar`（JSON では `"similar"`）、`FileOpError::NotOpen`（表示: 「この画像はすでに振り分け済みです」）、`Session::delete_at(&mut self, idx: usize, reason: DeletionReason) -> Result<(), FileOpError>`

- [ ] **Step 1: 失敗するテストを書く**（既存の `env` / `session` / `FaultyFs` を使う）

```rust
#[test] fn delete_at_marks_another_image_without_moving_the_cursor()
// 5 枚、削除フォルダなし。delete_at(3, Similar) → items[3] は Marked{Similar}、current() は Some(0)、ファイルは元の場所にある
#[test] fn delete_at_moves_to_the_delete_folder()
// 削除フォルダあり。delete_at(2, Similar) → trash に img3.jpg、status は Trashed{reason: Similar}
#[test] fn delete_at_on_the_current_image_advances()   // delete_at(0) → current() == Some(1)
#[test] fn delete_at_can_be_undone_one_by_one()
// delete_at(1)、delete_at(2) → undo で img3 が戻り current()==Some(2)、もう一度 undo で img2 が戻る。ファイルも元の場所
#[test] fn delete_at_rejects_closed_out_of_range_and_pending_conflict()
// 移動済み・削除済みの番号と items.len() は NotOpen。同名ファイルの確認待ち中は ConflictPending。どれも履歴は増えない
#[test] fn delete_at_keeps_state_when_the_move_fails()
// FaultyFs で削除フォルダへの移動を失敗させる → Err、status は Pending のまま、履歴なし、ファイルは元の場所
#[test] fn similar_deletions_are_listed_as_pending()   // pending_deletions() の reason が Similar
```

`dialogs.test.ts`: 理由が `"similar"` の削除予定に「類似画像の確認で削除」と出る。

- [ ] **Step 2: 失敗を確認** — `cargo test fileops::session::tests::delete_at`、`npm test -- dialogs`
- [ ] **Step 3: 実装** — `delete_at` は確認待ちと `is_open` を確かめて `delete_item(idx, reason)` を呼び、`idx == self.cursor` のときだけ `advance_from(idx)`。`delete_item` のラベルに `Similar => "削除（類似）"` を足す
- [ ] **Step 4: 通ることを確認** — `cargo test fileops`、`npm test`
- [ ] **Step 5: コミット** — `Feat: 番号を指定して画像を削除予定にできるようにする`

### Task 5: バックグラウンド走査（`similar/scan.rs`）と状態

**Files:**
- Create: `src-tauri/src/similar/scan.rs`
- Modify: `src-tauri/src/similar/mod.rs`、`src-tauri/src/state.rs`、`src-tauri/src/commands.rs`（`start_session` で `reset`）

**Interfaces:**
- Consumes: Task 1 の `dhash`、Task 3 の `Fingerprint`
- Produces:

```rust
// similar/mod.rs
pub fn fingerprint(registry: &Registry, path: &Path) -> Result<Fingerprint, String>  // デコード→ハッシュ・解像度、fs::metadata でサイズ

// similar/scan.rs
pub struct ScanJob { pub key: PathBuf, pub path: PathBuf }       // key: 元の場所、path: 今ある場所
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)] #[serde(rename_all = "camelCase")]
pub struct ScanStatus { pub generation: u64, pub done: usize, pub total: usize, pub running: bool, pub failed: usize }
pub type PrintFn = Arc<dyn Fn(&Path) -> Result<Fingerprint, String> + Send + Sync>;
impl SimilarScan {
    pub fn new() -> Self
    pub fn reset(&self, generation: u64)                         // 表を空にし、進行中の走査を中止する
    pub fn start(self: &Arc<Self>, generation: u64, jobs: Vec<ScanJob>, workers: usize, load: PrintFn) -> ScanStatus
    pub fn status(&self) -> ScanStatus
    pub fn get(&self, key: &Path) -> Option<Fingerprint>         // 成功したものだけ
}
// state.rs
pub similar: Arc<SimilarScan>   // AppState のフィールド
```

意味: `done` は表にある枚数（成功＋失敗）、`total` は `done` ＋まだ処理していない枚数、`failed` は失敗の枚数。`start` は、すでに表にある `key` と、進行中の走査に入っている `key` を除いて足す。進行中でなければワーカーを `workers` 本立てる。`start` の `generation` が `reset` で設定した値と違えば何もしない。

- [ ] **Step 1: 失敗するテストを書く**（`scan.rs`。`load` は偽物を渡し、終了は `status().running == false` になるまで 5 ms ごとに最大 5 秒待つヘルパーで待つ）

```rust
#[test] fn scans_every_job_and_reports_progress()   // 5 件 → done 5, total 5, running false, failed 0。get で値が取れる
#[test] fn failures_are_counted_and_not_returned()  // 1 件が Err → failed 1、done は 5、get は None
#[test] fn a_panicking_loader_counts_as_a_failure_and_the_scan_finishes()   // load が 1 件で panic → failed 1、running false
#[test] fn known_images_are_not_loaded_again()      // 2 回目の start（同じ 5 件＋新しい 1 件）で load が呼ばれるのは 1 回
#[test] fn reset_discards_results_of_an_older_scan()
// load を Barrier で止めたまま reset(2) → 止めていた load を進める → status は {generation: 2, done: 0, total: 0, running: false}、get は None
#[test] fn start_with_a_stale_generation_does_nothing()
```

`state.rs` / `commands.rs` のテスト: `start_session` をやり直すと `state.similar.status().generation` が新しい `generation` になり `done == 0`。

- [ ] **Step 2: 失敗を確認** — `cargo test similar::scan`
- [ ] **Step 3: 実装** — `Mutex<Inner { generation, table: HashMap<PathBuf, Option<Fingerprint>>, queue: VecDeque<ScanJob>, workers_alive: usize }>`。ワーカーはロックを取って 1 件取り出し、ロックを外して `std::panic::catch_unwind(AssertUnwindSafe(|| load(&job.path)))` を呼び、ロックを取り直して `generation` が同じときだけ表に入れる。キューが空か `generation` が変わったら終わる。`running` は `workers_alive > 0 || !queue.is_empty()`。`AppState::new` で `similar: Arc::new(SimilarScan::new())`、`start_session` で `state.similar.reset(generation)`
- [ ] **Step 4: 通ることを確認** — `cargo test similar`、`cargo test commands`
- [ ] **Step 5: コミット** — `Feat: 類似判定用の情報をバックグラウンドで集める`

### Task 6: コマンドと設定キー

**Files:**
- Modify: `src-tauri/src/commands.rs`、`src-tauri/src/lib.rs`（`generate_handler!` に 5 つ追加）、`src-tauri/src/config/mod.rs`、`src/api.ts`、`src/test/fixtures.ts`、`src/views/settings.ts`
- Test: `commands.rs`・`config/mod.rs` の `mod tests`、`src/views/settings.test.ts`、`src/api.test.ts`

**Interfaces:**
- Consumes: Task 3 `group` / `same_bytes`、Task 4 `delete_at`、Task 5 `SimilarScan` / `fingerprint`
- Produces（Rust コマンド。画面に渡した番号が古くないか `generation` で確かめる。設計書から足した引数）:

```rust
pub fn start_similar_scan(state) -> CmdResult<ScanStatus>
pub fn similar_status(state) -> ScanStatus                       // セッションがなくても呼べる
pub fn similar_groups(state, max_distance: u32) -> CmdResult<SimilarGroups>
pub fn delete_similar(state, generation: u64, index: usize) -> CmdResult<ActionResult>
pub fn keep_only(state, generation: u64, keep: usize, others: Vec<usize>) -> CmdResult<KeepOnlyResult>
```

```ts
// src/api.ts
export interface SimilarStatus { generation: number; done: number; total: number; running: boolean; failed: number }
export interface SimilarMember { index: number; name: string; width: number; height: number; size: number;
  similarity: number; largestResolution: boolean; largestSize: boolean; current: boolean }
export interface SimilarGroup { members: SimilarMember[]; exact: boolean; sameSize: boolean; sameResolution: boolean; similarNames: boolean }
export interface SimilarGroups { generation: number; groups: SimilarGroup[] }
export interface KeepOnlyResult { view: SessionView; deleted: number; error: string | null }
api.startSimilarScan(): Promise<SimilarStatus>
api.similarStatus(): Promise<SimilarStatus>
api.similarGroups(maxDistance: number): Promise<SimilarGroups>
api.deleteSimilar(generation: number, index: number): Promise<ActionResult>
api.keepOnly(generation: number, keep: number, others: number[]): Promise<KeepOnlyResult>
// Config["keys"] に toggle_similar: string; keep_only: string;
```

設定: `ActionKeys` に `toggle_similar`（既定 `"Ctrl+Shift+D"`、ラベル「類似画像の確認」）と `keep_only`（既定 `"Ctrl+Shift+K"`、ラベル「これだけ残す」）。`entries` / `entries_mut` は 10 個になる。`settings.ts` のキー一覧に `{ name: "toggle_similar", label: "類似画像の確認" }`、`{ name: "keep_only", label: "類似画像で「これだけ残す」" }`。

エラー文言: 古い `generation` は「画像の並びが変わりました。類似画像を開き直してください」。`keep_only` の事前チェック違反は「選んだ画像の状態が変わりました。もう一度選んでください」。

- [ ] **Step 1: 失敗するテストを書く**

`commands.rs`（`Fixture` を拡張: `src` に同じ絵の `dup-a.png`・`dup-b.png`（`dup-a` を縮めたもの）と、バイト列まで同じ `dup-a (1).png` を置くヘルパー `add_duplicates()`。走査の終了は `similar_status` をポーリングして待つ）:

```rust
#[test] fn similar_commands_fail_before_loading_a_folder()   // start_similar_scan / similar_groups / delete_similar / keep_only は Err
#[test] fn scan_then_groups_report_members_and_evidence()
// 走査後: グループ 1 つ（dup-a (1).png, dup-a.png, dup-b.png の並び順）。similarNames は true、exact は false（dup-b が違う）、
// dup-b を消したあと取り直すと exact == true・sameSize == true
#[test] fn groups_leave_out_processed_images()               // 1 枚を移動 → 取り直すとメンバーから外れる。残り 1 枚ならグループが消える
#[test] fn max_distance_is_clamped_to_the_limit()            // similar_groups(999) は 16 と同じ結果
#[test] fn delete_similar_marks_the_image_and_can_be_undone()
#[test] fn keep_only_deletes_the_rest_of_the_group()         // deleted == 2、keep は未処理のまま、pending_deletions の理由は Similar
#[test] fn keep_only_validates_before_deleting_anything()
// keep が others に含まれる／others に重複／others に処理済みがある／範囲外 → Err で、削除予定は 0 件のまま
#[test] fn keep_only_stops_at_the_first_failure_and_reports_it()
// 削除フォルダを設定したあとフォルダを消す → deleted == 0、error が Some、ファイルは元の場所
#[test] fn stale_generation_is_rejected()
// 設定で並び順を変えて番号が変わったあと、古い generation で delete_similar / keep_only → Err、削除予定 0 件
// （並び順の変更で generation が変わらない実装なら、apply_config で変えるよう直す）
#[test] fn rescanning_after_reorder_reuses_fingerprints()    // 並び順を変えてから start_similar_scan → done がすぐ total に等しい
```

`config/mod.rs`: `toggle_similar_and_keep_only_default_and_are_checked_for_duplicates`（項目のない TOML を読むと既定値、他のキーと同じ値にすると重複の Issue）。
`settings.test.ts`: キー一覧に 2 項目が出る。`api.test.ts`: 5 つの関数が正しいコマンド名と引数で `invoke` を呼ぶ。

- [ ] **Step 2: 失敗を確認** — `cargo test commands config`、`npm test`
- [ ] **Step 3: 実装** — `start_similar_scan` はセッションの未処理・保留の画像から `ScanJob { key: item.path, path: location(..) }` を作り、`state.similar.start(generation, jobs, workers, load)`（`workers` は `available_parallelism` を 1〜3 に丸める。`load` は `fingerprint(&registry, p)`）。`similar_groups` は未処理・保留で `state.similar.get` が取れるものを `Candidate` にして `group(.., max_distance.min(MAX_DISTANCE_LIMIT), same_bytes)`。`delete_similar` と `keep_only` は `with_session` の中で `generation` を確かめてから `delete_at(.., DeletionReason::Similar)`
- [ ] **Step 4: 通ることを確認** — `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`、`npm run typecheck && npm test`
- [ ] **Step 5: コミット** — `Feat: 類似画像のコマンドと、開くキー・これだけ残すキーの設定を追加する`

### Task 7: 類似画像の確認画面

**Files:**
- Create: `src/views/similar.ts`、`src/views/similar.test.ts`
- Modify: `src/store.ts`、`src/main.ts`、`src/views/sorter.ts`、`src/styles.css`
- Test: `src/views/similar.test.ts`、`src/main.test.ts`、`src/views/sorter.test.ts`

**Interfaces:**
- Consumes: Task 6 の `api.*` と型
- Produces:

```ts
// store.ts
export type Screen = "start" | "sort" | "settings" | "grid" | "similar";
store.similar: { generation: number; status: SimilarStatus | null; groups: SimilarGroup[] } | null

// views/similar.ts
export const THRESHOLD_DEFAULT = 6; export const THRESHOLD_MAX = 16;
export function clampThreshold(v: unknown): number          // 数値でなければ 6、0〜16 に丸める
export function similarityPercent(distance: number): number  // Math.round((1 - distance / 64) * 100)
export function openSimilar(): Promise<void>
export function closeSimilar(): void
export function renderSimilar(root: HTMLElement): void
export function handleSimilarKey(combo: string, e: Pick<KeyboardEvent, "repeat">): boolean
export function handleSimilarAction(action: string, el: HTMLElement): boolean
export function handleSimilarInput(el: HTMLInputElement | HTMLSelectElement): void
```

決めごと:
- 閾値は `localStorage` の `lumiwake.similarThreshold`（距離）。スライダーは `min=0 max=16 step=1`、左が厳しい。横に「類似度 91% 以上」と出す
- 絞り込みは `<select data-similar="filter">`: `all`（すべて）／`exact`（完全一致）／`names`（名前が似ている）／`resolution`（解像度が同じ）。画面側で `groups` を絞る
- ポーリング: `openSimilar` で `startSimilarScan` → `similarGroups`。`status.running` の間は 300 ms 後に `similarStatus` を呼び、`done` が増えていたら `similarGroups` を取り直す。画面を閉じたら止める
- `data-action`: `similar`（開く。仕分け画面のボタン）、`similar-close`、`similar-select`（タイル）、`similar-delete`、`similar-keep`。ダブルクリックは `dblclick` で `similar-select` のタイルを開く
- 削除・これだけ残す・取り消しは `actions.enqueue` に並べ、結果で `actions.setSession(view)` を呼んでからグループを取り直す。`e.repeat` と `actions.busy()` のときは何もしない（true を返す）
- 取り直したあとの選択: 同じ `index` があればそれ。なければ直前のグループ位置と同じ位置のグループの、直前と同じ位置（なければ末尾）の画像。グループがなければ `null`
- トースト: 削除「削除予定にしました: {name}」、これだけ残す「{n} 枚を削除予定にしました（取り消しで 1 枚ずつ戻せます）」、途中失敗「{n} 枚を削除予定にしましたが、途中で失敗しました: {error}」（エラー表示）
- 空の表示: 走査中でグループなし「判定中…」、走査後「似た画像は見つかりませんでした」、絞り込みで 0「該当するグループがありません」
- ヘッダーの進捗: 走査中「判定中 {done} / {total}」、終了後「{g} グループ・{n} 枚」。`failed > 0` なら「読めなかった画像 {failed} 枚」
- タイル: 幅 280px、`lumiUrl("grid/{generation}/{index}")`、ファイル名、`{width}×{height}`、`util.ts` の既存のサイズ表記、`類似度 {similarity}%`、印「解像度が最大」「サイズが最大」「現在の画像」
- 仕分け画面: 一覧ボタンの隣に `data-action="similar"` のボタン（`data-slot="similar-btn"`、title「類似画像（{キー}）」）、ヒントに「{キー} 類似画像」。`handleSorterKey` で `k.toggle_similar` → `openSimilar()`
- `main.ts`: `render` に `case "similar"`、keydown に `case "similar": return handleSimilarKey(combo, e)`、click に `store.screen === "similar" && handleSimilarAction(..)` と `case "similar": return await openSimilar()`、input で `handleSimilarInput`
- `renderSimilar` は、`store.similar` がない・`generation` がセッションと違う・`session.conflict` があるときは `closeSimilar()`

- [ ] **Step 1: 失敗するテストを書く**（`grid.test.ts` と同じ作り。`api` をモックし、`vi.useFakeTimers()` でポーリングを進める）

```ts
describe("類似画像の確認画面", () => {
  it("開くと走査を始め、進捗を出し、走査が進むたびにグループを取り直す");
  it("走査が終わったらポーリングを止め、グループ数と枚数を出す");
  it("同名ファイルの確認中・セッションがないときは開かない");
  it("走査の開始に失敗したらトーストを出して仕分け画面のまま");
  it("グループの根拠のバッジと、画像の目印・類似度・解像度を出す");
  it("← → でグループ内、↑ ↓ でグループ間、Home / End で最初と最後のグループへ移る");
  it("削除キーで選択中の 1 枚を deleteSimilar に渡し、グループを取り直す");
  it("keep_only のキーで、選択中を keep、同じグループの残りを others にして keepOnly を呼ぶ");
  it("キーリピートと連打では keepOnly を重ねて呼ばない");   // e.repeat は無視。busy のときも無視
  it("これだけ残すが途中で失敗したら、件数と理由をエラーで出す");
  it("取り消しキーで undo を呼び、グループを取り直す");
  it("取り直したあと、消えた画像の代わりに同じグループの近い画像を選ぶ");
  it("閾値のスライダーで similarGroups を呼び直し、値を保存する。保存値を復元し、範囲外は丸める");
  it("絞り込みで該当するグループだけを出す");
  it("Enter で選択中の画像を jumpTo で開いて閉じる");
  it("Esc と toggle_similar のキーで閉じる。振り分けキー・スキップは受け付けない");
  it("並び順が変わって generation が変わったら閉じる");
  it("サムネイルを読めなかったタイルはアイコンを出す");
});
```

`sorter.test.ts`: 類似画像ボタンを出し、`toggle_similar` で開く。`main.test.ts`: `similar` 画面のキー・クリック・入力が `similar.ts` に渡る。

- [ ] **Step 2: 失敗を確認** — `npm test -- similar sorter main`
- [ ] **Step 3: 実装** — `grid.ts` の作り（骨組みを 1 回だけ作り、`data-slot` を書き換える）にならう。スタイルは既存の `.grid-*` と色の変数を使い、`.similar-*` を足す
- [ ] **Step 4: 通ることを確認** — `npm run typecheck && npm test`
- [ ] **Step 5: 実際の画面で確認** — 一時ディレクトリにダミー画像（同じ絵の縮小版・マーク付き・別の絵）を作り、`npm run tauri dev` が使える環境ならアプリで、使えなければ Vite の画面を `api` のモック付きで Playwright から開き、表示とキー操作のスクリーンショットを確かめる
- [ ] **Step 6: コミット** — `Feat: 類似画像を確認して削除する画面を追加する`

### Task 8: ドキュメントとバージョン

**Files:**
- Modify: `site/src/content/docs/` の使い方・キー一覧・`reference/config.md`（日本語）と `site/src/content/docs/en/` の同じページ、`docs/requirements.md`（拡張計画に「類似画像の確認と削除」）、`docs/handoff-notes.md`（決定事項に 1 項目、範囲外にした案の Issue 番号）、`CLAUDE.md`（ディレクトリ構成に `similar/`、「4 モジュール構成」の記述）、`docs/superpowers/specs/2026-11-10-similar-images-design.md`（コマンドの `generation` 引数、類似度の表示が計算値であること）、`package.json`・`package-lock.json`・`src-tauri/Cargo.toml`・`src-tauri/Cargo.lock`・`src-tauri/tauri.conf.json` のバージョン（0.4.0 → 0.5.0。0.4.0 に上げたコミット `401844e` と同じファイルを変える）

- [ ] **Step 1: 説明ページ（日英）に、類似画像の確認の使い方・キー・設定項目 `toggle_similar` / `keep_only` を書く**
- [ ] **Step 2: 設計書・要件定義書・引き継ぎメモ・CLAUDE.md を更新する**
- [ ] **Step 3: バージョンを 0.5.0 に上げる**
- [ ] **Step 4: 確認** — `cd site && npm ci && npm run build`、リポジトリ全体で `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`、`npm run typecheck && npm test`
- [ ] **Step 5: コミット** — `Docs: 類似画像の確認を説明ページと設計メモに追加する`、`Chore: バージョンを 0.5.0 に上げる`
