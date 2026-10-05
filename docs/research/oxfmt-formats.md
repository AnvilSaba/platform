# Oxfmt への移行

調査日: 2026-10-06。導入版: 0.71.0。

Node.js と Oxfmt は mise のツールとして管理する。Rust は rustfmt、それ以外の対応形式は Oxfmt を使う（[npm バックエンド](https://mise.jdx.dev/dev-tools/backends/npm.html)、[公式導入手順](https://oxc.rs/docs/guide/usage/formatter/quickstart)）。

## Taplo 設定の移植

| Taplo                           | Oxfmt                         |
| ------------------------------- | ----------------------------- |
| `column_width = 120`            | `printWidth: 120`             |
| `exclude = [ "target/**" ]`     | `ignorePatterns` に移植       |
| `reorder_keys = false`          | TOML キー順を維持する既定動作 |
| `compact_arrays = false`        | 対応する設定なし              |
| `compact_entries = false`       | 対応する設定なし              |
| `compact_inline_tables = false` | 対応する設定なし              |

整形項目は導入版の [JSON スキーマ](https://raw.githubusercontent.com/oxc-project/oxc/oxfmt_v0.71.0/npm/oxfmt/configuration_schema.json) と [公式設定一覧](https://oxc.rs/docs/guide/usage/formatter/config-file-reference) で確認した。コメントアウトされていた依存キー整列ルールは移植しない。

Helm テンプレート・`deploy/rust/about.hbs` は構文解析できないため除外する。SQLx メタデータ・npm ロックファイル・変更履歴も生成元の形式を保つため除外する。

## 出力の比較

oxfmt 0.71.0 と Taplo 0.10.0 で、空・入れ子・複数行の配列、インラインテーブル、括弧を含む文字列を比較した。値とキー順が変わらないことも検証した。

| 入力      | Taplo           | Oxfmt           |
| --------- | --------------- | --------------- |
| `a=[1,2]` | `a = [ 1, 2 ]`  | `a = [1, 2]`    |
| `a=[]`    | `a = [  ]`      | `a = []`        |
| `a={x=1}` | `a = { x = 1 }` | `a = { x = 1 }` |

`bracketSpacing: true` と `false` は TOML に対して同一の出力になった。長い配列の改行・2 空白のインデントも両方で一致した。

[公式設定一覧](https://oxc.rs/docs/guide/usage/formatter/config-file-reference#bracketspacing) でも `bracketSpacing` の対象に TOML は含まれない。[oxc-toml の実装](https://github.com/oxc-project/oxc-toml/blob/main/src/formatter/mod.rs) は配列内の空白を `compact_arrays` で制御するが、この項目は Oxfmt の公開設定にない。Oxfmt 0.72.0 の[変更一覧](https://github.com/oxc-project/oxc/releases/tag/oxfmt_v0.72.0)にも設定追加はない。

従来の TOML 出力を完全に保つには、TOML を Taplo に任せる必要がある。oxfmt への統一を保つ場合は、配列内側の空白の差を受け入れる。

## mise による依存導入

`mise install` の postinstall hook は、ツールが導入済みでも実行される（[公式 Hooks](https://mise.jdx.dev/hooks.html#preinstall-postinstall-hook)）。

`[deps.npm]` の `auto = true` と `run = "npm ci"` を設定し、postinstall から `mise deps` を呼ぶ。入力が変わった場合や出力がなくなった場合だけ導入し、`mise run`・`mise exec` の前にも確認する。現在 experimental のため、その設定を有効にしている（[公式 Deps](https://mise.jdx.dev/dev-tools/deps.html)）。

Windows の oxfmt LSP がネイティブファイルを使用中だったため、毎回の `npm ci` は EPERM になった。破損した依存ディレクトリを `target/oxfmt-locked-node-modules` に保管し、再導入した。通常の再実行では依存管理の更新判定により不要な再導入を省く。

postinstall hook の失敗は mise の警告扱いになる。整形タスクでは自動依存チェックも実行するため、依存導入に失敗するとタスク実行を止める。

Oxfmt は `npm:oxfmt` として導入し、`npm ci` はアプリ依存用に維持する。mise の `[settings]` に `minimum_release_age = "7d"` を指定し、ツール共通の既定値とする。npm バックエンドは推移依存にも制限を適用する。通常の `npm ci` には `.npmrc` の `min-release-age=7` を使う（[mise の設定](https://mise.jdx.dev/configuration/settings.html#minimum-release-age)）。

共通 action の `minimum_release_age: 7d` は、action が取得する mise 本体への設定（[mise-action の入力定義](https://github.com/jdx/mise-action/blob/main/action.yml)）。`install-args: node rust npm:oxfmt` は整形 CI で不要な SQLx CLI の導入を省くための指定。
