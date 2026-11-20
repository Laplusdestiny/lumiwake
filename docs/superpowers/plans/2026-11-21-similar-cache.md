# 類似判定の結果の保存（Issue #21）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 類似判定の結果（dHash・幅・高さ）をキャッシュフォルダの JSON に保存し、アプリを終了して後日また起動しても、変わっていない画像と Lumiwake が移動した画像はデコードせずに使い回す。

**Architecture:** `similar/store.rs` に保存した記録（`PrintStore`）を新しく作り、画像ごとに「パス＋更新日時＋サイズ」で照合する。判定のワーカーが使う関数を、記録を引いてからデコードする `similar::fingerprint_cached` に替える。書き出しは途中（200 件ごと）・判定の終わり・終了時（`exit_app`）・アップデート後の再起動前（新コマンド `save_similar_cache`）に行い、そのときに移動した画像の記録を移動先のパスへ複製する。

**Tech Stack:** Rust（`serde` / `serde_json`、追加の依存なし）、Tauri 2、TypeScript + vitest

**Spec:** `docs/superpowers/specs/2026-11-21-similar-cache-design.md`

## Global Constraints

- 依存クレートは増やさない（`serde_json` は導入済み）
- 保存ファイル: `app_cache_dir()/similar-cache.json`
- 照合: パス・更新日時（ナノ秒）・サイズの 3 つがすべて一致したときだけ使う
- 形式のバージョン `STORE_VERSION = 1`。違えば全部捨てる
- 掃除: 最後に使ってから 90 日（`MAX_AGE_SECS = 90 * 24 * 60 * 60`）で捨てる。上限 `MAX_RECORDS = 200_000`、超えたら最後に使った日時の古いものから捨てる
- 途中の書き出し: 新しい記録が `FLUSH_EVERY = 200` 件たまるごと
- 起動時には読まない。最初に必要になったとき 1 回だけ読む
- ファイルがない・壊れている・バージョン違いは空として扱い、エラーにしない
- 書き出しは一時ファイル → 置き換え。失敗しても判定・終了は止めない（`eprintln!` で残すだけ）。変更がなければ書かない。ロックを持ったまま書かない
- 読めなかった画像、パスを UTF-8 で表せない画像、更新日時を取れない画像は保存しない
- テストの画像と保存先は一時ディレクトリだけを使う（実在の写真は使わない）
- コミットごとに `cargo fmt --check`・`cargo clippy --all-targets -- -D warnings`・`cargo test`（`src-tauri/`）、フロントを変えたら `npm run typecheck`・`npm test` が通ること
- コミットメッセージは既存の形式（`Feat: …` / `Docs: …` ＋日本語）で、末尾に `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`

## Review Focus

1. 画像を同じサイズのまま編集した（更新日時だけ変わる）→ 保存した記録を使わずに読み直し、記録を置き換える。Task 1・Task 2 のテストで確かめる
2. 保存先に書けない（親がファイル、権限がない）→ 判定は最後まで終わり、終了もできる。Task 2・Task 3 のテストで確かめる
3. 振り分けたあと取り消して元の場所に戻した → 元の場所の記録をそのまま使う（移動元の記録を消していない）。Task 3 のテストで確かめる
4. 判定を始めた直後、複数のワーカーが同時に初めて記録を引く → 保存ファイルを 1 回だけ読み、どのワーカーも前回の記録を使える（読み込み中に「空」と見なして読み直さない）。Task 1 のテストで確かめる。書き出しをロックの外で行うこと（判定を止めない）は最終レビューで確かめる
5. 前回までの保存ファイルが壊れている／古い形式 → 空として扱い、次の書き出しで作り直す。Task 1 のテストで確かめる

---

## File Structure

| ファイル | 役割 |
| --- | --- |
| `src-tauri/src/similar/store.rs`（新規） | 保存した記録 `PrintStore`、`FileStamp`、照合・複製・掃除・読み書き |
| `src-tauri/src/similar/mod.rs`（変更） | `pub mod store;`、`fingerprint_cached` |
| `src-tauri/src/similar/scan.rs`（変更） | 走査が終わったときに呼ぶ `on_idle` |
| `src-tauri/src/state.rs`（変更） | `AppState::new(config_path, cache_path)`、`prints: Arc<PrintStore>`、`save_prints()` |
| `src-tauri/src/commands.rs`（変更） | `start_similar_scan` で `fingerprint_cached` を使う、`exit_app` と新コマンド `save_similar_cache` で書き出す |
| `src-tauri/src/lib.rs`（変更） | キャッシュの保存先を渡す、`save_similar_cache` を登録 |
| `src-tauri/src/protocol.rs`（変更） | テストの `AppState::new` の引数 |
| `src/api.ts`・`src/updater.ts`（変更） | `api.saveSimilarCache`、再起動の前に呼ぶ |
| ドキュメント（変更） | 説明ページ（日英）`guides/usage.md` の類似画像の節、`docs/handoff-notes.md`、`CLAUDE.md` |

---

### Task 1: 保存した記録 `PrintStore`

**Files:**
- Create: `src-tauri/src/similar/store.rs`
- Modify: `src-tauri/src/similar/mod.rs`（`pub mod store;`）

**Interfaces:**
- Produces:
```rust
pub const STORE_VERSION: u32 = 1;
pub const MAX_AGE_SECS: u64 = 90 * 24 * 60 * 60;
pub const MAX_RECORDS: usize = 200_000;
pub const FLUSH_EVERY: usize = 200;

/// 照合に使うファイルの状態
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStamp { pub modified_ns: u64, pub size: u64 }
/// 今のファイルの状態。更新日時を取れない（エポックより前など）ときは Ok(None)
pub fn stamp(path: &Path) -> std::io::Result<Option<FileStamp>>;
/// 今の時刻（UNIX 秒）
pub fn now_secs() -> u64;

pub struct PrintStore { /* path: PathBuf, inner: Mutex<…> */ }
impl PrintStore {
    pub fn new(file: PathBuf) -> Self;                 // 読み込まない
    pub fn lookup(&self, path: &Path, stamp: FileStamp, now: u64) -> Option<Fingerprint>; // 一致したら最後に使った日時を更新
    pub fn insert(&self, path: &Path, stamp: FileStamp, print: Fingerprint, now: u64);    // 新しい記録（未保存の件数を数える）
    pub fn copy_record(&self, from: &Path, to: &Path, now: u64);                         // 移動先へ複製。from になければ何もしない
    pub fn unsaved(&self) -> usize;                                                      // 前回の書き出しからの新しい記録の件数
    pub fn save(&self, now: u64) -> std::io::Result<()>;                                 // 掃除して書き出す。変更がなければ書かない
}
```
- `Fingerprint` は `similar::group::Fingerprint`（`hash, width, height, size`）。記録には `hash, width, height` と `FileStamp`、最後に使った日時（UNIX 秒）を持ち、`lookup` は `size` を `stamp.size` から作って返す
- 初回の `lookup` / `insert` / `copy_record` / `save` で 1 回だけファイルを読む

- [ ] **Step 1: 失敗するテストを書く（`store.rs` の `mod tests`。保存先は `tempfile::tempdir()`）**

```rust
const DAY: u64 = 24 * 60 * 60;
fn st(ns: u64, size: u64) -> FileStamp { FileStamp { modified_ns: ns, size } }
fn fp(hash: u64, size: u64) -> Fingerprint { Fingerprint { hash, width: 4, height: 3, size } }

#[test] fn uses_a_record_only_when_path_time_and_size_match() {
    // insert("/a", st(1, 10), fp(7, 10)) のあと
    // lookup("/a", st(1,10)) == Some(fp(7,10))、st(2,10) / st(1,11) / 別パス "/b" は None
}
#[test] fn survives_a_new_store_on_the_same_file() {
    // insert → save(now) → 同じパスで PrintStore::new → lookup が Some
}
#[test] fn broken_old_or_missing_files_are_treated_as_empty() {
    // ファイルなし / "壊れた" という中身 / version を 0 にした JSON → lookup は None、パニックしない
    // そのあと insert + save したファイルは読み直せる（作り直される）
}
#[test] fn save_skips_when_nothing_changed() {
    // 新しい store で save → ファイルができない。insert のあと save でできる。
    // 2 回目の save の前にファイルを消しておくと、変更がないので作られない
}
#[test] fn save_writes_through_a_temporary_file_and_fails_quietly_when_the_folder_is_unwritable() {
    // 保存先の親を「ファイル」にした store: insert → save は Err、store は使い続けられる（lookup は Some）
    // 正常な store の save 後、保存先のフォルダに一時ファイルが残っていない
}
#[test] fn drops_records_unused_for_90_days() {
    // insert(now=0) の記録と insert(now=89*DAY) の記録 → save(now = 90*DAY + 1) → 読み直すと前者だけ None
    // lookup で使った記録（now=89*DAY に lookup）は残る
}
#[test] fn keeps_the_newest_when_over_the_limit() {
    // テスト用に上限を小さくした store（`PrintStore::with_limit(file, 2)` を #[cfg(test)] で用意）で 3 件 insert（now=1,2,3）
    // → save → 読み直すと now=1 の記録だけ None
}
#[test] fn copies_a_record_to_the_moved_path_and_keeps_the_original() {
    // insert("/src/a", …) → copy_record("/src/a", "/dst/a", now) → 両方 lookup で Some
    // 記録のない "/src/x" を copy_record しても "/dst/x" は None
}
#[test] fn counts_unsaved_records_until_saved() {
    // insert 3 回 → unsaved() == 3 → save → 0。lookup だけでは増えない
}
#[test] fn first_lookups_from_many_threads_all_see_the_saved_records() {
    // 10 件 insert → save → 新しい store を Arc で 8 スレッドに渡し、同時に lookup → すべて Some
}
#[test] fn stamp_reads_time_and_size_of_a_real_file() {
    // 一時ファイルに 5 バイト → stamp(p).unwrap().unwrap().size == 5。存在しないパスは Err
}
```

- [ ] **Step 2: 実行して失敗を確認** — `cd src-tauri && cargo test similar::store` → コンパイルエラー（モジュールがない）

- [ ] **Step 3: 実装** — JSON は `{"version":1,"records":[{"path":…,"modified_ns":…,"size":…,"hash":…,"width":…,"height":…,"last_used":…}]}`（`hash` は `u64`。JSON の数値として保てるよう文字列にするかは実装者の判断。`serde_json` は u64 を正確に扱える）。`save` はロックの中で掃除と写しだけを作り、ロックを外してから `<file>.tmp` に書いて `rename` する（親フォルダがなければ作る）。写した時点の「未保存の件数」だけを差し引く（書いている間に増えた分は残す）

- [ ] **Step 4: 実行して通ることを確認** — `cargo test similar::store` が PASS、`cargo test` 全体も PASS

- [ ] **Step 5: コミット** — `Feat: 類似判定の結果を保存する記録を用意する`

---

### Task 2: 判定で記録を使い、途中と終わりに書き出す

**Files:**
- Modify: `src-tauri/src/similar/mod.rs`（`fingerprint_cached`）、`src-tauri/src/similar/scan.rs`（`on_idle`）、`src-tauri/src/state.rs`、`src-tauri/src/commands.rs`（`start_similar_scan` と `AppState::new` の呼び出し）、`src-tauri/src/lib.rs`、`src-tauri/src/protocol.rs`（テストの `AppState::new`）

**Interfaces:**
- Consumes: Task 1 の `PrintStore`, `stamp`, `now_secs`, `FLUSH_EVERY`
- Produces:
```rust
// similar/mod.rs
/// 保存した記録があればデコードせずに返す。なければデコードして記録する
pub fn fingerprint_cached(registry: &Registry, store: &PrintStore, path: &Path) -> Result<Fingerprint, String>;
// similar/scan.rs
impl SimilarScan { pub fn with_on_idle(on_idle: Arc<dyn Fn() + Send + Sync>) -> Self; }
// state.rs
impl AppState { pub fn new(config_path: PathBuf, cache_path: PathBuf) -> Self; }
pub prints: Arc<PrintStore>   // AppState のフィールド
```
- `on_idle` は、走査の待ちがなくなり、読んでいる途中の画像もなくなったとき（最後のワーカーが抜けるとき）に、ロックを外してから 1 回呼ぶ。`AppState::new` は `on_idle` に「`prints.save(now_secs())` を呼び、失敗は `eprintln!`」を渡す
- `fingerprint_cached`: `stamp` → `lookup` → 当たれば返す。外れたらデコード（今の `fingerprint` と同じ）→ `insert` → `unsaved() >= FLUSH_EVERY` なら `save`（失敗は `eprintln!`）。`stamp` が `Ok(None)` なら記録を使わず保存もしない。`stamp` が `Err` ならそのエラーを返す（読めなかった画像）
- `lib.rs`: `app.path().app_cache_dir()?.join("similar-cache.json")`

- [ ] **Step 1: 失敗するテストを書く**

`similar/mod.rs` の tests（「デコードしていない」ことは、記録のあとで中身を壊したファイルが成功することで確かめる。壊すときは `set_modified` で更新日時を戻し、サイズも同じにする）:
```rust
/// 中身だけを壊す（サイズと更新日時は元のまま）
fn corrupt_keeping_stamp(p: &Path) { /* len バイトの 0 で上書きし、元の modified を set_modified で戻す */ }

#[test] fn cached_fingerprint_skips_decoding_unchanged_files() {
    // write_dummy → fingerprint_cached == fingerprint → corrupt_keeping_stamp → fingerprint_cached が Ok で同じ値
}
#[test] fn cached_fingerprint_rereads_files_that_changed() {
    // 記録のあと、同じサイズの別の画像に差し替えて更新日時だけ変える → 新しい値を返し、記録も置き換わる（lookup で新しい値）
}
#[test] fn cached_fingerprint_reports_missing_files() {
    // 存在しないパス → Err、記録は増えない（unsaved() == 0）
}
#[test] fn cached_fingerprint_saves_every_200_new_records() {
    // FLUSH_EVERY 枚の小さなダミー画像 → 最後の 1 枚のあとで保存ファイルができている、unsaved() == 0
}
```
`similar/scan.rs` の tests:
```rust
#[test] fn on_idle_runs_once_when_the_scan_finishes() {
    // with_on_idle(回数を数える) → start(5 件, 3 workers) → wait → 少し待って回数 == 1
    // 結果がすべて記録済みで待ちのない start でも（ワーカーが起きなければ）呼ばれない
}
```
`commands.rs` の tests（Fixture のキャッシュ保存先は一時ディレクトリの `cache/similar-cache.json`。終了してまた起動したことにするため、同じ設定ファイル・同じ保存先で `App` と `AppState` を作り直す `Fixture::relaunch(&mut self)` を足す。作り直す前に `save_similar_cache` 相当の書き出しが済んでいること）:
```rust
#[test] fn scan_results_are_saved_and_reused_after_the_app_restarts() {
    // f.start(); f.scan() → 保存ファイルがある
    // src の 1 枚を corrupt_keeping_stamp → 同じ設定・同じ保存先で AppState を作り直したアプリで start → scan
    // → failed == 0（デコードしていない）
}
#[test] fn scan_finishes_even_when_the_cache_cannot_be_written() {
    // 保存先の親をファイルにしたアプリ → start → scan → running == false、done == total
}
```

- [ ] **Step 2: 実行して失敗を確認** — `cargo test` → コンパイルエラー（`fingerprint_cached`・`with_on_idle`・`AppState::new` の引数）

- [ ] **Step 3: 実装** — 上の Interfaces どおり。`start_similar_scan` のロードを `similar::fingerprint_cached(&registry, &prints, p)` に替える（`prints` は `state.prints.clone()`）。`SimilarScan::new()` は `on_idle` なしのまま残す（既存のテスト用）

- [ ] **Step 4: 実行して通ることを確認** — `cargo test` 全体 PASS、`cargo clippy --all-targets -- -D warnings` 警告なし

- [ ] **Step 5: コミット** — `Feat: 類似判定で保存した結果を使い、変わっていない画像はデコードしない`

---

### Task 3: 移動した画像の記録を引き継ぎ、終了時に書き出す

**Files:**
- Modify: `src-tauri/src/state.rs`（`save_prints`）、`src-tauri/src/commands.rs`（`exit_app`、新コマンド `save_similar_cache`）、`src-tauri/src/lib.rs`（登録）
- Test: `src-tauri/src/commands.rs` の tests

**Interfaces:**
- Consumes: Task 1 の `copy_record`, `save`、Task 2 の `AppState.prints`
- Produces:
```rust
impl AppState {
    /// 移動した画像の記録を移動先へ複製してから書き出す。失敗はログに残すだけ
    pub fn save_prints(&self);
}
#[tauri::command] pub fn save_similar_cache(state: State<'_, AppState>);
```
- `save_prints`: ロックは `session` → `retired` の順（既存の `finalize_all` と同じ）。両方のセッションの各画像で、状態が `Moved { to }` / `Trashed { to, .. }` なら `prints.copy_record(&item.path, to, now)`。ロックを外してから `prints.save(now)`
- `exit_app` は `app.exit(0)` の前に `state.save_prints()` を呼ぶ。`save_similar_cache` は `save_prints()` を呼ぶだけ

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[test] fn moved_images_keep_their_records_at_the_destination() {
    // f.start(); f.scan(); f.act(Move { target: 0 })（1.png → a/1.png）; save_similar_cache(f.state())
    // a/1.png を corrupt_keeping_stamp → a を仕分け元として、同じ保存先で作り直したアプリで start_session("a") → scan → failed == 0
}
#[test] fn undoing_a_move_still_uses_the_original_record() {
    // scan → Move → save_similar_cache → undo → 元の src/1.png を corrupt_keeping_stamp
    // → 作り直したアプリで src を読み込み scan → failed == 0
}
#[test] fn records_are_carried_over_for_images_moved_before_switching_folders() {
    // scan → Move → 別の仕分け元に start_session（retired に移る）→ save_similar_cache → 移動先の記録がある
    // （削除予定を作って retired に残す。retired は削除予定があるときだけ残るので、Delete も 1 回行う）
}
```
（`corrupt_keeping_stamp` は Task 2 で `similar` のテスト用ヘルパーとして `pub(crate)` にしておき、ここでも使う）

- [ ] **Step 2: 実行して失敗を確認** — `cargo test moved_images` など → コンパイルエラー（`save_similar_cache` がない）

- [ ] **Step 3: 実装** — 上の Interfaces どおり。`lib.rs` の `generate_handler!` に `commands::save_similar_cache` を加える

- [ ] **Step 4: 実行して通ることを確認** — `cargo test` 全体 PASS、clippy 警告なし

- [ ] **Step 5: コミット** — `Feat: 振り分けた画像の類似判定の記録を移動先へ引き継ぎ、終了時に保存する`

---

### Task 4: アップデート後の再起動の前に書き出す

**Files:**
- Modify: `src/api.ts`（`saveSimilarCache: () => invoke<void>("save_similar_cache")`）、`src/updater.ts`（`install` の `relaunch()` の直前）
- Test: `src/api.test.ts`（`["saveSimilarCache", [], "save_similar_cache", undefined]` を表に足す）、`src/updater.test.ts`

**Interfaces:**
- Consumes: Task 3 のコマンド `save_similar_cache`

- [ ] **Step 1: 失敗するテストを書く** — `updater.test.ts`: 「再起動の前に類似判定の記録を書き出す」（`relaunch` より前に `save_similar_cache` が呼ばれる。呼び出し順を記録して確かめる）、「書き出しに失敗しても再起動する」（`save_similar_cache` が reject しても `relaunch` が呼ばれる）
- [ ] **Step 2: 実行して失敗を確認** — `npx vitest run src/updater.test.ts src/api.test.ts` → FAIL
- [ ] **Step 3: 実装** — `await api.saveSimilarCache().catch(() => {})` を `relaunch()` の直前に置く
- [ ] **Step 4: 実行して通ることを確認** — `npm run typecheck && npm test` PASS
- [ ] **Step 5: コミット** — `Feat: アップデート後の再起動の前に類似判定の記録を保存する`

---

### Task 5: ドキュメント

**Files:**
- Modify: `site/src/content/docs/guides/usage.md`・`site/src/content/docs/en/guides/usage.md`（「類似画像の確認」/「Similar images」の節の箇条書きに 1 項目）、`docs/handoff-notes.md`（類似画像の項に保存の要点を追記し、残課題の「判定結果の保存（#21）」を外す）、`CLAUDE.md`（ディレクトリ構成の `similar/` の説明に「判定結果の保存」を加える）

- [ ] **Step 1: 書く** — 説明ページに書く内容: 判定結果を保存し、2 回目からは変わっていない画像をすぐ判定できること（アプリを終了しても残る）、Lumiwake で振り分けた画像は移動先でも使えること、保存場所（Linux `~/.cache/io.github.laplusdestiny.lumiwake/similar-cache.json`、Windows `%LOCALAPPDATA%\io.github.laplusdestiny.lumiwake\similar-cache.json`）、消してもよいこと（次回の判定が遅くなるだけ）
- [ ] **Step 2: 確認** — `cd site && npm ci && npm run build` が成功
- [ ] **Step 3: コミット** — `Docs: 類似判定の結果の保存を説明ページに書く`

---

### Task 6: 全体の確認

- [ ] **Step 1:** `cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` が成功
- [ ] **Step 2:** `npm run typecheck && npm test` が成功
- [ ] **Step 3:** 一時ディレクトリにダミー画像（数百枚）を作り、`similar::fingerprint_cached` を 2 回通す小さな計測（テストの中で時間を表示するだけでよい）で、2 回目がデコードなしで速いことを確かめ、数値を報告に書く
