# 変更履歴とリリース

Bot、MC Link Server、公開 API、Platform Helm Chart は独立してバージョン管理します。タグ接頭辞はそれぞれ `bot`、`mc-link-server`、`public-api`、`chart` で、形式は `<対象>/vX.Y.Z` です。変更履歴は各リリース対象の `CHANGELOG.md` に生成されます。

`db-migrator` は、アプリのリリースコミットに含まれる DB スキーマを適用するデプロイ用成果物です。

## コミット規約

[Conventional Commits](https://www.conventionalcommits.org/ja/v1.0.0/) に従います。

## GitHub Actions からリリースする

GitHub の Actions 画面で **Release** を選び、**Run workflow** からリリース対象と bump 種別を指定します。通常は `auto` を使用してください。`dry_run` はデフォルトで有効です。実際にリリースする場合だけ無効にしてください。

ワークフローは次を自動実行します。

1. `git-cliff` で次のバージョンを算出する
2. 対象固有の履歴から、バージョンファイルと完全な `CHANGELOG.md` を再生成する
3. `chore(release): <app>/vX.Y.Z` をコミットする
4. 同名の Git タグを作成して `main` へプッシュする
5. アプリではアプリイメージと `db-migrator` イメージに同じリリースタグを渡してビルドし、Chart では OCI Chart を公開する

`dry_run` を有効にした場合は、次のタグの算出と既存タグとの重複確認に加えて、生成予定の CHANGELOG をログへ表示します。リリース・デプロイは行いません。

`deploy` はデフォルトで無効です。有効にした実リリースだけが、イメージまたは Chart の公開成功後に本番へデプロイします。

アプリのデプロイは、同じ Git リビジョンの Migrator Job を実行して正常完了を待ち、成功した場合のみ対象アプリを更新します。担当者は **Release でアプリを選び、dry_run=false・deploy=true** を指定するだけで、マイグレーションを個別に操作する必要はありません。DB 変更のないリリースでも常に Job を実行し、SQLx が適用済みマイグレーションをスキップします。

## ローカルでリリース内容を確認する

`mise install` で管理対象の Rust と `git-cliff` を導入し、リポジトリルートから実行します。
スクリプトは [Cargo の単一ファイルパッケージ機能](https://doc.rust-lang.org/cargo/reference/unstable.html#script)を使います。リポジトリで固定した nightly を使用するため、別の `cargo-script` ツールの導入は不要です。

```powershell
mise :release prepare --app bot --bump auto --dry-run
mise :release prepare --app mc-link-server --bump auto --dry-run
mise :release prepare --app public-api --bump auto --dry-run
mise :release prepare --app chart --bump auto --dry-run
```

`major`、`minor`、`patch` を明示することもできます。実際にバージョンファイルと CHANGELOG を更新する場合は `--dry-run` を外します。ローカルのスクリプトはコミット・タグ作成・プッシュを行いません。

`mise :release prepare --help` で引数を確認できます。task は引数をそのまま Rust スクリプトへ渡すため、対象名や引数の定義を mise 側に重複させません。

## shebang と Windows での起動

`scripts/release.rs` に `#!/usr/bin/env -S mise exec -- cargo -Zscript` を指定しています。Unix 系でファイルを直接実行する場合は `chmod +x scripts/release.rs` で実行権限を付けると、shebang 経由でも mise 管理のツールを使用できます。

[Windows 自体は shebang を解釈しません](https://mise.jdx.dev/tasks/file-tasks.html#windows)。Windows でも同じ `mise :release ...` を使用します。task の `file` は Rust スクリプトを指し、`shell = "cargo -Zscript"` で共通の `task_config.shell` を上書きして Cargo を起動します。この方法なら `.rs` のファイル関連付けや Windows 用のラッパーは不要です。

## 変更履歴だけを生成する

リリース済みの履歴と現在の未リリース変更をまとめて再生成します。`full_history = true` の対象は全履歴、それ以外は基準タグより後から `HEAD` までが対象です。

```powershell
mise :release changelog --app bot
mise :release changelog --app mc-link-server
mise :release changelog --app public-api
mise :release changelog --app chart
```

`--tag <対象>/vX.Y.Z` でリリースタグを指定でき、`--output <パス>` で出力先を変更できます。相対パスはリポジトリルートから解決します。

`--output -` はファイルを作成せず、変更履歴の本文だけを標準出力へ出します。`-` を標準入出力として扱うのは CLI の慣習で、このスクリプトが明示的に解釈します。省略時は設定の CHANGELOG に書き込みます。mise の task 接頭辞を本文に混ぜないよう、標準出力を使う場合は `--output interleave` を指定します。

```powershell
mise run --output interleave release changelog --app bot --output -
mise run --output interleave release changelog --app bot --output - > changelog-preview.md
```

前者の `--output interleave` は mise の表示設定、後者の `--output -` はスクリプトの出力先です。`>` によるファイルへのリダイレクトはシェルが行います。

GitHub Actions で `prepare` の標準出力からタグを取得する場合は、`mise run --output interleave release prepare ...` を使います。task 名の接頭辞を標準出力に付けず、タグだけを取得できます。

## スクリプトの実装方針

PowerShell 版の方針を引き継ぎ、リリース対象ごとのバージョンファイル、変更ログ出力先、対象パス、基準タグ、表示名、全履歴を含めるかどうかは `scripts/release-config.toml` で一元管理します。対象名は TOML のテーブル名から取得し、Rust の列挙型や条件分岐にプロジェクト名をハードコードしません。対象追加は設定に記述します（公開・デプロイ先を追加する場合は対応するワークフローも変更します）。

設定は serde で型付きで読み込み、未知のフィールドはエラーにします。引数解析は既存アプリと同じ bpaf を使います。設定は実行時に読むため、設定だけの変更でスクリプトを再コンパイルする必要はありません。

Migrator とマイグレーション SQL の変更は、各アプリの履歴と自動 bump の対象に含めます。
ルートの `Cargo.toml`・`Cargo.lock` は変更履歴と自動 bump の対象パスから除外します。共通依存だけの更新をリリースする場合は bump を明示してください。全差分は各バージョン見出しの GitHub リンクから確認できます。
バージョンの解析・更新形式は `version_file` のファイル名で判定します。`Cargo.toml` は toml_edit でコメントや既存の配置を保持し、`Cargo.lock` の対象ローカルパッケージも更新します。パッケージ名はマニフェストから取得し、同名のレジストリ依存は更新しません。`Chart.yaml` は Chart の `version` のみを更新し、`appVersion` は保持します。

`full_history = false` の対象は `baseline_tag` が必須で、基準タグより前の変更は旧コミット履歴へのリンクで案内します。リンク先のリポジトリ名は `cliff.toml` の `remote.github` から取得します。

初回リリースはバージョンファイルの値を使い、明示的な bump を拒否します。既存タグの重複や外部コマンドの失敗はエラーにします。CHANGELOG の生成とバージョンの検証を済ませてからファイルを更新します。dry-run はプレビューを標準エラーと GitHub Actions のジョブ概要に表示し、バージョンファイル・ロックファイル・CHANGELOG を変更しません。`prepare` の標準出力は次のタグだけなので、ワークフローで直接取得できます。

スクリプトはワークスペース外の単一ファイルパッケージなので、`mise :fmt`・`mise :fmt:check` に個別の rustfmt を含め、`mise :release:check` で Clippy とテストを実行します。Rust CI でもこの検証を実行します。

## マイグレーションの互換性

通常のマイグレーションは、稼働中の旧アプリと更新後の新アプリの両方が動く後方互換な変更にします。列・テーブルの追加は先行適用できますが、旧アプリが使う列の削除など、即座に旧版を壊す変更はアプリ更新前に実行しません。

破壊的変更は expand-contract として、新スキーマの追加、アプリの新スキーマ対応、全利用箇所の移行完了、後続リリースでの旧スキーマ削除の順に進めます。Helm のロールバックは DB マイグレーションを取り消しません。切り戻し対象も含めた互換性を確認します。
