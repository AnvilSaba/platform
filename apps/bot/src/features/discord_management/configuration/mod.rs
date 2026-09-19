//! 希望構成と対応 state を扱う共有モジュールです。
//!
//! このモジュールは TOML/JSON の形式を Discord や Poise から分離します。
//! `export`・`bind`・`plan`・`apply` はここで検証済みの入力を共有します。

use super::domain::{ManagementError, SCHEMA_VERSION};

mod model;

pub(crate) use model::*;
