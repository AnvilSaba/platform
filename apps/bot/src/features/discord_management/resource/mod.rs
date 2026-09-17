//! 管理対象リソースごとの差分計算・変換をまとめる内部モジュールです。
//!
//! Discord との通信はここでは行いません。通信を必要とする処理は `port` の
//! Interface を通じて上位のワークフローから受け取ります。

pub(super) mod channel;
pub(super) mod role;

use std::{collections::BTreeSet, fmt};

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

/// 現在の順序を tie-break に使い、列挙対象の順序と省略対象の相互順序を同時に
/// 満たす全体順序を返します。
///
/// 参照専用対象は `fixed` に含めます。固定対象の現在位置をまたぐ要求は循環した
/// 制約になるため、通信前にエラーとして診断できます。
pub(super) fn stable_relative_order<I>(
    current: &[I],
    requested: &[I],
    fixed: &BTreeSet<I>,
) -> Result<Vec<I>, ()>
where
    I: Copy + Ord,
{
    let mut current_indices = std::collections::BTreeMap::new();
    for (index, id) in current.iter().copied().enumerate() {
        if current_indices.insert(id, index).is_some() {
            return Err(());
        }
    }
    let requested_set = requested.iter().copied().collect::<BTreeSet<_>>();
    if requested_set.len() != requested.len() || requested.iter().any(|id| !current_indices.contains_key(id)) {
        return Err(());
    }

    let mut edges = vec![Vec::<usize>::new(); current.len()];
    let mut indegree = vec![0_usize; current.len()];
    let mut add_edge = |from: usize, to: usize| {
        if from == to || edges[from].contains(&to) {
            return;
        }
        edges[from].push(to);
        indegree[to] += 1;
    };

    for pair in requested.windows(2) {
        add_edge(current_indices[&pair[0]], current_indices[&pair[1]]);
    }
    let omitted = current
        .iter()
        .copied()
        .filter(|id| !requested_set.contains(id))
        .collect::<Vec<_>>();
    for pair in omitted.windows(2) {
        add_edge(current_indices[&pair[0]], current_indices[&pair[1]]);
    }

    for (anchor_index, anchor) in current.iter().copied().enumerate() {
        if !fixed.contains(&anchor) {
            continue;
        }
        for before in 0..anchor_index {
            add_edge(before, anchor_index);
        }
        for after in (anchor_index + 1)..current.len() {
            add_edge(anchor_index, after);
        }
    }

    let mut ready = BTreeSet::new();
    for (index, degree) in indegree.iter().copied().enumerate() {
        if degree == 0 {
            ready.insert(index);
        }
    }
    let mut ordered_indices = Vec::with_capacity(current.len());
    while let Some(index) = ready.pop_first() {
        ordered_indices.push(index);
        for next in edges[index].iter().copied() {
            indegree[next] -= 1;
            if indegree[next] == 0 {
                ready.insert(next);
            }
        }
    }
    (ordered_indices.len() == current.len())
        .then(|| ordered_indices.into_iter().map(|index| current[index]).collect())
        .ok_or(())
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
