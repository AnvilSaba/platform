# 本番デプロイ手順

Rust アプリと CloudNativePG 管理の PostgreSQL を k3s へ配置します。DB はアプリの Helm release と独立して管理します。初回セットアップでは DB・Secret を準備し、Migrator の正常完了後にアプリを起動します。
従来の StatefulSet を使用している環境は、Chart を更新する前に [既存 DB の移行手順](cloudnativepg-migration.md) を実施してください。

## 1. デプロイ環境のセットアップ

### 1.1 Linux サーバー

Ubuntu または Debian などの Linux サーバーを用意します。

必要なものは次のとおりです。

- k3s
- kubectl
- Helm
- flock(通常は `util-linux` に含まれます)
- Public な GHCR イメージ
- Cloudflare remotely-managed Tunnel とトークン
- Bot の本番設定
- アプリ・Migrator ごとの DB ロールのパスワードを持つ Secret

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

通常設定と Secret は [Secret / Config の準備と運用](secret-config.md) に従って用意します。
Bot の通常設定は Git 管理の `deploy/config/production/bot.toml` から ConfigMap にし、Token は専用 Secret の環境変数で渡します。
DB 資格情報はサービス別 Secret、Cloudflare Token は `cloudflared` を使用します。GHCR イメージは Public を前提とし、取得用 Secret は不要です。
Secret は SOPS + age で暗号化したマニフェストだけを Git 管理し、ノードに平文の原本を配置しません。
新規環境への適用と age 秘密鍵の配置は手順書に沿って行ってください。

<a id="initial-setup"></a>

### 初回の DB 準備と配置順序

以下は新規環境向けです。Secret と namespace を準備した後、デプロイ先のリポジトリルートで実行します。デプロイ先には `kubectl` と `helm` を用意し、直接実行します。
CloudNativePG 1.30.1 を導入し、DB の作成、初期権限の設定、Secret のパスワードとロール所属の反映まで自動で行います。アプリのテーブルは Migrator が作成します。

```bash
kubectl apply --server-side -f https://raw.githubusercontent.com/cloudnative-pg/cloudnative-pg/v1.30.1/releases/cnpg-1.30.1.yaml
kubectl rollout status deployment/cnpg-controller-manager -n cnpg-system --timeout=5m
kubectl apply -k deploy/postgres/cloudnativepg
kubectl wait cluster/platform-db -n anvilsaba --for=condition=Ready --timeout=10m
kubectl apply -f deploy/postgres/cloudnativepg/roles.yaml
kubectl wait databaserole -n anvilsaba -l app.kubernetes.io/part-of=platform-database \
  --for=jsonpath='{.status.applied}'=true --timeout=5m
```

既存 DB の移行では新規構築用の `deploy/postgres/cloudnativepg` を適用せず、[移行手順](cloudnativepg-migration.md) のインポート専用設定で新しい Cluster を作成します。
Bot・MC Link Server・公開 API のタグは起動前も必須です。公開済みのタグを指定し、アプリと Job を止めた状態で Chart を配置します。

```bash
chartVersion='<CHART_VERSION>'
helm upgrade --install platform oci://ghcr.io/anvilsaba/charts/platform \
  --version "$chartVersion" -n anvilsaba -f deploy/helm/platform/values.prod.yaml \
  --set-string bot.image.tag='<BOT_IMAGE_TAG>' \
  --set-string mcLinkServer.image.tag='<MC_IMAGE_TAG>' \
  --set-string publicApi.image.tag='<API_IMAGE_TAG>' \
  --set bot.replicas=0 --set mcLinkServer.replicas=0 --set publicApi.replicas=0 \
  --set cloudflared.replicas=0 --set dbMigrator.enabled=false --timeout 10m
```

```bash
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
kubectl wait cluster/platform-db -n anvilsaba --for=condition=Ready --timeout=5m
kubectl get databaserole -n anvilsaba
```

ログを確認します。

```bash
kubectl logs -n anvilsaba deployment/bot --tail=100
kubectl logs -n anvilsaba deployment/mc-link-server --tail=100
kubectl logs -n anvilsaba deployment/public-api --tail=100
kubectl logs -n anvilsaba deployment/cloudflared --tail=100
kubectl logs -n anvilsaba -l cnpg.io/cluster=platform-db --tail=100
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
