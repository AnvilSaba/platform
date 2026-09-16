//! 管理対象リソースごとの差分計算・変換をまとめる内部モジュールです。
//!
//! Discord との通信はここでは行いません。通信を必要とする処理は `port` の
//! Interface を通じて上位のワークフローから受け取ります。

pub(super) mod channel;
pub(super) mod role;

use std::fmt;

/// リソースの属性変更を同じ形式で表示します。
pub(super) fn render_change_line(
    output: &mut String,
    logical_id: impl fmt::Display,
    discord_id: impl fmt::Display,
    attribute: &str,
    current: impl fmt::Display,
    desired: impl fmt::Display,
) {
    output.push_str(&format!(
        "- {logical_id} ({discord_id}) {attribute}: {current} -> {desired}\n"
    ));
}

/// 名前などの文字列を JSON 形式で引用し、改行や引用符を安全に表示します。
pub(super) fn display_quoted_string(value: &str) -> String {
    serde_json::to_string(value).expect("文字列は JSON へ直列化できます")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_quoted_string_escapes_json_syntax() {
        assert_eq!(display_quoted_string("旧\"名\n改行"), "\"旧\\\"名\\n改行\"");
    }

    #[test]
    fn render_change_line_uses_one_display_format() {
        let mut output = String::new();
        render_change_line(&mut output, "role", 100_u64, "name", "旧", "新");
        assert_eq!(output, "- role (100) name: 旧 -> 新\n");
    }
}
