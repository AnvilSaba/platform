#!/usr/bin/env -S cargo -Zscript
---cargo
[package]
edition = "2024"

[dependencies]
anyhow = "1"
regex = "1"
toml_edit = "0.25"
---

use std::fs;

use anyhow::{Context, Result, ensure};
use regex::Regex;
use toml_edit::{DocumentMut, Item};

fn builder_image(source: &str) -> Result<String> {
    let document = source.parse::<DocumentMut>()?;
    let channel = document
        .get("toolchain")
        .and_then(|toolchain| toolchain.get("channel"))
        .and_then(Item::as_str)
        .context("rust-toolchain.toml の toolchain.channel に文字列を指定してください")?;
    ensure!(
        Regex::new(r"\Anightly(?:-[0-9]{4}-[0-9]{2}-[0-9]{2})?\z")?.is_match(channel),
        "対応するイメージがありません。nightly または nightly-YYYY-MM-DD を指定してください"
    );
    Ok(format!(
        "rustlang/rust:{}",
        channel.replacen("nightly", "nightly-trixie", 1)
    ))
}

fn main() -> Result<()> {
    let source = fs::read_to_string("rust-toolchain.toml").context("rust-toolchain.toml を読み込めません")?;
    println!("{}", builder_image(&source)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_builder_images_and_rejects_unsupported_channels() {
        for (channel, expected) in [
            ("nightly-2026-09-25", Some("rustlang/rust:nightly-trixie-2026-09-25")),
            ("nightly", Some("rustlang/rust:nightly-trixie")),
            ("stable", None),
            ("1.99", None),
            ("1.99.1", None),
            ("beta", None),
            ("stable-2026-09-25", None),
            ("nightly-2026-09", None),
            ("custom-toolchain", None),
            ("nightly-2026-09-25\nimage=unexpected", None),
            ("1.99.1\nimage=unexpected", None),
        ] {
            let source = format!("[toolchain]\nchannel = {channel:?}\n");
            let image = builder_image(&source);
            match expected {
                Some(expected) => assert_eq!(image.unwrap(), expected),
                None => assert!(image.is_err(), "{channel:?}"),
            }
        }
    }

    #[test]
    fn rejects_invalid_toolchain_files() {
        for source in ["[", "[toolchain]", "[toolchain]\nchannel = 1"] {
            assert!(builder_image(source).is_err(), "{source:?}");
        }
    }
}
