# 既存 DB の CloudNativePG 移行

この手順は管理者が実行する停止を伴う移行です。従来の `postgres` StatefulSet / `postgres-data` PVC を維持したまま、別の PVC に DB 全体を論理コピーし、検証後にアプリの接続先を変更します。既存の PVC を CloudNativePG に直接渡しません。

新しい Chart は PostgreSQL の StatefulSet・Service・PVC を含みません。**移行前に通常のデプロイを実行すると旧 DB のリソースが削除されます。** 自動デプロイと Config / Secret の自動同期を停止し、以下の順序で実施してください。新規環境は [本番デプロイ手順](deployment.md#initial-setup) を使用します。

## 1. 事前確認と旧 PVC の保護

Secret の準備を除くコマンド例は、デプロイ先の Bash で実行します。デプロイ先に `kubectl` と `helm` を用意し、リポジトリルートから直接実行して、接続先の context と namespace を確認します。Secret の復号・適用は age 秘密鍵を持つ管理端末で行います。

```bash
kubectl config current-context
helm status platform -n anvilsaba
kubectl get statefulset/postgres pvc/postgres-data -n anvilsaba
kubectl exec -n anvilsaba postgres-0 -- psql -U platform_admin -d platform \
  -c 'SELECT version(), pg_database_size(current_database());' \
  -c '\du' -c '\dn+' -c '\dx'
```

対象は PostgreSQL 18、DB `platform`、所有者 `platform_admin`、このリポジトリのアプリ / Migrator / runtime ロールです。追加のロール・スキーマ・拡張がある場合はインポート設定と検証対象を拡張し、互換性を確認します。移行元の `postgres/databasePassword` が実際の管理者パスワードと一致することも確認します。

旧 PVC を消さないため、**旧 Chart の PVC テンプレートに keep を追加した release を先に保存**します。稼働中 PVC への annotation だけでは、Helm の保存済み manifest が変更されません。[Helm のリソース保持仕様](https://helm.sh/docs/howto/charts_tips_and_tricks/#tell-helm-not-to-uninstall-a-resource) を参照してください。

```bash
umask 077
backupDir="$HOME/platform-db-migration-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$backupDir"
oldChartVersion='<CURRENT_CHART_VERSION>'
helm get values platform -n anvilsaba -o yaml > "$backupDir/old-values.yaml"
helm get manifest platform -n anvilsaba > "$backupDir/old-manifest.yaml"
helm pull oci://ghcr.io/anvilsaba/charts/platform --version "$oldChartVersion" \
  --untar --untardir "$backupDir"
```

取得した `$backupDir/platform/templates/postgres-pvc.yaml` の `metadata.annotations` に `helm.sh/resource-policy: keep` を追加します。その他のテンプレートは変更せず、以下を実行します。旧 Chart が参照する旧 Secret はこの段階では変更しません。

```bash
helm upgrade platform "$backupDir/platform" -n anvilsaba \
  --reuse-values --no-hooks --set dbMigrator.enabled=false
helm get manifest platform -n anvilsaba > "$backupDir/protected-manifest.yaml"
sourcePV=$(kubectl get pvc/postgres-data -n anvilsaba -o jsonpath='{.spec.volumeName}')
kubectl patch pv "$sourcePV" --type=merge \
  -p '{"spec":{"persistentVolumeReclaimPolicy":"Retain"}}'
kubectl get pv "$sourcePV" -o custom-columns='NAME:.metadata.name,RECLAIM:.spec.persistentVolumeReclaimPolicy'
```

保存された manifest の `postgres-data` に keep があり、旧 PV が Retain であることを確認します。元の release の値、Chart、暗号化済みの旧 Secret 4 ファイルも Git 外のバックアップ先へ保存します。旧形式の Secret は変更前のリビジョンから取得してください。keep の付いた旧 Chart は切り戻しにも使用します。

## 2. リハーサルと書き込み停止

別 namespace と別 Cluster 名でリハーサルを実施し、復元時間、必要容量、権限、アプリ操作を確認します。移行先の初期値は 10Gi ですが、インポート中は dump と復元データ・インデックスを同時に保存します。移行元の使用量だけで判断せず、必要なら `deploy/postgres/cloudnativepg/cluster.yaml` の容量を増やします。

インポートは新規 Cluster の初期化時だけ実行されます。リハーサル済みの Cluster を本番の最終コピーとして再利用しません。[CloudNativePG のインポート仕様](https://cloudnative-pg.io/docs/1.30/database_import/#the-monolith-type) を参照してください。

本番作業では Bot・MC Link Server・公開 API・Migrator と、その他すべての書き込み元を停止します。自動デプロイも停止したままにします。

```bash
helm upgrade platform "$backupDir/platform" -n anvilsaba --reuse-values --no-hooks \
  --set bot.replicas=0 --set mcLinkServer.replicas=0 --set publicApi.replicas=0 \
  --set cloudflared.replicas=0 --set dbMigrator.enabled=false
kubectl get pods,jobs -n anvilsaba
kubectl exec -n anvilsaba postgres-0 -- psql -U platform_admin -d platform \
  -c 'SELECT usename, application_name, state FROM pg_stat_activity WHERE datname = current_database();'
```

残っている Migrator Job や外部クライアントの書き込みも終了させます。以降は切り替え完了まで旧 DB への書き込みを許可しません。

DB とロールを Git 外にバックアップします。Bash のバイナリ出力を保持するリダイレクトを使用し、ダンプは別ホストにも保管します。ロールのバックアップにはパスワードを含めず、資格情報は暗号化済み Secret で保存します。事前にこのバックアップからの復元を確認してください。

```bash
kubectl exec -n anvilsaba postgres-0 -- pg_dump -U platform_admin -d platform -Fc \
  > "$backupDir/platform.dump"
kubectl exec -n anvilsaba postgres-0 -- pg_dumpall -U platform_admin --roles-only --no-role-passwords \
  > "$backupDir/roles.sql"
kubectl exec -i -n anvilsaba postgres-0 -- psql -XqAt -U platform_admin -d platform \
  -v ON_ERROR_STOP=1 < deploy/postgres/verify-migration.sql > "$backupDir/source.jsonl"
```

各コマンドの終了コードを確認し、エラーがあれば進みません。dump は機密データとして扱います。

## 3. DB 専用 Secret の準備

`bot-database`、`mc-link-server-database`、`public-api-database`、`db-migrator-database` を新規作成します。DB の資格情報だけを持つ `kubernetes.io/basic-auth` の Secret で、旧パスワードの実値を維持しています。既存のサービス用 Secret の型を変更したり削除したりする必要はありません。旧 DB 用 `postgres` Secret も維持します。

以下は管理端末の PowerShell 7 で実行します。管理端末では mise で SOPS と kubectl を用意し、デプロイ先のクラスタに接続する kubeconfig と [Secret 手順](secret-config.md) の age 秘密鍵を設定します。途中で失敗した場合は停止を維持して復旧します。

```powershell
foreach ($secretName in @('bot-database', 'mc-link-server-database', 'public-api-database', 'db-migrator-database')) {
  $decrypted = mise exec -- sops decrypt "deploy/config/production/secrets/$secretName.sops.yaml"
  if ($LASTEXITCODE -ne 0) { throw '復号に失敗しました' }
  $decrypted | mise exec -- kubectl apply -f -
  if ($LASTEXITCODE -ne 0) { throw 'DB 専用 Secret の作成に失敗しました' }
  Remove-Variable decrypted
}
```

## 4. インポートとロール反映

デプロイ先の Bash に戻って実行します。

```bash
kubectl apply --server-side -f https://raw.githubusercontent.com/cloudnative-pg/cloudnative-pg/v1.30.1/releases/cnpg-1.30.1.yaml
kubectl rollout status deployment/cnpg-controller-manager -n cnpg-system --timeout=5m
kubectl get cluster/platform-db -n anvilsaba
```

移行先 `platform-db` が未作成であることを確認します。既存 Cluster がある場合は進めず、作成経緯とデータを確認します。新規構築用の `deploy/postgres/cloudnativepg` を先に適用しないでください。

```bash
kubectl apply -k deploy/postgres/import
kubectl wait cluster/platform-db -n anvilsaba --for=condition=Ready --timeout=30m
kubectl apply -f deploy/postgres/cloudnativepg/roles.yaml
kubectl wait databaserole -n anvilsaba -l app.kubernetes.io/part-of=platform-database \
  --for=jsonpath='{.status.applied}'=true --timeout=5m
kubectl get databaserole -n anvilsaba \
  -o custom-columns='NAME:.metadata.name,APPLIED:.status.applied,SECRET_VERSION:.status.secretResourceVersion'
```

monolith 方式で DB 全体・ロール・所有者・ACL・シーケンス・`_sqlx_migrations` を移します。取り込み後に DatabaseRole を適用することで、初期化 SQL やロール作成との競合を避け、DB 専用 Secret のパスワードを反映します。4 つのログイン用 DatabaseRole の `secretResourceVersion` と各 DB 専用 Secret の `metadata.resourceVersion` が一致することも確認します。

旧 DB の初期管理者は `platform_admin`、移行先は `postgres` のため、PostgreSQL 18 の所属付与で旧 grantor を指定した SQL が失敗する場合があります。CNPG 1.30.1 のインポートはこのエラーを記録して続行します。DatabaseRole の `inRoles` で必要な 3 つの runtime 所属を移行先管理者から再付与するため、**Cluster Ready だけで完了とせず、ロール反映と次段の比較を必須**にします。追加の所属や ADMIN / INHERIT / SET オプションがある環境は別途再現・検証してください。[CNPG 1.30.1 の所属取り込み処理](https://github.com/cloudnative-pg/cloudnative-pg/blob/v1.30.1/pkg/management/postgres/logicalimport/roleinheritance.go#L55-L82) に対応した手順です。

インポート後はこの Cluster を維持します。新規構築用の base に切り替えて初期化のやり直しを要求せず、容量などの運用変更も import overlay 側から適用します。インポート済み Cluster に manifest を再適用しても DB は再コピーされません。

## 5. コピーの検証と接続先の切り替え

```bash
targetPod=$(kubectl get cluster/platform-db -n anvilsaba -o jsonpath='{.status.currentPrimary}')
kubectl exec -i -n anvilsaba "$targetPod" -c postgres -- psql -XqAt -U postgres -d platform \
  -v ON_ERROR_STOP=1 < deploy/postgres/verify-migration.sql > "$backupDir/target.jsonl"
diff -u "$backupDir/source.jsonl" "$backupDir/target.jsonl"
```

履歴・件数・シーケンス値・所有者・ロール所属・テーブル / 列 / 関数の権限が一致することを確認します。検証 SQL は失敗履歴とアプリ / Migrator の過剰権限も検出します。件数の一致だけでは全データの一致を保証しないため、主要データとアプリ操作も確認します。

CloudNativePG は取り込んだ `platform_admin` の SUPERUSER を外します。この差だけは意図した変更で、比較 SQL の出力対象から除外しています。管理作業は primary Pod 内の `postgres` で行います。SECURITY DEFINER 関数の所有者と動作も確認してください。

移行対応 Chart へ更新します。停止状態を維持し、DB のホストを明示して Migrator を先に実行します。既存イメージのタグを維持し、同時にアプリのリリースを変更しません。

```bash
newChartVersion='<MIGRATION_CHART_VERSION>'
helm upgrade platform oci://ghcr.io/anvilsaba/charts/platform \
  --version "$newChartVersion" -n anvilsaba --reuse-values \
  -f deploy/helm/platform/values.prod.yaml --set-string postgres.serviceName=platform-db-rw \
  --set bot.replicas=0 --set mcLinkServer.replicas=0 --set publicApi.replicas=0 \
  --set cloudflared.replicas=0 --set dbMigrator.enabled=true \
  --set-string dbMigrator.image.tag='sha-<CURRENT_RELEASE_SHA>' --timeout 10m
kubectl logs -n anvilsaba job/db-migrator
kubectl get pvc/postgres-data -n anvilsaba
```

旧 StatefulSet と Service はこの更新で削除されます。旧 PVC は前段で保存した keep により残ります。Migrator が正常完了し、旧 PVC が残っていることを確認してアプリを起動します。

```bash
helm upgrade platform oci://ghcr.io/anvilsaba/charts/platform \
  --version "$newChartVersion" -n anvilsaba --reuse-values --set dbMigrator.enabled=false \
  --set bot.replicas=1 --set mcLinkServer.replicas=1 --set publicApi.replicas=1 \
  --set cloudflared.replicas=1 --timeout 10m
```

各アプリのログ、Discord 連携、リンクコード発行 / 連携 / 解除、`/whitelist.json` と DB の identity 採番を確認します。必要な停止前の replica 数が 1 以外なら元の値に戻します。検証後に自動デプロイ・同期を再開します。

## 6. 切り戻しとデータ保持

新 DB に書き込みを開始する前なら、アプリを停止したまま新 Cluster への接続を止め、バックアップした旧サービス用 Secret と keep 付きの旧 Chart / 値を再適用します。新しい DB 専用 Secret は旧 Chart が参照しません。`postgres` の起動と接続を確認してから旧アプリを再開します。Helm rollback だけでは外部管理の Secret は戻りません。

**新 DB への書き込み開始後は旧 DB が古くなるため、単純な接続先の切り戻しではデータを失います。** 再停止して新 DB のバックアップと差分の取り込み方針を決めてください。検証と保持期間が終わるまで旧 PVC・PV・バックアップを削除しません。

新 DB の StorageClass は `Retain`、Cluster は Flux の prune を無効にしています。アプリの Helm uninstall は DB を削除しません。ただし **Cluster の直接削除や namespace の削除では PVC が削除されます**。Retain が残すのは PV とディスクのデータで、自動復旧ではありません。回復には元の PV / ノード / データディレクトリを特定し、再バインドなどの手作業が必要です。[Kubernetes の Retain 仕様](https://kubernetes.io/docs/concepts/storage/persistent-volumes/#retain) を参照してください。

local-path はノードに依存するため、ノード障害も Retain では防げません。独立した保存先へのバックアップと復元確認を続けます。この変更では保存先未指定のため、定期バックアップ / WAL アーカイブは構成していません。
