# DB Migrator

専用 Kubernetes Job から PostgreSQL のマイグレーションを適用するアプリ。
`sqlx::migrate!()` で migrations/ の SQL をバイナリに埋め込む。
Bot、MC Link Server、公開 API はこのアプリを実行せず、起動時に適用履歴を検証する。

```powershell
# DATABASE_URL と DATABASE_PASSWORD は専用マイグレーションロールの値を使用する。
cargo run --locked -p db-migrator
```

配置・ロール・適用順序は [独立リリース手順](../../docs/independent-deployment.md) を参照する。
