# 変更履歴とリリース

Bot、MC Link Server、公開 API、DB マイグレーション、Platform Helm Chart は独立してバージョン管理します。タグ接頭辞はそれぞれ `bot`、`mc-link-server`、`public-api`、`db-migrator`、`chart` で、形式は `<対象>/vX.Y.Z` です。変更履歴は各リリース対象の `CHANGELOG.md` に生成されます。

## コミット規約

[Conventional Commits](https://www.conventionalcommits.org/ja/v1.0.0/) に従うこと

## GitHub Actions からリリースする

GitHub の Actions 画面で **Release** を選び、**Run workflow** からリリース対象と bump 種別を指定します。通常は `auto` を使用してください。`dry_run` はデフォルトで有効です。実際にリリースする場合だけ無効にしてください。

Workflow は次を自動実行します。

1. `git-cliff` で次のバージョンを算出する
2. 対象固有の履歴から、バージョンファイルと完全な `CHANGELOG.md` を再生成する
3. `chore(release): <app>/vX.Y.Z` をコミットする
4. 同名の Git tag を作成して `main` へ push する
5. `workflow_call` で対象の公開Workflowを呼び出し、アプリではコンテナイメージ、ChartではOCI Chartを公開する

`dry_run` を有効にした場合は、次のタグの算出と既存タグとの重複確認に加えて、生成予定の CHANGELOG をログへ表示します。リリース・デプロイは行いません。

`deploy`はデフォルトで無効です。有効にした実リリースだけが、イメージまたはChartの公開成功後に対象別の`production/<対象>` Environmentを使って本番へSSHデプロイします。手動デプロイも同じ環境を使い、Deployments の履歴を対象ごとに記録します。

## ローカルでリリース内容を確認する

`git-cliff` をインストールしたうえで、リポジトリルートから実行します。

```powershell
./scripts/prepare-release.ps1 -App bot -Bump auto
./scripts/prepare-release.ps1 -App mc-link-server -Bump auto
./scripts/prepare-release.ps1 -App public-api -Bump auto
./scripts/prepare-release.ps1 -App db-migrator -Bump auto
./scripts/prepare-release.ps1 -App chart -Bump auto
```

`major`、`minor`、`patch` を明示することもできます。

## 変更履歴だけを生成する

リリース済みの履歴と現在の未リリース変更をまとめて再生成します。各アプリの基準タグの親から `HEAD` までが対象です。

```powershell
./scripts/generate-changelog.ps1 -App bot
./scripts/generate-changelog.ps1 -App mc-link-server
./scripts/generate-changelog.ps1 -App public-api
./scripts/generate-changelog.ps1 -App db-migrator
./scripts/generate-changelog.ps1 -App chart
```

リリース対象ごとのバージョンファイル、変更ログ出力先、対象パス、基準タグは`scripts/release-config.psd1`で一元管理します。
ルートの `Cargo.toml`・`Cargo.lock` は変更履歴と自動 bump の対象パスから除外します。共通依存だけの更新をリリースする場合は bump を明示してください。全差分は各バージョン見出しの GitHub リンクから確認できます。
バージョンの解析・更新形式は `VersionFile` のファイル名で判定します。`Cargo.toml` は Cargo.lock の対象パッケージも更新し、`Chart.yaml` は Chart のバージョンを更新します。
