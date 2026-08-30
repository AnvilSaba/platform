@{
    bot = @{
        VersionFile   = "apps/bot/Cargo.toml"
        Changelog     = "apps/bot/CHANGELOG.md"
        Paths         = @(
            "apps/bot/**"
            "crates/bot-macros/**"
            "Cargo.toml"
            "Cargo.lock"

            # リリースしていないがモノレポ化前の変更を含めるため
            "src/**"
            "macros/**"
        )
        BaselineTag   = "bot/v3.5.0"
        DisplayName   = "Bot"
    }
    mcguildlink = @{
        VersionFile   = "apps/mcguildlink/gradle.properties"
        Changelog     = "apps/mcguildlink/CHANGELOG.md"
        Paths         = @("apps/mcguildlink/**")
        BaselineTag   = "mcguildlink/v1.0.0"
        DisplayName   = "MCGuildLink"
    }
    chart = @{
        VersionFile   = "deploy/helm/platform/Chart.yaml"
        Changelog     = "deploy/helm/platform/CHANGELOG.md"
        Paths         = @("deploy/helm/platform/**")
        FullHistory   = $true
        DisplayName   = "Platform Helm Chart"
    }
}
