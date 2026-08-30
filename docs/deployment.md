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

## 3. 初期デプロイ

GitHub ActionsでChart、Bot、MCGuildLinkをGHCRへ登録します。`latest`ではなく、実際に生成された固定バージョンを使用します。初期デプロイでは両方のイメージタグが必要なため、手動で実行します。

各対象の初回リリースでは、デフォルトのまま成果物だけを公開します。Chartと両方のイメージが揃ってから、以下の初期デプロイを実行してください。

```bash
CHART_VERSION='<CHART_VERSION>'
BOT_IMAGE_TAG='<BOT_RELEASE_TAG>'
MCGUILDLINK_IMAGE_TAG='<MCGUILDLINK_RELEASE_TAG>'
```

まずdry-runします。

```bash
helm upgrade --install platform oci://ghcr.io/anvilsaba/charts/platform \
  --version "$CHART_VERSION" \
  --namespace anvilsaba \
  --create-namespace \
  --set-string 'imagePullSecrets[0].name=ghcr-pull' \
  --set-string bot.image.tag="$BOT_IMAGE_TAG" \
  --set-string mcguildlink.image.tag="$MCGUILDLINK_IMAGE_TAG" \
  --dry-run
```

内容を確認したら、`--dry-run`を外して実行します。

```bash
helm upgrade --install platform oci://ghcr.io/anvilsaba/charts/platform \
  --version "$CHART_VERSION" \
  --namespace anvilsaba \
  --create-namespace \
  --set-string 'imagePullSecrets[0].name=ghcr-pull' \
  --set-string bot.image.tag="$BOT_IMAGE_TAG" \
  --set-string mcguildlink.image.tag="$MCGUILDLINK_IMAGE_TAG" \
  --atomic --timeout 10m
```

## 4. デプロイの更新

GitHub ActionsのReleaseで`dry_run=false`かつ`deploy=true`を指定すると、成果物の公開成功後にSSH経由で自動更新します。

- Botリリース：Botのイメージタグだけを更新
- MCGuildLinkリリース：MCGuildLinkのイメージタグだけを更新
- Chartリリース：Chartバージョンだけを更新

WorkflowはリリースタグのコミットからChartバージョンを取得し、リポジトリ内の`scripts/deploy-production.sh`をSSH標準入力で本番サーバーへ渡します。スクリプトを本番サーバーへ配置する必要はありません。Helmの更新では新しいChartのデフォルトへ既存のリリース値を重ねるため、リリースしていないイメージタグも維持されます。

公開済みリリースをデプロイし直す場合は、GitHub Actionsの**Production deployment**を実行し、対象と既存リリースタグを指定します。新しいタグや成果物は作成せず、指定したリリースのデプロイだけを実行します。Chartには、そのリリースタグのコミットに記録されたバージョンを使用します。

Botだけを手動更新する場合の同等コマンドは次のとおりです。

```bash
helm upgrade platform oci://ghcr.io/anvilsaba/charts/platform \
  --version '<CHART_VERSION>' \
  --namespace anvilsaba \
  --reset-then-reuse-values \
  --set-string bot.image.tag='<NEW_BOT_TAG>' \
  --atomic --timeout 10m
```

MCGuildLinkの場合は`mcguildlink.image.tag`を指定します。Chartだけを更新する場合はイメージタグを指定せず、`--version`だけを新しいChartバージョンへ変更します。

リリースは本番デプロイまで直列化され、サーバー上でもユーザー単位のファイルロックを取得します。`--atomic`により更新失敗時は直前のHelm revisionへ戻ります。本番へ反映する場合だけReleaseで`deploy=true`を指定してください。

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
