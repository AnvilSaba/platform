@{
    bot = @{
        VersionFile   = "apps/bot/Cargo.toml"
        Changelog     = "apps/bot/CHANGELOG.md"
        Paths         = @(
            "apps/bot/**"
            "apps/db-migrator/**"
            "deploy/rust/**"
            "crates/bot-macros/**"
            "crates/platform-database/**"
            "migrations/**"
            ".sqlx/**"
            ".cargo/**"

            # リリースしていないがモノレポ化前の変更を含めるため
            "src/**"
            "macros/**"
        )
        BaselineTag   = "bot/v3.5.0"
        DisplayName   = "Bot"
    }
    'mc-link-server' = @{
        VersionFile = 'apps/mc-link-server/Cargo.toml'
        Changelog = 'apps/mc-link-server/CHANGELOG.md'
        Paths = @('apps/mc-link-server/**', 'apps/db-migrator/**', 'crates/platform-database/**', 'migrations/**', '.sqlx/**', '.cargo/**', 'deploy/rust/**')
        FullHistory = $true
        DisplayName = 'mc-link-server'
    }
    'public-api' = @{
        VersionFile = 'apps/public-api/Cargo.toml'
        Changelog = 'apps/public-api/CHANGELOG.md'
        Paths = @('apps/public-api/**', 'apps/db-migrator/**', 'crates/platform-database/**', 'migrations/**', '.sqlx/**', '.cargo/**', 'deploy/rust/**')
        FullHistory = $true
        DisplayName = 'public-api'
    }
    chart = @{
        VersionFile   = "deploy/helm/platform/Chart.yaml"
        Changelog     = "deploy/helm/platform/CHANGELOG.md"
        Paths         = @("deploy/helm/platform/**")
        FullHistory   = $true
        DisplayName   = "Platform Helm Chart"
    }
}
