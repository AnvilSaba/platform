# 変更履歴とリリース

Bot、MCGuildLink、Platform Helm Chart は独立してバージョン管理します。タグはそれぞれ `bot/vX.Y.Z`、`mcguildlink/vX.Y.Z`、`chart/vX.Y.Z` です。変更履歴は各リリース対象の `CHANGELOG.md` に生成されます。

## コミット規約

次の Conventional Commits を使用してください。

- `fix:` は PATCH（例: `1.2.3` → `1.2.4`）
- `feat:` は MINOR（例: `1.2.3` → `1.3.0`）
- `feat!:`、`fix!:`、または `BREAKING CHANGE:` は MAJOR（例: `1.2.3` → `2.0.0`）
- `docs:`、`refactor:`、`test:`、`chore:` などは変更履歴へ分類されます

リリース対象固有の変更だけが対象です。Bot では `apps/bot`、`crates/bot-macros` と Rust のルートマニフェスト、MCGuildLink では `apps/mcguildlink`、Chart では `deploy/helm/platform` 以下を追跡します。

## GitHub Actions からリリースする

GitHub の Actions 画面で **Release** を選び、**Run workflow** からリリース対象と bump 種別を指定します。通常は `auto` を使用してください。`dry_run` はデフォルトで有効です。実際にリリースする場合だけ無効にしてください。

Workflow は次を自動実行します。

1. `git-cliff` で次のバージョンを算出する
2. 対象固有の履歴から、バージョンファイルと完全な `CHANGELOG.md` を再生成する
3. `chore(release): <app>/vX.Y.Z` をコミットする
4. 同名の Git tag を作成して `main` へ push する
5. `workflow_call` で対象の公開Workflowを呼び出し、アプリではコンテナイメージ、ChartではOCI Chartを公開する

`dry_run` を有効にした場合は、次のタグの算出と既存タグとの重複確認に加えて、生成予定の CHANGELOG をログへ表示します。バージョンファイル・実際の CHANGELOG、commit、tag、push、イメージ・Chart公開、デプロイは行いません。

`deploy`はデフォルトで無効です。有効にした実リリースだけが、イメージまたはChartの公開成功後に`production` Environmentを使って本番へSSHデプロイします。各対象の初回リリースでは公開だけを行い、Chartと両方のイメージが揃ってから[本番デプロイ手順](deployment.md)に従って初期デプロイします。

対象にタグが1件もない初回実行は、既存のバージョン（Bot は `3.5.0`、MCGuildLink は `1.0.0`、Chart は `0.1.0`）をそのまま初回タグとして使用します。2回目以降は未リリースの Conventional Commits に基づいて bump します。Chartの初回CHANGELOGは`deploy/helm/platform`の全履歴から生成します。

Chartを実際にリリースすると、`ghcr.io/anvilsaba/charts/platform:<version>`へOCI形式で公開します。同じリリースの参照先は次の形式です。

```text
oci://ghcr.io/anvilsaba/charts/platform
```

取得・デプロイ時は、意図しないChart更新を避けるため`--version`でバージョンを明示してください。

## ローカルでリリース内容を準備する

`git-cliff` をインストールしたうえで、リポジトリルートから実行します。

```powershell
./scripts/prepare-release.ps1 -App bot -Bump auto
./scripts/prepare-release.ps1 -App mcguildlink -Bump auto
./scripts/prepare-release.ps1 -App chart -Bump auto
```

`major`、`minor`、`patch` を明示することもできます。スクリプトはファイルと変更履歴を更新しますが、コミット・タグ・push は行いません。最後に出力されるタグと差分を確認してからコミットしてください。

## 変更履歴だけを生成する

リリース済みの履歴と現在の未リリース変更をまとめて再生成します。各アプリの基準タグの親から `HEAD` までが対象です。

```powershell
./scripts/generate-changelog.ps1 -App bot
./scripts/generate-changelog.ps1 -App mcguildlink
./scripts/generate-changelog.ps1 -App chart
```

サイトではこのコマンドをビルド時に実行することで、コミット済みの変更ログを待たずに未リリース欄も表示できます。CIで実行する場合は、checkout時に全履歴とタグを取得してください。

BotとMCGuildLinkの変更ログの末尾には、対象外にした過去のコミット履歴へのリンクが`v3.5.0以前`または`v1.0.0以前`として追加されます。Chartは初回から全履歴を変更ログへ含めます。

リリース対象ごとのバージョンファイル、変更ログ出力先、対象パス、基準タグは`scripts/release-config.psd1`で一元管理します。
