# 旧 Kotlin 版から分離構成への移行

この文書は、旧 Kotlin 版・SQLite・Chart 0.x から、MC Link Server・Bot・公開 API・PostgreSQL の分離構成へ移行するための手順です。通常のリリース・配置は[リリース手順](releases.md)と[独立デプロイ手順](independent-deployment.md)を参照してください。

## リリースの準備

Bot・mc-link-server・public-api・db-migrator を `deploy=false` で先行リリースし、公開済みイメージのタグを用意します。分離構成の Chart の初回公開は Release Action で **chart / bump=major / deploy=false** を手動指定します。旧構成の値だけとは互換性がないため、初回のメジャー更新を自動の bump 判定に任せません。

旧 Kotlin 版はリリース・CHANGELOG 生成の対象から外れています。既存の `mcguildlink/v*` と CHANGELOG は過去の履歴として保持します。移行検証には `apps/mcguildlink` のソース・Dockerfile、公開済みイメージと旧 Chart を使用します。分離構成の Chart は旧版の Deployment・Service・PVC・設定を含みません。

## 旧版停止とデータ保護

旧版を停止し、SQLite のバックアップと旧 Chart・values・設定・イメージタグを保存します。旧版と Rust 版の業務書き込みを同時に解禁しません。Kotlin 版は PostgreSQL のスキーマ互換期間の対象外です。

## 旧 Chart からの更新前の SQLite 保護

新 Chart から削除されたリソースは、旧 Helm release の更新時に削除対象になる。旧版を停止し、SQLite のバックアップと旧 Chart・values を保存する。切り戻しに必要な `mcguildlink-data` PVC は、更新前に旧 Chart の PVC テンプレートへ `metadata.annotations.helm.sh/resource-policy: keep` を追加したローカルの旧 Chart で一度更新し、`helm get manifest platform -n anvilsaba` の PVC に注釈が保存されたことを確認する。旧 Chart で更新するときも旧版の replicas=0 を維持する。

PVC へ直接 annotate するだけでは、旧 release の保存済み manifest に反映されないため、更新時の保持を保証できない。保護とバックアップを確認してから新 Chart の初期配置へ進む。保護した PVC は新 Chart に管理させず、移行・切り戻し検証を終えるまで保持する。[Helm の保持指定](https://helm.sh/docs/howto/charts_tips_and_tricks/#tell-helm-not-to-uninstall-a-resource)と[保存済み manifest の制約](https://github.com/helm/helm/issues/8132)を参照する。

## 分離構成のセットアップ

旧 SQLite の PVC・設定・イメージは新 Chart の管理対象から外し、移行検証・切り戻し用に保持します。実データ・設定の変換と代表データによる検証は、分離構成へ切り替える前に完了させます。

[DB と Secret の準備](independent-deployment.md#db-と-secret-の準備)と[配置順序](independent-deployment.md#配置順序)に従い、PostgreSQL→DB ロール・Secret→専用 Job→アプリの順に配置します。新 Chart には Bot・MC・API の公開済みイメージタグを設定します。MC/API は既定 replicas=1 なので、DB 準備中は明示的に0へ上書きし、準備後に1へ戻します。

初期セットアップを完了してからデプロイ Action を使用します。Action はセットアップ済み Helm release の値を引き継ぎ、対象のイメージタグだけを更新します。

## 公開経路と切り戻し

旧本番の Cloudflare Tunnel 転送先 `http://mcguildlink-http:8080` と hostname は移行まで維持します。切替時は同じ hostname の転送先を public-api の Service へ変更し、`/whitelist.json` の応答と JSON 形式を確認します。MC の Service 公開先も新サーバーへ切り替え、実クライアントで検証します。

移行・起動確認中は業務書き込みを停止します。書き込み解禁前に検証が失敗した場合は新アプリを停止し、保存した旧 Chart・values・SQLite・設定・イメージと旧公開経路に戻します。新 Chart の Helm rollback だけでは PostgreSQL の内容は戻りません。書き込み解禁後の PostgreSQL→SQLite の逆移行はこの手順の対象外です。
