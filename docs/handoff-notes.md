# 引き継ぎメモ

claude.ai のプロジェクト「画像振り分けアプリケーション」での検討内容のまとめ（2026-09-28 時点）。

## これまでの流れ

1. **既存アプリの調査** — Vector で公開されている Typict と 月鏡 を調べ、受け継ぐべき設計思想を抽出（`reference-apps.md`）
2. **要件定義** — 対話形式で要件を詰め、要件定義書を作成。コメントでのやり取りを経て、未決事項の大半を解決済み（`requirements.md`）
3. **UI モック** — 4 画面のモックを作成（`ui-mockups/`）
4. **命名** — 候補（Pigeonhole, PicKey, Pixshelf, Utsushi など）を検討し、他アプリとかぶりにくい **Lumiwake** に決定
5. **リポジトリ準備** — README / .gitignore（Rust・Tauri＋Node）/ LICENSE（MIT）を用意。`gh repo create lumiwake --public` で作成する想定

## 検討の途中で変わったこと（古い情報に注意）

- 削除: 当初「常に削除待ちフォルダへ移動」→ **削除フォルダは設定で指定。未指定ならファイルは動かさず、その場で削除予定として記録**。既定の削除フォルダ名・場所は設けない
- 同名ファイル: 当初「既存を残す／上書き」の 2 択 → **リネーム・スキップを加えた 4 択**
- リリース契機: 当初「main への PR マージ時」→ **Release を手動で作成したとき**
- 対応形式: UI モックの設定画面は「JPEG / PNG / WebP / GIF」だが、要件では **BMP / TIFF / AVIF / HEIC も最低限対応**
- 当初は `trash` クレートで OS のごみ箱を使う案もあったが、SMB 上ではごみ箱に入らず完全削除になるため、削除フォルダ／削除予定方式に落ち着いた

## 残っている未決事項

- 拡大表示や複数枚選択を将来の機能に含めるか
- ~~自動アップデートに対応できる Linux の配布形式の確認~~ → 解決（下記「実装フェーズ」）
- ~~Tauri の軽量性、WebView の予約キーの扱い~~ → Linux では確認済み（下記）。Windows（WebView2）は未確認

## 実装フェーズ（Claude Code、2026-09-28）

MVP（手動仕分け）を実装した。詳細は PR と各コミットを参照。

### 実装時に決めたこと

- **フロント**: フレームワークなしの TypeScript + Vite（軽量性優先）。画像は `lumi://` カスタムプロトコルで、Rust がデコード・縮小（最大 2560px、JPEG／透過は PNG）したものを渡す。WebView には任意のパスではなく「セッション番号＋画像番号」だけを渡す
- **キー判定**: `KeyboardEvent.code`（物理キー）から `Ctrl+Shift+Alt+キー` の正規形を作り、Rust 側の正規化と一致させる（Shift で記号に変わる問題を避けるため）。キーリピートでの連続移動はしない
- **「既存を残す」**: 移動しようとした画像を削除予定（削除フォルダ指定時はそこへ移動）にする。「スキップ」との違いを持たせるための解釈。要確認
- **「上書き」**: 既存ファイルはすぐ消さず、削除フォルダへ退避（未指定なら `名前.lumiwake-old.拡張子` にリネーム）して削除予定にする。Undo で元に戻せる
- **終了時の完全削除の対象**: このセッション（起動中）に削除予定にしたファイルだけ。削除フォルダに以前からあるファイルには触れない（誤って写真フォルダを削除フォルダに指定した場合の事故を防ぐため）
- **仕分け元の切り替え**: 前のフォルダの削除予定は終了時の確認まで引き継ぐ（Undo 履歴は切り替えで失われる）
- **HEIC / AVIF**: Cargo の feature（`heic` / `avif`）。Linux 版のリリースは Ubuntu 24.04 で両方有効にしてビルド。Windows 版は未対応（未対応形式として読み込み時にスキップ・件数表示）
- **設定の自動保存**（前回の仕分け元・表示モード）は TOML を読み直して該当項目だけ書き換え、手編集を上書きしない。GUI で保存したときはコメントが消える（toml_edit で保持するのは今後の改善候補）

### 確認したこと

- **updater と Linux の配布形式**: tauri-plugin-updater 2.13 は AppImage・deb・rpm の更新に対応（deb は `pkexec`/`sudo` で `dpkg -i`）
- **WebView の予約キー（Linux / WebKitGTK）**: keydown の `preventDefault` で Ctrl+R などの再読み込みを止められることを確認。設定画面では F5・Ctrl+R・Ctrl+P・Alt+← などを割り当てると警告する。Windows の WebView2 では未確認
- **軽量性**（Linux、`--features avif,heic` のリリースビルド）: 実行ファイル 12 MB、`.deb` 5.1 MiB、`.AppImage` 81 MiB（AppImage は WebKitGTK 一式を同梱するため大きい）。常駐はしない。Windows の `.msi` は未計測

### 残課題

- Windows 版の HEIC / AVIF（vcpkg で libheif・libdav1d を用意するか、要件どおりプラグイン方式で DLL を後から置けるようにするか）
- updater の公開鍵の設定とシークレット登録（`docs/release.md`）
- Windows（WebView2）での実機確認（予約キー・SMB 上の移動・起動速度）
- 実際の SMB 共有での動作確認（自動テストは別ボリュームを模した障害注入で確認済み）
- アニメーション GIF は 1 枚目だけを表示している
- AI 候補（拡張計画）、一括確認画面（モック C）

## 参考メモ

- HEIC・AVIF のライブラリは、軽量性を保てるなら同梱。重くなるならプラグイン扱い（利用者がアプリ設定フォルダ内の `plugins` などに置けば有効化）。未導入形式は読み込み時にスキップして表示
- AI 拡張では、フォルダ名が日本語中心のため、日本語対応の CLIP 系モデルか、フォルダごとの英語説明文の設定が必要。判定は先読み・一括で行い表示を待たせない。端末上で動かすことを優先
- 月鏡のチェックリスト（タグ）型分類と、Typict の「予約→実行」型は、AI の「提案→確認」UI（一括確認画面）と相性がよい

## AI 候補機能フェーズ（2026-10-03 着手）

仕様は `requirements.md` の「拡張計画」、背景は `ai-suggestion-handoff.md`、モックは `ui-mockups/ai/`。

- 着手範囲: 土台（Suggester・設定・SQLite キャッシュ）→ 候補ストリップ UI → systemone → local → 設定画面・キー衝突対策 → CI・説明ページ
- 「該当なし」カード＝Space（スキップ）は開発者が確認済み（2026-10-03）
- 既定の `toggle_paths` は WebView2 の印刷と衝突しうる Ctrl+Shift+P から Ctrl+Shift+O へ変更（CONFIG_VERSION 3。v2 は main 側の仕分け元履歴の移行で使用済み。旧既定のままの設定だけ自動で移行）
- 一括確認画面（`future-batch-review.html`）は実装しない
- Phase 1（土台）の実装で決めたこと:
  - 振り分け先の識別子は、正規化したフルパス（`suggest/choices.rs::target_id`）。フォルダを改名・移動すると別の振り分け先として扱い、キャッシュは引き継がない
  - 診断キャッシュ・履歴は `<アプリのデータフォルダ>/suggest.sqlite`（開けない場合はメモリ上のみで動作）。診断は画像の中身の BLAKE3 ハッシュで引く（SMB 上では初回にファイル全体を読む）
  - `outcomes`（最終的な振り分け先）は、ファイル操作の `SortEvent`（振り分け確定・Undo）を単一ワーカーが記録。削除・スキップ・「既存を残す」は記録しない
  - 外部送信の同意は `suggest/factory.rs` に集約（未同意なら外部バックエンドを作らない）。送信除外のフォルダは選択肢・候補カードのどちらにも出さない
  - 診断の先読みは表示用の先読みとは別の単一ワーカー（外部 API への同時リクエストを増やさない）
- Phase 3（systemone）で確認・決めたこと（2026-10-03、Cloudflare の公式モデルページと schema-input.json / schema-output.json で確認）:
  - リクエスト: `model`・`state`・`questions` ＋ `images`（`{content_type, base64}`）。質問は choice 型 1 問（ID `dest`）。選択肢の ID は `f0`・`f1`…と `none` で、フォルダ名・パスは API に渡さない
  - レスポンス: `answers.dest = { type: "choice", choice, probabilities{選択肢 ID: 確率}, confidence }`、`usage = { input_tokens, output_tokens }`。形が違うときはエラーにし、推測で補わない
  - `model` はエンドポイント末尾（`clef` / `clef-flash`）から判別し、設定の `model` を明示した場合は一致を検証（食い違うと API が 400）。エンドポイントの `{account}` が未置換なら送らない。https 以外には送らない（ローカルの自前ホスト確認用に localhost の http のみ許可）
  - HTTP クライアントは `ureq`（rustls）。テストは Transport を差し替えたモックと、ローカル TCP サーバーのみで、外部 API は叩かない。**実 API での確認は未実施（開発者の API キーで手動確認が必要）**
  - 接続テストは合成した 8×8 の白画像と仮の選択肢だけを送る
  - 未実装（Phase 5）: 同意ダイアログ、設定画面の System One 項目（コマンドは `set_external_consent` / `ai_key_status` / `test_systemone` を用意済み）
- Phase 2・5 の進捗（2026-10-03）:
  - 仕分け画面に候補ストリップ（local は「一致度」、systemone は確率％＋「該当なし」＝Space）、ヘッダーの AI バッジ（外部送信時は橙）、未評価フォルダの再診断帯（失敗時のみ再診断ボタン）、キー割り当て一覧の強調を実装。静的モックと同じ構成であることを、Tauri をモックした確認ページ（Chromium のスクリーンショット）で確認した。**実際の Tauri アプリ（`npm run tauri dev`）での目視確認は未実施**
  - 設定画面に「AI 候補」を追加。System One を選ぶと同意ダイアログを出し、同意は下書きに入れて「保存」で永続化
  - `tauri-plugin-prevent-default` を導入（Tab / Shift+Tab は設定画面の操作のため無効化しない）。**Windows 側（`platform-windows` の設定）は未ビルド・未確認**
  - 未実装: 設定画面の的中率・診断履歴の表示、local バックエンド（Phase 4）、CI への反映と説明ページ（Phase 6）
- Phase 4（local）で決まったこと: ONNX Runtime とモデルは**初回に自動ダウンロード**（開発者の選択）。第一候補は日本語対応モデル（rinna/japanese-clip-vit-b-16 など。ONNX 化の方法と配布元は未調査）。取得元・ライセンス・ハッシュ検証・失敗時の扱いを決めてから実装する
