# k3dリハーサルで切り戻しを選んだ場合の手順

## R0. 新版へ戻るための設定とDBを保存する

切り戻しを選んだ場合はR1より先に実行します。入口と順序は[リハーサルの入口表](mcguildlink-k3d-rehearsal.md#手順一覧)を参照してください。新PostgreSQLのPVCと凍結状態は、旧版復帰中も保持します。旧Bot・旧MCGuildLinkの業務データは復元したSQLiteを使います。Podman VM全体の複製や、新しいクラスタの構築は不要です。

```powershell
$newValuesFile = Join-Path $rehearsalDir 'new-values-before-rollback.yaml'
$newValues = helm get values platform --kube-context $rehearsalContext -n $rehearsalNamespace -o yaml
if ($LASTEXITCODE -ne 0) { throw '新版valuesの保存に失敗しました' }
[IO.File]::WriteAllText($newValuesFile, ($newValues -join "`n"), [Text.UTF8Encoding]::new($false))
Remove-Variable newValues
$pgBackupFile = Join-Path $rehearsalDir ('platform-before-rollback-' + [guid]::NewGuid().ToString('N') + '.sql')
$pgDump = kubectl --context $rehearsalContext -n $rehearsalNamespace exec postgres-0 -- pg_dump -U platform_admin -d platform --format=plain
if ($LASTEXITCODE -ne 0) { throw '新PostgreSQLのバックアップに失敗しました' }
[IO.File]::WriteAllText($pgBackupFile, ($pgDump -join "`n"), [Text.UTF8Encoding]::new($false))
Remove-Variable pgDump
```

DBバックアップは手元に保存する保護用です。通常の新版復帰では、保持したPostgreSQLをそのまま使い、SQLを再投入しません。旧版復帰中に作ったSQLiteの変更は、新版PostgreSQLへ再移行しません。

## R1. 新版を停止し、旧Chartと設定を復元する

新版Bot・MC・APIを停止し、Pod終了を確認します。PostgreSQLの凍結は維持します。保存した設定をSecretへ戻し、旧アプリを0のまま旧Chartへ更新すると、削除済み `mcguildlink-data` の代わりに新しい空PVCが作成されます。

通常の移行手順の続きとして実行しません。切り戻しを選んだ場合だけ実行してください。変数は[リハーサル手順1](mcguildlink-k3d-rehearsal.md)で定義した値を使用します。

```powershell
$postgresStorageSize = kubectl --context $rehearsalContext -n $rehearsalNamespace get pvc postgres-data -o jsonpath='{.spec.resources.requests.storage}'
if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrEmpty($postgresStorageSize)) { throw 'PostgreSQLのPVC容量を取得できません' }
kubectl --context $rehearsalContext -n $rehearsalNamespace scale deployment/bot deployment/mc-link-server deployment/public-api --replicas=0
if ($LASTEXITCODE -ne 0) { throw '新アプリの停止に失敗しました' }
$newPods = kubectl --context $rehearsalContext -n $rehearsalNamespace get pods -l 'app.kubernetes.io/name in (bot,mc-link-server,public-api)' -o name
if ($LASTEXITCODE -ne 0) { throw '新アプリのPod確認に失敗しました' }
if ($newPods) {
    kubectl --context $rehearsalContext -n $rehearsalNamespace wait --for=delete pod -l 'app.kubernetes.io/name in (bot,mc-link-server,public-api)' --timeout=5m
    if ($LASTEXITCODE -ne 0) { throw '新アプリのPodが終了しませんでした' }
}
$snapshotPath = Join-Path $rehearsalDir 'app.db'
$hashRecord = @(Select-String -Path (Join-Path $rehearsalDir 'sqlite-source/snapshot.sha256.txt') -Pattern '^\s*Hash\s*:\s*([A-Fa-f0-9]{64})\s*$')
if ($hashRecord.Count -ne 1) { throw '保存したSHA256が読み取れません' }
$snapshotHash = $hashRecord[0].Matches[0].Groups[1].Value
if ((Get-FileHash $snapshotPath -Algorithm SHA256).Hash -ne $snapshotHash) { throw '保存したDBが変化しています' }
kubectl --context $rehearsalContext -n $rehearsalNamespace create secret generic bot-config "--from-file=config.toml=$(Join-Path $rehearsalDir 'config/bot-old.toml')" --dry-run=client -o yaml | kubectl --context $rehearsalContext -n $rehearsalNamespace apply -f -
if ($LASTEXITCODE -ne 0) { throw '旧Bot設定の復元に失敗しました' }
kubectl --context $rehearsalContext -n $rehearsalNamespace create secret generic mcguildlink-config "--from-file=app.toml=$(Join-Path $rehearsalDir 'config/mcguildlink-old.toml')" --dry-run=client -o yaml | kubectl --context $rehearsalContext -n $rehearsalNamespace apply -f -
if ($LASTEXITCODE -ne 0) { throw '旧MCGuildLink設定の復元に失敗しました' }
# 新PGは作り直した設定を維持する。
helm upgrade platform $oldChart --kube-context $rehearsalContext -n $rehearsalNamespace --reset-values -f $oldValues -f $oldOverrides --set bot.replicas=0 --set mcguildlink.replicas=0 --set postgres.username=platform_admin --set postgres.database=platform --set postgres.secretName=postgres-db-credentials "--set=postgres.storageSize=$postgresStorageSize" --timeout 10m
if ($LASTEXITCODE -ne 0) { throw '旧Chartへの更新に失敗しました' }
$restoredPvcUid = kubectl --context $rehearsalContext -n $rehearsalNamespace get pvc mcguildlink-data -o jsonpath='{.metadata.uid}'
if ($LASTEXITCODE -ne 0) { throw '復元用PVCがありません' }
if (Test-Path (Join-Path $rehearsalDir 'sqlite-source/pvc-uid.txt')) {
    $oldPvcUid = (Get-Content (Join-Path $rehearsalDir 'sqlite-source/pvc-uid.txt') -Raw).Trim()
    if ($restoredPvcUid -eq $oldPvcUid) { throw '旧PVCが残っています。新しいPVCへの復元になっていません' }
}
```

## R2. 保存したDBファイルを新しいPVCへ復元する

次に、新しいPVCへ書き込む作業Podを作成します。アプリと同じUID/GIDでコピーし、WALを取り込み済みの `app.db` だけを配置します。古いWAL/SHMは復元しません。PVCは作業Podのマウント時にプロビジョニングされるため、PodがReadyになってからコピーします。

```powershell
$restorePod = 'mcguildlink-sqlite-restore'
@"
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
    seccompProfile:
      type: RuntimeDefault
  containers:
    - name: restore
      image: busybox:1.37.0
      command: ["sleep", "3600"]
      securityContext:
        allowPrivilegeEscalation: false
        readOnlyRootFilesystem: true
        capabilities:
          drop: ["ALL"]
      volumeMounts:
        - name: data
          mountPath: /restore
  volumes:
    - name: data
      persistentVolumeClaim:
        claimName: mcguildlink-data
"@ | kubectl --context $rehearsalContext -n $rehearsalNamespace create -f -
if ($LASTEXITCODE -ne 0) { throw '復元Podの作成に失敗しました' }
try {
    kubectl --context $rehearsalContext -n $rehearsalNamespace wait --for=condition=Ready "pod/$restorePod" --timeout=3m
    if ($LASTEXITCODE -ne 0) { throw '復元Podが起動しませんでした' }
    kubectl --context $rehearsalContext -n $rehearsalNamespace exec $restorePod -- sh -c 'test ! -e /restore/app.db && test ! -e /restore/app.db-wal && test ! -e /restore/app.db-shm && test ! -e /restore/.app.db.restore'
    if ($LASTEXITCODE -ne 0) { throw '復元先にDBファイルが既にあります。上書きしません' }
    Push-Location $rehearsalDir
    try {
        kubectl --context $rehearsalContext -n $rehearsalNamespace cp './app.db' "${restorePod}:/restore/.app.db.restore" -c restore --no-preserve
        if ($LASTEXITCODE -ne 0) { throw 'DBファイルのコピーに失敗しました' }
    } finally { Pop-Location }
    $copiedHash = kubectl --context $rehearsalContext -n $rehearsalNamespace exec $restorePod -- sha256sum /restore/.app.db.restore
    if ($LASTEXITCODE -ne 0 -or ($copiedHash -split '\s+')[0] -ne $snapshotHash) { throw 'コピーしたDBのSHA256が一致しません' }
    kubectl --context $rehearsalContext -n $rehearsalNamespace exec $restorePod -- chmod 600 /restore/.app.db.restore
    if ($LASTEXITCODE -ne 0) { throw 'DBファイルの権限設定に失敗しました' }
    $ownerMode = kubectl --context $rehearsalContext -n $rehearsalNamespace exec $restorePod -- stat -c '%u:%g %a' /restore/.app.db.restore
    if ($LASTEXITCODE -ne 0 -or $ownerMode.Trim() -ne '65532:65532 600') { throw 'DBの所有者・権限が一致しません' }
    kubectl --context $rehearsalContext -n $rehearsalNamespace exec $restorePod -- mv /restore/.app.db.restore /restore/app.db
    if ($LASTEXITCODE -ne 0) { throw '復元DBの配置に失敗しました' }
} finally {
    kubectl --context $rehearsalContext -n $rehearsalNamespace delete pod $restorePod --wait=true --timeout=3m
}
```

## R3. 旧版を起動し、公開経路とデータを確認する

成功したらTunnelの同じhostnameの宛先を `http://mcguildlink-http:8080` に戻し、旧アプリを起動します。旧MCGuildLinkは復元した新PVCを使用し、新PostgreSQLには書き込みません。復元に失敗した場合はアプリを起動せず、保存済みDBとハッシュを保持して原因を確認します。

```powershell
helm upgrade platform $oldChart --kube-context $rehearsalContext -n $rehearsalNamespace --reuse-values --set bot.replicas=1 --set mcguildlink.replicas=1 --timeout 10m
if ($LASTEXITCODE -ne 0) { throw '旧アプリの起動に失敗しました' }
kubectl --context $rehearsalContext -n $rehearsalNamespace rollout status deployment/mcguildlink --timeout=5m
kubectl --context $rehearsalContext -n $rehearsalNamespace rollout status deployment/bot --timeout=5m
```

旧版の起動、同じhostname/25600ポート、保存時点と一致する紐付け一覧、ブロック拒否、旧版のホワイトリストを確認します。保存時点に未使用コードがあれば同じ値を再表示できることも確認し、0件なら未検証と記録します。過去の回の0件記録は[実施記録](mcguildlink-rehearsal-results.md)を参照してください。

旧専用Botのパネル・操作も確認し、必要なら旧Botの登録操作を行います。新Botのグローバルコマンド登録がDiscord側に残っているだけでは、旧版復帰成功と扱いません。確認が成功して初めて切り戻し完了です。

切り戻しのみなら凍結を維持して終了します。切り戻し後解禁を選んだ場合は[U1](mcguildlink-k3d-unfreeze-test.md)へ進みます。
