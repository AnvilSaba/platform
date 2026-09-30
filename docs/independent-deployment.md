# 分離サービスの統合環境と独立リリース

Issue #64、親仕様 #55 と ADR 0001・0005 に従う。ここでの手順は統合環境向けで、本番切替は行わない。
既存の本番手順は Kotlin 版向けのまま保持する。

## 配布単位

| 対象 / タグ接頭辞 | イメージ | Helm 設定 | 接続ロール |
|---|---|---|---|
| bot | ghcr.io/anvilsaba/bot | bot | platform_bot |
| mc-link-server | ghcr.io/anvilsaba/mc-link-server | mcLinkServer | platform_mcguildlink |
| public-api | ghcr.io/anvilsaba/public-api | publicApi | platform_public_api |
| db-migrator | ghcr.io/anvilsaba/db-migrator | migration | platform_migrator_job |
| 旧 Kotlin（移行検証のみ・リリース対象外） | 既存の ghcr.io/anvilsaba/mcguildlink | mcguildlink | SQLite |

GitHub Actions の Release は各対象を別タグでビルド・公開する。通常は deploy=false とする。
Production deployment と既存デプロイスクリプトは対象の image.tag を更新し、他のタグ・replicas を維持する。旧 Kotlin 版の更新処理は持たず、既存イメージを保持する。
Rust 版の既定 replicas は 0。イメージのリリースだけでは本番の Minecraft 接続先を切り替えない。
Bot の監査送信役は1つなので replicas=1 を維持する。公開 API の HTTP Service は ClusterIP のままにする。

Rust の4イメージは deploy/rust/Dockerfile の cargo-chef・cargo-zigbuild を共用する。QEMU は使用しない。イメージ公開処理は .github/actions/build-image に集約する。旧 Kotlin 版の Dockerfile はローカルの移行検証用に保持する。

```powershell
podman build -f deploy/rust/Dockerfile --build-arg TARGETARCH=amd64 --build-arg PACKAGE=mc-link-server --build-arg BINARY=mc-link-server --build-arg SOURCE_DIR=apps/mc-link-server -t localhost/anvilsaba/mc-link-server:test .
podman build -f deploy/rust/Dockerfile --build-arg TARGETARCH=amd64 --build-arg PACKAGE=public-api --build-arg BINARY=public-api --build-arg SOURCE_DIR=apps/public-api -t localhost/anvilsaba/public-api:test .
podman build -f deploy/rust/Dockerfile --build-arg TARGETARCH=amd64 --build-arg PACKAGE=db-migrator --build-arg BINARY=db-migrator --build-arg SOURCE_DIR=apps/db-migrator -t localhost/anvilsaba/db-migrator:test .
k3d image import localhost/anvilsaba/mc-link-server:test localhost/anvilsaba/public-api:test localhost/anvilsaba/db-migrator:test -c anvilsaba
```

## DB と Secret の準備

新規の統合 DB を管理者で作成し、`deploy/postgres/bootstrap.sql` を `psql -v ON_ERROR_STOP=1 -f` で一度適用する。
既存 DB では作成済みのロールを再作成せず、[database.md](database.md) の権限を確認する。
旧所有者で適用済みの DB はこの初期化 SQL の対象外。既存のマイグレーション所有者を維持し、専用 Job のみがその資格情報を使用する。
アプリへ DB・スキーマ所有権、CREATEROLE、スーパーユーザー権限を渡さない。

psql の `\password <ロール名>` で各 LOGIN ロールに別パスワードを設定し、同じ値を次の Secret の password キーへ登録する。
実値はコミットしない。PostgreSQL 初期化ユーザーの Secret `postgres` はアプリ用と別に保持する。

| Secret | DB ロール |
|---|---|
| bot-database | platform_bot |
| mc-link-server-database | platform_mcguildlink |
| public-api-database | platform_public_api |
| migration-database | platform_migrator_job |

`mc-link-server-config` Secret の config.toml キーには次を入れる。コンテナではループバックに bind しない。

```toml
[server]
listen = "0.0.0.0:25565"
```

Secret はアプリと同じ namespace に作成する。`mcLinkServer.configSecretName` と `mcLinkServer.databaseSecretName` で名前を変更できるが、キーはそれぞれ `config.toml` と `password` を使用する。旧 Kotlin 用の `mcguildlink-config`（`app.toml`）は流用しない。DB ロール名 `platform_mcguildlink` は既存の PostgreSQL 権限に合わせて維持する。

Bot の設定は `apps/bot/config.sample.toml` を基に bot-config の config.toml キーへ登録する。
実 Discord 確認を行う場合だけ開発専用 token・guild・監査チャンネルを使い、Bot replicas=1 を指定する。
通常の統合環境は Bot replicas=0 とし、Discord 配送は Rust テストの送信境界で検証する。

## 配置順序

まず既存の開発手順で k3d と namespace、PostgreSQL Secret を準備する。
初回はアプリと Job を無効にして PostgreSQL を先に配置し、Ready 後に DB ロールと Secret を準備する。続いて Job を完了させ、その後でアプリを起動する。post-install/post-upgrade hook は PostgreSQL の配置後に実行され、Helm は Job の完了を待つ。旧 Kotlin 用 PVC を保持したまま replicas=0 にするため、一括の --wait は指定せず、各リソースの Ready を個別に確認する。

```powershell
helm upgrade --install platform deploy/helm/platform -n anvilsaba --create-namespace -f deploy/helm/platform/values.integration.yaml --set mcLinkServer.replicas=0 --set publicApi.replicas=0 --set migration.enabled=false --timeout 10m
kubectl rollout status statefulset/postgres -n anvilsaba
Get-Content deploy/postgres/bootstrap.sql -Raw | kubectl exec -i -n anvilsaba postgres-0 -- psql -U platform_migrator -d platform -v ON_ERROR_STOP=1
# ここで「DB と Secret の準備」に従い、各ロールのパスワードと Secret を設定する。
helm upgrade platform deploy/helm/platform -n anvilsaba -f deploy/helm/platform/values.integration.yaml --set mcLinkServer.replicas=0 --set publicApi.replicas=0 --set migration.enabled=true --timeout 10m
kubectl logs -n anvilsaba job/db-migrator
helm upgrade platform deploy/helm/platform -n anvilsaba -f deploy/helm/platform/values.integration.yaml --set migration.enabled=false --timeout 10m
kubectl rollout status deployment/mc-link-server -n anvilsaba
kubectl rollout status deployment/public-api -n anvilsaba
kubectl run curl-test -n anvilsaba --rm --restart=Never -i --image=curlimages/curl:8.15.0 -- http://public-api:8080/whitelist.json
```

専用 apps/db-migrator アプリは `sqlx::migrate!()` で SQL をバイナリへ埋め込み、sqlx の適用履歴・チェックサム・ロックを使用する。platform-database は接続設定・互換性検証を共有するライブラリとして保持し、リリース対象にしない。
失敗した Job は残し、ログと履歴を確認してから再実行する。アプリ自身は DDL を実行しない。
Job を無効化した通常のリリースではマイグレーションは実行しない。

## 互換期間と更新順序

既存3アプリは必要な migration ID・成功状態・チェックサムを起動時に検証する。
未適用や不一致ではポートを開く前に失敗する。未知の追加履歴は許可するが、互換性の自動判定ではない。

1. 追加的なテーブル・列・権限を専用 Job で先に適用する。既存 SQL は変更しない。
2. 旧アプリが新スキーマで動くことを検証した後、変更が必要なアプリだけを更新する。
3. 旧アプリがすべて停止し、切り戻し対象から外れるまで旧列・関数・権限を保持する。この期間が旧新版の共存期間となる。
4. 破壊的変更は別リリースで、影響する全アプリを対応・停止した後に適用する。

アプリの切り戻しはタグだけを旧版へ戻す。追加的マイグレーションは残す。
Helm のロールバックでは DB を戻せない。down SQL はデータ削除を含むため運用の切り戻しに使わない。
Kotlin 版は PostgreSQL 共存の対象外で、旧 SQLite と PVC・設定・イメージを保持する。
旧版と Rust 版の業務書き込みを同時に解禁しない。実データの移行・切り戻し検証は後続の移行作業で実施する。

## 自動検証と手動確認

テスト用 PostgreSQL の DATABASE_URL を指定して `cargo test --workspace --locked` を実行する。
既存のテストでコード発行・再利用・消費、紐付け、解除、関連ブロック、HTTP 一覧、503、監査の原子性、配送失敗・再開を確認する。
実 Discord の常時接続は不要。`platform-database` の互換性テストは旧・新の必須 ID リストを同じ DB で適用前後に検証する。
`scripts/check-deployment.ps1` は分離構成・本番既定値・必須タグを検証する。

統合クラスタでは Pod の起動、HTTP、PostgreSQL 再起動後のデータ保持を確認する。
障害確認はテスト環境で PostgreSQL を一時停止し、API の /whitelist.json が503になることと、復旧後に200へ戻ることを確認する。
実 Discord の配送と Minecraft Java 26.3 の本人認証→ダイアログ→結果表示→切断は手動確認とする。
Minecraft の確認は `kubectl port-forward -n anvilsaba service/mc-link-server 25565:25565` を使用し、本番ポートへ接続しない。

## 公開 API の Cloudflare Tunnel 経路

cloudflared は `cloudflare-tunnel` Secret の `token` を使う remotely-managed Tunnel。公開 hostname と転送先は Cloudflare 側で管理し、Helm の配置・リリースでは変更しない。[Cloudflare の Kubernetes 手順](https://developers.cloudflare.com/tunnel/guides/kubernetes/)を参照する。

開発専用 Tunnel と hostname で確認する場合は、Cloudflare の Published application の Service URL を `http://public-api:8080` に設定する。これは cloudflared と public-api が同じ namespace にある場合の宛先。別 namespace からは `http://public-api.<配置namespace>.svc.cluster.local:8080` を使う。`publicApi.port` を変更した場合は転送先ポートも合わせる。

開発専用 token を同じ namespace の `cloudflare-tunnel` Secret の `token` キーに登録し、統合構成を `--set cloudflared.replicas=1` で更新する。先に public-api の Ready とクラスタ内の `http://public-api:8080/whitelist.json` を確認し、その後 `https://<DEV_WHITELIST_HOSTNAME>/whitelist.json` の200と既存JSON形式を確認する。DB 停止時の503も外部経路で確認する。Tunnel 経由の実確認は手動で、通常の自動テストには要求しない。

public-api は ClusterIP のまま公開し、NodePort・LoadBalancer は不要。本番 Tunnel の現在の `http://mcguildlink-http:8080` と hostname は維持する。本番切替時には別途、同じ hostname の転送先を public-api へ変更する必要があり、切り戻しでは旧転送先に戻す。この作業では本番経路を変更しない。
