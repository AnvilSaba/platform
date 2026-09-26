# Bot の紐付けコード発行

Issue #56 の機能は `apps/bot/src/features/mcguildlink/` に配置する。
`LinkCodes` がコード発行の Port、`LinkReply` が ephemeral 応答の Port であり、
PostgreSQL と Discord のアダプターを差し替えられる。DB テストは実 PostgreSQL、
応答テストはテスト用応答先を使い、Discord への接続を必要としない。

## マイグレーションと権限

sqlx-cli 0.8.6 を使用する。新規ファイルは `sqlx migrate add -r <名前>` で生成する。
ルートの `migrations/` にある up/down は、Bot 起動処理とは別に実行する。
運用時の実行主体は専用マイグレーション Job とし、Bot の認証情報では実行しない。
Job のデプロイ構成は後続のデプロイ Issue で扱う。

```powershell
cargo install sqlx-cli --version 0.8.6 --locked --no-default-features --features postgres,rustls
# DATABASE_URL はマイグレーション専用ユーザーの接続先
sqlx migrate run
sqlx migrate info
```

マイグレーションユーザーには対象 DB のスキーマ作成権限と、初回の権限ロール作成のための
`CREATEROLE` が必要。運用管理者が `mcguildlink_bot` NOLOGIN ロールを事前作成する場合は
ロール作成権限は不要である。
Bot のログインユーザーは運用側で別途作成し、`GRANT mcguildlink_bot TO <Botユーザー>` を実行する。
Bot に DB 所有権、スキーマ作成権限、マイグレーションロールは与えない。
発行に必要なテーブルと列だけにアクセスでき、Minecraft アカウント・紐付け・ブロックの変更はできない。

`sqlx migrate revert` は紐付け・未使用コード・ブロック情報を含む `mcguildlink` スキーマを削除する。
開発用の空 DB での往復検証に使う。データを保持した切り戻しには使わない。
クラスタ全体で共有される `mcguildlink_bot` ロールは down 後も保持する。

## Bot の設定と操作

`config.sample.toml` の `[mcguildlink]` を設定し、環境変数 `MCGUILDLINK_DATABASE_URL` に
Bot ユーザーの PostgreSQL 接続先を指定して再起動する。設定がなければ DB 接続を行わない。
Bot はスキーマ互換範囲にバージョン 1 が含まれることを起動時に確認する。
未適用・非互換・接続失敗では Discord 接続前に起動を失敗させる。Bot は DDL を実行しない。
この機能の有効・無効の変更には再起動が必要。

対象ギルドで指定した moderator ロールを持つ利用者が `/create_panel` を実行する。
開始ボタンは旧版と同じ `start_link_button` を使用する。旧パネルの開始ボタンも処理できる。
一覧・解除などの操作は後続 Issue の対象。新規パネルには開始ボタンのみ表示する。
対象外ギルドの操作は処理しない。

発行済みの未使用コードを再表示する。期限はなく、名前が変わっても同じコードを返す。
同一 Discord アカウントの同時要求は行ロックで直列化する。
コードの衝突は一意制約で拒否し、最大 16 回まで再生成する。
ブロック済みの利用者にはコードを返さない。

## 検証

テスト用 PostgreSQL 18 を起動し、`DATABASE_URL` にテスト専用の管理ユーザー接続先を設定する。
sqlx テストはテストごとの DB を作成するため、`CREATEDB` と初回の `CREATEROLE` が必要。
Bot 権限のテストでは `SET ROLE mcguildlink_bot` により実際のアクセス権で発行する。

```powershell
cargo check --workspace --locked --all-targets
cargo test -p bot --locked mcguildlink
cargo test --workspace --locked
```

CI も PostgreSQL を起動して実行する。実 Discord での手動確認は、開発用 Bot を対象ギルドに接続し、
moderator のパネル設置、通常利用者の開始・再表示、ブロック済み利用者の拒否、
コードと接続案内が本人だけに見えることを確認する。
