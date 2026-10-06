# 本番デプロイ手順

Rust アプリと PostgreSQL を k3s へ配置します。初回セットアップでは DB・Secret を準備し、Migrator の正常完了後にアプリを起動します。

## 1. デプロイ環境のセットアップ

### 1.1 Linux サーバー

Ubuntu または Debian などの Linux サーバーを用意します。

必要なものは次のとおりです。

- k3s
- kubectl
- Helm
- flock(通常は `util-linux` に含まれます)
- GHCR からイメージを取得する権限
- Cloudflare remotely-managed Tunnel とトークン
- Bot の本番設定
- PostgreSQL の初期化ユーザーとアプリ・Migrator ごとの DB ロールのパスワード

### 1.2 k3s

`deploy/k3s/config.yaml` をサーバー上の次の場所へ配置してから k3s を起動します。

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

以降は `sudo` なしで実行するため、kubeconfig を設定します。

```bash
mkdir -p ~/.kube
sudo cp /etc/rancher/k3s/k3s.yaml ~/.kube/config
sudo chown "$USER":"$USER" ~/.kube/config
chmod 600 ~/.kube/config
echo 'export KUBECONFIG="$HOME/.kube/config"' >> ~/.zshrc
source ~/.zshrc
kubectl get nodes
```

### 1.3 Secret と設定

本番設定はリポジトリ外に置き、Git へ追加しません。
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

| Secret                        | DB ロール               |
| ----------------------------- | ----------------------- |
| bot-db-credentials            | platform_bot            |
| mc-link-server-db-credentials | platform_mc_link_server |
| public-api-db-credentials     | platform_public_api     |
| db-migrator-db-credentials    | platform_db_migrator    |

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

Migrator はアプリのリリースと同じリビジョンのイメージを使います。Helm は Job の正常完了まで待機します。
失敗時はアプリを起動せず Job のログと DB 履歴を確認します。成功後にアプリと cloudflared を起動し、Job を無効に戻します。

```bash
helm upgrade platform oci://ghcr.io/anvilsaba/charts/platform \
  --version "$chartVersion" -n anvilsaba --reuse-values --set dbMigrator.enabled=false \
  --set bot.replicas=1 --set mcLinkServer.replicas=1 --set publicApi.replicas=1 \
  --set cloudflared.replicas=1 --timeout 10m
```

更新時の互換性と切り戻しは [リリース手順](releases.md) を参照してください。
アプリは起動時に必要なマイグレーション履歴だけを読み取り、定義・適用履歴の不足、適用失敗、チェックサム不一致では起動を拒否します。スキーマ変更と DB 全体の履歴の整合性は Migrator Job が管理します。

### 1.4 Cloudflare Tunnel

Cloudflare 側の Published application の Service URL を次に設定します。

```text
http://public-api:8080
```

### 1.5 GitHub Actions の本番環境

デプロイ用の共通資格情報は、既存のリポジトリのシークレット・変数を使用します。

| 名前                     | 種別         | 内容                                     |
| ------------------------ | ------------ | ---------------------------------------- |
| `DEPLOY_SSH_HOST`        | シークレット | 本番サーバーのホスト名または IP アドレス |
| `DEPLOY_SSH_USER`        | シークレット | k3s を操作できるデプロイ用ユーザー       |
| `DEPLOY_SSH_PRIVATE_KEY` | シークレット | デプロイ専用 SSH 秘密鍵                  |
| `DEPLOY_SSH_KNOWN_HOSTS` | シークレット | 検証済みの本番サーバー公開ホスト鍵       |
| `DEPLOY_SSH_PORT`        | 変数         | SSH ポート                               |

`DEPLOY_SSH_KNOWN_HOSTS` には接続先とポートに対応する行を登録し、別経路で確認したホスト鍵のフィンガープリントと一致することを確認してください。

デプロイ用ユーザーには次の準備が必要です。

- SSH 公開鍵を `authorized_keys` へ登録する
- `kubectl` と `helm` が sudo なしで k3s を操作できるようにする

## 2. GitHub Actions からデプロイ

GitHub の Actions 画面で **Production deployment** を選び、**Run workflow** からデプロイ対象と Git タグを指定します。

## 3. デプロイ後の確認

稼働中の Pod のイメージを確認します。

```bash
kubectl get pods -n anvilsaba \
  -o custom-columns='POD:.metadata.name,IMAGE:.status.containerStatuses[*].image,IMAGE_ID:.status.containerStatuses[*].imageID'
```

```bash
helm status platform -n anvilsaba
kubectl get pods,deploy,statefulset,service,pvc -n anvilsaba
kubectl get events -n anvilsaba --sort-by=.lastTimestamp
```

各ワークロードが Ready になることを確認します。

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

Bot の Discord アプリケーションコマンドは、実行中の Pod に TTY 付きで接続して操作します。

```bash
kubectl attach -n anvilsaba -it --detach-keys=ctrl-c deployment/bot
```

Discord 接続の READY 後、コンソールがコマンドを受け付けるようになります。
利用可能なコマンドは `help` で確認可能です。
上記の接続では Ctrl+C で接続だけが終了し、Bot は稼働を続けます。`--detach-keys` を省略した場合の切断キーは Ctrl+P、続けて Ctrl+Q です。

次を確認して完了とします。

- Bot・MC Link Server・公開 API が正常起動し、Bot が Discord へ接続できる
- Cloudflare 経由で `/whitelist.json` を取得できる
- `mc-link-server` Service が本番の `25600/TCP` をコンテナ内の `25565/TCP` へ転送している
- PostgreSQL 再起動後もデータが残る
- Secret の実値が Git やログに含まれていない

## 4. メモ

### Oracle Cloud Infrastructure Ubuntu での Pod 起動直後の外向き通信

OCI Ubuntu のホストファイアウォールでは `FORWARD` に catch-all `REJECT` が存在します。

k3s の kube-router NetworkPolicy コントローラーが新規 Pod 用のファイアウォールルールを作成するまでに短い遅延があるため、Pod 作成直後の外向き通信が一時的に `Host is unreachable` となる場合があります。

現在の環境では約 0.1 秒後には正常に通信できることを確認しています。

Discord Bot では Serenity の Gateway URL 取得がこの期間に失敗すると警告が出ますが、既定の `wss://gateway.discord.gg` へフォールバックし、その後正常に接続することを確認しています。

現時点では実害がないため、ホスト側の `FORWARD` ルールは変更しません。
