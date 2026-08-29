# 本番デプロイ手順

## 1. デプロイ構成

本番はLinuxサーバー上のk3sへ、GHCRのDockerイメージをHelmでデプロイします。アプリケーションのソースコード全体は本番サーバーに不要です。

- Botイメージ：`ghcr.io/anvilsaba/bot:<tag>`
- MCGuildLinkイメージ：`ghcr.io/anvilsaba/mcguildlink:<tag>`
- Helm Chart：`deploy/helm/platform`
- namespace：`anvilsaba`
- release：`platform`

## 2. デプロイ環境のセットアップ

### 2.1 Linuxサーバー

UbuntuまたはDebianなどのLinuxサーバーを用意します。k3sはWindowsへネイティブインストールできないため、本番ではLinuxを使用します。

必要なものは次のとおりです。

- k3s
- kubectl
- Helm
- GHCRからイメージを取得する権限
- Cloudflare remotely-managed Tunnelとtoken
- Bot・MCGuildLinkの本番設定
- PostgreSQL password

### 2.2 k3s

`deploy/k3s/config.yaml`をサーバー上の次の場所へ配置してからk3sを起動します。

```text
/etc/rancher/k3s/config.yaml
```

```bash
sudo mkdir -p /etc/rancher/k3s
sudo cp deploy/k3s/config.yaml /etc/rancher/k3s/config.yaml
curl -sfL https://get.k3s.io | sh -
sudo systemctl enable --now k3s
```

接続を確認します。

```bash
sudo kubectl get nodes
sudo kubectl get storageclass
```

以降は`sudo`なしで実行するため、kubeconfigを設定します。

```bash
mkdir -p ~/.kube
sudo cp /etc/rancher/k3s/k3s.yaml ~/.kube/config
sudo chown "$USER":"$USER" ~/.kube/config
chmod 600 ~/.kube/config
echo 'export KUBECONFIG="$HOME/.kube/config"' >> ~/.zshrc
source ~/.zshrc
kubectl get nodes
```

別の管理端末から接続する場合は、kubeconfig内の`127.0.0.1`をk3sサーバーのIPまたはDNS名へ変更し、API Serverの`6443/TCP`へ到達できるようにします。kubeconfigは秘密情報として扱います。

### 2.3 Helm Chart

本番サーバーへ次のディレクトリを配置します。

```text
deploy/helm/platform/
```

Gitリポジトリ全体を配置する必要はありません。scp、rsync、リリース成果物などでChartだけを配布できます。

### 2.4 Secretと設定

本番設定はリポジトリ外に置き、Gitへ追加しません。

Secretへ登録する元ファイルは、本番サーバー上のroot管理領域に配置します。`/opt/`は特定ユーザー専用ではありませんが、設定ファイルの用途が明確になるため、ここでは`/etc/anvilsaba/`を使用します。

```text
/etc/anvilsaba/secrets/bot/config.toml
/etc/anvilsaba/secrets/mcguildlink/app.toml
```

設定ファイルを別端末から転送する場合は、例えば次のように配置します。

```bash
scp ./app.toml <本番ユーザー>@<本番サーバー>:/tmp/mcguildlink-app.toml
scp ./config.toml <本番ユーザー>@<本番サーバー>:/tmp/bot-config.toml
```

本番サーバー上で、転送したファイルを所定の場所へ移動します。`install`の第1引数が配置元、第2引数が配置先です。

```bash
sudo install -d -o root -g root -m 700 /etc/anvilsaba/secrets/bot
sudo install -d -o root -g root -m 700 /etc/anvilsaba/secrets/mcguildlink
sudo install -o root -g root -m 600 /tmp/bot-config.toml \
  /etc/anvilsaba/secrets/bot/config.toml
sudo install -o root -g root -m 600 /tmp/mcguildlink-app.toml \
  /etc/anvilsaba/secrets/mcguildlink/app.toml
sudo rm /tmp/bot-config.toml /tmp/mcguildlink-app.toml
```

```bash
sudo kubectl create namespace anvilsaba --dry-run=client -o yaml | sudo kubectl apply -f -

sudo kubectl -n anvilsaba create secret generic bot-config \
  --from-file=config.toml=/etc/anvilsaba/secrets/bot/config.toml \
  --dry-run=client -o yaml | sudo kubectl apply -f -

sudo kubectl -n anvilsaba create secret generic mcguildlink-config \
  --from-file=app.toml=/etc/anvilsaba/secrets/mcguildlink/app.toml \
  --dry-run=client -o yaml | sudo kubectl apply -f -

sudo kubectl -n anvilsaba create secret generic postgres \
  --from-literal=password='<POSTGRES_PASSWORD>' \
  --dry-run=client -o yaml | sudo kubectl apply -f -

sudo kubectl -n anvilsaba create secret generic cloudflare-tunnel \
  --from-literal=token='<TUNNEL_TOKEN>' \
  --dry-run=client -o yaml | sudo kubectl apply -f -
```

### 2.5 Cloudflare Tunnel

Cloudflare側のPublished applicationのserviceを次に設定します。

```text
http://mcguildlink-http:8080
```

## 3. 初期デプロイ

GitHub ActionsでBotとMCGuildLinkのイメージをGHCRへ登録します。`latest`ではなく、実際に生成された固定tagを使用します。

```bash
BOT_IMAGE_TAG='<BOT_COMMIT_SHA_OR_RELEASE_TAG>'
MCGUILDLINK_IMAGE_TAG='<MCGUILDLINK_COMMIT_SHA_OR_RELEASE_TAG>'
```

まずdry-runします。

```bash
helm upgrade --install platform ./deploy/helm/platform \
  --namespace anvilsaba \
  --create-namespace \
  -f ./deploy/helm/platform/values.prod.yaml \
  --set-string bot.image.tag="$BOT_IMAGE_TAG" \
  --set-string mcguildlink.image.tag="$MCGUILDLINK_IMAGE_TAG" \
  --dry-run
```

内容を確認したら、`--dry-run`を外して実行します。

```bash
helm upgrade --install platform ./deploy/helm/platform \
  --namespace anvilsaba \
  --create-namespace \
  -f ./deploy/helm/platform/values.prod.yaml \
  --set-string bot.image.tag="$BOT_IMAGE_TAG" \
  --set-string mcguildlink.image.tag="$MCGUILDLINK_IMAGE_TAG" \
  --wait --timeout 10m
```

## 4. デプロイの更新

新しいイメージtagを指定して同じHelmコマンドを実行します。BotとMCGuildLinkは独立してリリースされるため、それぞれ実在するtagを指定します。

```bash
helm upgrade platform ./deploy/helm/platform \
  --namespace anvilsaba \
  -f ./deploy/helm/platform/values.prod.yaml \
  --set-string bot.image.tag='<NEW_BOT_TAG>' \
  --set-string mcguildlink.image.tag='<NEW_MCGUILDLINK_TAG>' \
  --wait --timeout 10m
```

履歴の確認とロールバックは次のとおりです。

```bash
helm history platform -n anvilsaba
helm rollback platform <REVISION> -n anvilsaba --wait --timeout 10m
```

## 5. デプロイ後の確認

```bash
helm status platform -n anvilsaba
kubectl get pods,deploy,statefulset,service,pvc -n anvilsaba
kubectl get events -n anvilsaba --sort-by=.lastTimestamp
```

各WorkloadがReadyになることを確認します。

```bash
kubectl rollout status deployment/bot -n anvilsaba --timeout=5m
kubectl rollout status deployment/mcguildlink -n anvilsaba --timeout=5m
kubectl rollout status deployment/cloudflared -n anvilsaba --timeout=5m
kubectl rollout status statefulset/postgres -n anvilsaba --timeout=5m
```

ログを確認します。

```bash
kubectl logs -n anvilsaba deployment/bot --tail=100
kubectl logs -n anvilsaba deployment/mcguildlink --tail=100
kubectl logs -n anvilsaba deployment/cloudflared --tail=100
kubectl logs -n anvilsaba statefulset/postgres --tail=100
```

次を確認して完了とします。

- BotとMCGuildLinkが正常起動し、Discordへ接続できる
- cloudflaredがTunnelへ接続している
- Cluster内から`/whitelist.json`を取得できる
- Cloudflare経由で`/whitelist.json`を取得できる
- `mcguildlink-http`がClusterIPで、HTTPが直接公開されていない
- Minecraft用Serviceが`25565/TCP`を公開している
- MCGuildLink再起動後もSQLiteのデータが残る
- PostgreSQL再起動後もデータが残る
- Secretの実値がGitやログに含まれていない
