# リリース手順

Release を GitHub 上で手動作成（公開）すると、`.github/workflows/release.yml` が Windows・Linux 向けのパッケージをビルドして、その Release に添付する。自動アップデート用の署名付きファイルと `latest.json` も同じ Release に添付される。

## 初回だけ行う準備: updater の署名鍵

自動アップデートの更新ファイルには署名が必須。**鍵は一度決めたら変えない**（変えると配布済みのバージョンが更新できなくなる）。

1. 鍵を作る（パスワードを聞かれる）

   ```sh
   npx tauri signer generate -w ~/.tauri/lumiwake.key
   ```

   `~/.tauri/lumiwake.key`（秘密鍵）と `~/.tauri/lumiwake.key.pub`（公開鍵）ができる。秘密鍵はリポジトリに入れず、安全な場所に保管する。

2. 公開鍵をアプリに埋め込む

   `src-tauri/tauri.conf.json` の `plugins.updater.pubkey` の `REPLACE_WITH_UPDATER_PUBLIC_KEY` を、`lumiwake.key.pub` の中身（1 行の文字列）に置き換えてコミットする。

3. 秘密鍵を GitHub のシークレットに登録する

   リポジトリの Settings → Secrets and variables → Actions で次を登録する。

   | 名前 | 値 |
   | --- | --- |
   | `TAURI_SIGNING_PRIVATE_KEY` | `lumiwake.key` の中身 |
   | `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 鍵を作るときに設定したパスワード |

   シークレットが未設定のままリリースすると、ワークフローは最初に失敗する。

## リリースのたびに行うこと

1. バージョンを上げる（3 か所をそろえる）
   - `package.json` の `version`
   - `src-tauri/Cargo.toml` の `version`
   - `src-tauri/tauri.conf.json` の `version`
2. main にマージする（main へのマージだけではリリースは作られない）
3. GitHub で Release を作成する
   - タグは `v` ＋ バージョン（例: `v0.2.0`）。`tauri.conf.json` の version と一致しないとワークフローが失敗する
   - 「Publish release」で公開すると、ビルドが始まる
4. Actions の完了後、Release に次が添付されていることを確認する
   - Windows: `.msi`（と署名 `.sig`）
   - Linux: `.deb`、`.AppImage`（と署名 `.sig`）
   - `latest.json`

## 配布物について

| OS | 形式 | 自動アップデート | HEIC / AVIF |
| --- | --- | --- | --- |
| Windows | `.msi` | 対応 | 未対応（下記） |
| Linux | `.AppImage` | 対応（AppImage を置き換える） | 対応 |
| Linux | `.deb` | 対応（`dpkg -i` で更新。管理者権限が必要） | 対応（`libheif1`・`libdav1d7` に依存） |

- Linux 版は HEIC / AVIF 用のライブラリ（libheif ≥ 1.17、libdav1d）を標準パッケージで用意できる Ubuntu 24.04 でビルドしている。そのため、glibc が古いディストリビューション（Ubuntu 22.04 以前など）では動かない場合がある。
- Windows 版で HEIC / AVIF に対応するには、vcpkg で libheif・libdav1d を用意してビルドする必要がある（未対応。`docs/handoff-notes.md` の残課題を参照）。未対応の形式は読み込み時にスキップされ、その件数が表示される。
- updater プラグイン（2.13 時点）は Linux の AppImage・deb・rpm の更新に対応している。deb の更新は `pkexec` / `sudo` 経由で `dpkg -i` を実行する。

## ローカルでのビルド

```sh
npm ci
npm run tauri build                      # 既定の形式のみ
npm run tauri build -- --features avif,heic  # Linux で HEIC/AVIF も含める
```

ローカルビルドでは更新ファイル（署名）は作らない（`createUpdaterArtifacts` はリリース時だけ `src-tauri/tauri.release.conf.json` で有効にしている）。
