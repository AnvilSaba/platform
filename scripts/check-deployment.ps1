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
    } elseif ($manifest -match 'name: mc-link-server\r?\n|name: public-api|name: platform-migrate') {
        throw '既定構成で Rust サービスまたは Job が有効になっている'
    }
}
$job = (& helm template platform $chart @tags --set migration.enabled=true --set-string migration.image.tag=ci) -join "`n"
if ($LASTEXITCODE -ne 0 -or $job -notmatch 'kind: Job' -or $job -notmatch 'name: platform-migrate') {
    throw 'マイグレーション Job を配置できない'
}
& helm template platform $chart @tags --set mcLinkServer.replicas=1 2>$null | Out-Null
if ($LASTEXITCODE -eq 0) { throw 'Rust サービスの必須タグ検証がない' }
& helm template platform $chart @tags --set migration.enabled=true 2>$null | Out-Null
if ($LASTEXITCODE -eq 0) { throw 'Job の必須タグ検証がない' }
# 必須タグ不足の想定した終了コードを CI 呼び出し元へ引き継がない。
$global:LASTEXITCODE = 0
Write-Host '配置の検証に成功しました。'
