#!/usr/bin/env -S mise exec -- cargo -Zscript
---cargo
[package]
edition = "2024"

[dependencies]
anyhow = "1"
---

use std::process::Command;

use anyhow::{Result, ensure};

fn main() -> Result<()> {
    let chart = concat!(env!("CARGO_MANIFEST_DIR"), "/../deploy/helm/platform");
    let components = ["bot", "mcLinkServer", "publicApi", "dbMigrator"];
    let tags: Vec<String> = components
        .iter()
        .flat_map(|component| ["--set-string".into(), format!("{component}.image.tag=legacy")])
        .collect();
    let render = |extra: &[String]| {
        Command::new("helm")
            .args(["template", "platform", chart])
            .args(&tags)
            .args(extra)
            .output()
    };
    let digest = format!("sha256:{}", "a".repeat(64));
    let mut args = vec!["--set".into(), "dbMigrator.enabled=true".into()];
    args.extend(
        components
            .iter()
            .flat_map(|component| ["--set-string".into(), format!("{component}.image.digest={digest}")]),
    );
    let output = render(&args)?;
    ensure!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let output = String::from_utf8(output.stdout)?;
    for component in ["bot", "mc-link-server", "public-api", "db-migrator"] {
        ensure!(
            output.contains(&format!("image: \"ghcr.io/anvilsaba/{component}@{digest}\"")),
            "digest が反映されません: {component}"
        );
    }
    ensure!(
        output.contains("helm.sh/hook: pre-install,pre-upgrade") && output.contains("helm.sh/hook-weight: \"0\""),
        "migration hook が変わっています"
    );
    let legacy = render(&[])?;
    ensure!(legacy.status.success());
    let legacy = String::from_utf8(legacy.stdout)?;
    ensure!(
        legacy.contains("ghcr.io/anvilsaba/bot:legacy") && !legacy.contains("kind: Job"),
        "tag 互換性または Migrator 無効時の動作が変わっています"
    );
    for settings in [
        vec!["--set-string", "bot.image.tag="],
        vec!["--set-string", "bot.image.digest=sha256:bad"],
        vec![
            "--set",
            "dbMigrator.enabled=true",
            "--set-string",
            "dbMigrator.image.tag=",
        ],
    ] {
        ensure!(
            !render(&settings.into_iter().map(String::from).collect::<Vec<_>>())?
                .status
                .success(),
            "不正な image 参照を受け付けました"
        );
    }
    println!("digest・既存 tag・Migrator の描画検証に成功しました。");
    Ok(())
}
