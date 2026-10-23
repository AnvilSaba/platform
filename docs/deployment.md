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

[Flux の本番 GitOps 手順](flux.md) に従って、Flux の初回接続、Secret・Config、CloudNativePG、DB、ロール、アプリの順に引き継ぎます。デプロイ先では `kubectl` を直接実行し、mise は不要です。

Flux 本体・Operator・DB・ロール・Secret・Config・アプリを Git 管理します。初期状態では Config と DB、HelmRelease の同期を保留しています。既存 DB は先に [移行手順](cloudnativepg-migration.md) で旧 PVC の保護・バックアップ・書き込み停止を行い、新規環境だけ空の DB を作成するパスを選びます。

Migrator はアプリの配置・更新より前に実行し、正常完了するまでアプリを適用しません。アプリのタグと Migrator の完全な SHA タグ、公開済み Chart のバージョンは `deploy/flux/apps` で固定します。初回の引き継ぎ時は稼働中 release の設定と移行対応 Chart を確認してから同期を有効にします。

更新時の互換性と切り戻しは [リリース手順](releases.md) を参照してください。
アプリは起動時に必要なマイグレーション履歴だけを読み取り、定義・適用履歴の不足、適用失敗、チェックサム不一致では起動を拒否します。スキーマ変更と DB 全体の履歴の整合性は Migrator Job が管理します。

### 1.4 Cloudflare Tunnel

Cloudflare 側の Published application の Service URL を次に設定します。

```text
http://public-api:8080
```

### 1.5 GitHub Actions の本番環境

`production/<対象>` の Environment を使用します。Actions はクラスタへの SSH 接続を行わず、公開済みリリースのバージョンを Git に記録します。従来の `DEPLOY_SSH_*` は不要です。

`GITHUB_TOKEN` の `contents: write` と、ブランチ保護設定が本番設定の更新コミットを許可することを確認します。書き込めない場合は担当者が同じ設定変更を PR で反映します。クラスタの読み取り用 Git 認証が必要な場合は、Actions の資格情報とは別に Flux 用の認証を用意します。

## 2. GitHub Actions からデプロイ

GitHub の Actions 画面で **Production deployment** を選び、**Run workflow** からデプロイ対象と公開済み Git タグを指定します。Actions が本番設定の更新コミットを作成し、Flux が同期します。Actions の成功だけで配置完了とは判断せず、HelmRelease の Ready を確認します。

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

## image の固定

Bot、mc-link-server、public-api、db-migrator の `image.digest` に
`sha256:<64桁の小文字hex>` を指定できる。指定時は `repository@digest` を描画し、
`image.tag` は追跡情報として残せる。digest が空なら既存の tag 指定を使う。
Migrator が有効な場合も同じ規則とし、参照が欠けた場合は描画を失敗させる。
source 移管時には既存 Chart 1.0.0 を維持し、新版導入は別の更新 PR とする。
