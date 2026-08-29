# 開発・個別テスト・統合テスト手順

## 1. 開発環境に必要なもの

開発時はWindows上で各アプリを個別に検証し、全体の統合テストではDocker DesktopまたはPodman上のk3dを使用します。

- Rust/Cargo（`rust-toolchain.toml`に合わせる）
- JDK 25
- Docker DesktopまたはPodman
- Helm
- kubectl
- k3d（Kubernetes統合テストを行う場合）
- Git

本番用のDiscord token、Cloudflare Tunnel token、データベースの本番passwordは開発環境へ持ち込まないでください。

## 2. 個別テスト

### 2.1 Rust / Bot

リポジトリルートで実行します。

```powershell
cargo check --workspace --locked --all-targets
cargo test --workspace --locked
cargo build --package bot --locked
```

### 2.2 MCGuildLink

```powershell
Push-Location apps/mcguildlink
.\gradlew.bat --no-daemon test
.\gradlew.bat --no-daemon installDist
Pop-Location
```

### 2.3 Helm Chart

```powershell
helm lint deploy/helm/platform `
  --set bot.image.tag=test `
  --set mcguildlink.image.tag=test

helm template platform deploy/helm/platform `
  --namespace anvilsaba `
  --set bot.image.tag=test `
  --set mcguildlink.image.tag=test | Out-Null
```

### 2.4 コンテナイメージ

```powershell
podman machine start

podman build --file apps/bot/Dockerfile `
  --tag localhost/anvilsaba/bot:test .

podman build --file apps/mcguildlink/Dockerfile `
  --tag localhost/anvilsaba/mcguildlink:test .
```

Botの設定構文を確認します。

```powershell
podman run --rm `
  --volume "${PWD}/apps/bot/config.sample.toml:/app/config.toml:ro" `
  localhost/anvilsaba/bot:test --check-config
```

MCGuildLinkの起動ファイルを確認します。

```powershell
podman run --rm --entrypoint /runtime/bin/java `
  localhost/anvilsaba/mcguildlink:test --version
```

## 3. 統合テスト環境のセットアップ

### 3.1 k3dクラスタを作成

k3dはコンテナ内でk3sを動かす開発用ツールです。WindowsではDocker DesktopまたはPodmanの利用を前提にします。

```powershell
k3d cluster create anvilsaba
kubectl get nodes
```

以降のコマンドは、`kubectl`のcontextが`k3d-anvilsaba`になっている状態で実行します。

```powershell
kubectl config current-context
```

### 3.2 イメージをクラスタへ読み込む

```powershell
k3d image import localhost/anvilsaba/bot:test -c anvilsaba
k3d image import localhost/anvilsaba/mcguildlink:test -c anvilsaba
```

### 3.3 開発用Secretを作成

サンプル設定を使用します。DiscordやCloudflareへの接続を実際に確認する場合は、開発専用のtokenを用意してください。

```powershell
kubectl create namespace anvilsaba --dry-run=client -o yaml | kubectl apply -f -

kubectl -n anvilsaba create secret generic bot-config `
  --from-file=config.toml=apps/bot/config.sample.toml `
  --dry-run=client -o yaml | kubectl apply -f -

kubectl -n anvilsaba create secret generic mcguildlink-config `
  --from-file=app.toml=apps/mcguildlink/app/config/app.example.toml `
  --dry-run=client -o yaml | kubectl apply -f -

kubectl -n anvilsaba create secret generic postgres `
  --from-literal=password=dev-password `
  --dry-run=client -o yaml | kubectl apply -f -

kubectl -n anvilsaba create secret generic cloudflare-tunnel `
  --from-literal=token=dev-token `
  --dry-run=client -o yaml | kubectl apply -f -
```

### 3.4 Helmでデプロイ

開発時の標準構成ではCloudflare Tunnelを使用しません。クラスタ内部のServiceへ直接アクセスして、アプリケーション間の連携を確認します。`cloudflared`を含めた構成を検証する場合だけ、開発専用のTunnel tokenを使用してください。

```powershell
helm upgrade --install platform deploy/helm/platform `
  --namespace anvilsaba `
  --create-namespace `
  -f deploy/helm/platform/values.dev.yaml
```

## 4. 統合テスト手順

### 4.1 Tunnelを使わない通常の統合テスト

開発時は、次の内部経路を標準とします。

```text
テスト用Pod
  -> http://mcguildlink-http:8080
  -> MCGuildLink
```

この方法では外部公開が発生せず、Kubernetes内部のService、MCGuildLink、データベース、Whitelist JSON生成を確認できます。通常の開発・統合テストではCloudflare Tunnelを使用しないでください。

### 4.2 リソースとログ

```powershell
kubectl get pods,deploy,statefulset,service,pvc -n anvilsaba
kubectl get events -n anvilsaba --sort-by=.lastTimestamp
kubectl logs -n anvilsaba deployment/bot --tail=100
kubectl logs -n anvilsaba deployment/mcguildlink --tail=100
```

全PodがReadyになり、CrashLoopBackOffや継続的な再起動がないことを確認します。

### 4.3 Cluster内HTTP

```powershell
kubectl run curl-test -n anvilsaba --rm --restart=Never -i `
  --image=curlimages/curl:8.15.0 `
  -- http://mcguildlink-http:8080/whitelist.json
```

`mcguildlink-http`がClusterIPであることも確認します。

```powershell
kubectl get service mcguildlink-http -n anvilsaba -o wide
```

### 4.4 永続化

MCGuildLinkの再起動後もSQLiteのデータが残ること、PostgreSQLの再起動後もデータが残ることを確認します。

```powershell
kubectl exec -n anvilsaba deployment/mcguildlink -- sh -c 'date -Iseconds > /app/data/.persistence-test'
kubectl rollout restart deployment/mcguildlink -n anvilsaba
kubectl rollout status deployment/mcguildlink -n anvilsaba --timeout=5m
kubectl exec -n anvilsaba deployment/mcguildlink -- cat /app/data/.persistence-test
```

テスト終了後は一時ファイルを削除します。

### 4.5 開発用Tunnelを使う外部経路テスト

Cloudflare経由の公開経路まで確認する必要がある場合だけ、開発専用のTunnelを構成します。

```text
インターネット
  -> 開発用Cloudflare Tunnel
  -> cloudflared
  -> mcguildlink-http
```

このテストには、開発専用のTunnel tokenとhostnameを使用します。本番用のTunnel token、hostname、Cloudflare設定は開発環境で使用しないでください。

開発用Cloudflare Tunnelを構成した場合は、クラスタ外から次を確認します。

```powershell
curl.exe -i https://<DEV_WHITELIST_HOSTNAME>/whitelist.json
```

Minecraft接続を確認する場合は、`mcguildlink-minecraft` Serviceの公開アドレスと`25565/TCP`を確認します。

Tunnelの接続自体は`cloudflared`からCloudflareへの外向き通信で成立するため、サーバー側で受信ポートを開ける必要はありません。ただし、Tunnel経由のHTTPを確認する場合はCloudflare側に公開hostnameが必要です。外部公開せずに`cloudflared`だけ起動しても、Cloudflare経由の経路テストにはなりません。

開発時の使い分けは次のとおりです。

| テスト段階 | cloudflared | 確認内容 |
|---|---:|---|
| 個別テスト | 使用しない | Bot・MCGuildLink単体 |
| 通常の統合テスト | 使用しない | Kubernetes内部連携 |
| 外部経路テスト | 開発用のみ使用 | Cloudflare経由のWhitelist |
| 本番確認 | 本番用を使用 | 実際の公開経路 |

## 5. テスト環境の削除

開発用クラスタは必要なときに削除して作り直せます。

```powershell
k3d cluster delete anvilsaba
```
