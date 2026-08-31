# 本番デプロイ手順

## 1. デプロイ構成

本番はLinuxサーバー上のk3sへ、GHCRのDockerイメージをHelmでデプロイします。アプリケーションのソースコード全体は本番サーバーに不要です。

- Botイメージ：`ghcr.io/anvilsaba/bot:<tag>`
- MCGuildLinkイメージ：`ghcr.io/anvilsaba/mcguildlink:<tag>`
- Helm Chart：`oci://ghcr.io/anvilsaba/charts/platform`
- namespace：`anvilsaba`
- release：`platform`

## 2. デプロイ環境のセットアップ

### 2.1 Linuxサーバー

UbuntuまたはDebianなどのLinuxサーバーを用意します。k3sはWindowsへネイティブインストールできないため、本番ではLinuxを使用します。

必要なものは次のとおりです。

- k3s
- kubectl
- Helm
- flock（通常は`util-linux`に含まれます）
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

ChartはOCI形式でGHCRから取得するため、本番サーバーへリポジトリやChartを配置する必要はありません。Chartがprivateの場合は、デプロイ用ユーザーでGHCRへログインします。

```bash
echo '<GITHUB PAT>' | helm registry login ghcr.io \
  --username '<GITHUB USERNAME>' \
  --password-stdin
```

PATには対象パッケージの`read:packages`権限が必要です。取得できることを確認します。

```bash
helm show chart oci://ghcr.io/anvilsaba/charts/platform --version '<CHART_VERSION>'
```

### 2.4 Secretと設定

本番設定はリポジトリ外に置き、Gitへ追加しません。

Secretへ登録する元ファイルは、本番サーバー上のroot管理領域に配置します。

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

kubectl -n anvilsaba create secret docker-registry ghcr-pull \
  --docker-server=ghcr.io \
  --docker-username='<GITHUB USERNAME>' \
  --docker-password='GITHUB PERSONAL ACCESS TOKEN>'
```

### 2.5 Cloudflare Tunnel

Cloudflare側のPublished applicationのserviceを次に設定します。

```text
http://mcguildlink-http:8080
```

### 2.6 GitHub Actionsの本番環境

GitHubのSettingsから`production` Environmentを作成し、次のEnvironment secretsを登録します。

| 名前 | 内容 |
| --- | --- |
| `DEPLOY_SSH_HOST` | 本番サーバーのホスト名またはIPアドレス |
| `DEPLOY_SSH_USER` | k3sを操作できるデプロイ用ユーザー |
| `DEPLOY_SSH_PRIVATE_KEY` | デプロイ専用SSH秘密鍵 |
| `DEPLOY_SSH_KNOWN_HOSTS` | 検証済みの本番サーバー公開ホスト鍵 |

SSHポートが22以外の場合は、Environment variable `DEPLOY_SSH_PORT`も設定します。`DEPLOY_SSH_KNOWN_HOSTS`には接続先とポートに対応する行を登録し、別経路で確認したホスト鍵fingerprintと一致することを確認してください。

デプロイ用ユーザーには次の準備が必要です。

- SSH公開鍵を`authorized_keys`へ登録する
- `kubectl`と`helm`がsudoなしでk3sを操作できるようにする
- 2.3の`helm registry login`を同じユーザーで実行する

必要に応じて`production` EnvironmentへRequired reviewersを設定すると、本番デプロイ前に承認を挟めます。

## 3. GitHub Actions からデプロイ
GitHub の Actions 画面で **Production deployment** を選び、**Run workflow** からデプロイ対象と Git Tag を指定します。

## 4. デプロイ後の確認

動いてる Pod のイメージ確認
```bash
kubectl get pods -n anvilsaba \
  -o custom-columns='POD:.metadata.name,IMAGE:.status.containerStatuses[*].image,IMAGE_ID:.status.containerStatuses[*].imageID'
```

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

## Pod 起動直後の外向き通信

OCI Ubuntu のホスト firewall では `FORWARD` に catch-all `REJECT` が存在する。

k3s の kube-router NetworkPolicy controller が新規 Pod 用の firewall rule を作成するまでに短い遅延があるため、Pod 作成直後の外向き通信が一時的に `Host is unreachable` となる場合がある。

現在の環境では約 0.1 秒後には正常に通信できることを確認している。

Discord Bot では Serenity の Gateway URL 取得がこの期間に失敗すると警告が出るが、既定の `wss://gateway.discord.gg` へフォールバックし、その後正常に接続することを確認している。

現時点では実害がないため、ホスト側の `FORWARD` ルールは変更しない。
