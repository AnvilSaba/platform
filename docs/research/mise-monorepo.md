# mise monorepo 移行補助調査

対象資料: [Monorepo Tasks](https://mise.jdx.dev/tasks/monorepo.html)、[Task Configuration](https://mise.jdx.dev/tasks/task-configuration.html)。公式サイトの表示版は `v2026.10.3`。主担当が検証する `2026.9.18` と同一版とは確認できないため、以下は資料上の仕様であり、対象版の実挙動を保証しない。

## 確認できたこと

- ルートの `mise.toml` に `monorepo_root = true` と `[monorepo].config_roots` を設定すると、列挙した config root のタスクが共有名前空間に読み込まれ、ルートからの相対パスを付けた `//projects/frontend:build` 形式になる。[Monorepo Tasks](https://mise.jdx.dev/tasks/monorepo.html)
- `config_roots` は glob を受け付ける。`*` は一階層だけ（例: `services/*`）で、再帰的な `**` は資料上サポートされない。[Monorepo Tasks](https://mise.jdx.dev/tasks/monorepo.html)
- 子タスクから root タスクへは、依存タスク名に `//:db:test:up` のような絶対パス形式を指定できると読める。資料は `//` が monorepo root 起点の絶対パスであること、`depends` が依存タスクを表すことを示している。[Monorepo Tasks](https://mise.jdx.dev/tasks/monorepo.html)、[Task Configuration](https://mise.jdx.dev/tasks/task-configuration.html)
- root の `[tools]` と `[env]` は子 config に継承され、子側の値が同じキーを上書きし、新しい値を追加する。`[vars]` も同じ階層をたどり、子 config の vars は子タスクのテンプレートで利用できる。[Monorepo Tasks](https://mise.jdx.dev/tasks/monorepo.html)
- `[task_config]` は通常その config file のタスク範囲に適用される。`cascade = true` で descendant config root に継承され、子の値が個別フィールドを上書きする。`shell` もこの規則に従い、タスク自身の明示的な `shell` が優先される。[Task Configuration](https://mise.jdx.dev/tasks/task-configuration.html)

## `-E` と版差

- 指定された2資料内では、`-E` という CLI オプションの説明や、`-E` による環境継承の挙動を確認できなかった。したがって「root の環境を `-E` で継承できる」という主張は、この調査の一次資料だけでは確定できない。`2026.9.18` で主担当のローカル検証が必要。
- 資料には、タスクの `env` はそのタスク固有で、`depends` タスクへは渡らないという記載がある。また `deny_env`/`deny_all` などの環境サンドボックス設定も別途存在する。[Task Configuration](https://mise.jdx.dev/tasks/task-configuration.html)
- `monorepo_root`、`config_roots`、root の tools/vars/env、`task_config.cascade`/`shell` の上記記述は現行資料の内容であり、`2026.9.18` での導入時期・差分は資料から判定できない。差分確認には主担当の対象版実行結果が必要。

## ローカル検証

Windows の mise `2026.9.18` で以下を確認した。

- `config_roots = ["apps/*", "crates/*", "deploy/helm/platform"]` で全8プロジェクトを検出し、各ディレクトリで `mise tasks validate` が成功した。ルートでの `validate` は子タスクを読み込まないため、個別に実行した。
- `task_config.cascade = true` により、子タスクの shell はルートと同じ `cmd /d /c` になった。親の Rust、Node.js、Oxfmt、SQLx CLI も子ディレクトリで有効だった。
- 子ディレクトリのさらに配下から `:test` を実行でき、`//:db:test:up` はルートの作業ディレクトリで実行された。DB Migrator のDBテストと platform-signal のテストが成功した。
- Docker／Podman の `-E` 指定は、子タスクから依存するルートDBタスクにも適用された。dry-run でコンテナエンジン、ルートのスクリプト・マウントパス、Cargo 引数の受け渡しを確認した。
- 全アプリ起動タスクの dry-run では、DB起動・マイグレーションの後に3アプリを起動する順序を確認した。
- 全7アプリ・クレートの `check`、Helm の lint・テンプレート生成、`mise :fmt` と `mise :fmt:check` が成功した。

Docker／Podman での実起動と全アプリの常駐起動は未実行。DBテストの実行には既定の WSL Container CLI を使用した。
