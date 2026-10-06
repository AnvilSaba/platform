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

`mise install` で管理対象の `git-cliff` を導入し、リポジトリルートから実行します。

```powershell
mise exec -- pwsh -File ./scripts/prepare-release.ps1 -App bot -Bump auto
mise exec -- pwsh -File ./scripts/prepare-release.ps1 -App mc-link-server -Bump auto
mise exec -- pwsh -File ./scripts/prepare-release.ps1 -App public-api -Bump auto
mise exec -- pwsh -File ./scripts/prepare-release.ps1 -App chart -Bump auto
```

`major`、`minor`、`patch` を明示することもできます。

## 変更履歴だけを生成する

リリース済みの履歴と現在の未リリース変更をまとめて再生成します。各アプリの基準タグの親から `HEAD` までが対象です。

```powershell
mise exec -- pwsh -File ./scripts/generate-changelog.ps1 -App bot
mise exec -- pwsh -File ./scripts/generate-changelog.ps1 -App mc-link-server
mise exec -- pwsh -File ./scripts/generate-changelog.ps1 -App public-api
mise exec -- pwsh -File ./scripts/generate-changelog.ps1 -App chart
```

リリース対象ごとのバージョンファイル、変更ログ出力先、対象パス、基準タグは `scripts/release-config.psd1` で一元管理します。
Migrator とマイグレーション SQL の変更は、各アプリの履歴と自動 bump の対象に含めます。
ルートの `Cargo.toml`・`Cargo.lock` は変更履歴と自動 bump の対象パスから除外します。共通依存だけの更新をリリースする場合は bump を明示してください。全差分は各バージョン見出しの GitHub リンクから確認できます。
バージョンの解析・更新形式は `VersionFile` のファイル名で判定します。`Cargo.toml` は `Cargo.lock` の対象パッケージも更新し、`Chart.yaml` は Chart のバージョンを更新します。

## マイグレーションの互換性

通常のマイグレーションは、稼働中の旧アプリと更新後の新アプリの両方が動く後方互換な変更にします。列・テーブルの追加は先行適用できますが、旧アプリが使う列の削除など、即座に旧版を壊す変更はアプリ更新前に実行しません。

破壊的変更は expand-contract として、新スキーマの追加、アプリの新スキーマ対応、全利用箇所の移行完了、後続リリースでの旧スキーマ削除の順に進めます。Helm のロールバックは DB マイグレーションを取り消しません。切り戻し対象も含めた互換性を確認します。
