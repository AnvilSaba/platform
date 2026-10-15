#!/usr/bin/env -S mise exec -- cargo -Zscript
---cargo
[package]
edition = "2024"

[dependencies]
anyhow = "1"
heck = "0.5"
regex = "1"
yaml-edit = "0.3.2"
---

use std::{env, fs, process::Command};

use anyhow::{Context, Result, ensure};
use heck::ToLowerCamelCase;
use regex::Regex;
use yaml_edit::{YamlFile, path::YamlPath};

fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    ensure!(
        matches!(args.len(), 2 | 3),
        "使い方: promote-production.rs <対象> <リリースタグ> [Migrator タグ]"
    );
    let target = &args[0];
    let release_ref = &args[1];
    let pattern = Regex::new(r"^([a-z][a-z0-9]*(?:-[a-z0-9]+)*)/v([0-9]+\.[0-9]+\.[0-9]+)$")?;
    let release = pattern
        .captures(release_ref)
        .context("リリースタグの形式が不正です。")?;
    ensure!(&release[1] == target, "デプロイ対象とリリースタグが一致しません。");
    let output = Command::new("git")
        .args(["rev-parse", "--verify", &format!("refs/tags/{release_ref}^{{commit}}")])
        .output()
        .context("リリースタグを確認できません。")?;
    ensure!(output.status.success(), "リリースタグを解決できません: {release_ref}");
    let revision = String::from_utf8(output.stdout)?;
    let path = if target == "chart" {
        "deploy/flux/apps/chart.yaml"
    } else {
        "deploy/flux/apps/values.yaml"
    };
    let config = YamlFile::from_path(path)?;
    ensure!(
        config.documents().count() == 1,
        "設定は単一の YAML ドキュメントにしてください。"
    );
    let document = config.document().context("設定の YAML ドキュメントがありません。")?;
    if target == "chart" {
        document.try_set_path("spec.ref.tag", &release[2])?;
    } else {
        let image_path = format!("{}.image.tag", target.to_lower_camel_case());
        document
            .try_get_path(&image_path)
            .with_context(|| format!("本番設定に対象のイメージタグがありません: {target}"))?;
        let expected_tag = format!("sha-{}", revision.trim());
        if let Some(migrator_tag) = args.get(2).filter(|tag| !tag.is_empty()) {
            ensure!(
                migrator_tag == &expected_tag,
                "Migrator とアプリの Git revision が一致しません。"
            );
        }
        document.try_set_path(&image_path, &release[2])?;
        document.try_set_path("dbMigrator.enabled", true)?;
        document.try_set_path("dbMigrator.image.tag", expected_tag.as_str())?;
    }
    fs::write(path, config.to_string())?;
    Ok(())
}
