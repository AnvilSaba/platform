# 開発・個別テスト・統合テスト手順

Bot・MC Link Server・公開 API の開発と検証を説明します。当面は `mise :up` による擬似的な統合テストを使います。

## 1. 開発環境に必要なもの

ローカルで各アプリを個別に検証し、全体の動作確認では mise で DB とアプリを起動します。

- [mise](https://mise.jdx.dev/getting-started.html)
- WSL Container CLI(切り替え可)
- Docker Desktop または Podman(DB の起動先を切り替える場合)
- Git

### ローカル開発

リポジトリルートで実行します。

```powershell
mise trust
mise install
```

`mise :fmt` で 対応ファイルを整形し、`mise :fmt:check` で検査します。

DB は WSL Container CLI/Podman/Docker の `run`・`start`・`exec` を直接使用して管理します。
既定は `wslc` を使用します。引数で Podman/Docker に切り替えられます。
ローカル DB の初期化 SQL は `dev/postgres/` にまとめています。
`bootstrap.sql` でロールと権限を用意し、`passwords.sql` で開発用パスワードを設定します。
開発用・テスト用 DB の起動待ちには同じディレクトリの `wait.sh` を使います。

旧 `deploy/postgres/` をマウントした開発用コンテナがある場合は、アプリを停止して次を実行します。
データ用 volume は保持し、SQL の参照元パスを更新するためにコンテナだけを再作成します。以下は既定の `wslc` の例です。

```powershell
mise :down
mise exec -- wslc rm platform-local-postgres
mise :db:up
```

```powershell
mise :test                  # WSL Container CLI(既定)
mise -E podman :test        # Podman
mise -E docker :test        # Docker
mise //apps/public-api:run
mise :db:test:down
```

`mise :up` で開発用 DB の起動・マイグレーション後、3アプリを並列実行します。
実行中はログが流れ続けます。Ctrl+C でアプリを終了し、`mise :down` で DB を停止します。
Bot は `apps/bot/config.sample.toml` を `apps/bot/config.toml` にコピーして開発用設定を用意します。
ローカル用 Token と認証の合言葉は `apps/bot/.env` に保存します。Bot の `run` タスクは mise の [dotenv 読み込み](https://mise.jdx.dev/environments/) で `DISCORD_BOT_TOKEN` と `AUTH_KEYWORD` を取得します。
既存の `.env` がなければ次を実行し、`.env` の置換用文字列を開発用 Token と合言葉に変更してください。`.env` は Git とコンテナのビルド対象から除外しています。

```powershell
Copy-Item apps/bot/.env.example apps/bot/.env
```

本番用 Token と合言葉は [infrastructure](https://github.com/AnvilSaba/infrastructure/blob/main/docs/secret-config.md) で管理します。既存 `config.toml` の `bot.token` と `auth.keyword` は `.env` へ移して削除します。
`apps/bot` で `mise exec -- cargo run --locked -p bot -- --check-config` を実行すると、Token・合言葉・DB 接続なしで通常設定だけを検証できます。

### GitHub Actions の依存関係

[gh-actions-lock](https://github.com/github/gh-actions-lock) で依存関係を固定します。ワークフローと composite action ではバージョンタグを指定し、同じリポジトリのアクションは `$/` 形式で参照します。

```powershell
mise exec -- gh extension install github/gh-actions-lock --pin v0.1.6
mise exec -- gh actions-lock
mise exec -- gh actions-lock --verify-local
```

ワークフローやアクションの `uses` を変更したら `mise exec -- gh actions-lock` を再実行し、生成されたロックファイルも変更に含めます。ロックファイルは手で編集しません。既存のブランチ・部分バージョン参照を最新コミットへ更新する場合は `mise exec -- gh actions-lock --relock`、上流との整合性を再検証する場合は `mise exec -- gh actions-lock --verify` を使用します。

### マイグレーションの追加と SQL の検証

追加は `mise exec -- sqlx migrate add -r migration_name` で行い、生成された up/down を編集します。`migration_name` は変更内容に合う名前に置き換えてください。適用済みのファイルは変更しません。

SQL・スキーマ変更後は、リポジトリルートで次を実行し、更新された `.sqlx/` もコミットします。DB 起動・マイグレーションは自動です。

```powershell
mise :sqlx:prepare
mise :sqlx:check
```

適用だけなら `mise :db:migrate` を使います。互換性・切り戻しは [リリース手順](releases.md#マイグレーションの互換性) を参照してください。
`mise exec -- sqlx migrate revert` は空の開発用 DB でのみ使用し、`DATABASE_URL` をその DB のマイグレーション専用ユーザーに設定します。初期 down はスキーマと保存データを削除するため、データを保持する切り戻しには使いません。

## 2. 個別テスト

### 2.1 Rust

`check`/`test` はルートで実行するとワークスペース全体を対象にします。
個別のアプリ・クレートには `//<ディレクトリ>:check`/`//<ディレクトリ>:test` を使用します。

全体の一覧は `mise tasks ls --all`、現在のディレクトリのタスクは `mise tasks ls` で確認できます。
テスト名や `--test` などの Cargo 引数は `--` の後に渡せます。

```powershell
mise //apps/bot:check
mise -E podman //apps/public-api:test -- --test whitelist
mise //crates/platform-signal:test
```

アプリ・クレートのディレクトリやその配下では、`:check`/`:test`/`:run` でそのプロジェクトのタスクを実行できます。
ルートのタスクを指定する場合は `//:fmt:check` のように書きます。

```powershell
cd apps/public-api
mise :check
mise -E docker :test -- --test whitelist
mise //:fmt:check
```

パスの `...` は配下のプロジェクトに一致します。次の例は、リポジトリ内のどのディレクトリからでも実行できます。

```powershell
mise //apps/...:check
mise //crates/...:test
```

### 2.2 Helm Chart

```powershell
mise //deploy/helm/platform:check
```

開発・統合確認用の values は `dev/helm/values.dev.yaml` と `dev/helm/values.integration.yaml` に置いています。
Chart に適用する場合は `mise exec -- helm template platform deploy/helm/platform -f dev/helm/values.dev.yaml` のように指定します。

## 3. 擬似的な統合テスト

開発用 DB と 3 アプリを起動し、アプリ間の連携を確認します。Kubernetes 上のデプロイや Cloudflare Tunnel は検証対象に含みません。

```powershell
mise :up
```

- Bot・MC Link Server・公開 API が正常起動し、Bot が開発用 Discord サーバーへ接続できる
- Discord でコードを発行し、Minecraft から `localhost:25565` へ接続して紐付けできる
- `http://localhost:8080/whitelist.json` に紐付けが反映される
- DB とアプリの再起動後も、紐付け・ブロック・未使用コードが保持される

実 Minecraft 接続は [MC Link Server の確認項目](../apps/mc-link-server/README.md) に従います。
開発用 DB を停止したときの公開 API の 503 と、再起動後の 200 も確認します。

Ctrl+C でアプリを終了し、DB を停止します。データは保持されます。

```powershell
mise :down
```
