@{
    bot = @{
        VersionFile   = "apps/bot/Cargo.toml"
        Changelog     = "apps/bot/CHANGELOG.md"
        Paths         = @(
            "apps/bot/**/*"
            "crates/bot-macros/**/*"
            "Cargo.toml"
            "Cargo.lock"

            "deploy/**/*"

            # リリースしていないがモノレポ化前の変更を含めるため
            "src/**/*"
            "macros/**/*"
        )
        BaseCommit    = "59d55a6"
        BaselineTag   = "bot/v3.5.0"
        LegacySection = @"

## 3.5.0 以前

以前の変更は[旧Botコミット履歴](https://github.com/AnvilSaba/platform/commits/59d55a6)を参照してください。
"@
    }
    mcguildlink = @{
        VersionFile   = "apps/mcguildlink/gradle.properties"
        Changelog     = "apps/mcguildlink/CHANGELOG.md"
        Paths         = @("apps/mcguildlink/**/*")
        BaseCommit    = "ead3cf1"
        BaselineTag   = "mcguildlink/v1.0.0"
        LegacySection = @"

## 1.0.0 以前

以前の変更は[旧MCGuildLinkコミット履歴](https://github.com/AnvilSaba/platform/commits/86cc7f2)を参照してください。
"@
    }
}

