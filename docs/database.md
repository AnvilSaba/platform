# DB のセットアップとマイグレーション

## 共有 DB

各アプリは PostgreSQL の共有 DB `platform` に接続する。
Helm の `postgres.database` もこの名前を使用する。
接続先は環境変数 `DATABASE_URL` で指定し、アプリごとに異なる DB ユーザーを使う。

```text
postgres://<ユーザー>:<パスワード>@<ホスト>:5432/platform
```

DB を手動で作成する場合は、管理者として以下を実行する。

```sql
CREATE ROLE platform_migrator LOGIN;
CREATE DATABASE platform OWNER platform_migrator;
```

ログインユーザーのパスワードは運用環境の Secret 管理を通じて設定する。
マイグレーションユーザーには対象 DB のスキーマ作成権限が必要。
初回に権限ロールを作る場合は `CREATEROLE` も必要だが、管理者が次のロールを
事前に作成すれば、マイグレーションユーザーにロール作成権限を与える必要はない。

```sql
CREATE ROLE mcguildlink_bot NOLOGIN;
```

マイグレーション適用後、Bot のログインユーザーを作成し、必要な権限ロールを付与する。

```sql
CREATE ROLE platform_bot LOGIN;
GRANT CONNECT ON DATABASE platform TO platform_bot;
GRANT mcguildlink_bot TO platform_bot;
```

Bot に DB 所有権、スキーマ作成権限、マイグレーションロールは与えない。
`mcguildlink_bot` の権限はマイグレーションで定義し、必要なテーブルと列に限定する。

## マイグレーション

sqlx-cli 0.8.6 を使用する。リポジトリルートで実行する。

```powershell
cargo install sqlx-cli --version 0.8.6 --locked --no-default-features --features postgres,rustls
sqlx migrate add -r <名前>
```

`migrations/` に up/down のペアが生成される。
適用時は `DATABASE_URL` をマイグレーション専用ユーザーの接続先に設定する。

```powershell
sqlx migrate run
sqlx migrate info
```

運用時は専用マイグレーション Job が実行する。アプリ自身はスキーマ変更を行わない。
Job のデプロイ構成は後続のデプロイ Issue で扱う。

```powershell
sqlx migrate revert
```

初期マイグレーションの down は `mcguildlink` スキーマと保存データを削除する。
開発用の空 DB での往復検証に使い、データを保持する切り戻しには使わない。
クラスタ共有の `mcguildlink_bot` ロールは down 後も保持する。
