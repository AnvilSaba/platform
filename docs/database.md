# DB のセットアップとマイグレーション

## 共有 DB

各アプリは PostgreSQL の共有 DB `platform` に接続する。
Helm の `postgres.database` もこの名前を使用する。
アプリごとに異なる DB ユーザーを使い、接続先は必須の環境変数 `DATABASE_URL` に設定する。

```text
postgres://platform_bot:<パスワード>@<ホスト>:5432/platform
```

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

起動時は `public._sqlx_migrations` を読み取り、必須の適用履歴・チェックサム・成功状態を検証する。
Bot ロールには `version`・`success`・`checksum` の読み取り権限だけを付与する。
履歴テーブルの作成や未適用マイグレーションの実行は起動処理で行わない。
履歴の欠落、不一致、失敗記録、アプリが対応していない履歴があれば起動を拒否する。

対応済みマイグレーションのうち、アプリが必須とする番号までの適用を要求する。
必須番号より後の対応済み変更は、未適用・適用済みの両方を許容する。
互換追加の前に、その履歴に対応した旧版・新版を用意し、互換性を確認してから適用する。
非互換変更では互換期間を終え、対応していない旧版への切り戻しを行わない。
適用済みのマイグレーションファイルは変更せず、新しいファイルを追加する。

```powershell
sqlx migrate revert
```

初期マイグレーションの down は `mcguildlink` スキーマと保存データを削除する。
開発用の空 DB での往復検証に使い、データを保持する切り戻しには使わない。
クラスタ共有の `mcguildlink_bot` ロールは down 後も保持する。

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
