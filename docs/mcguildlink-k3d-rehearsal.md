# 最新の旧リリースからのk3d移行リハーサル

新構成だけの起動確認ではなく、最新の旧リリースを起動した状態から、同じクラスタ・namespace・Helm releaseを更新し、書き込み解禁前に旧版へ戻します。移行完了後、旧 `apps/mcguildlink` を削除するときに移行スクリプトもまとめて削除します。

## 再現する旧環境

2026-10-01にGitHubの公開タグを確認した基準は次のとおりです。GitHub Release一覧は未登録で、リリースタグを基準にしています。実行時もタグと対応する公開イメージを確認し、使用したdigestを記録します。開発中のHEADから旧版をビルドして置き換えません。

| 対象 | 基準 |
|---|---|
| Chart | `chart/v0.2.1` / OCI Chart `0.2.1` |
| Bot | `bot/v3.5.1` / `ghcr.io/anvilsaba/bot:v3.5.1` |
| Kotlin MCGuildLink | `mcguildlink/v1.0.2` / `ghcr.io/anvilsaba/mcguildlink:v1.0.2` |
| Kubernetes | 本番と同じk3sバージョンをk3dの `--image` に指定 |
| namespace / release | `anvilsaba` / `platform` |
| 旧MC公開経路 | LoadBalancer Service `mcguildlink-minecraft`、本番既定の25600/TCP |
| 旧HTTP公開経路 | 開発専用Tunnel・hostname → `http://mcguildlink-http:8080` |
| 保存先 | SQLite PVC `mcguildlink-data`、PostgreSQL PVC `postgres-data` |

旧Chartの既定PostgreSQLは `anvilsaba` DB / `anvilsaba` ユーザー / Secret `postgres` です。新構成の `platform` / `platform_admin` / `postgres-db-credentials` に値を変えるだけでは、既存PVC内のDB・ロール・passwordは初期化し直されません。今回は、旧PostgreSQLの中途半端な設定を捨ててPVCから作り直す手順を検証します。最新の旧BotはPostgreSQLを使わず、MCGuildLinkの業務データはSQLiteにあるため、削除対象はPostgreSQLだけです。

旧リリースのイメージ公開はarm64のみです。対応するノードで実行してください。amd64でタグのソースから再ビルドする場合は、公開イメージそのものの再現ではないことを記録し、本番と同じarm64環境での最終確認を別途行います。

## 1. 旧リリースを起動する

Python 3.11以降、k3d、kubectl、Helm、Docker DesktopまたはPodmanを用意します。コマンドはリポジトリルートのPowerShellで実行します。作業ディレクトリはGitの外に作ります。

```powershell
$rehearsalDir = Join-Path $env:TEMP 'mcguildlink-rehearsal'
New-Item -ItemType Directory -Path $rehearsalDir -Force | Out-Null
$rehearsalContext = 'k3d-mcguildlink-rehearsal'
$rehearsalNamespace = 'anvilsaba'
# 実際の本番k3sバージョンのイメージへ置換する。
k3d cluster create mcguildlink-rehearsal --image '<本番と同じk3sイメージ>' --port '127.0.0.1:25600:25600@server:0' --timeout 120s
kubectl --context $rehearsalContext create namespace $rehearsalNamespace
helm pull oci://ghcr.io/anvilsaba/charts/platform --version 0.2.1 --untar --untardir $rehearsalDir
$oldChart = Join-Path $rehearsalDir 'platform'
```

旧Chart自身の `values.prod.yaml` をコピーしてリハーサル用valuesを作り、Botタグ `v3.5.1`、MCGuildLinkタグ `v1.0.2` を設定します。変更してよい差分は開発専用の認証情報・Discordギルド/ロール/チャンネル・hostname・ストレージ容量/クラスです。旧 `app.toml` とBot設定を旧リリースの形式で用意し、`mcguildlink-config` / `bot-config` に登録します。Secret `postgres` には使い捨てpassword、`cloudflare-tunnel` には開発専用Tunnel tokenを登録します。privateイメージにはpull Secretも用意します。

```powershell
$oldValues = Join-Path $rehearsalDir 'old-values.yaml'
helm upgrade --install platform $oldChart --kube-context $rehearsalContext -n $rehearsalNamespace -f $oldValues --timeout 10m
kubectl --context $rehearsalContext -n $rehearsalNamespace rollout status deployment/mcguildlink --timeout=5m
kubectl --context $rehearsalContext -n $rehearsalNamespace rollout status deployment/bot --timeout=5m
kubectl --context $rehearsalContext -n $rehearsalNamespace rollout status statefulset/postgres --timeout=5m
```

旧版で実際にコード発行・紐付け・ブロックを操作し、多対多・未使用コード・ブロックグループを作ります。Javaクライアントは旧リリースが対応するバージョンを使用し、新Rust版の確認時はJava 26.3へ切り替えます。旧版のDBへ後からfixtureだけを直接INSERTする方法は移行元再現の代わりにしません。

`localhost:25600` のMC接続と開発専用hostnameの `/whitelist.json` を確認し、コード・UUID・ホワイトリスト・設定値・各replicasを記録します。HTTPのport-forwardは診断の補助で、Tunnelを含む公開経路の再現完了とは扱いません。

## 2. 停止・SQLite保護・保存済みmanifestへのkeep指定

```powershell
helm get values platform --all --kube-context $rehearsalContext -n $rehearsalNamespace | Set-Content (Join-Path $rehearsalDir 'old-effective-values.yaml')
helm get manifest platform --kube-context $rehearsalContext -n $rehearsalNamespace | Set-Content (Join-Path $rehearsalDir 'old-manifest.yaml')
kubectl --context $rehearsalContext -n $rehearsalNamespace scale deployment/mcguildlink deployment/bot --replicas=0
kubectl --context $rehearsalContext -n $rehearsalNamespace wait --for=delete pod -l app.kubernetes.io/name=mcguildlink --timeout=5m
kubectl --context $rehearsalContext -n $rehearsalNamespace wait --for=delete pod -l app.kubernetes.io/name=bot --timeout=5m
```

停止後に `mcguildlink-data` を読み取り専用でマウントする作業Podから `app.db` と存在するWAL/SHMを取り出します。[SQLite保護手順](mcguildlink-migration.md#旧版停止とデータ保護)に従いスナップショットを作り、元ファイルのハッシュを保存します。作業Podも停止します。

取得したローカル旧ChartのSQLite PVCテンプレートに `metadata.annotations.helm.sh/resource-policy: keep` を追加し、旧版を0のまま旧Chartで更新します。live PVCへのannotateだけで済ませません。

```powershell
helm upgrade platform $oldChart --kube-context $rehearsalContext -n $rehearsalNamespace -f $oldValues --set mcguildlink.replicas=0 --set bot.replicas=0 --timeout 10m
helm get manifest platform --kube-context $rehearsalContext -n $rehearsalNamespace
```

保存済みmanifestの `mcguildlink-data` にkeep指定があることを確認します。旧設定Secret・イメージ・旧values・開発Tunnelの旧宛先も保持します。

## 3. PostgreSQLを削除して作り直す

以下は専用リハーサルクラスタだけで実行します。旧PostgreSQLのロールpasswordをSecretと異なる値に変えておくと、「中途半端な設定を引き継がない」ことも再現できます。削除前のDB名・ロールを記録します。

```powershell
# 旧PGのみ停止。SQLiteのPVCは削除しない。
kubectl --context $rehearsalContext -n $rehearsalNamespace scale statefulset/postgres --replicas=0
kubectl --context $rehearsalContext -n $rehearsalNamespace wait --for=delete pod/postgres-0 --timeout=5m
kubectl --context $rehearsalContext -n $rehearsalNamespace delete pvc postgres-data
kubectl --context $rehearsalContext -n $rehearsalNamespace wait --for=delete pvc/postgres-data --timeout=5m
kubectl --context $rehearsalContext -n $rehearsalNamespace delete secret postgres
kubectl --context $rehearsalContext -n $rehearsalNamespace create secret generic postgres-db-credentials --from-literal=password=admin-rehearsal
```

新構成のMC・API・Migratorイメージは[独立デプロイ手順](independent-deployment.md#配布単位)でビルドします。新Rust Botもこのブランチの `deploy/rust/Dockerfile` でPACKAGE/BINARYを `bot`、SOURCE_DIRを `apps/bot` としてビルドし、`localhost/anvilsaba/bot:test` としてimportします。新Chartのvaluesは旧値を継承せず、同じreleaseを更新します。旧版と別namespaceに新環境を作る方法では、Helmによる旧リソース削除やPVC保持を検証できません。

```powershell
k3d image import localhost/anvilsaba/bot:test localhost/anvilsaba/mc-link-server:test localhost/anvilsaba/public-api:test localhost/anvilsaba/db-migrator:test -c mcguildlink-rehearsal
helm upgrade platform deploy/helm/platform --kube-context $rehearsalContext -n $rehearsalNamespace --reset-values -f deploy/helm/platform/values.integration.yaml --set bot.image.repository=localhost/anvilsaba/bot --set bot.replicas=0 --set mcLinkServer.replicas=0 --set mcLinkServer.port=25600 --set publicApi.replicas=0 --set dbMigrator.enabled=false
kubectl --context $rehearsalContext -n $rehearsalNamespace rollout status statefulset/postgres --timeout=5m
kubectl --context $rehearsalContext -n $rehearsalNamespace get pvc mcguildlink-data postgres-data
Get-Content deploy/postgres/bootstrap.sql -Raw | kubectl --context $rehearsalContext -n $rehearsalNamespace exec -i postgres-0 -- psql -U platform_admin -d platform -v ON_ERROR_STOP=1
```

`postgres-data` のUIDが変わり、旧DB/旧ロールが残っていないことを確認します。`mcguildlink-data` のUIDと元ファイルのハッシュは変わらないことを確認します。

[DBとSecretの準備](independent-deployment.md#db-と-secret-の準備)に従い4つのLOGINへ別々の使い捨てpasswordを設定し、各DB Secretと一致させます。Botの設定は旧設定から変換して同じ `bot-config` に登録します。旧設定ファイルのコピーは手元に保持します。

```powershell
helm upgrade platform deploy/helm/platform --kube-context $rehearsalContext -n $rehearsalNamespace --reuse-values --set dbMigrator.enabled=true --timeout 10m
kubectl --context $rehearsalContext -n $rehearsalNamespace logs job/db-migrator
helm upgrade platform deploy/helm/platform --kube-context $rehearsalContext -n $rehearsalNamespace --reuse-values --set dbMigrator.enabled=false
Get-Content scripts/mcguildlink-cutover-freeze.sql -Raw | kubectl --context $rehearsalContext -n $rehearsalNamespace exec -i postgres-0 -- psql -U platform_admin -d platform -v ON_ERROR_STOP=1
```

## 4. 旧SQLiteを移行し、新版へ経路を切り替える

```powershell
python scripts/migrate-mcguildlink.py (Join-Path $rehearsalDir 'app.db') (Join-Path $rehearsalDir 'import.sql') --source-utc-offset=+00:00
Get-Content (Join-Path $rehearsalDir 'import.sql') -Raw | kubectl --context $rehearsalContext -n $rehearsalNamespace exec -i postgres-0 -- psql -U platform_admin -d platform -v ON_ERROR_STOP=1
python scripts/migrate-mcguildlink.py (Join-Path $rehearsalDir 'app.db') (Join-Path $rehearsalDir 'verify.sql') --source-utc-offset=+00:00 --verify-only
Get-Content (Join-Path $rehearsalDir 'verify.sql') -Raw | kubectl --context $rehearsalContext -n $rehearsalNamespace exec -i postgres-0 -- psql -U platform_admin -d platform -v ON_ERROR_STOP=1
helm upgrade platform deploy/helm/platform --kube-context $rehearsalContext -n $rehearsalNamespace --reuse-values --set bot.replicas=1 --set mcLinkServer.replicas=1 --set publicApi.replicas=1 --set cloudflared.replicas=1
```

UTCオフセットは旧JVMの設定に合わせます。新Chartでprivateイメージを使う場合はpull Secretの指定も設定します。各Podの起動、スキーマ互換性、凍結中の書き込み拒否を確認します。凍結中の業務書き込みエラーは想定内です。

同じ開発専用hostnameのTunnel宛先を `http://public-api:8080` へ変更し、旧版とホワイトリストのUUID・名前・ブロック除外を比較します。MCは同じ `localhost:25600` から新版へ接続します。port-forwardで新Serviceへ直接つなぎ直すだけで切替成功と扱いません。再照合し、未使用コードが消費されず監査ログ・outboxが空であることを確認します。

## 5. 解禁せず旧版へ切り戻す

新版Bot・MC・APIを停止し、Pod終了を確認します。PostgreSQLの凍結は維持します。旧Bot設定を `bot-config` に戻し、同じreleaseを保存した旧Chartへ戻します。

```powershell
# 新PGは作り直した設定を維持する。旧anvilsabaのDB/ロールを復元する操作ではない。
helm upgrade platform $oldChart --kube-context $rehearsalContext -n $rehearsalNamespace --reset-values -f $oldValues --set bot.replicas=0 --set mcguildlink.replicas=0 --set postgres.username=platform_admin --set postgres.database=platform --set postgres.secretName=postgres-db-credentials --timeout 10m
```

旧Chartへ戻る際に新しいDB Secret/DB名を上書きしないことを確認します。旧Bot/MCGuildLinkは新PostgreSQLへ書き込まず、旧MCGuildLinkは保持した同じSQLite PVCを使用します。元SQLiteのハッシュを確認して旧replicasを復元し、Tunnel宛先を `http://mcguildlink-http:8080` に戻します。

旧版の起動、同じhostname/25600ポート、未使用コード再表示、紐付け一覧、ブロック拒否を確認します。これが成功して初めて切り戻しリハーサル完了です。

## 6. 書き込み解禁を確認する回と補助テスト

切り戻しを確認した後、別の新規クラスタで手順1から繰り返します。今回は切り戻さず、新アプリ停止中に `scripts/mcguildlink-cutover-unfreeze.sql` を適用して再開し、実際の旧未使用コードの消費・再利用拒否・ブロック応答・コード保持・新しい監査配送を確認します。

`test-mcguildlink-migration.py --psql ...` は、スクリプトの異常系を確認する補助テストです。代表fixtureを空DBへ移すだけのこのテストを、旧環境からのリハーサル完了とは扱いません。実行する場合はさらに別の空テストDBを使います。

## 結果と後片付け

| 確認項目 | 結果 |
|---|---|
| 最新旧リリース・同じk3s・旧公開経路の起動 | 未実施 |
| 旧版から作ったSQLite・未使用コード・ブロック | 未実施 |
| PostgreSQL削除後のDB/ロール/password再初期化 | 未実施 |
| 同じrelease更新後のSQLite PVC保持 | 未実施 |
| 新版起動・経路切替・凍結中の全件一致 | 未実施 |
| 同じreleaseでの旧版復帰・公開経路と旧データ維持 | 未実施 |
| 別クラスタでの解禁後のコード消費・監査 | 未実施 |

k3dでの実行はまだ行っていません。実施時は旧新のイメージdigest、values差分、PVC UID、DB名/ロール、SQL照合結果、各公開経路の確認結果を記録します。終了後は `k3d cluster delete mcguildlink-rehearsal` で専用クラスタを削除し、作業ディレクトリのDBコピー・SQL・設定を片付けます。本番環境ではこの文書の削除コマンドを実行しません。
