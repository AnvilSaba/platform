# 旧 Kotlin 版から分離構成への移行

この文書は、旧 Kotlin 版・SQLite・Chart 0.x から、MC Link Server・Bot・公開 API・PostgreSQL の分離構成へ移行するための手順です。通常のリリース・配置は[リリース手順](releases.md)と[独立デプロイ手順](independent-deployment.md)を参照してください。

リハーサル結果を踏まえた本番作業の順序は[本番移行手順](mcguildlink-production-cutover.md)にまとめています。切り戻しを選ぶ場合の操作は別文書の[本番切り戻し手順](mcguildlink-production-rollback.md)を参照してください。

移行用スクリプトは移行と切り戻し確認が終わるまで保持し、旧 `apps/mcguildlink` の削除時にまとめて削除します。[k3dリハーサル手順](mcguildlink-k3d-rehearsal.md)に具体的なコマンドと確認項目を記載しています。

## リリースの準備

Bot・mc-link-server・public-api を `deploy=false` で先行リリースし、公開済みイメージのタグを用意します。各アプリのリリースで同じ revision の `db-migrator` image も自動公開されるため、Migrator の独立リリースは不要です。初期セットアップ用の Migrator は Bot のリリースコミットに対応する `sha-<完全なGit SHA>` タグを使用します。分離構成の Chart の初回公開は Release Action で **chart / bump=major / deploy=false** を手動指定します。旧構成の値だけとは互換性がないため、初回のメジャー更新を自動の bump 判定に任せません。

旧 Kotlin 版はリリース・CHANGELOG 生成の対象から外れています。既存の `mcguildlink/v*` と CHANGELOG は過去の履歴として保持します。移行検証には `apps/mcguildlink` のソース・Dockerfile、公開済みイメージと旧 Chart を使用します。分離構成の Chart は旧版の Deployment・Service・PVC・設定を含みません。

## 旧版停止とデータ保護

この手順は開発・統合環境でリハーサルし、本番では別作業として実行します。まず旧 Kotlin Deployment と既存 Bot を replicas=0 にし、全 Pod の終了を確認します。CronJob・手動起動など別の書き込み元も停止します。停止時点の旧 Chart、`helm get values --all`、設定 Secret、イメージタグ、replicas、Cloudflare 転送先、Minecraft 公開先を手元に保存します。認証情報を含むファイルはコミットしません。Kotlin 版は PostgreSQL のスキーマ互換期間の対象外です。

旧 DB は `/app/data/app.db`、設定は `/app/config/app.toml` です。停止後に旧 PVC を作業 Pod に読み取り専用でマウントし、`app.db` と存在する `app.db-wal`・`app.db-shm` を同じディレクトリへ取り出します。SQLite の WAL を無視して DB 本体だけをコピーしません。Python の `sqlite3.Connection.backup()` で検証用の一体化したスナップショットを作り、そのスナップショットを移行入力とします。旧 PVC の元ファイルには移行処理を書き込みません。

## 旧 Chart からの更新前の SQLite 保存

新Chartへの更新時に旧SQLite PVC `mcguildlink-data` は削除します。停止後にWALを含めて取り出し、一体化した `app.db` の整合性とSHA256を確認してから更新します。旧Chart・values・設定ファイル・イメージも保存します。

PVCの保持指定は使用しません。切り戻しでは、旧Chartで空のPVCを作り直し、保存した `app.db` を配置してSHA256・所有者・権限を確認してから旧アプリを起動します。具体的な停止・保存は[k3dリハーサル手順の手順2](mcguildlink-k3d-rehearsal.md)、復元は別文書の[切り戻し手順](mcguildlink-k3d-rollback.md)に記載しています。

## 分離構成のセットアップ

旧SQLiteは保存したDBファイルから復元します。元PVCは保持せず、DBスナップショット・旧設定・旧イメージを切り戻し用に保存します。実データ・設定の変換と代表データによる検証は、分離構成へ切り替える前に完了させます。

既存PostgreSQLの中途半端なpassword・ロール設定は引き継がず、停止後にPostgreSQLのPVCを削除して新構成の設定で初期化する方針です。現行の最新旧BotはPostgreSQLを使わず、移行対象の業務データはSQLiteにあります。[k3dリハーサル手順](mcguildlink-k3d-rehearsal.md)で、最新旧リリースの起動から同じreleaseの更新、PostgreSQL再初期化、SQLiteファイルからの復元、切り戻しまでを確認します。

[独立デプロイ手順の「DB と Secret の準備」](independent-deployment.md#db-secrets)と[配置順序](independent-deployment.md#配置順序)に従い、PostgreSQL→DB ロール・Secret→専用 Job の順に配置します。新 Chart には Bot・MC・API の公開済みイメージタグを設定し、`bot.replicas=0`、`mcLinkServer.replicas=0`、`publicApi.replicas=0` を明示します。空のPostgreSQLへ移行し、既存PostgreSQLデータとのマージは行いません。

初期セットアップを完了してからデプロイ Action を使用します。Action はセットアップ済み Helm release の値を引き継ぎ、対象のイメージタグだけを更新します。

## 設定値の引き継ぎ

旧 `app.toml` の値を次のように配置します。Bot は既存の `config.toml` に `[mcguildlink]` を追加し、他機能の設定・owners・既存 Bot token を維持します。旧専用 Bot の token は統合 Bot の token へ置き換えます。ID は数値を変えず、Rust TOML では文字列として記載します。

| 旧設定 | Rust 構成の配置 |
|---|---|
| `bot.guild` | `mcguildlink.guild_id = "旧ID"` |
| `bot.moderator_role` | `mcguildlink.moderator_role_id = "旧ID"` |
| `bot.log_channel` | `mcguildlink.audit_channel_id = "旧ID"` |
| `bot.display_server_address` | `mcguildlink.display_server_address` に同じ文字列 |
| `minecraft_server.port` | Helm `mcLinkServer.port` に同じ公開ポート。コンテナ内は25565 |
| `minecraft_server.timeout` | 引き継がない。Rust版の入力待ちは既存の固定5分を使用 |
| `web.port` | Helm `publicApi.port` に同じ Service ポート。コンテナ内は8080。Tunnel の転送先も同じポートへ変更 |
| `minecraft_server.address` / `web.address` | Pod 内は `0.0.0.0`。旧アドレスによる公開範囲の制限は Service・Tunnel・クラスタの通信制御に引き継ぐ |

直接起動では `MC_LINK_SERVER_LISTEN` と `PUBLIC_API_LISTEN` に旧 address:port を指定できます。新規の DB 接続設定は各サービス専用の `DATABASE_URL` と Secret の `DATABASE_PASSWORD` に配置します。生成済み `static/whitelist.json` は移さず、公開 API が DB から生成します。

旧 Secret と新 Secret の上記各値を照合し、対象ギルド・管理ロール・監査チャンネル・表示アドレス・公開ポートをリハーサル記録に残します。旧パネルの操作は専用 Bot に紐づくため、解禁後に統合 Bot の `/create_panel` で同じ場所へ設置し直します。

## 移行と全件照合

アプリを停止した状態で、管理者として `scripts/mcguildlink-cutover-freeze.sql` を `psql -X -v ON_ERROR_STOP=1 -f` で適用します。標準の `platform_bot`・`platform_mc_link_server` から書き込みロールを外し、起動確認に必要な読み取り権限を付けます。公開 API はもともと読み取り専用です。別ロール・直接付与などで書き込み権限が残れば処理全体が失敗します。独自の LOGIN 名を使う構成では SQL の LOGIN 名を実際の設定に合わせ、書き込みが拒否されることを確認します。凍結後に bootstrap を再実行すると書き込みロールが復活するので実行しません。

Python 3.11 以降で、保存した SQLite スナップショットから SQL を生成します。`--source-utc-offset` は旧 JVM のタイムゾーンです。標準の旧コンテナはUTCですが、`TZ`・`-Duser.timezone` の独自設定を確認してから指定してください。夏時間をまたぐローカル日時のデータは単一オフセットでは処理できないため、旧環境で日時をUTCに正規化した検証用コピーが必要です。

次はコマンドの書式です。`<...>` は実際のパスへ置き換える項目で、そのまま実行するブロックではありません。本番では[本番手順5](mcguildlink-production-cutover.md#5-migratorを完了させ凍結してデータを移行する)の具体的なコマンドを使います。

```text
python scripts/migrate-mcguildlink.py <停止後のSQLiteスナップショット> <移行SQLの保存先> --source-utc-offset=+00:00
psql -X -v ON_ERROR_STOP=1 -f <移行SQLの保存先>
python scripts/migrate-mcguildlink.py <同じSQLiteスナップショット> <照合SQLの保存先> --source-utc-offset=+00:00 --verify-only
psql -X -v ON_ERROR_STOP=1 -f <照合SQLの保存先>
```

psql の接続先は移行専用ユーザーのテスト DB とし、`PGHOST`・`PGPORT`・`PGDATABASE`・`PGUSER` とパスワード管理で指定します。SQL の保存先は毎回未作成のパスを使います。SQL は名前・コードを含むため、認証情報と同じように保管し、コミットしません。

移行処理は1トランザクションで7テーブルをロックし、移行先の業務データ・監査ログ・outbox が空であることを確認します。内部アカウントIDとブロックグループIDを保持し、不要になったリンク等の旧行IDだけを除きます。UUID BLOB は標準UUIDへ、ローカル日時はUTCへ変換し、ID採番の開始位置も調整します。ブロックを先に取り込み、既存の紐付け・コードがブロック制約に違反していれば失敗します。Snowflake が SQLite の REAL に丸められている場合も復元できないため停止し、元の情報を確認します。

全テーブルの全列を双方向に照合し、件数・関係・未使用コード・日時・ブロックグループとメンバーの一致を確認してから commit します。不一致や制約違反は全体を rollback します。既存データの上書き・削除や再実行時のマージは行いません。監査ログ・outbox は空のままにし、過去のDiscord投稿は取り込みません。

## 起動確認と書き込み解禁

1. 凍結を維持して新アプリを起動し、スキーマ互換性と待受を確認します。Bot のコード発行・MC の紐付け・Bot の退出/BAN処理・監査配送の更新は DB で拒否されます。この段階の書き込み失敗ログは想定内です。実際のコード消費等を確認したい場合は、専用リハーサル DB だけで解禁します。
2. 公開 API の `/whitelist.json` を取得し、保存済みSQLiteから生成した比較用JSONとUUID・名前・ブロック対象の除外を比較します。停止前の旧公開応答とは取得時点が異なるため、参考記録として保持します。公開経路を切り替える前にクラスタ内で確認します。[本番手順2](mcguildlink-production-cutover.md#2-旧アプリを停止し復元に必要なものを保存する)に比較用JSONの生成コマンドを記載しています。
3. 旧設定との照合、MC の Status/本人認証/ダイアログまでの確認、公開経路の確認後、`--verify-only` で再照合します。監査ログ・outbox が空であることも確認します。
4. 切り戻しの判断期限はここです。失敗時は次節の手順を実行し、解禁しません。
5. 解禁を決めたら新アプリを一度停止し、管理者で `scripts/mcguildlink-cutover-unfreeze.sql` を適用します。Bot・MC・API を起動して通常運用へ進みます。ここから旧 SQLite への切り戻しは対象外です。監査は以後のイベントから記録・配送されます。

## 自動検証

本番手順の比較用JSONと切り戻し用PostgreSQL設定の生成は、DB接続なしで文書に掲載したコードをそのまま検証できます。多対多の重複排除、Discord・Minecraftのブロック除外、元SQLiteのハッシュ不変、既存JSONの上書き拒否に加え、PVCが削除前のまま・削除済み・再作成済みの各設定を確認します。

```powershell
python -X utf8 scripts/test-mcguildlink-migration.py --document-check
```

bootstrap と sqlx マイグレーションを適用した専用の空テスト DB で実行します。psql が必要です。本番 DB では実行しません。

```text
python scripts/test-mcguildlink-migration.py --psql psql <専用テストDBの接続指定>
```

代表データで多対多・欠番ID・UUID BLOB・日本語・非UTC時刻・未使用コードの保持・ブロック制約・採番・再実行拒否・不一致検出・凍結中の書き込み拒否・解禁後の権限復帰を検証します。元SQLiteのハッシュ不変と旧版相当の問い合わせで、解禁前に元のデータへ戻れることを確認します。コードの実消費、実際の旧版再起動、Secret変換、公開経路とMinecraft Java 26.3は[k3dリハーサル手順](mcguildlink-k3d-rehearsal.md)で確認し、結果を記録します。

## 公開経路と切り戻し

旧本番の Cloudflare Tunnel 転送先 `http://mcguildlink-http:8080` と hostname は移行まで維持します。切替時は同じ hostname の転送先を public-api の Service へ変更し、`/whitelist.json` の応答と JSON 形式を確認します。MC の Service 公開先も新サーバーへ切り替え、実クライアントで検証します。

書き込み解禁前の切り戻しは次の順にリハーサルします。

以下は新Chart配置後の順序です。本番で旧版停止直後や新Chart配置途中に失敗した場合は、[本番切り戻しR0](mcguildlink-production-rollback.md#r0-失敗した段階に応じて開始位置を選ぶ)から、残っているPVCと配置段階に応じた入口を選びます。

1. Rust Bot・MC・APIを停止し、全Pod終了を確認します。PostgreSQLの凍結は解除しません。
2. 保存した旧Chart・アプリのvalues・Secret・イメージへ戻します。旧版・Botのreplicas=0を維持して空のSQLite PVCを作り直し、作業Podから保存済み `app.db` をコピーします。保存したSHA256との一致、所有者65532:65532、権限600を確認して作業Podを削除します。PostgreSQLは再初期化後のDB名・管理ユーザー・Secret設定を維持し、削除した旧PostgreSQLの復元は行いません。
3. Cloudflare の同じ hostname を旧 `http://mcguildlink-http:8080`（独自ポートなら旧値）へ戻し、MCの旧Service・公開ポートを復元します。
4. 保存したDBファイルが変わっていないことと、新しいPVCへの復元内容の一致を確認して旧版・Botを起動します。旧 `/whitelist.json`、未使用コードの再表示、ブロック対象の拒否を確認します。起動後はSQLiteが更新される可能性があるため、起動前の復元ファイルのSHA256を照合します。
5. 切り戻しの成功と保存した設定値・公開先の一致を記録します。再移行する場合は別の空PostgreSQL DBでやり直します。

新 Chart の Helm rollback だけでは PostgreSQL の内容は戻りません。down SQL は使いません。本番切替の実行、解禁後の逆移行、バックアップ・復旧設計はこの作業の対象外です。
