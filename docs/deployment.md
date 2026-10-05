# 本番デプロイ手順

Rust アプリと PostgreSQL を k3s へ配置します。初回セットアップでは DB・Secret を準備し、Migrator の正常完了後にアプリを起動します。

## 1. デプロイ構成

本番はLinuxサーバー上のk3sへ、GHCRのDockerイメージをHelmでデプロイします。アプリケーションのソースコード全体は本番サーバーに不要です。

- Botイメージ：`ghcr.io/anvilsaba/bot:<tag>`
- MC Link Serverイメージ：`ghcr.io/anvilsaba/mc-link-server:<tag>`
- 公開 APIイメージ：`ghcr.io/anvilsaba/public-api:<tag>`
- DB Migratorイメージ：`ghcr.io/anvilsaba/db-migrator:sha-<完全なGit SHA>`
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
- Bot の本番設定
- PostgreSQL の初期化ユーザーとアプリ・Migrator ごとの DB ロールのパスワード

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
Bot の設定は `apps/bot/config.sample.toml` を基に用意し、Secret 登録元のファイルは root 管理領域に配置します。
MC Link Server と公開 API は設定ファイルを使用しません。

```bash
scp ./config.toml <本番ユーザー>@<本番サーバー>:/tmp/bot-config.toml
```

本番サーバー上で配置・登録します。

```bash
sudo install -d -o root -g root -m 700 /etc/anvilsaba/secrets/bot
sudo install -o root -g root -m 600 /tmp/bot-config.toml \
  /etc/anvilsaba/secrets/bot/config.toml
sudo rm /tmp/bot-config.toml
kubectl create namespace anvilsaba --dry-run=client -o yaml | kubectl apply -f -
sudo kubectl -n anvilsaba create secret generic bot-config \
  --from-file=config.toml=/etc/anvilsaba/secrets/bot/config.toml \
  --dry-run=client -o yaml | sudo kubectl apply -f -
kubectl -n anvilsaba create secret generic cloudflare-tunnel \
  --from-literal=token='<TUNNEL_TOKEN>' \
  --dry-run=client -o yaml | kubectl apply -f -
kubectl -n anvilsaba create secret docker-registry ghcr-pull \
  --docker-server=ghcr.io \
  --docker-username='<GITHUB USERNAME>' \
  --docker-password='<GITHUB PERSONAL ACCESS TOKEN>'
```

PostgreSQL 初期化ユーザーの Secret は `postgres-db-credentials` の `password` キーを使います。

```bash
kubectl -n anvilsaba create secret generic postgres-db-credentials \
  --from-literal=password='<POSTGRES_PASSWORD>' \
  --dry-run=client -o yaml | kubectl apply -f -
```

アプリ・Migrator の接続資格情報は次のように分け、DB ロールのパスワードと対応する Secret の値を揃えます。

| Secret | DB ロール |
|---|---|
| bot-db-credentials | platform_bot |
| mc-link-server-db-credentials | platform_mc_link_server |
| public-api-db-credentials | platform_public_api |
| db-migrator-db-credentials | platform_db_migrator |

共有 DB は `platform` です。Helm は接続先を `DATABASE_URL`、各 Secret の `password` を `DATABASE_PASSWORD` として渡します。後者は URL 内のパスワードより優先します。

ロール作成は `bootstrap.sql` が行います。アプリには runtime ロール経由で必要なテーブル・列の権限だけを与え、DB 所有権・スキーマ作成権限・マイグレーションロール・将来のテーブルへのデフォルト権限は与えません。DB・スキーマの作成権限は Migrator に与えますが、`CREATEROLE` は不要です。

<a id="initial-setup"></a>

### 初回の DB 準備と配置順序

以下は k3s に接続できる管理端末のリポジトリルートで実行します。既存 DB は再初期化せず、通常の更新を行います。
Bot・MC Link Server・公開 API のタグは起動前も必須です。公開済みのタグを指定し、まずアプリと Job を止めた状態で PostgreSQL を配置します。

```bash
chartVersion='<CHART_VERSION>'
helm upgrade --install platform oci://ghcr.io/anvilsaba/charts/platform \
  --version "$chartVersion" -n anvilsaba -f deploy/helm/platform/values.prod.yaml \
  --set-string bot.image.tag='<BOT_IMAGE_TAG>' \
  --set-string mcLinkServer.image.tag='<MC_IMAGE_TAG>' \
  --set-string publicApi.image.tag='<API_IMAGE_TAG>' \
  --set bot.replicas=0 --set mcLinkServer.replicas=0 --set publicApi.replicas=0 \
  --set cloudflared.replicas=0 --set dbMigrator.enabled=false --timeout 10m
kubectl rollout status statefulset/postgres -n anvilsaba --timeout=5m
kubectl exec -i -n anvilsaba postgres-0 -- psql -U platform_admin -d platform \
  -v ON_ERROR_STOP=1 < deploy/postgres/bootstrap.sql
kubectl exec -it -n anvilsaba postgres-0 -- psql -U platform_admin -d platform
```

psql 内で `\password platform_bot`、`\password platform_mc_link_server`、`\password platform_public_api`、`\password platform_db_migrator` を順に実行し、各ロールに別のパスワードを設定して `\q` で終了します。
上の表の各 Secret を同じパスワードで登録します。次の例を Secret ごとに実行してください。

```bash
kubectl -n anvilsaba create secret generic '<SECRET_NAME>' \
  --from-literal=password='<ROLE_PASSWORD>' \
  --dry-run=client -o yaml | kubectl apply -f -
helm upgrade platform oci://ghcr.io/anvilsaba/charts/platform \
  --version "$chartVersion" -n anvilsaba --reuse-values \
  --set dbMigrator.enabled=true --set-string dbMigrator.image.tag='sha-<RELEASE_SHA>' --timeout 10m
kubectl logs -n anvilsaba job/db-migrator
```

Migrator はアプリのリリースと同じ revision のイメージを使います。Helm は Job の正常完了まで待機します。
失敗時はアプリを起動せず Job のログと DB 履歴を確認します。成功後にアプリと cloudflared を起動し、Job を無効に戻します。

```bash
helm upgrade platform oci://ghcr.io/anvilsaba/charts/platform \
  --version "$chartVersion" -n anvilsaba --reuse-values --set dbMigrator.enabled=false \
  --set bot.replicas=1 --set mcLinkServer.replicas=1 --set publicApi.replicas=1 \
  --set cloudflared.replicas=1 --timeout 10m
```

更新時の互換性と切り戻しは [リリース手順](releases.md) を参照してください。
アプリは起動時に必要なマイグレーション履歴だけを読み取り、定義・適用履歴の不足、適用失敗、チェックサム不一致では起動を拒否します。スキーマ変更と DB 全体の履歴の整合性は Migrator Job が管理します。

### 2.5 Cloudflare Tunnel

Cloudflare 側の Published application の Service URL を次に設定します。

```text
http://public-api:8080
```

これは cloudflared と公開 API が同じ namespace にある場合の宛先です。
別 namespace からは `http://public-api.anvilsaba.svc.cluster.local:8080` を使います。`publicApi.port` は Service 側のポートだけを変更するため、変更時は Tunnel の転送先ポートも合わせます。
Helm のリリースだけでは Cloudflare 側の転送先は変更されません。

### 2.6 GitHub Actionsの本番環境

デプロイ対象ごとに、次の4つのEnvironmentを作成します。

```text
production/bot
production/mc-link-server
production/public-api
production/chart
```

手動デプロイでは次の指定で対象別の環境を使い、Releaseでは`inputs.app`を使います。[GitHubのWorkflow構文](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idenvironment)
migration はデプロイ対象アプリの Environment 内で自動実行します。

```yaml
environment: production/${{ inputs.target }}
```

デプロイ用の共通資格情報は、既存のリポジトリ secrets/variablesを使用します。

| 名前 | 内容 |
| --- | --- |
| `DEPLOY_SSH_HOST` | 本番サーバーのホスト名またはIPアドレス（repository secret） |
| `DEPLOY_SSH_USER` | k3sを操作できるデプロイ用ユーザー（repository secret） |
| `DEPLOY_SSH_PRIVATE_KEY` | デプロイ専用SSH秘密鍵（repository secret） |
| `DEPLOY_SSH_KNOWN_HOSTS` | 検証済みの本番サーバー公開ホスト鍵（repository secret） |
| `DEPLOY_SSH_PORT` | SSHポート（repository variable、未設定時は22） |

各Environmentへ同じ値を複製する必要はありません。対象ごとに設定を変える場合は、そのEnvironmentに同名のsecretまたはvariableを登録します。[Secretsの優先順位](https://docs.github.com/en/actions/how-tos/write-workflows/choose-what-workflows-do/use-secrets)、[Variablesの優先順位](https://docs.github.com/en/actions/reference/workflows-and-actions/variables#configuration-variable-precedence)

承認を必須にする場合は、各EnvironmentにRequired reviewersを設定します。[Environmentの管理](https://docs.github.com/en/actions/how-tos/deploy/configure-and-manage-deployments/manage-environments)

`DEPLOY_SSH_KNOWN_HOSTS`には接続先とポートに対応する行を登録し、別経路で確認したホスト鍵fingerprintと一致することを確認してください。

デプロイ用ユーザーには次の準備が必要です。

- SSH公開鍵を`authorized_keys`へ登録する
- `kubectl`と`helm`がsudoなしでk3sを操作できるようにする
- 2.3の`helm registry login`を同じユーザーで実行する

各Environmentは共通のHelm release（`platform`）を更新するため、workflowの共通concurrencyとサーバー側の`flock`による排他を維持します。Environmentを分けても、同一releaseへの同時更新を許可しません。

## 3. GitHub Actions からデプロイ
Action は初期セットアップ済みの Helm release の値を引き継ぎ、指定した対象のタグだけを更新します。対象に応じて`production/bot`、`production/mc-link-server`、`production/public-api`、`production/chart`のEnvironmentを参照します。アプリでは、指定した Git tag と同じ revision の Migrator Job を先行実行し、成功後にのみアプリを更新します。Job は最後に無効な通常状態へ戻します。

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
kubectl rollout status deployment/mc-link-server -n anvilsaba --timeout=5m
kubectl rollout status deployment/public-api -n anvilsaba --timeout=5m
kubectl rollout status deployment/cloudflared -n anvilsaba --timeout=5m
kubectl rollout status statefulset/postgres -n anvilsaba --timeout=5m
```

ログを確認します。

```bash
kubectl logs -n anvilsaba deployment/bot --tail=100
kubectl logs -n anvilsaba deployment/mc-link-server --tail=100
kubectl logs -n anvilsaba deployment/public-api --tail=100
kubectl logs -n anvilsaba deployment/cloudflared --tail=100
kubectl logs -n anvilsaba statefulset/postgres --tail=100
```

BotのDiscordアプリケーションコマンドは、実行中のPodにTTY付きで接続して操作します。

```bash
kubectl attach -n anvilsaba -it --detach-keys=ctrl-c deployment/bot
```

Discord接続のREADY後、コンソールがコマンドを受け付けるようになります。
利用可能なコマンドは `help` で確認可能です。
上記の接続では Ctrl+C で接続だけが終了し、Botは稼働を続けます。`--detach-keys` を省略した場合の切断キーはCtrl+P、続けてCtrl+Qです。

次を確認して完了とします。

- Bot・MC Link Server・公開 API が正常起動し、Bot が Discord へ接続できる
- cloudflaredがTunnelへ接続している
- Cluster内から`/whitelist.json`を取得できる
- Cloudflare経由で`/whitelist.json`を取得できる
- `public-api`がClusterIPで、HTTPが直接公開されていない
- `mc-link-server` Service が本番の `25600/TCP` をコンテナ内の `25565/TCP` へ転送している
- PostgreSQL再起動後もデータが残る
- Secretの実値がGitやログに含まれていない

## 監査配送の再開

送信失敗は60秒後から自動再試行し、待機時間を倍増します（上限1時間）。Discord の403・404では再試行を停止します。

設定・権限を修正後、対象サーバーのモデレーターロールを持つ管理者が `/audit_retry event_id:<イベントID>` で1件、`/audit_retry` で停止中の全件を再試行待ちに戻します。

## Pod 起動直後の外向き通信

OCI Ubuntu のホスト firewall では `FORWARD` に catch-all `REJECT` が存在する。

k3s の kube-router NetworkPolicy controller が新規 Pod 用の firewall rule を作成するまでに短い遅延があるため、Pod 作成直後の外向き通信が一時的に `Host is unreachable` となる場合がある。

現在の環境では約 0.1 秒後には正常に通信できることを確認している。

Discord Bot では Serenity の Gateway URL 取得がこの期間に失敗すると警告が出るが、既定の `wss://gateway.discord.gg` へフォールバックし、その後正常に接続することを確認している。

現時点では実害がないため、ホスト側の `FORWARD` ルールは変更しない。
