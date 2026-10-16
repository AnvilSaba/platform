# Helm Chart と配置先の管理

platform はアプリ、DB Migrator、コンテナイメージ、Helm Chart を開発・公開します。
クラスタ構成、環境別の values と Config/Secret、Flux、CloudNativePG は
[infrastructure](https://github.com/AnvilSaba/infrastructure) で管理します。

## Chart の確認

```powershell
mise //deploy/helm/platform:check
```

`deploy/helm/platform/` にはテンプレート、共通の既定値、ローカル開発・統合確認用の values を置きます。
本番・staging の設定や配置版は置きません。

Bot、MC Link Server、Public API、DB Migrator の `image.digest` に
`sha256:<64桁の小文字hex>` を指定すると `repository@digest` を描画します。
digest が空なら `image.tag` を使用します。タグと digest の選択は呼び出し側が行います。
DB Migrator は有効な場合にアプリのインストール・更新前の Helm hook として実行します。

公開チャネルとリリース方法は [リリース手順](releases.md) を参照してください。
実環境の構築・配置・Secret の管理は
[infrastructure のデプロイ手順](https://github.com/AnvilSaba/infrastructure/blob/main/docs/deployment.md)を参照してください。
