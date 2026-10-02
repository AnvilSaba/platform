# 本番移行で切り戻しを選んだ場合の手順

書き込み解禁前に問題が見つかり、旧版へ戻すと判断した場合だけ実行します。通常の[本番移行手順](mcguildlink-production-cutover.md)の続きではありません。解禁後のPostgreSQLからSQLiteへの逆移行には使用できません。

本番移行手順で定義した `$productionContext`・`$productionNamespace`・`$releaseName`・`$cutoverDir`・`$oldChart`・`$oldValues` と、`k`・`h`・`db` 関数を、本番移行手順と同じLinuxの作業用zshで使用します。別のシェルから行う場合は、`setopt ERR_EXIT PIPE_FAIL; umask 077` を設定し、保存した `$cutoverDir/context.zsh` を `source` して復元します。コマンドは一括貼り付けせず、各番号を確認してから進めます。

## R0. 失敗した段階に応じて開始位置を選ぶ

| 失敗した段階 | 戻り方 |
|---|---|
| 旧版停止後、PostgreSQL再初期化の開始前 | 下の「旧環境をそのまま再開」を実行して終了。PVCの復元は不要 |
| PostgreSQL再初期化・新Chart配置の途中 | R1から実行。存在するDeploymentだけ停止し、残っているPVCを確認する |
| 新Chart配置後〜解禁開始前 | R1から実行 |
| 解禁SQLの実行を開始した後 | この文書を実行しない。接続切断時も、書き込みが解禁された可能性がある |

本番移行手順は、PostgreSQLのPVC削除前に `postgres-rebuild-started`、解禁SQL実行前に `unfreeze-started` を作成します。次の確認が失敗した場合は切り戻しを進めません。解禁SQLが確実にrollbackされた場合も、DBの凍結状態と業務書き込みがなかったことを確認するまでは記録を削除しません。

```zsh
[[ ! -e "$cutoverDir/unfreeze-started" ]]
```

### 旧環境をそのまま再開する場合

`postgres-rebuild-started` がない場合だけ実行します。旧Chart・設定・SQLite PVCはそのままなので、保存した旧valuesで停止前のreplicasを復元します。Cloudflareの宛先を変更していた場合は、R4の説明に従って旧宛先へ戻します。このブロックが成功したらR5へ進み、R1〜R3は実行しません。

```zsh
[[ ! -e "$cutoverDir/postgres-rebuild-started" ]]
h upgrade "$releaseName" "$oldChart" --reset-values -f "$oldValues" --timeout 10m
k rollout status deployment/mcguildlink --timeout=5m
k rollout status deployment/bot --timeout=5m
```

## R1. 新アプリを停止する

R0でPostgreSQL再初期化の開始後と確認した場合に実行します。新旧アプリのうち存在するDeploymentだけを停止し、全Pod終了を確認します。凍結済みなら解除しません。保存した旧SQLiteスナップショットのSHA256を確認します。

```zsh
[[ -e "$cutoverDir/postgres-rebuild-started" && ! -e "$cutoverDir/unfreeze-started" ]]
appDeployments=$(k get deployment -l 'app.kubernetes.io/name in (bot,mcguildlink,mc-link-server,public-api)' -o name)
if [[ -n "$appDeployments" ]]; then
  for deployment in "${(@f)appDeployments}"; do
    k scale "$deployment" --replicas=0
  done
fi
newPods=$(k get pods -l 'app.kubernetes.io/name in (bot,mcguildlink,mc-link-server,public-api)' -o name)
if [[ -n "$newPods" ]]; then
  k wait --for=delete pod -l 'app.kubernetes.io/name in (bot,mcguildlink,mc-link-server,public-api)' --timeout=5m
fi
(cd "$cutoverDir" && sha256sum -c sqlite-source/snapshot.sha256)
snapshotHash=$(sha256sum "$cutoverDir/app.db" | cut -d ' ' -f 1)
```

## R2. 旧Chartと設定を戻す

保存した旧Bot・MCGuildLink設定をSecretへ戻します。旧Chart・旧values・旧イメージで同じreleaseを更新し、旧アプリのreplicas=0を維持します。

PostgreSQLのPVCが削除前と同じUIDなら旧設定を使います。削除済み・作り直し済みなら、移行用のDB名・管理ユーザー・管理者Secretを使います。容量とStorageClassは現在のPVCから取得し、まだ存在しない場合は保存した旧PVCの値を使います。ここでPostgreSQLのPVCを削除したり、down SQLを適用したりしません。


```zsh
sudo install -d -o root -g root -m 700 /etc/anvilsaba/secrets/bot
sudo install -o root -g root -m 600 "$cutoverDir/config/bot-old.toml" /etc/anvilsaba/secrets/bot/config.toml
k create secret generic bot-config --from-file="config.toml=$cutoverDir/config/bot-old.toml" --dry-run=client -o yaml | k apply -f -
k create secret generic mcguildlink-config --from-file="app.toml=$cutoverDir/config/mcguildlink-old.toml" --dry-run=client -o yaml | k apply -f -
k get pvc postgres-data --ignore-not-found -o json > "$cutoverDir/rollback-postgres-pvc.json"
python3 - "$cutoverDir" <<'PY'
import json, pathlib, sys
root = pathlib.Path(sys.argv[1])
old = json.loads((root / 'old-postgres-pvc.json').read_text())
current_text = (root / 'rollback-postgres-pvc.json').read_text().strip()
current = json.loads(current_text) if current_text else None
spec = (current or old)['spec']
postgres = {'storageSize': spec['resources']['requests']['storage'], 'storageClassName': spec.get('storageClassName', '')}
if current is None or current['metadata']['uid'] != old['metadata']['uid']:
    postgres.update(username='platform_admin', database='platform', secretName='postgres-db-credentials')
(root / 'rollback-postgres-values.json').write_text(json.dumps({'postgres': postgres}), encoding='utf-8')
PY
h upgrade "$releaseName" "$oldChart" --reset-values -f "$oldValues" -f "$cutoverDir/rollback-postgres-values.json" --set bot.replicas=0 --set mcguildlink.replicas=0 --timeout 10m
restoredPvcUid=$(k get pvc mcguildlink-data -o jsonpath='{.metadata.uid}')
[[ -n "$restoredPvcUid" ]]
originalPvcUid=$(cat "$cutoverDir/sqlite-source/pvc-uid.txt")
if [[ "$restoredPvcUid" == "$originalPvcUid" ]]; then
  printf '%s\n' '元のSQLite PVCが残っています。R3は実行せずR4へ進みます。'
else
  printf '%s\n' '新しいSQLite PVCです。R3で保存済みDBを復元します。'
fi
```

## R3. DBファイルを復元する

R2で新しいSQLite PVCと表示された場合だけ実行します。旧Chartで作成された空のPVCへ、作業Podから保存した単一の `app.db` をコピーします。コピー先が空であること、SHA256一致、所有者65532:65532、権限600を確認し、作業Podを削除します。WALを取り込んだスナップショットに古いWAL/SHMを追加しません。

以下のLinux用コマンドで、作業Podから復元します。失敗後に別シェルで再開する場合はR1・R2から再実行して変数とPVCを確認します。R3は復元済みファイルのハッシュ・権限が一致すれば再コピーせず、不完全な一時ファイルは別名で保持してコピーを再試行します。


```zsh
restorePod=mcguildlink-sqlite-restore
[[ "$restoredPvcUid" != "$originalPvcUid" ]]
k create -f - <<YAML
apiVersion: v1
kind: Pod
metadata:
  name: $restorePod
spec:
  restartPolicy: Never
  automountServiceAccountToken: false
  securityContext:
    runAsNonRoot: true
    runAsUser: 65532
    runAsGroup: 65532
    fsGroup: 65532
    fsGroupChangePolicy: OnRootMismatch
    seccompProfile: {type: RuntimeDefault}
  containers:
    - name: restore
      image: busybox:1.37.0
      command: [sleep, "3600"]
      securityContext:
        allowPrivilegeEscalation: false
        readOnlyRootFilesystem: true
        capabilities: {drop: [ALL]}
      volumeMounts:
        - {name: data, mountPath: /restore}
  volumes:
    - name: data
      persistentVolumeClaim: {claimName: mcguildlink-data}
YAML
(
  trap 'k delete pod "$restorePod" --ignore-not-found --wait=true --timeout=3m' EXIT
  k wait --for=condition=Ready "pod/$restorePod" --timeout=3m
  k exec "$restorePod" -- sh -c 'test ! -e /restore/app.db-wal && test ! -e /restore/app.db-shm'
  if k exec "$restorePod" -- test -e /restore/app.db; then
    copiedHash=$(k exec "$restorePod" -- sha256sum /restore/app.db | cut -d ' ' -f 1)
    ownerMode=$(k exec "$restorePod" -- stat -c '%u:%g %a' /restore/app.db)
    [[ "$copiedHash" == "$snapshotHash" && "$ownerMode" == '65532:65532 600' ]]
    printf '%s\n' '保存データと一致する復元済みDBを確認しました。再コピーしません。'
  else
    if k exec "$restorePod" -- test -e /restore/.app.db.restore; then
      # 失敗したコピーを削除・上書きせず、調査用に保持する。
      k exec "$restorePod" -- mv /restore/.app.db.restore "/restore/.app.db.restore.failed-$(date +%Y%m%d-%H%M%S)"
    fi
    k cp "$cutoverDir/app.db" "${restorePod}:/restore/.app.db.restore" -c restore --no-preserve
    copiedHash=$(k exec "$restorePod" -- sha256sum /restore/.app.db.restore | cut -d ' ' -f 1)
    [[ "$copiedHash" == "$snapshotHash" ]]
    k exec "$restorePod" -- chmod 600 /restore/.app.db.restore
    ownerMode=$(k exec "$restorePod" -- stat -c '%u:%g %a' /restore/.app.db.restore)
    [[ "$ownerMode" == '65532:65532 600' ]]
    k exec "$restorePod" -- mv /restore/.app.db.restore /restore/app.db
  fi
)
```

## R4. 旧版の公開経路と稼働を復元する

Cloudflareで既存本番Tunnelの `api.anvilsaba.org` のService URLを、`http://mcguildlink-http:8080` へ変更します。TypeはHTTP、URLは `mcguildlink-http:8080` です。公開hostnameは `api.anvilsaba.org` のままで、URLに `/whitelist.json` は付けません。旧Chartで再作成したHTTP Serviceと旧MCの公開アドレス・ポートを確認し、旧アプリを起動します。旧専用Botのパネル操作、旧Botの他機能、紐付け・ブロック・未使用コード、公開ホワイトリストを確認します。必要なBotコマンドを旧版の定義で登録します。


Cloudflareの宛先を旧版へ戻してから、次を実行します。

```zsh
h upgrade "$releaseName" "$oldChart" --reuse-values --set bot.replicas=1 --set mcguildlink.replicas=1 --timeout 10m
k rollout status deployment/mcguildlink --timeout=5m
k rollout status deployment/bot --timeout=5m
```

本番hostnameの `/whitelist.json` を取得し、停止後スナップショットから生成した比較基準と照合します。Pythonが正常終了すれば一致です。

```zsh
curl --fail --silent --show-error -A 'Mozilla/5.0' "https://$productionHostname/whitelist.json" > "$cutoverDir/restored-whitelist.json"
python3 - "$cutoverDir/expected-whitelist.json" "$cutoverDir/restored-whitelist.json" <<'PY'
import json, pathlib, sys
def rows(path):
    return sorted((x['uuid'], x['name']) for x in json.loads(pathlib.Path(path).read_text()))
if rows(sys.argv[1]) != rows(sys.argv[2]):
    raise ValueError('旧版へ戻した公開ホワイトリストが保存データと一致しません')
PY
```

## R5. 結果を記録する

切り戻し理由・復元したイメージと設定・DB照合・公開経路・利用再開時刻を記録して終了します。解禁SQLは実行しません。旧版再開後にSQLiteが更新された場合、次回の移行には改めて停止して取得した最新のスナップショットを使います。
