[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateSet("bot", "mcguildlink")]
    [string] $App,

    [string] $Tag
)

$ErrorActionPreference = "Stop"
$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Set-Location $repositoryRoot

if (-not (Get-Command git-cliff -ErrorAction SilentlyContinue)) {
    throw "git-cliff が見つかりません。https://git-cliff.org/docs/installation/ を参照してインストールしてください。"
}

$releaseConfig = Import-PowerShellDataFile (Join-Path $PSScriptRoot "release-config.psd1")
$settings = $releaseConfig[$App]

$tagPattern = "^$([regex]::Escape($App))/v[0-9]+\.[0-9]+\.[0-9]+$"
$baselinePattern = "^$([regex]::Escape($settings.BaselineTag))$"
$cliffArgs = @(
    "--config", "cliff.toml",
    "--tag-pattern", $tagPattern,
    "--skip-tags", $baselinePattern,
    "--output", $settings.Changelog
)
if ($Tag) {
    if ($Tag -notmatch $tagPattern) {
        throw "タグ '$Tag' は $App のタグ形式ではありません。"
    }
    $cliffArgs += @("--tag", $Tag)
}
foreach ($path in $settings.Paths) { $cliffArgs += @("--include-path", $path) }
$cliffArgs += "$($settings.BaseCommit)..HEAD"

& git-cliff @cliffArgs
if ($LASTEXITCODE -ne 0) { throw "変更履歴の生成に失敗しました。" }

$changelogPath = (Resolve-Path $settings.Changelog).Path
[System.IO.File]::AppendAllText(
    $changelogPath,
    $settings.LegacySection.TrimEnd() + [Environment]::NewLine,
    [System.Text.UTF8Encoding]::new($false)
)

Write-Output $settings.Changelog
