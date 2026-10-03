# 変更履歴とリリース

Bot、MC Link Server、公開 API、Platform Helm Chart は独立してバージョン管理します。タグ接頭辞はそれぞれ `bot`、`mc-link-server`、`public-api`、`chart` で、形式は `<対象>/vX.Y.Z` です。変更履歴は各リリース対象の `CHANGELOG.md` に生成されます。

`db-migrator` は、アプリのリリースコミットに含まれる DB schema を適用するデプロイ用成果物です。独立したリリース・デプロイ対象、SemVer タグ、CHANGELOG は持ちません。`apps/db-migrator` と OCI image、`platform` chart 内の Kubernetes Job は維持します。Cargo.toml の version は Cargo のパッケージメタデータとしてのみ使用します。

## コミット規約

[Conventional Commits](https://www.conventionalcommits.org/ja/v1.0.0/) に従うこと

## GitHub Actions からリリースする

GitHub の Actions 画面で **Release** を選び、**Run workflow** からリリース対象と bump 種別を指定します。通常は `auto` を使用してください。`dry_run` はデフォルトで有効です。実際にリリースする場合だけ無効にしてください。

Workflow は次を自動実行します。

1. `git-cliff` で次のバージョンを算出する
2. 対象固有の履歴から、バージョンファイルと完全な `CHANGELOG.md` を再生成する
3. `chore(release): <app>/vX.Y.Z` をコミットする
4. 同名の Git tag を作成して `main` へ push する
5. `workflow_call` で公開Workflowを呼び出す。アプリではアプリ image と `db-migrator` image に同じリリースタグを渡してビルドし、ChartではOCI Chartを公開する

`dry_run` を有効にした場合は、次のタグの算出と既存タグとの重複確認に加えて、生成予定の CHANGELOG をログへ表示します。リリース・デプロイは行いません。

`deploy`はデフォルトで無効です。有効にした実リリースだけが、イメージまたはChartの公開成功後に対象別の`production/<対象>` Environmentを使って本番へSSHデプロイします。アプリと Migrator のどちらかのビルドが失敗した場合はデプロイを開始しません。手動デプロイも同じ環境を使い、Deployments の履歴を対象ごとに記録します。

アプリのデプロイは、同じ Git revision の Migrator Job を実行して正常完了を待ち、成功した場合のみ対象アプリを更新します。担当者は **Release でアプリを選び、dry_run=false・deploy=true** を指定するだけで、migration を個別に操作する必要はありません。DB 変更のないリリースでも常に Job を実行し、SQLx が適用済み migration をスキップします。

アプリ image は従来どおり `vX.Y.Z` でデプロイします。Migrator image はチェックアウトしたコミットから生成した `sha-<完全な40桁のGit SHA>` を使い、SemVer タグは付けません。各アプリのビルド Workflow は SHA を入力として受け取る必要がなく、同じリリースタグをチェックアウトすることで revision を揃えます。複数アプリの同じ SemVer が Migrator のタグで衝突することはありません。後日の手動デプロイも、指定したアプリの Git tag から SHA を解決し、対応する Migrator を使います。Migrator の SHA が一致しない指定はデプロイ前に拒否します。

この方式の導入前に公開したリリースには、対応する完全な SHA タグの Migrator が必要です。同じ revision から Migrator を公開するか、新しいアプリリリースを作成してから使用します。初期の DB・ロール・Secret の準備は[独立デプロイ手順](independent-deployment.md#配置順序)を参照してください。

## ローカルでリリース内容を確認する

`git-cliff` をインストールしたうえで、リポジトリルートから実行します。

```powershell
./scripts/prepare-release.ps1 -App bot -Bump auto
./scripts/prepare-release.ps1 -App mc-link-server -Bump auto
./scripts/prepare-release.ps1 -App public-api -Bump auto
./scripts/prepare-release.ps1 -App chart -Bump auto
```

`major`、`minor`、`patch` を明示することもできます。

## 変更履歴だけを生成する

リリース済みの履歴と現在の未リリース変更をまとめて再生成します。各アプリの基準タグの親から `HEAD` までが対象です。

```powershell
./scripts/generate-changelog.ps1 -App bot
./scripts/generate-changelog.ps1 -App mc-link-server
./scripts/generate-changelog.ps1 -App public-api
./scripts/generate-changelog.ps1 -App chart
```

リリース対象ごとのバージョンファイル、変更ログ出力先、対象パス、基準タグは`scripts/release-config.psd1`で一元管理します。
Migrator と migration SQL の変更は、各アプリの履歴と自動 bump の対象に含めます。
ルートの `Cargo.toml`・`Cargo.lock` は変更履歴と自動 bump の対象パスから除外します。共通依存だけの更新をリリースする場合は bump を明示してください。全差分は各バージョン見出しの GitHub リンクから確認できます。
バージョンの解析・更新形式は `VersionFile` のファイル名で判定します。`Cargo.toml` は Cargo.lock の対象パッケージも更新し、`Chart.yaml` は Chart のバージョンを更新します。

## Migration の互換性

通常の migration は、稼働中の旧アプリと更新後の新アプリの両方が動く後方互換な変更にします。列・テーブルの追加は先行適用できますが、旧アプリが使う列の削除など、即座に旧版を壊す変更はアプリ更新前に実行しません。

破壊的変更は expand-contract として、新 schema の追加、アプリの新 schema 対応、全利用箇所の移行完了、後続リリースでの旧 schema 削除の順に進めます。Helm の rollback は DB migration を取り消しません。切り戻し対象も含めた互換性を確認します。
