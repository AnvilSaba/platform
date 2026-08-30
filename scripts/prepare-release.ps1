[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string] $App,

    [ValidateSet("auto", "major", "minor", "patch")]
    [string] $Bump = "auto",

    [switch] $DryRun
)

$ErrorActionPreference = "Stop"
$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Set-Location $repositoryRoot

if (-not (Get-Command git-cliff -ErrorAction SilentlyContinue)) {
    throw "git-cliff が見つかりません。https://git-cliff.org/docs/installation/ を参照してインストールしてください。"
}

$releaseConfig = Import-PowerShellDataFile (Join-Path $PSScriptRoot "release-config.psd1")
$settings = $releaseConfig[$App]

$versionText = Get-Content -Raw $settings.VersionFile
$versionPattern = if ($App -eq "bot") { '(?m)^version = "(?<version>\d+\.\d+\.\d+)"$' } else { '(?m)^version=(?<version>\d+\.\d+\.\d+)$' }
$versionMatch = [regex]::Match($versionText, $versionPattern)
if (-not $versionMatch.Success) {
    throw "$($settings.VersionFile) から現在のバージョンを取得できません。"
}
$currentVersion = $versionMatch.Groups["version"].Value
$tagPattern = "^$([regex]::Escape($App))/v[0-9]+\.[0-9]+\.[0-9]+$"
$existingTags = @(git tag --list "$App/v*")

if ($existingTags.Count -eq 0) {
    if ($Bump -ne "auto") {
        throw "初回リリースでは bump 種別を指定できません。現在の v$currentVersion を基準タグとして作成してください。"
    }
    $nextVersion = $currentVersion
    Write-Host "$App には既存タグがないため、初回バージョン v$nextVersion を使用します。"
} else {
    $cliffBumpArgs = @("--config", "cliff.toml", "--bumped-version", "--tag-pattern", $tagPattern)
    foreach ($path in $settings.Paths) { $cliffBumpArgs += @("--include-path", $path) }
    if ($Bump -ne "auto") { $cliffBumpArgs += @("--bump", $Bump) }
    $nextVersion = (& git-cliff @cliffBumpArgs).Trim() -replace "^$([regex]::Escape($App))/v", ""
    if ($LASTEXITCODE -ne 0 -or $nextVersion -notmatch '^\d+\.\d+\.\d+$') {
        throw "git-cliff で次のバージョンを算出できませんでした。"
    }
}

$nextTag = "$App/v$nextVersion"
if (git rev-parse --verify --quiet "refs/tags/$nextTag") {
    throw "タグ $nextTag は既に存在します。"
}

if ($DryRun) {
    Write-Host "ドライラン: $nextTag を作成予定です。バージョンファイルと変更履歴は更新しません。"
    $previewPath = Join-Path ([System.IO.Path]::GetTempPath()) "${App}-changelog-preview-$([guid]::NewGuid()).md"
    try {
        & (Join-Path $PSScriptRoot "generate-changelog.ps1") -App $App -Tag $nextTag -Output $previewPath | Out-Null
        Write-Host "--- CHANGELOG プレビュー ($($settings.Changelog)) ---"
        Get-Content -Path $previewPath | ForEach-Object { Write-Host $_ }
        Write-Host "--- CHANGELOG プレビュー終了 ---"
        if ($env:GITHUB_STEP_SUMMARY) {
            $summary = @(
                "## CHANGELOG プレビュー: ``$nextTag``",
                "",
                (Get-Content -Path $previewPath -Raw),
                ""
            ) -join [Environment]::NewLine
            [System.IO.File]::AppendAllText(
                $env:GITHUB_STEP_SUMMARY,
                $summary,
                [System.Text.UTF8Encoding]::new($false)
            )
        }
    } finally {
        if (Test-Path $previewPath) { Remove-Item -LiteralPath $previewPath -Force }
    }
} elseif ($App -eq "bot") {
    $updated = [regex]::Replace($versionText, $versionPattern, "version = `"$nextVersion`"", 1)
    Set-Content -Path $settings.VersionFile -Value $updated -NoNewline

    $lockText = Get-Content -Raw "Cargo.lock"
    $lockPattern = '(?ms)(\[\[package\]\]\r?\nname = "bot"\r?\nversion = ")\d+\.\d+\.\d+("\r?\n)'
    $lockUpdated = [regex]::Replace($lockText, $lockPattern, "`${1}$nextVersion`${2}", 1)
    Set-Content -Path "Cargo.lock" -Value $lockUpdated -NoNewline
} else {
    $updated = [regex]::Replace($versionText, $versionPattern, "version=$nextVersion", 1)
    Set-Content -Path $settings.VersionFile -Value $updated -NoNewline
}

if (-not $DryRun) {
    & (Join-Path $PSScriptRoot "generate-changelog.ps1") -App $App -Tag $nextTag | Out-Null
}

Write-Output $nextTag
