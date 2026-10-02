# k3dリハーサルで書き込み解禁後の動作を確認する

入口は[リハーサルの入口表](mcguildlink-k3d-rehearsal.md#手順一覧)を参照してください。直接解禁する場合はU1を飛ばしてU2へ進みます。切り戻し後はU1で保存した新版設定へ戻して同じクラスタで実行できます。Podman VMの複製や最初からの構築は不要です。解禁後の変更を旧SQLiteへ戻す処理は用意していません。

## U1. 旧版復帰テスト後、保存した新版へ戻す

[切り戻し手順R0](mcguildlink-k3d-rollback.md)で新版valuesとDBバックアップを保存し、R1〜R3で旧版復帰を確認してから実行します。旧版と新版を同時起動しません。旧版でのテスト結果を記録しておきます。

```powershell
$newValuesFile = Join-Path $rehearsalDir 'new-values-before-rollback.yaml'
if (-not (Test-Path $newValuesFile)) { throw '切り戻し前に保存した新版valuesがありません' }
kubectl --context $rehearsalContext -n $rehearsalNamespace scale deployment/bot deployment/mcguildlink --replicas=0
if ($LASTEXITCODE -ne 0) { throw '旧アプリの停止に失敗しました' }
$oldPods = kubectl --context $rehearsalContext -n $rehearsalNamespace get pods -l 'app.kubernetes.io/name in (bot,mcguildlink)' -o name
if ($LASTEXITCODE -ne 0) { throw '旧Podの確認に失敗しました' }
if ($oldPods) {
    kubectl --context $rehearsalContext -n $rehearsalNamespace wait --for=delete pod -l 'app.kubernetes.io/name in (bot,mcguildlink)' --timeout=5m
    if ($LASTEXITCODE -ne 0) { throw '旧Podが終了しませんでした' }
}
$botSecret = kubectl --context $rehearsalContext -n $rehearsalNamespace create secret generic bot-config "--from-file=config.toml=$(Join-Path $rehearsalDir 'config/bot-new.toml')" --dry-run=client -o yaml
if ($LASTEXITCODE -ne 0) { throw '新Bot設定の読み込みに失敗しました' }
$botSecret -join "`n" | kubectl --context $rehearsalContext -n $rehearsalNamespace apply -f -
if ($LASTEXITCODE -ne 0) { throw '新Bot設定の復元に失敗しました' }
Remove-Variable botSecret
helm upgrade platform $newChart --kube-context $rehearsalContext -n $rehearsalNamespace --reset-values -f $newValuesFile --set bot.replicas=1 --set mcLinkServer.replicas=1 --set publicApi.replicas=1 --set dbMigrator.enabled=false
if ($LASTEXITCODE -ne 0) { throw '新版への復帰に失敗しました' }
foreach ($service in @('bot', 'mc-link-server', 'public-api')) {
    kubectl --context $rehearsalContext -n $rehearsalNamespace rollout status "deployment/$service" --timeout=5m
    if ($LASTEXITCODE -ne 0) { throw "新版が起動しませんでした: $service" }
}
Get-Content (Join-Path $rehearsalDir 'verify.sql') -Raw | kubectl --context $rehearsalContext -n $rehearsalNamespace exec -i postgres-0 -- psql -X -U platform_admin -d platform -v ON_ERROR_STOP=1
if ($LASTEXITCODE -ne 0) { throw '保存時点の移行データと一致しません。解禁しません' }
```

同じTunnel hostnameの宛先を `http://public-api:8080` に戻し、新Botのコンソールで `register` を実行します。ホワイトリストを照合します。PostgreSQLの再初期化・SQLiteの再インポートは行いません。

## U2. 新アプリを停止して書き込みを解禁する

リハーサル手順4が完了して直接解禁する場合、または切り戻し後のU1が完了した場合に実行します。新アプリの起動・公開経路・verify.sqlの全件照合・書き込み凍結を確認済みであることが前提です。変数はリハーサルで定義した同じクラスタのものを使います。

```powershell
kubectl --context $rehearsalContext -n $rehearsalNamespace scale deployment/bot deployment/mc-link-server deployment/public-api --replicas=0
if ($LASTEXITCODE -ne 0) { throw '新アプリの停止に失敗しました' }
$pods = kubectl --context $rehearsalContext -n $rehearsalNamespace get pods -l 'app.kubernetes.io/name in (bot,mc-link-server,public-api)' -o name
if ($LASTEXITCODE -ne 0) { throw 'Pod確認に失敗しました' }
if ($pods) {
    kubectl --context $rehearsalContext -n $rehearsalNamespace wait --for=delete pod -l 'app.kubernetes.io/name in (bot,mc-link-server,public-api)' --timeout=5m
    if ($LASTEXITCODE -ne 0) { throw '新アプリのPodが終了しませんでした' }
}
Get-Content scripts/mcguildlink-cutover-unfreeze.sql -Raw | kubectl --context $rehearsalContext -n $rehearsalNamespace exec -i postgres-0 -- psql -X -U platform_admin -d platform -v ON_ERROR_STOP=1
if ($LASTEXITCODE -ne 0) { throw '書き込み解禁に失敗しました' }
helm upgrade platform $newChart --kube-context $rehearsalContext -n $rehearsalNamespace --reuse-values --set bot.replicas=1 --set mcLinkServer.replicas=1 --set publicApi.replicas=1
if ($LASTEXITCODE -ne 0) { throw '新アプリの再開に失敗しました' }
foreach ($service in @('bot', 'mc-link-server', 'public-api')) {
    kubectl --context $rehearsalContext -n $rehearsalNamespace rollout status "deployment/$service" --timeout=5m
    if ($LASTEXITCODE -ne 0) { throw "新アプリが起動しませんでした: $service" }
}
```

## U3. 実際の操作を確認する

保存時点の未使用コードが0件なら、1・2は新Botで発行したコードで消費を確認し、移行前コードの保持・消費は未検証と記録します。1件以上なら移行前コードで実施します。過去の結果は[実施記録](mcguildlink-rehearsal-results.md)を参照してください。

1. 移行前に発行した未使用コードを新Botで再表示し、同じ値であることを確認します。
2. そのコードでMC接続から紐付けを完了し、Botの一覧と公開ホワイトリストに反映されることを確認します。
3. 消費したコードを別の未紐付けMinecraftアカウントで使用し、再利用が拒否されることを確認します。
4. 未紐付けのDiscordアカウントで新しいコードを発行し、再表示しても同じ値であることを確認します。
5. 移行済みのブロック対象について、コード発行・紐付けが拒否され、公開ホワイトリストから除外されることを確認します。
6. 成功した紐付け操作の新しい監査ログがDBに残り、設定したDiscord監査チャンネルへ配送されることを確認します。過去データの移行だけでは監査投稿が発生しないことも確認します。

解禁後はテスト操作で業務データが変わるため、旧SQLiteとの全件一致は要求しません。操作前後の紐付け・コード・ブロック・監査の結果を記録して、この回を終了します。
