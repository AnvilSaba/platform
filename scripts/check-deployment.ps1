$ErrorActionPreference = 'Stop'
$chart = Join-Path $PSScriptRoot '../deploy/helm/platform'
$tags = @('--set-string', 'bot.image.tag=ci', '--set-string', 'mcguildlink.image.tag=ci')
foreach ($values in @('values.dev.yaml', 'values.prod.yaml', 'values.integration.yaml')) {
    & helm lint $chart -f "$chart/$values" @tags
    if ($LASTEXITCODE -ne 0) { throw "Helm lint 失敗: $values" }
    $manifest = (& helm template platform $chart -f "$chart/$values" @tags) -join "`n"
    if ($LASTEXITCODE -ne 0) { throw "Helm render 失敗: $values" }
    if ($values -eq 'values.integration.yaml') {
        foreach ($name in @('mc-link-server', 'public-api')) {
            if ($manifest -notmatch "name: $name") { throw "サービスがない: $name" }
        }
        if ($manifest -notmatch 'name: DATABASE_PASSWORD') { throw 'DB Secret 設定がない' }
        foreach ($reference in @(
            'name: mc-link-server-database, key: password',
            'name: public-api-database, key: password',
            'name: cloudflare-tunnel, key: token'
        )) {
            if (-not $manifest.Contains($reference)) { throw "Secret 参照がない: $reference" }
        }
        $documents = $manifest -split '(?m)^---\s*$'
        $mc = ($documents | Where-Object { $_ -match 'kind: Deployment\nmetadata:\n  name: mc-link-server\n' }) -join "`n"
        $api = ($documents | Where-Object { $_ -match 'kind: Deployment\nmetadata:\n  name: public-api\n' }) -join "`n"
        if ($mc -notmatch 'tcpSocket: \{ port: tcp \}' -or $mc -match 'PUBLIC_API_LISTEN|httpGet:|public-api-database|volumeMounts:|volumes:') {
            throw 'MC Link Server に API 用の設定が混在している'
        }
        if ($api -notmatch 'httpGet: \{ path: /whitelist.json, port: http \}' -or $api -match 'PUBLIC_API_LISTEN|volumeMounts:|volumes:|tcpSocket:|mc-link-server-database') {
            throw '公開 API に MC 用の設定が混在している'
        }
    } elseif ($manifest -match 'name: mc-link-server\r?\n|name: public-api|name: db-migrator') {
        throw '既定構成で Rust サービスまたは Job が有効になっている'
    }
}
$productionMc = (& helm template platform $chart -f "$chart/values.prod.yaml" @tags --set-string mcLinkServer.image.tag=ci) -join "`n"
if ($LASTEXITCODE -ne 0 -or $productionMc -notmatch 'containerPort: 25565' -or $productionMc -notmatch 'port: 25600, targetPort: tcp') {
    throw '本番 MC Link Server の Service:25600 → コンテナ:25565 が不正'
}
$customPorts = (& helm template platform $chart -f "$chart/values.integration.yaml" --set mcLinkServer.port=25601 --set publicApi.port=8081) -join "`n"
if ($LASTEXITCODE -ne 0 -or $customPorts -notmatch 'containerPort: 25565' -or $customPorts -notmatch 'containerPort: 8080' -or $customPorts -notmatch 'port: 25601, targetPort: tcp' -or $customPorts -notmatch 'port: 8081, targetPort: http' -or $customPorts -match 'MC_LINK_SERVER_LISTEN|PUBLIC_API_LISTEN') {
    throw 'Service の公開ポート変更でコンテナの待受設定が変更された'
}
$job = (& helm template platform $chart @tags --set migration.enabled=true --set-string migration.image.tag=ci) -join "`n"
if ($LASTEXITCODE -ne 0 -or $job -notmatch 'kind: Job' -or $job -notmatch 'name: db-migrator') {
    throw 'マイグレーション Job を配置できない'
}
foreach ($service in @('mcLinkServer', 'publicApi')) {
    & helm template platform $chart @tags --set "$service.replicas=1" 2>$null | Out-Null
    if ($LASTEXITCODE -eq 0) { throw "$service の必須タグ検証がない" }
}
& helm template platform $chart @tags --set migration.enabled=true 2>$null | Out-Null
if ($LASTEXITCODE -eq 0) { throw 'Job の必須タグ検証がない' }
# 必須タグ不足の想定した終了コードを CI 呼び出し元へ引き継がない。
$global:LASTEXITCODE = 0
Write-Host '配置の検証に成功しました。'
