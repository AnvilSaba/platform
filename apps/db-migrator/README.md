# DB Migrator

専用 Kubernetes Job から PostgreSQL のマイグレーションを適用するアプリ。
各アプリのリリースと同じ Git revision から生成するデプロイ用成果物で、独立したリリース・デプロイ対象ではない。OCI image は `sha-<完全なGit SHA>` タグを使い、独自の SemVer タグ・CHANGELOG は生成しない。
`sqlx::migrate!()` で migrations/ の SQL をバイナリに埋め込む。
Bot、MC Link Server、公開 API はこのアプリを実行せず、起動時に適用履歴を検証する。

```powershell
# DATABASE_URL と DATABASE_PASSWORD は専用マイグレーションロールの値を使用する。
cargo run --locked -p db-migrator
```

通常のアプリデプロイでは Job の実行・正常完了待機を自動で行う。失敗した場合はアプリを更新しない。DB 変更がなく、全 migration が適用済みでも正常終了する。

配置・ロール・初回セットアップは [本番デプロイ手順](../../docs/deployment.md#initial-setup)、リリースと互換性の方針は [リリース手順](../../docs/releases.md) を参照する。
