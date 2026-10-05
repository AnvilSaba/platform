# 開発・個別テスト・統合テスト手順

Bot・MC Link Server・公開 API の開発と検証を説明します。通常のローカル開発は mise、クラスタの統合テストは k3d を使います。

## 1. 開発環境に必要なもの

開発時はWindows上で各アプリを個別に検証し、全体の統合テストではDocker DesktopまたはPodman上のk3dを使用します。

- WSL Container CLI（切り替え可）
- Docker DesktopまたはPodman（切り替え・k3d 統合テストに使用）
- Helm
- kubectl
- k3d（Kubernetes統合テストを行う場合）
- Git

本番用のDiscord token、Cloudflare Tunnel token、データベースの本番passwordは開発環境へ持ち込まないでください。

### Rust ツールチェーン

workspace は SQLx `0.9` を指定し、ライブラリと CLI を `0.9.0` に揃えます。
各利用クレートの `build.rs` は `cargo:rerun-if-changed=../../migrations` を出力し、
Rust ソースを変更せずに新しいマイグレーションを追加した場合も再ビルドします。
これは [SQLx 0.9.0 の公式資料](https://docs.rs/sqlx/0.9.0/sqlx/macro.migrate.html#stable-rust-cargo-build-script)
にある Stable Rust の Cargo build script による検知方法です。

Nightly は、ReFS の増分コンパイル修正、Cargo の `min-publish-age`、rustfmt の `group_imports` も利用するため固定しています。
GitHub Actions の SQLx CLI も `mise.toml` から導入するため、CLI のバージョン指定は `mise.toml` に集約します。

### ローカル開発

リポジトリルートで実行します。

```powershell
mise trust
mise install
```

DB は WSL Container CLI／Podman／Docker の `run`・`start`・`exec` を直接使用して管理します。
既定は `wslc` を使用します。引数で Podman／Docker に切り替えられます。

```powershell
mise run test                 # WSL Container CLI（既定）
mise -E podman run test        # Podman
mise -E docker run test        # Docker
mise run run:public-api
mise run db:test:down
```

`mise run up` で開発用 DB の起動・マイグレーション後、3アプリを並列実行します。
実行中はログが流れ続けます。Ctrl+C でアプリを終了し、`mise run down` で DB を停止します。
Bot は `apps/bot/config.toml` に開発用設定が必要です。
`mise -E podman run up`／`mise -E docker run up` で DB の起動先を切り替えられます。

### マイグレーションの追加と SQL の検証

追加は `mise exec -- sqlx migrate add -r migration_name` で行い、生成された up/down を編集します。`migration_name` は変更内容に合う名前に置き換えてください。適用済みのファイルは変更しません。

SQL・スキーマ変更後は、リポジトリルートで次を実行し、更新された `.sqlx/` もコミットします。DB 起動・マイグレーションは自動です。

```powershell
mise run sqlx:prepare
mise run sqlx:check
```

適用だけなら `mise run db:migrate` を使います。互換性・切り戻しは [リリース手順](releases.md#migration-の互換性) を参照してください。
`mise exec -- sqlx migrate revert` は空の開発用 DB でのみ使用し、`DATABASE_URL` をその DB のマイグレーション専用ユーザーに設定します。初期 down はスキーマと保存データを削除するため、データを保持する切り戻しには使いません。

## 2. 個別テスト

### 2.1 Rust

リポジトリルートで実行します。`check`／`test` はワークスペース全体、`check:<クレート名>`／`test:<クレート名>` は個別のアプリ・クレートを対象にします。

一覧は `mise tasks ls` で確認できます。DB が必要なテストはテスト用 DB を自動起動し、`DATABASE_URL` を設定します。
テスト名や `--test` などの Cargo 引数は `--` の後に渡せます。

```powershell
mise run check:bot
mise -E podman run test:public-api -- --test whitelist
```

### 2.2 Helm Chart

```powershell
mise run check:helm
```

### 2.3 コンテナイメージ

Rust アプリは共通の Dockerfile でビルドします。

```powershell
podman machine start
foreach ($package in @('bot', 'mc-link-server', 'public-api', 'db-migrator')) {
  podman build --file deploy/rust/Dockerfile --build-arg TARGETARCH=amd64 `
    --build-arg "PACKAGE=$package" --build-arg "BINARY=$package" `
    --build-arg "SOURCE_DIR=apps/$package" --tag "localhost/anvilsaba/${package}:test" .
  if ($LASTEXITCODE -ne 0) { throw "$package のビルドに失敗しました" }
}
```

Bot の設定構文を確認します。

```powershell
podman run --rm `
  --env DATABASE_URL=postgres://platform_bot:bot-dev-password@postgres:5432/platform `
  --volume "${PWD}/apps/bot/config.sample.toml:/app/config.toml:ro" `
  localhost/anvilsaba/bot:test --check-config
```

## 3. 統合テスト環境のセットアップ

k3d はコンテナ内で k3s を動かす開発用ツールです。Windows では Docker Desktop または Podman を使用します。

```powershell
k3d cluster create anvilsaba
kubectl config current-context
kubectl get nodes
kubectl create namespace anvilsaba --dry-run=client -o yaml | kubectl apply -f -
```

以降は context が `k3d-anvilsaba` になっている状態で実行します。
通常の統合環境では `values.integration.yaml` を使い、Bot と Cloudflare Tunnel は停止します。

```powershell
k3d image import localhost/anvilsaba/mc-link-server:test localhost/anvilsaba/public-api:test localhost/anvilsaba/db-migrator:test -c anvilsaba
kubectl -n anvilsaba create secret generic postgres-db-credentials `
  --from-literal=password=dev-password --dry-run=client -o yaml | kubectl apply -f -
helm upgrade --install platform deploy/helm/platform -n anvilsaba `
  -f deploy/helm/platform/values.integration.yaml --set mcLinkServer.replicas=0 `
  --set publicApi.replicas=0 --set dbMigrator.enabled=false --timeout 10m
kubectl rollout status statefulset/postgres -n anvilsaba --timeout=5m
Get-Content deploy/postgres/bootstrap.sql -Raw | kubectl exec -i -n anvilsaba postgres-0 -- psql -U platform_admin -d platform -v ON_ERROR_STOP=1
Get-Content deploy/postgres/local-passwords.sql -Raw | kubectl exec -i -n anvilsaba postgres-0 -- psql -U platform_admin -d platform -v ON_ERROR_STOP=1
```

`local-passwords.sql` は開発専用です。新規のテスト DB だけに適用し、本番には使用しません。
DB ロールと同じパスワードをアプリ・Migrator ごとの Secret に登録します。

```powershell
$databaseSecrets = @{
  'bot-db-credentials' = 'bot-dev-password'
  'mc-link-server-db-credentials' = 'mc-dev-password'
  'public-api-db-credentials' = 'api-dev-password'
  'db-migrator-db-credentials' = 'migrator-dev-password'
}
foreach ($secret in $databaseSecrets.GetEnumerator()) {
  kubectl -n anvilsaba create secret generic $secret.Key `
    --from-literal="password=$($secret.Value)" --dry-run=client -o yaml | kubectl apply -f -
}
helm upgrade platform deploy/helm/platform -n anvilsaba `
  -f deploy/helm/platform/values.integration.yaml --set mcLinkServer.replicas=0 `
  --set publicApi.replicas=0 --set dbMigrator.enabled=true --timeout 10m
kubectl logs -n anvilsaba job/db-migrator
```

Helm が Migrator の正常完了を確認した後にアプリを起動します。Job が失敗した場合は起動へ進まず、ログと DB 履歴を確認します。

```powershell
helm upgrade platform deploy/helm/platform -n anvilsaba `
  -f deploy/helm/platform/values.integration.yaml --set dbMigrator.enabled=false --timeout 10m
kubectl rollout status deployment/mc-link-server -n anvilsaba --timeout=5m
kubectl rollout status deployment/public-api -n anvilsaba --timeout=5m
```

実 Discord の確認時のみ、開発専用設定を `bot-config` Secret の `config.toml` キーへ登録します。
Bot イメージを `k3d image import localhost/anvilsaba/bot:test -c anvilsaba` で読み込み、Helm upgrade に `--set bot.image.repository=localhost/anvilsaba/bot --set bot.replicas=1` を追加します。

## 4. 統合テスト手順

```powershell
kubectl get pods,deploy,statefulset,service,pvc -n anvilsaba
kubectl get events -n anvilsaba --sort-by=.lastTimestamp
kubectl logs -n anvilsaba deployment/mc-link-server --tail=100
kubectl logs -n anvilsaba deployment/public-api --tail=100
kubectl run curl-test -n anvilsaba --rm --restart=Never -i `
  --image=curlimages/curl:8.15.0 `
  -- http://public-api:8080/whitelist.json
```

起動対象の Pod が Ready になり、継続的な再起動がないことを確認します。
公開 API は ClusterIP を維持し、通常はクラスタ内部から検証します。
PostgreSQL と各アプリの再起動後も、紐付け・ブロック・未使用コードが保持されることを確認します。
テスト環境で PostgreSQL を一時停止し、公開 API の503と復旧後の200も確認します。
実 Minecraft 接続は [MC Link Server の確認項目](../apps/mc-link-server/README.md) に従います。
開発専用 Tunnel を確認する場合は、専用 token を `cloudflare-tunnel` Secret の `token` キーへ登録し、`--set cloudflared.replicas=1` で起動します。
Cloudflare の Service URL を `http://public-api:8080` に設定し、開発用 hostname から `/whitelist.json` の200と既存 JSON 形式を確認します。
Minecraft 接続には開発用 `mc-link-server` Service の公開アドレスと `25565/TCP` を使用します。

## 5. テスト環境の削除

開発用クラスタは必要なときに削除して作り直せます。

```powershell
k3d cluster delete anvilsaba
```
