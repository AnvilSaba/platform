@{
    bot = @{
        VersionFile   = "apps/bot/Cargo.toml"
        Changelog     = "apps/bot/CHANGELOG.md"
        Paths         = @(
            "apps/bot/**"
            "crates/bot-macros/**"
            "crates/platform-database/**"
            "migrations/**"
            ".sqlx/**"
            ".cargo/**"
            "Cargo.toml"
            "Cargo.lock"

            # リリースしていないがモノレポ化前の変更を含めるため
            "src/**"
            "macros/**"
        )
        BaselineTag   = "bot/v3.5.0"
        DisplayName   = "Bot"
    }
    'mcguildlink-old' = @{
        VersionFile   = "apps/mcguildlink/gradle.properties"
        Changelog     = "apps/mcguildlink/CHANGELOG.md"
        Paths         = @("apps/mcguildlink/**")
        FullHistory   = $true
        DisplayName   = "MCGuildLink"
    }
    'mc-link-server' = @{
        VersionFile = 'apps/mcguildlink-rust/Cargo.toml'
        Changelog = 'apps/mcguildlink-rust/CHANGELOG.md'
        Paths = @('apps/mcguildlink-rust/**', 'crates/platform-database/**', 'migrations/**', '.sqlx/**', '.cargo/**', 'Cargo.toml', 'Cargo.lock', 'deploy/Dockerfile.rust')
        FullHistory = $true
        DisplayName = 'mc-link-server'
    }
    'public-api' = @{
        VersionFile = 'apps/public-api/Cargo.toml'
        Changelog = 'apps/public-api/CHANGELOG.md'
        Paths = @('apps/public-api/**', 'crates/platform-database/**', 'migrations/**', '.sqlx/**', '.cargo/**', 'Cargo.toml', 'Cargo.lock', 'deploy/Dockerfile.rust')
        FullHistory = $true
        DisplayName = 'public-api'
    }
    'platform-database' = @{
        VersionFile = 'crates/platform-database/Cargo.toml'
        Changelog = 'crates/platform-database/CHANGELOG.md'
        Paths = @('crates/platform-database/**', 'crates/platform-database/**', 'migrations/**', '.sqlx/**', '.cargo/**', 'Cargo.toml', 'Cargo.lock', 'deploy/Dockerfile.rust')
        FullHistory = $true
        DisplayName = 'platform-database'
    }
    chart = @{
        VersionFile   = "deploy/helm/platform/Chart.yaml"
        Changelog     = "deploy/helm/platform/CHANGELOG.md"
        Paths         = @("deploy/helm/platform/**")
        FullHistory   = $true
        DisplayName   = "Platform Helm Chart"
    }
}
