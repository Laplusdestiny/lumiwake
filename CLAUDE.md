# Lumiwake — Claude Code 向けプロジェクトガイド

キーボード操作で画像をすばやくフォルダへ振り分ける、Windows / Linux 向けの軽量デスクトップアプリ。
claude.ai のプロジェクト「画像振り分けアプリケーション」で要件定義・UI 検討・命名まで済ませ、実装を Claude Code に引き継いだ。
MVP（手動仕分け）は実装済み。実装の経緯・決定事項・残課題は `docs/handoff-notes.md` の「実装フェーズ」を参照。

## 必読ドキュメント

- 要件定義書（仕様の正本）: @docs/requirements.md
- 引き継ぎメモ（経緯・決定事項・残課題）: @docs/handoff-notes.md
- UI モック: `docs/ui-mockups/`（静的 HTML。ブラウザで開いて確認。説明は `docs/ui-mockups/README.md`）
- 参考にした既存アプリの調査: `docs/reference-apps.md`
- AI 振り分け候補機能の引き継ぎ（MVP 後の拡張。経緯・決定理由・未決事項）: @docs/ai-suggestion-handoff.md
- AI 候補機能の UI モック: `docs/ui-mockups/ai/`（静的 HTML。説明は `docs/ui-mockups/ai/README.md`）

仕様で迷ったら要件定義書を優先する。UI モックは雰囲気と配置の参考であり、モック内の文言・対応形式などが要件定義書と食い違う場合は要件定義書が正しい。

## 決定済みの前提

- アプリ名: **Lumiwake**（lumi＝光 ＋ 振り分け）。GitHub の Public リポジトリ、MIT ライセンス（著作権者 Laplusdestiny）
- 技術: Tauri（Rust ＋ WebView）。画像デコードとファイル操作はすべて Rust 側。フロントは表示とキー入力の受け渡しのみ
- Rust コアは 4 モジュール構成: スキャナ / デコーダ / ファイル操作 / 設定管理。移動・削除予定・Undo の整合性はファイル操作モジュールが一手に持つ
- 画像デコード: `image` クレート（AVIF は avif-native ＋ libdav1d）、HEIC は `libheif-rs`
- MVP は手動仕分けのみ。AI 候補提示は MVP 後の拡張（ローカル CLIP と System One 互換 API の切り替え式）。RAW・動画は対象外
- UI: ダークテーマ。サイドバー型（中央プレビュー＋右に振り分け先リスト）をデフォルトにし、表示モード切り替え可能。振り分け先リストは名前だけでなくフルパスも表示（同名フォルダの区別のため）
- UI 文言は日本語（説明ページは日本語／英語）
- 配布: GitHub Releases。Release を手動作成 → GitHub Actions（tauri-action）で Windows(.msi) / Linux(.deb, .AppImage) をビルドして添付。Tauri updater プラグイン＋`latest.json` で自動アップデート
- 説明ページ: Astro Starlight で GitHub Pages（日英）

## 実装時に守ること

- コミットは細かく分ける。1 コミット＝1 つの開発内容とし、何を開発したかが差分でわかるようにする（無関係な変更を混ぜない）
- **ユーザーの画像ファイルを失わせない**のが最優先。特に以下は要件の中核なので、テストを先に書いてから実装する
  - 削除キーはファイルを消さない（削除フォルダへ移動、または未指定時はその場で「削除予定」として記録するだけ）。完全削除はアプリ終了時の確認後のみ
  - 別ドライブ／SMB 間の移動（コピー→元を削除）が途中で失敗しても、欠損や重複を残さない
  - Undo は複数段。履歴はメモリ上のみ（永続化しない）
  - 同名ファイル衝突時は「既存を残す／上書き／両方残す（リネーム）／スキップ」の 4 択
- テストや動作確認で実在の写真フォルダを操作しない。一時ディレクトリにダミー画像を生成して使う
- 設定は GUI と TOML の両方から編集でき、GUI の変更は TOML に書き戻す
- Tauri やクレートのバージョン・API は変化が速いので、使う前に公式ドキュメントで最新を確認する（Tauri 2 系を想定）
- WebView の予約キーとの衝突、軽量性（配布サイズ・起動速度）は実装初期に検証する

## 最初に進める順番（案）

1. Tauri 2 ＋ フロント（軽量なもの）で雛形作成、Windows / Linux 両方でビルドが通ることを確認
2. スキャナ（フォルダ走査・除外・形式判定）と、JPEG/PNG 程度のデコード→表示
3. ファイル操作モジュール（移動・スキップ・削除予定・複数段 Undo・同名衝突）をテスト駆動で
4. キー割り当て（修飾キー対応）と TOML 設定
5. 先読み、HEIC/AVIF、設定画面、終了時の削除確認
6. CI（tauri-action）、署名付き updater、Starlight の説明ページ

## ディレクトリ構成

- `src/` — 画面（素の TypeScript + Vite）。表示とキー入力の受け渡しだけ。Rust 呼び出しは `src/api.ts` に集約
- `src-tauri/src/` — Rust コア
  - `scanner.rs` スキャナ / `decoder/` デコーダ・先読みキャッシュ / `fileops/` ファイル操作 ★ / `config/` 設定管理
  - `commands.rs` 画面から呼ぶ Tauri コマンド、`protocol.rs` 画像を返す `lumi://` プロトコル、`state.rs` 共有状態
- `src-tauri/tests/fixtures/` — テスト用の極小 HEIC / AVIF（ダミー画像。実在の写真は置かない）
- `site/` — 説明ページ（Astro Starlight、日英）。依存は別の package.json
- `.github/workflows/` — `ci.yml`（PR / main）、`release.yml`（Release 公開時）、`pages.yml`（説明ページ）

## コマンド

```sh
npm ci                       # フロントの依存
npm run tauri dev            # 開発起動
npm run typecheck            # フロントの型チェック
npm test                     # フロントのテスト（vitest）
npm run tauri build          # パッケージ作成（Linux で HEIC/AVIF も含めるなら -- --features avif,heic）

cd src-tauri
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test                   # Rust のテスト
cargo test --features avif,heic   # HEIC/AVIF も（libheif-dev・libheif-plugin-libde265・libdav1d-dev が必要）

cd site && npm ci && npm run build   # 説明ページ
```

Linux で Tauri をビルドするには `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev` が必要（HEIC/AVIF を有効にするなら `libheif-dev libheif-plugin-libde265 libdav1d-dev` も。HEIC のデコードには libde265 プラグインが必要）。
リリース手順と updater の署名鍵は `docs/release.md`。
