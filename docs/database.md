# DB のセットアップとマイグレーション

## 共有 DB

各アプリは PostgreSQL の共有 DB `platform` に接続する。
Helm の `postgres.database` もこの名前を使用する。
アプリごとに異なる DB ユーザーを使い、接続先は必須の環境変数 `DATABASE_URL` に設定する。

```text
postgres://platform_bot:<パスワード>@<ホスト>:5432/platform
```

`DATABASE_PASSWORD` を設定した場合は URL 内のパスワードより優先する。
Helm 配備では `DATABASE_URL` にパスワードを含めず、Bot 専用の Secret から `DATABASE_PASSWORD` を渡す。

設定の不足・不正、DB 接続失敗、スキーマ非互換では Bot は起動しない。
sqlx-cli と統合テストも `DATABASE_URL` を使用する。

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
CREATE ROLE platform_bot_runtime NOLOGIN;
CREATE ROLE platform_mcguildlink_runtime NOLOGIN;
CREATE ROLE platform_public_api_runtime NOLOGIN;
```

マイグレーション適用後、アプリ別のログインユーザーを作成し、それぞれの権限ロールを付与する。

```sql
CREATE ROLE platform_bot LOGIN PASSWORD '<Bot 専用パスワード>';
GRANT CONNECT ON DATABASE platform TO platform_bot;
GRANT platform_bot_runtime TO platform_bot;
CREATE ROLE platform_mcguildlink LOGIN PASSWORD '<MCGuildLink 専用パスワード>';
GRANT CONNECT ON DATABASE platform TO platform_mcguildlink;
GRANT platform_mcguildlink_runtime TO platform_mcguildlink;
CREATE ROLE platform_public_api LOGIN PASSWORD '<公開 API 専用パスワード>';
GRANT CONNECT ON DATABASE platform TO platform_public_api;
GRANT platform_public_api_runtime TO platform_public_api;
```

Bot 用 Secret（`bot.databaseSecretName`、既定 `bot-database`）の `password` には
`platform_bot` のパスワードを設定する。Helm は `bot.databaseUsername`、
`postgres.serviceName`、`postgres.port`、`postgres.database` から `DATABASE_URL` を作る。

アプリには DB 所有権、スキーマ作成権限、マイグレーションロールを与えない。
`platform_bot_runtime` は Bot 全体の権限ロール、`platform_mcguildlink_runtime` は Minecraft 接続サービスの権限ロール、`platform_public_api_runtime` はホワイトリスト公開専用の読み取りロールとする。
ログインユーザーと分離し、各機能のマイグレーションで必要なテーブル・列の権限だけを追加する。
将来のテーブルへ一括で権限を付与するデフォルト権限は設定しない。

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

起動時は `public._sqlx_migrations` を読み取り、必須の適用履歴・チェックサム・成功状態を検証する。
Bot ロールには `version`・`success`・`checksum` の読み取り権限だけを付与する。
履歴テーブルの作成や未適用マイグレーションの実行は起動処理で行わない。
各アプリは利用するスキーマに必要なマイグレーション ID を明示的に列挙する。
必須 ID がアプリに埋め込まれた定義にない場合、DB に未適用の場合、適用失敗または
チェックサム不一致の場合は起動を拒否する。期待チェックサムには `sqlx::migrate!()` の定義を使う。
必須でない履歴は、未知・未適用・失敗・チェックサム不一致のいずれも、それだけでは起動を拒否しない。
必要な推移的依存関係は必須リストに列挙し、レビューで確認する。
DB 全体の履歴の整合性と適用は専用マイグレーション Job が管理する。
未知の変更が互換的かどうかを起動時に自動判定するものではない。
破壊的変更は影響するアプリを先に対応させ、旧版停止後に適用する。
適用済みのマイグレーションファイルは変更せず、新しいファイルを追加する。

```powershell
sqlx migrate revert
```

初期マイグレーションの down は `mcguildlink` スキーマと保存データを削除する。
開発用の空 DB での往復検証に使い、データを保持する切り戻しには使わない。
クラスタ共有の `platform_bot_runtime` ロールは down 後も保持する。

## SQL のコンパイル時検証

SQL は `sqlx::query!` と `sqlx::query_scalar!` でコンパイル時に検証する。
リポジトリの `.sqlx/` に保存したメタデータを使うため、通常のビルドは DB 接続を必要としない。
`.cargo/config.toml` は `SQLX_OFFLINE=true` を設定する。

SQL またはスキーマを変更した場合は、開発用 DB にマイグレーションを適用し、
その接続先を `DATABASE_URL` に指定してメタデータを更新する。

```powershell
sqlx migrate run
cargo sqlx prepare --workspace -- --all-targets --locked
cargo sqlx prepare --check --workspace -- --all-targets --locked
```

`prepare` は DB に接続して検証する。更新された `.sqlx/` もコミットする。
CI はマイグレーション適用後の DB とメタデータの一致を確認する。
