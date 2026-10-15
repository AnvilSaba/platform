# Flux による本番の GitOps

Flux 本体・CloudNativePG Operator・DB・ロール・Secret・Config・アプリの Helm release を Git から同期します。Actions はイメージと Chart を公開し、本番で使うバージョンを Git に記録します。クラスタへの SSH 接続は不要です。

## リポジトリと管理対象

現在はこのリポジトリの `deploy/` にまとめます。アプリ・Migrator・DB の互換性に関わる変更を同じリポジトリでレビューできます。運用担当や書き込み権限を分ける必要が出たら別リポジトリへ移し、`GitRepository` の URL と各同期パス、Actions の本番設定更新先を変更します。[Flux のリポジトリ構成例](https://fluxcd.io/flux/guides/repository-structure/) も参照してください。

| 定義                            | 役割                                                         |
| ------------------------------- | ------------------------------------------------------------ |
| `deploy/flux/production`        | `main` を参照する GitRepository と各 Flux Kustomization      |
| `deploy/flux/controllers`       | Flux 2.9.6 の固定マニフェスト                                |
| `deploy/postgres/operator`      | CloudNativePG 1.30.1 の固定マニフェスト                      |
| `deploy/config/production`      | namespace・ConfigMap・SOPS Secret                            |
| `deploy/postgres/import`        | 既存 DB からインポートする Cluster                           |
| `deploy/postgres/roles`         | Secret のパスワードを反映する DatabaseRole                   |
| `deploy/flux/apps/chart.yaml`   | 公開済み OCI Chart の固定バージョン                          |
| `deploy/flux/apps/values.yaml`  | アプリと Migrator の固定イメージタグ                         |
| `deploy/flux/apps/release.yaml` | `anvilsaba` の既存 release `platform` を引き継ぐ HelmRelease |

適用順序は Config / Secret と Operator → DB Ready → ロール反映 → Helm release です。`dependsOn` と health check により、インポート完了前にロールを上書きしません。DatabaseRole は `applied=true` と `observedGeneration` の一致まで待ちます。[依存関係と health check の仕様](https://fluxcd.io/flux/components/kustomize/kustomizations/#dependencies)

DB・Operator・ロール・アプリ・Flux 本体の Kustomization は `prune: false` / `deletionPolicy: Orphan` とし、定義や同期設定を削除しても管理対象を自動削除しません。Config は削除同期を有効にしていますが、共有 namespace は prune 除外です。Cluster や namespace の直接削除、ノード障害はこの設定では防げません。

## 初回の接続

デプロイ先の Bash で実行します。mise は不要です。Flux は [2.9.6](https://github.com/fluxcd/flux2/releases/tag/v2.9.6) に固定し、以後の更新は `deploy/flux/controllers/kustomization.yaml` で管理します。既存の Flux がある場合は先にバージョンと既存の同期対象を確認します。

```bash
kubectl apply --server-side -f https://github.com/fluxcd/flux2/releases/download/v2.9.6/install.yaml
kubectl rollout status deployment/source-controller -n flux-system --timeout=5m
kubectl rollout status deployment/kustomize-controller -n flux-system --timeout=5m
kubectl rollout status deployment/helm-controller -n flux-system --timeout=5m
```

age 秘密鍵を持つ管理端末で [Secret 手順](secret-config.md#4-flux-に引き継ぐ) に従い、`flux-system/sops-age` を一度だけ作成します。秘密鍵は Git に登録しません。Flux の kustomize-controller 内蔵 SOPS / age が PQ 鍵に対応する必要があります。

`source.yaml` の参照先は `AnvilSaba/platform` の `main` です。非公開リポジトリの場合は読み取り専用の Git 認証 Secret を `flux-system` に用意し、`GitRepository.spec.secretRef.name` を設定してください。認証情報の実値はこのリポジトリに保存しません。

上記の設定が参照先ブランチにあることを確認してから適用します。

```bash
kubectl apply -k deploy/flux/production
kubectl wait gitrepository/platform -n flux-system --for=condition=Ready --timeout=5m
kubectl wait kustomization/platform -n flux-system --for=condition=Ready --timeout=5m
kubectl wait kustomization/cnpg-operator -n flux-system --for=condition=Ready --timeout=5m
```

初期状態では `platform-config`・`platform-db` と HelmRelease の `suspend: true` により、既存の Secret 変更・DB インポート・アプリ切り替えを保留します。解除はライブの patch ではなく、以下の手順で Git を変更して行います。

## DB とアプリの引き継ぎ

既存 DB がある場合は [DB 移行手順](cloudnativepg-migration.md) のバックアップと書き込み停止を先に行います。`platform-db.spec.path` は `./deploy/postgres/import` を維持します。新規環境だけ `./deploy/postgres/cloudnativepg` に変更し、空の DB を作成します。

1. `production/config.yaml` と `production/database.yaml` の `platform-db` の `spec.suspend` を `false` にして Git に反映します。Secret、DB、ロールが依存順に同期されます。
2. 以下の Ready を確認し、既存 DB では移行 SQL と主要データも比較します。パスワードの反映はログイン用 DatabaseRole の `secretResourceVersion` と各 Secret の `resourceVersion` の一致まで確認します。
3. 移行対応 Chart を公開し、`apps/chart.yaml` の `spec.ref.tag` をそのバージョンにします。初期値の `1.0.0` は既存タグの記録で、移行対応版を示しません。旧 DB を内包する Chart では解除しません。
4. `apps/values.yaml` の3アプリのタグと Migrator を、引き継ぐリリースに揃えます。初期値はローカルのリリースタグを元にした値で、稼働環境の現在値を取得したものではありません。既存 release の values でカスタマイズした replica 数なども Git に移してください。Flux は `--reuse-values` を使いません。
5. `apps/release.yaml` の `spec.suspend` を `false` にして Git に反映します。Migrator の正常完了後にアプリが配置されます。移行中は後述の停止設定を使用します。

```bash
kubectl wait kustomization/platform-config -n flux-system --for=condition=Ready --timeout=5m
kubectl wait kustomization/platform-db -n flux-system --for=condition=Ready --timeout=35m
kubectl wait kustomization/platform-db-roles -n flux-system --for=condition=Ready --timeout=5m
kubectl get databaserole -n anvilsaba
kubectl wait helmrelease/platform -n anvilsaba --for=condition=Ready --timeout=15m
kubectl logs -n anvilsaba job/db-migrator
```

停止したまま移行対応 Chart と Migrator を適用するには、`apps/values.yaml` の `bot`・`mcLinkServer`・`publicApi`・`cloudflared` に `replicas: 0` を記録します。Migrator が正常完了し旧 PVC が保持されていることを確認してから、停止前の replica 数を Git に戻します。

インポート後も `platform-db` は import のパスを維持します。同じ Cluster への再適用ではデータを再コピーしません。

## リリースと切り戻し

**Release** の `deploy=true` または **Production deployment** により、対象の公開済みタグから `values.yaml` / `chart.yaml` を更新するコミットを `main` に作成します。アプリの変更では対象アプリと同じリビジョンの Migrator だけを更新し、他のアプリと Chart の固定値を維持します。Chart 更新では Chart の固定値だけを更新します。

GitHub Actions の `GITHUB_TOKEN` に `contents: write` が必要です。ブランチ保護などで Actions から `main` に書き込めない場合は更新を拒否し、クラスタを直接変更しません。担当者が同じ設定変更を PR として反映してください。Actions の成功は Git への記録完了を示します。デプロイ完了は HelmRelease の Ready と各 Pod のログで確認します。

Migrator は `pre-install,pre-upgrade` hook です。失敗した場合は新しいアプリを適用しません。upgrade の失敗時は Helm が旧 release へ切り戻しますが、DB の変更は取り消しません。Git のバージョンを戻す場合も [スキーマ互換性](releases.md#マイグレーションの互換性) を確認します。

Flux 管理後に通常の `helm upgrade` を直接実行すると Git の設定へ戻されます。緊急の手動操作では親 `platform` と対象の Kustomization / HelmRelease を一時停止し、復旧後は Git と状態を揃えてから再開します。旧 DB へ切り戻す場合は Config / Secret の同期も停止します。

Git の更新後は、対象 Kustomization の `status.lastAppliedRevision` が反映するコミットになったことも確認します。`kubectl wait --for=condition=Ready` だけでは前回の Ready を拾う場合があります。HelmRelease でも `status.observedGeneration` と `metadata.generation` の一致、今回の設定に対応する `status.lastAttemptedConfigDigest` / `status.lastAttemptedRevision`、Pod のイメージを確認します。

ConfigMap と環境変数 Secret の更新だけでは Pod は再起動しません。[パスワードの反映・再起動手順](secret-config.md) に従います。
