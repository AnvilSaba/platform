#!/usr/bin/env -S mise exec -- cargo -Zscript
---cargo
[package]
edition = "2024"

[dependencies]
anyhow = "1"
bpaf = { version = "0.9", features = ["derive"] }
regex = "1"
serde = { version = "1", features = ["derive"] }
toml_edit = { version = "0.25", features = ["serde"] }
---

use std::{
    collections::BTreeMap,
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, Result, bail, ensure};
use bpaf::Bpaf;
use regex::Regex;
use serde::Deserialize;
use toml_edit::{DocumentMut, Item, value};

#[derive(Clone, Debug, Bpaf)]
#[bpaf(options)]
/// リリース準備・変更履歴の生成
enum Options {
    /// 次のバージョンと変更履歴を準備する
    #[bpaf(command)]
    Prepare {
        /// TOML に定義したリリース対象
        #[bpaf(long, argument("APP"))]
        app: String,
        /// auto / major / minor / patch
        #[bpaf(
            long,
            argument("KIND"),
            fallback(String::from("auto")),
            guard(|bump| matches!(bump.as_str(), "auto" | "major" | "minor" | "patch"), "auto / major / minor / patch を指定してください")
        )]
        bump: String,
        /// ファイルを変更せずプレビューする
        #[bpaf(long)]
        dry_run: bool,
    },
    /// 変更履歴だけを生成する
    #[bpaf(command)]
    Changelog {
        /// TOML に定義したリリース対象
        #[bpaf(long, argument("APP"))]
        app: String,
        /// 生成するリリースタグ（省略時は未リリース）
        #[bpaf(long, argument("TAG"))]
        tag: Option<String>,
        /// 出力先（- で標準出力、省略時は設定の CHANGELOG）
        #[bpaf(long, argument("PATH"))]
        output: Option<PathBuf>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    version_file: PathBuf,
    changelog: PathBuf,
    paths: Vec<String>,
    baseline_tag: Option<String>,
    #[serde(default)]
    full_history: bool,
    display_name: String,
}

fn read(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("{} を読み込めません", path.display()))
}

fn run(command: &mut Command) -> Result<String> {
    let program = command.get_program().to_string_lossy().into_owned();
    let output = command
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()
        .with_context(|| format!("{program} を実行できません。mise install と mise exec を使用してください"))?;
    ensure!(output.status.success(), "{program} が失敗しました: {}", output.status);
    Ok(String::from_utf8(output.stdout)?)
}

fn tag_pattern(app: &str) -> Result<Regex> {
    Ok(Regex::new(&format!(
        r"^{}/v[0-9]+\.[0-9]+\.[0-9]+$",
        regex::escape(app)
    ))?)
}

fn baseline<'a>(app: &str, target: &'a Target) -> Result<&'a str> {
    let tag = target
        .baseline_tag
        .as_deref()
        .context("full_history=false には baseline_tag が必要です")?;
    ensure!(
        tag_pattern(app)?.is_match(tag),
        "基準タグ '{tag}' は {app} のタグ形式ではありません"
    );
    Ok(tag)
}

fn cliff(root: &Path, app: &str, target: &Target) -> Result<Command> {
    ensure!(!target.paths.is_empty(), "リリース対象の paths は空にできません");
    let mut command = Command::new("git-cliff");
    command.current_dir(root).args([
        "--config",
        "cliff.toml",
        "--offline",
        "--tag-pattern",
        tag_pattern(app)?.as_str(),
    ]);
    for path in &target.paths {
        command.arg(format!("--include-path={path}"));
    }
    Ok(command)
}

fn changelog(root: &Path, app: &str, target: &Target, tag: Option<&str>) -> Result<String> {
    let mut command = cliff(root, app, target)?;
    if let Some(tag) = tag {
        ensure!(
            tag_pattern(app)?.is_match(tag),
            "タグ '{tag}' は {app} のタグ形式ではありません"
        );
        command.args(["--tag", tag]);
    }
    if target.full_history {
        command.arg(run(Command::new("git").current_dir(root).args(["rev-parse", "HEAD"]))?.trim());
    } else {
        let tag = baseline(app, target)?;
        command.args([
            "--skip-tags",
            &format!("^{}$", regex::escape(tag)),
            &format!("{tag}..HEAD"),
        ]);
    }
    let mut text = run(&mut command)?;
    if !target.full_history {
        // Reuse the git-cliff remote configuration instead of hardcoding the repository.
        #[derive(Deserialize)]
        struct CliffConfig {
            remote: Remote,
        }
        #[derive(Deserialize)]
        struct Remote {
            github: GitHub,
        }
        #[derive(Deserialize)]
        struct GitHub {
            owner: String,
            repo: String,
        }
        let config: CliffConfig = toml_edit::de::from_str(&read(&root.join("cliff.toml"))?)?;
        let remote = config.remote.github;
        let tag = baseline(app, target)?;
        let version = tag
            .strip_prefix(&format!("{app}/v"))
            .context("基準タグにバージョンがありません")?;
        text.push_str(&format!(
            "\n## {version} 以前\n\n以前の変更は[旧{}コミット履歴](https://github.com/{}/{}/commits/{tag})を参照してください。\n",
            target.display_name, remote.owner, remote.repo
        ));
    }
    Ok(text)
}

fn chart_version_pattern() -> Result<Regex> {
    Ok(Regex::new(r"(?m)^version: ([0-9]+\.[0-9]+\.[0-9]+)(\r?)$")?)
}

fn current_version(path: &Path, text: &str) -> Result<String> {
    let version = match path.file_name().and_then(|name| name.to_str()) {
        Some("Cargo.toml") => {
            let manifest = text.parse::<DocumentMut>()?;
            manifest
                .get("package")
                .and_then(|package| package.get("version"))
                .and_then(Item::as_str)
                .context("package.version がありません")?
                .to_owned()
        }
        Some("Chart.yaml") => chart_version_pattern()?
            .captures(text)
            .context("Chart の version がありません")?[1]
            .to_owned(),
        _ => bail!("未対応のバージョンファイルです: {}", path.display()),
    };
    ensure!(
        Regex::new(r"^[0-9]+\.[0-9]+\.[0-9]+$")?.is_match(&version),
        "未対応のバージョンです: {version}"
    );
    Ok(version)
}

fn set_version(item: &mut Item, version: &str) -> Result<()> {
    let decor = item.as_value().context("version が値ではありません")?.decor().clone();
    *item = value(version);
    *item.as_value_mut().context("version が値ではありません")?.decor_mut() = decor;
    Ok(())
}

fn cargo_versions(manifest: &str, lock: &str, next: &str) -> Result<(String, String)> {
    let mut manifest = manifest.parse::<DocumentMut>()?;
    let package = manifest
        .get("package")
        .and_then(Item::as_table)
        .context("package がありません")?;
    let name = package
        .get("name")
        .and_then(Item::as_str)
        .context("package.name がありません")?
        .to_owned();
    let current = package
        .get("version")
        .and_then(Item::as_str)
        .context("package.version がありません")?
        .to_owned();
    let mut lock = lock.parse::<DocumentMut>()?;
    let mut matching = lock["package"]
        .as_array_of_tables_mut()
        .context("Cargo.lock に package がありません")?
        .iter_mut()
        .filter(|entry| entry.get("name").and_then(Item::as_str) == Some(&name) && !entry.contains_key("source"));
    let entry = matching
        .next()
        .context("Cargo.lock に対象のローカルパッケージがありません")?;
    ensure!(
        entry.get("version").and_then(Item::as_str) == Some(&current),
        "Cargo.toml と Cargo.lock のバージョンが一致しません"
    );
    ensure!(
        matching.next().is_none(),
        "Cargo.lock のローカルパッケージが重複しています"
    );
    set_version(&mut entry["version"], next)?;
    drop(matching);
    set_version(&mut manifest["package"]["version"], next)?;
    Ok((manifest.to_string(), lock.to_string()))
}

fn prepare(root: &Path, app: &str, target: &Target, bump: &str, dry_run: bool) -> Result<String> {
    let version_path = root.join(&target.version_file);
    let version_text = read(&version_path)?;
    let current = current_version(&version_path, &version_text)?;
    let tags = run(Command::new("git")
        .current_dir(root)
        .args(["tag", "--list", &format!("{app}/v*")]))?;
    let next = if tags.trim().is_empty() {
        ensure!(
            bump == "auto",
            "初回リリースでは bump 種別を指定できません。現在の v{current} を基準タグとして作成してください"
        );
        eprintln!("{app} には既存タグがないため、初回バージョン v{current} を使用します。");
        current
    } else {
        let mut command = cliff(root, app, target)?;
        command.arg("--bumped-version");
        if bump != "auto" {
            command.args(["--bump", bump]);
        }
        let version = run(&mut command)?;
        let version = version
            .trim()
            .strip_prefix(&format!("{app}/v"))
            .unwrap_or(version.trim())
            .to_owned();
        ensure!(
            Regex::new(r"^[0-9]+\.[0-9]+\.[0-9]+$")?.is_match(&version),
            "git-cliff で次のバージョンを算出できませんでした"
        );
        version
    };
    let tag = format!("{app}/v{next}");
    let status = Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "--verify", "--quiet", &format!("refs/tags/{tag}")])
        .stdout(Stdio::null())
        .status()?;
    ensure!(!status.success(), "タグ {tag} は既に存在します");
    ensure!(status.code() == Some(1), "既存タグの確認に失敗しました: {status}");
    // Generate and validate everything before writing release files.
    let text = changelog(root, app, target, Some(&tag))?;
    if dry_run {
        eprintln!("ドライラン: {tag} を作成予定です。バージョンファイルと変更履歴は更新しません。");
        eprintln!(
            "--- CHANGELOG プレビュー ({}) ---\n{text}--- CHANGELOG プレビュー終了 ---",
            target.changelog.display()
        );
        if let Some(path) = env::var_os("GITHUB_STEP_SUMMARY") {
            writeln!(
                fs::OpenOptions::new().create(true).append(true).open(path)?,
                "## CHANGELOG プレビュー: `{tag}`\n\n{text}"
            )?;
        }
    } else {
        if version_path.file_name().and_then(|name| name.to_str()) == Some("Cargo.toml") {
            let lock_path = root.join("Cargo.lock");
            let (manifest, lock) = cargo_versions(&version_text, &read(&lock_path)?, &next)?;
            fs::write(&version_path, manifest)?;
            fs::write(lock_path, lock)?;
        } else {
            // Only the chart version changes; appVersion keeps its existing meaning.
            let updated = chart_version_pattern()?
                .replace(&version_text, format!("version: {next}${{2}}"))
                .into_owned();
            fs::write(&version_path, updated)?;
        }
        fs::write(root.join(&target.changelog), text)?;
    }
    Ok(tag)
}

fn write_changelog(root: &Path, output: &Path, text: &str, stdout: &mut impl Write) -> Result<()> {
    if output.as_os_str() == "-" {
        stdout.write_all(text.as_bytes())?;
    } else {
        fs::write(root.join(output), text)?;
        writeln!(stdout, "{}", output.display())?;
    }
    Ok(())
}

fn main() -> Result<()> {
    let options = options().run();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("リポジトリルートがありません")?;
    // Target identifiers and release policy live only in this runtime configuration.
    let targets: BTreeMap<String, Target> = toml_edit::de::from_str(&read(&root.join("scripts/release-config.toml"))?)?;
    let app = match &options {
        Options::Prepare { app, .. } | Options::Changelog { app, .. } => app,
    };
    let target = targets.get(app).with_context(|| {
        format!(
            "未対応のリリース対象です: {app}（利用可能: {}）",
            targets.keys().cloned().collect::<Vec<_>>().join(", ")
        )
    })?;
    match &options {
        Options::Prepare { app, bump, dry_run } => println!("{}", prepare(root, app, target, bump, *dry_run)?),
        Options::Changelog { app, tag, output } => {
            let text = changelog(root, app, target, tag.as_deref())?;
            let output = output.as_deref().unwrap_or(&target.changelog);
            write_changelog(root, output, &text, &mut std::io::stdout().lock())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_policy_and_version_updates() -> Result<()> {
        let mut stdout = Vec::new();
        let text = "# 変更履歴\n\n## 未リリース\n";
        write_changelog(Path::new("unused-root"), Path::new("-"), text, &mut stdout)?;
        assert_eq!(stdout, text.as_bytes());
        assert!(write_changelog(Path::new("unused-root"), Path::new("-"), text, &mut &mut [][..]).is_err());
        assert!(matches!(
            options().run_inner(&["changelog", "--app", "new.project", "--output", "-"][..]),
            Ok(Options::Changelog { output: Some(output), .. }) if output.as_os_str() == "-"
        ));
        let targets: BTreeMap<String, Target> = toml_edit::de::from_str(
            r#"
            ["new.project"]
            version_file = "apps/new/Cargo.toml"
            changelog = "apps/new/CHANGELOG.md"
            paths = ["apps/new/**"]
            baseline_tag = "new.project/v1.0.0"
            display_name = "New project"
        "#,
        )?;
        let target = &targets["new.project"];
        assert!(!target.full_history);
        assert_eq!(baseline("new.project", target)?, "new.project/v1.0.0");
        assert!(baseline("other", target).is_err());
        assert!(tag_pattern("new.project")?.is_match("new.project/v1.2.3"));
        assert!(!tag_pattern("new.project")?.is_match("newXproject/v1.2.3"));
        assert!(!tag_pattern("new.project")?.is_match("new.project/v1.2.3-rc.1"));
        assert!(
            options()
                .run_inner(&["prepare", "--app", "new.project", "--bump", "invalid"][..])
                .is_err()
        );
        assert!(
            options()
                .run_inner(&["prepare", "--app", "new.project", "--dry-run"][..])
                .is_ok()
        );
        let manifest =
            "[package]\nname = \"new-package\"\nversion = \"1.2.3\" # keep\n\n[dependencies]\nother = \"1.2.3\"\n";
        let lock = "# keep\nversion = 4\n\n[[package]]\nname = \"new-package\"\nversion = \"1.2.3\"\n\n[[package]]\nname = \"new-package\"\nversion = \"1.2.3\"\nsource = \"registry+https://example.com\"\n";
        let (updated, updated_lock) = cargo_versions(manifest, lock, "2.0.0")?;
        assert_eq!(
            updated,
            manifest.replacen("version = \"1.2.3\"", "version = \"2.0.0\"", 1)
        );
        assert_eq!(
            updated_lock,
            lock.replacen("version = \"1.2.3\"", "version = \"2.0.0\"", 1)
        );
        assert!(cargo_versions(manifest, "version = 4\n", "2.0.0").is_err());
        assert!(cargo_versions(manifest, &lock.replace("1.2.3", "0.1.0"), "2.0.0").is_err());
        assert!(cargo_versions("[package]\nversion = '1.2.3'\n", lock, "2.0.0").is_err());
        assert!(current_version(Path::new("Cargo.toml"), "[dependencies]\n").is_err());
        assert!(current_version(Path::new("Cargo.toml"), "[package]\nname = 'new-package'\n").is_err());
        let config = "[new]\nversion_file='Cargo.toml'\nchangelog='CHANGELOG.md'\npaths=['src/**']\ndisplay_name='New'\ntypo=true\n";
        assert!(toml_edit::de::from_str::<BTreeMap<String, Target>>(config).is_err());
        let chart = "apiVersion: v2\nversion: 1.2.3\nappVersion: \"1.2.3\"\n";
        assert_eq!(current_version(Path::new("Chart.yaml"), chart)?, "1.2.3");
        assert_eq!(
            chart_version_pattern()?.replace(chart, "version: 2.0.0${2}"),
            chart.replacen("version: 1.2.3", "version: 2.0.0", 1)
        );
        let chart_crlf = chart.replace('\n', "\r\n");
        assert_eq!(
            chart_version_pattern()?.replace(&chart_crlf, "version: 2.0.0${2}"),
            chart_crlf.replacen("version: 1.2.3", "version: 2.0.0", 1)
        );
        Ok(())
    }
}
