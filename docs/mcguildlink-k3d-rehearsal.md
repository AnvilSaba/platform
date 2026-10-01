# k3dでのSQLite移行リハーサル

専用クラスタで、移行・Rust版の起動確認・書き込み解禁前の切り戻しを確認します。移行スクリプトとこの手順は、移行完了後の旧 `apps/mcguildlink` 削除時にまとめて削除します。

必要なものはPython 3.11以降、k3d、kubectl、Helm、Docker DesktopまたはPodmanです。コマンドはリポジトリルートのPowerShellで実行します。以下のpasswordは使い捨てクラスタ専用です。

## クラスタとイメージ

[独立デプロイ手順](independent-deployment.md#配布単位)のコマンドで `mc-link-server:test`・`public-api:test`・`db-migrator:test` のローカルイメージをビルドしておきます。

```powershell
k3d cluster create mcguildlink-rehearsal --timeout 120s
$rehearsalContext = 'k3d-mcguildlink-rehearsal'
$rehearsalNamespace = 'mcguildlink-rehearsal'
kubectl --context $rehearsalContext create namespace $rehearsalNamespace
k3d image import localhost/anvilsaba/mc-link-server:test localhost/anvilsaba/public-api:test localhost/anvilsaba/db-migrator:test -c mcguildlink-rehearsal
kubectl --context $rehearsalContext -n $rehearsalNamespace create secret generic postgres-db-credentials --from-literal=password=admin-rehearsal
helm upgrade --install platform deploy/helm/platform --kube-context $rehearsalContext -n $rehearsalNamespace -f deploy/helm/platform/values.integration.yaml --set bot.replicas=0 --set mcLinkServer.replicas=0 --set publicApi.replicas=0 --set dbMigrator.enabled=false
kubectl --context $rehearsalContext -n $rehearsalNamespace rollout status statefulset/postgres --timeout=5m
Get-Content deploy/postgres/bootstrap.sql -Raw | kubectl --context $rehearsalContext -n $rehearsalNamespace exec -i postgres-0 -- psql -U platform_admin -d platform -v ON_ERROR_STOP=1
```

DBロールのpasswordとSecretを揃えます。Botは通常のリハーサルでは起動しません。

```powershell
@'
ALTER ROLE platform_db_migrator PASSWORD 'migrator-rehearsal';
ALTER ROLE platform_mc_link_server PASSWORD 'mc-rehearsal';
ALTER ROLE platform_public_api PASSWORD 'api-rehearsal';
ALTER ROLE platform_bot PASSWORD 'bot-rehearsal';
'@ | kubectl --context $rehearsalContext -n $rehearsalNamespace exec -i postgres-0 -- psql -U platform_admin -d platform -v ON_ERROR_STOP=1
kubectl --context $rehearsalContext -n $rehearsalNamespace create secret generic db-migrator-db-credentials --from-literal=password=migrator-rehearsal
kubectl --context $rehearsalContext -n $rehearsalNamespace create secret generic mc-link-server-db-credentials --from-literal=password=mc-rehearsal
kubectl --context $rehearsalContext -n $rehearsalNamespace create secret generic public-api-db-credentials --from-literal=password=api-rehearsal
kubectl --context $rehearsalContext -n $rehearsalNamespace create secret generic bot-db-credentials --from-literal=password=bot-rehearsal
helm upgrade platform deploy/helm/platform --kube-context $rehearsalContext -n $rehearsalNamespace --reuse-values --set dbMigrator.enabled=true --timeout 10m
kubectl --context $rehearsalContext -n $rehearsalNamespace logs job/db-migrator
helm upgrade platform deploy/helm/platform --kube-context $rehearsalContext -n $rehearsalNamespace --reuse-values --set dbMigrator.enabled=false
```

## 移行スクリプトの自動確認

まず空の `platform` DBで実行します。このテストは代表SQLiteを一時作成し、移行・照合・制約・凍結/解除・途中失敗のrollback・元SQLiteの不変を検証します。DBに代表データが残ります。アプリはまだ起動しません。

```powershell
python scripts/test-mcguildlink-migration.py --psql kubectl --context $rehearsalContext -n $rehearsalNamespace exec -i postgres-0 -- psql -U platform_admin -d platform
```

「成功」を確認します。途中で失敗した場合はクラスタを作り直します。非空DBへ再実行して上書きする処理はありません。

## 移行後の公開APIとMC

起動確認前に業務書き込みを止めます。

```powershell
Get-Content scripts/mcguildlink-cutover-freeze.sql -Raw | kubectl --context $rehearsalContext -n $rehearsalNamespace exec -i postgres-0 -- psql -U platform_admin -d platform -v ON_ERROR_STOP=1
helm upgrade platform deploy/helm/platform --kube-context $rehearsalContext -n $rehearsalNamespace --reuse-values --set mcLinkServer.replicas=1 --set publicApi.replicas=1
kubectl --context $rehearsalContext -n $rehearsalNamespace rollout status deployment/mc-link-server --timeout=5m
kubectl --context $rehearsalContext -n $rehearsalNamespace rollout status deployment/public-api --timeout=5m
kubectl --context $rehearsalContext -n $rehearsalNamespace port-forward service/public-api 18080:8080
```

別のPowerShellから `curl.exe http://127.0.0.1:18080/whitelist.json` を実行します。期待結果は次の1件です。多対多の紐付けでも重複せず、ブロック済みMinecraftアカウントを含みません。

```json
[{"uuid":"12345678-1234-5678-9abc-123456789abc","name":"Player"}]
```

MC確認は別のPowerShellで同じcontext/namespaceを指定し、`kubectl ... port-forward service/mc-link-server 25566:25565` を実行します。Java 26.3から `localhost:25566` に接続し、本人認証とコード入力画面を確認します。凍結中は有効コードでも業務書き込みが拒否され、紐付けとコード消費が発生しないことをDBで確認します。入力待ちは既存の固定5分です。

## 旧版への切り戻しリハーサル

旧版再起動まで確認する回は、上の代表データテストとは別の新規クラスタで行います。[旧Kotlin版の配置手順](development-and-integration-testing.md)に従い、保存した旧Chart 0.x・開発専用Discord設定で起動してください。旧版のnamespaceを新Rust版と分けておくと、旧SQLite PVCを保持したまま停止・再開できます。

1. 旧版で未使用コード・紐付け・ブロックを用意し、旧ホワイトリスト、設定、replicasを保存します。
2. 旧版を停止し、Pod終了後にSQLite一式を取り出します。[移行手順](mcguildlink-migration.md#旧版停止とデータ保護)に従い、WALを含む検証用スナップショットを作ります。元ファイルのハッシュを保存します。
3. 上のセットアップで空の新DBを用意し、凍結SQLを適用します。旧スナップショットから移行・照合SQLを生成します。

```powershell
python scripts/migrate-mcguildlink.py .rehearsal/app.db .rehearsal/import.sql --source-utc-offset=+00:00
Get-Content .rehearsal/import.sql -Raw | kubectl --context $rehearsalContext -n $rehearsalNamespace exec -i postgres-0 -- psql -U platform_admin -d platform -v ON_ERROR_STOP=1
python scripts/migrate-mcguildlink.py .rehearsal/app.db .rehearsal/verify.sql --source-utc-offset=+00:00 --verify-only
Get-Content .rehearsal/verify.sql -Raw | kubectl --context $rehearsalContext -n $rehearsalNamespace exec -i postgres-0 -- psql -U platform_admin -d platform -v ON_ERROR_STOP=1
```

`.rehearsal` は事前に作成し、出力ファイルは未作成のパスにします。UTCオフセットは旧JVMの設定に合わせます。SQLite・生成SQL・設定Secretはコミットしません。

4. Rust版を凍結状態で起動し、旧版とホワイトリストを比較します。再度照合SQLを実行し、監査ログ・outboxが空であることを確認します。
5. Rust版を停止し、元SQLiteのハッシュが変わっていないことを確認して旧版を保存したreplicasで再開します。旧公開先に接続し、コード再表示・一覧・ブロックを確認します。この回は解禁SQLを実行しません。

## コードの実消費を確認する回

切り戻し確認とは別の、新規クラスタとコピーした旧DBで行います。実クライアントのUUIDを含むデータ、または旧版で取得した未使用コードを使います。

アプリを停止して `scripts/mcguildlink-cutover-unfreeze.sql` を適用し、再起動します。未使用コードで紐付けが成功し、同じコードの再利用が拒否され、ホワイトリストと新しい監査が更新されることを確認します。ブロック済みMinecraftアカウントではブロック応答になり、未使用コードが保持されることも確認します。実Discordの配送まで見る場合だけ、開発専用Bot設定Secretを登録し、Botを1個起動します。

## 記録と後片付け

| 確認項目 | 結果 |
|---|---|
| 移行スクリプトの自動確認 | 未実施 |
| 移行後のAPI出力 | 未実施 |
| 凍結中の業務書き込み拒否 | 未実施 |
| 旧版再起動と未使用コード・ブロック維持 | 未実施 |
| 別クラスタでのコード消費・再利用拒否・監査 | 未実施 |

この文書のk3d手順はまだ実行していません。結果欄は実施後に更新します。確認を終えたら `k3d cluster delete mcguildlink-rehearsal` で専用クラスタを削除し、ローカルのDBコピーと生成SQLも削除します。
