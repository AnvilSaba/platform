# Secret / Config の準備と運用

通常設定は `deploy/config/production/bot.toml`、秘密情報は同ディレクトリの `secrets/*.sops.yaml` に分離します。
本番 Config、age 公開鍵、暗号化済み Secret は登録済みです。以下は新規環境の準備と設定更新の手順です。age 秘密鍵は Git 外で保持します。
認証の合言葉は Bot 専用 Secret の `authKeyword` に保存し、環境変数 `AUTH_KEYWORD` で渡します。通常設定の `auth.keyword` は削除します。

| 利用者         | 通常設定                         | Secret / キー                                                                                                       |
| -------------- | -------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| Bot            | `bot` ConfigMap の `config.toml` | `bot/token` → `DISCORD_BOT_TOKEN`、`bot/databasePassword` → `DATABASE_PASSWORD`、`bot/authKeyword` → `AUTH_KEYWORD` |
| MC Link Server | Helm values のポート等           | `mc-link-server/databasePassword`                                                                                   |
| 公開 API       | Helm values のポート等           | `public-api/databasePassword`                                                                                       |
| Migrator       | Helm values の DB 接続先等       | `db-migrator/databasePassword`                                                                                      |
| PostgreSQL     | Helm values の DB 名等           | `postgres/databasePassword`                                                                                         |
| cloudflared    | Helm values の起動引数等         | `cloudflared/token` → `TUNNEL_TOKEN`                                                                                |

ConfigMap と Secret の名前はサービス名に揃えます。同じ namespace 内でも種別が異なるため、どちらも `bot` という名前にできます。
Secret はサービスごとに 1 つにまとめ、Token は `token`、DB パスワードは `databasePassword`、認証の合言葉は `authKeyword` に揃えます。
DB 接続先はパスワードなしの `DATABASE_URL`、パスワードはサービスごとの Secret から渡します。
GHCR イメージは Public とし、取得用の認証 Secret は使いません。
Pod は `secretKeyRef` で必要なキーだけを受け取り、Kubernetes API の ServiceAccount Token は自動マウントしません。
Pod 用の Secret 読み取り RBAC は追加しません。

## 1. 鍵と公開設定を用意する

管理端末に mise を導入し、リポジトリルートで `mise install` を実行します。
以下の作成・暗号化例は PowerShell 7 用です。kubectl も mise で導入します。クラスタへの接続には kubeconfig を設定してください。
管理端末は mise で固定した age 1.3.2 と SOPS 3.13.3 を使用します。どちらも PQ 鍵に対応しています。

age の PQ 鍵（X25519 + ML-KEM-768 ハイブリッド）をリポジトリ外に作成します。既存の PQ 鍵は上書きせず再利用できます。従来の X25519 鍵はこの構成には使いません。

```powershell
$ageDirectory = Join-Path $env:USERPROFILE '.config/sops/age'
New-Item -ItemType Directory -Force -Path $ageDirectory | Out-Null
$ageKeyFile = Join-Path $ageDirectory 'keys-pq.txt'
mise exec -- age-keygen -pq -o $ageKeyFile
if ($LASTEXITCODE -ne 0) { throw 'age 鍵の作成に失敗しました' }
$recipient = mise exec -- age-keygen -y $ageKeyFile
if ($LASTEXITCODE -ne 0) { throw 'age 公開鍵の取得に失敗しました' }
(Get-Content .sops.yaml -Raw).Replace('REPLACE_WITH_AGE_PQ_PUBLIC_KEY', $recipient.Trim()) |
  Set-Content -Encoding utf8NoBOM .sops.yaml
```

公開鍵を設定した `.sops.yaml` は Git 管理します。秘密鍵は端末のファイル権限で本人だけが読める状態にし、Git 外でバックアップします。
SOPS に PQ 秘密鍵の場所を明示します。公開鍵は `age1pq1…`、秘密鍵は `AGE-SECRET-KEY-PQ-1…` の形式です。従来鍵の受信者を追加しません。

```powershell
$env:SOPS_AGE_KEY_FILE = $ageKeyFile
```

`deploy/config/production/bot.toml` の Guild / Channel / Role ID、所有者、メッセージ、各制限を本番値へ変更します。
サンプルの `000000000000000000` を残さないでください。Token や Credential は追加しません。
MC Link Server / 公開 API 等の通常設定は `deploy/helm/platform/values.prod.yaml` に置きます。

## 2. サービス別 Secret を暗号化する

`deploy/config/samples/*.sample.yaml` は Kubernetes Secret の構造サンプルです。置換用の値が入っているので、そのまま適用しません。
まずサンプルを暗号化し、SOPS のエディターで本番値へ変更します。Git に置くファイルは最初から暗号化済みです。
リポジトリルートで `$secretName` を表の各 Secret 名に変更して実行します。

```powershell
$secretName = 'bot' `
$secretPath = "deploy/config/production/secrets/$secretName.sops.yaml" `
Get-Content "deploy/config/samples/$secretName.sample.yaml" -Raw |
  mise exec -- sops encrypt --filename-override $secretPath --input-type yaml --output-type yaml --output $secretPath `
if ($LASTEXITCODE -ne 0) { throw 'SOPS 暗号化に失敗しました' } `
mise exec -- sops edit $secretPath
```

既存の Bot Secret に合言葉を追加する場合は `mise exec -- sops edit deploy/config/production/secrets/bot.sops.yaml` で開き、`stringData.authKeyword` に実値を入力します。通常のエディターで暗号化ファイルを直接編集しません。

SOPS は一時的に復号した内容をエディターで開き、保存後に再暗号化します。
必要に応じて `$env:SOPS_EDITOR = 'code --wait'` 等でエディターを指定します。
平文を扱うのは管理端末の編集用一時ファイルだけです。ノードに平文の `.env` や Secret 原本を配置しません。

暗号化した 6 ファイルを `deploy/config/production/secrets/kustomization.yaml` に登録します。

```yaml
apiVersion: kustomize.config.k8s.io/v1beta1
kind: Kustomization
resources:
  - bot.sops.yaml
  - mc-link-server.sops.yaml
  - public-api.sops.yaml
  - db-migrator.sops.yaml
  - postgres.sops.yaml
  - cloudflared.sops.yaml
```

```powershell
mise exec -- kustomize build deploy/config/production
```

Kustomize が生成できることを確認し、本番値・必要な Secret の登録は担当者が確認します。
SOPS の [公式手順](https://getsops.io/docs/usage/advanced/) に従い、変更時は `mise exec -- sops edit <ファイル>` を使います。
名前や namespace を含む暗号化ファイルの平文部分も MAC の対象です。暗号文や SOPS メタデータを直接編集しません。

## 3. Flux 導入前に適用する

既存 Helm は Secret と ConfigMap を参照し、値を Helm release に保存しません。
管理端末から以下を実行します。平文はパイプだけを通し、ノードのデプロイディレクトリには保存しません。
SOPS 復号は Kustomize による namespace 変更より先に行います。

```powershell
mise exec -- kubectl apply -f deploy/config/production/namespace.yaml
if ($LASTEXITCODE -ne 0) { throw 'namespace の準備に失敗しました' }
$secrets = Get-ChildItem deploy/config/production/secrets/*.sops.yaml
foreach ($file in $secrets) {
  $decrypted = mise exec -- sops decrypt $file.FullName
  if ($LASTEXITCODE -ne 0) { throw '復号に失敗しました' }
  $decrypted | mise exec -- kubectl apply -f -
  if ($LASTEXITCODE -ne 0) { throw 'Secret の適用に失敗しました' }
  Remove-Variable decrypted
}
```

Kustomize の本番ルートには暗号化 Secret も含まれるので、Flux 導入前は **build の出力をそのまま apply しません**。
ConfigMap は次のように同じ本番設定ファイルから適用します。

```powershell
mise exec -- kubectl -n anvilsaba create configmap bot `
  --from-file=config.toml=deploy/config/production/bot.toml --dry-run=client -o yaml |
  mise exec -- kubectl apply -f -
```

その後は [本番デプロイ手順](deployment.md#initial-setup) の DB 準備、Migrator、アプリ起動の順序を守ります。
DB ロールのパスワードは対応する Secret の値と一致させます。既存 DB は再初期化しません。

通常設定は固定名 ConfigMap を `subPath` でマウントするため、更新後は `mise exec -- kubectl -n anvilsaba rollout restart deployment/bot` が必要です。
Secret の環境変数も自動更新されないため、更新した Secret を利用するワークロードだけを再起動します。
PostgreSQL の Secret 更新だけでは既存 DB のパスワードは変わりません。DB ロールの変更と対応する Secret・Pod の更新を合わせて行います。

## 4. Flux に引き継ぐ

`deploy/flux/platform-config.sample.yaml` は同じ Kustomize ルートを参照する Flux Kustomization のサンプルです。
Flux の `GitRepository/platform` を用意し、導入済みの `flux-system` に PQ 秘密鍵だけを渡します。
PQ 鍵の復号には対応した kustomize-controller が必要です。この手順は [v1.9.0 の SOPS / age 依存関係](https://github.com/fluxcd/kustomize-controller/blob/v1.9.0/go.mod) を対応基準とします。管理端末の age を更新しても、controller 内蔵の復号処理は更新されません。

```powershell
mise exec -- kubectl -n flux-system create secret generic sops-age `
  --from-file=identity.agekey=$ageKeyFile --dry-run=client -o yaml |
  mise exec -- kubectl apply -f -
mise exec -- kubectl apply -f deploy/flux/platform-config.sample.yaml
```

`.agekey` で終わるキー名と `decryption.secretRef` は [Flux の SOPS 復号仕様](https://fluxcd.io/flux/components/kustomize/kustomizations/#decryption) に対応しています。
HelmRelease を後から導入する場合は Config の Kustomization への `dependsOn` を設定し、既存のマイグレーション順序を維持します。
固定名 ConfigMap と環境変数 Secret の更新後の再起動も同じです。Flux による Config / Secret の反映だけでは Pod は再起動しません。

## 5. 既存環境の移行

古い `config.toml` の `bot.token` を `DISCORD_BOT_TOKEN`、`auth.keyword` を `AUTH_KEYWORD` に移し、ファイルから削除します。開発用は Git 除外済みの `apps/bot/.env`、本番用は Bot 専用 Secret に保存します。古い項目を残すと起動・再読み込みは拒否されます。
開発用の Git 除外済み `apps/bot/config.toml` も同じ移行が必要です。既存の `.env` や設定をサンプルで上書きしないでください。
既存の DB 資格情報は各サービスの `<service>/databasePassword` に移します。Bot は Token、DB パスワード、認証の合言葉を同じ `bot` に入れます。DB 側のパスワードは変更せず、既存値と一致させます。
新しい ConfigMap と Secret を先に用意し、対応する新しい Bot イメージと Chart を一緒に切り替えます。
旧 Bot イメージは Token を設定ファイルに要求するため、新しい ConfigMap との組み合わせでは起動できません。
切り戻し時もイメージ・Chart・設定方式の互換性を揃えてください。
移行を確認したら、旧 `bot-config` **Secret** と旧平文の Secret 原本を削除します。新しい `bot` ConfigMap と `bot` Secret は残します。

```powershell
mise exec -- kubectl -n anvilsaba delete secret bot-config
```

Git に追加する前に、暗号化済みファイルだけが変更されていることを確認します。
`.gitignore` は `.env`、`.agekey`、`*.plain.yaml` を除外しますが、Git への強制追加や過去の履歴は防ぎません。
以前に実際の資格情報をコミットした場合は失効・再発行し、履歴からの削除は別途対応してください。
