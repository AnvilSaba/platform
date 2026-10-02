# 最新の旧リリースからのk3d移行リハーサル

新構成だけの起動確認ではなく、最新の旧リリースを起動した状態から、同じクラスタ・namespace・Helm releaseを新版へ更新します。切り戻しの検証は別文書で明示的に選んで実行します。移行完了後、旧 `apps/mcguildlink` を削除するときに移行スクリプトもまとめて削除します。

## VM準備と接続の前提

コマンドはPowerShell 7で実行します（UTF-8の入出力と、後続のEncoding/RNG APIを使用）。Windows PowerShell 5.1は対象外です。

専用rootful VM `podman-machine-rehearsal` が作成・起動済みであることを前提にします。既存VM・既存クラスタは削除・再初期化・設定変更しません。未準備なら手順1へ進まず、専用VMだけを準備します。同名VMがある場合はinitせず、停止中ならstartだけを実行してください。

```powershell
# 同名VMが存在しない場合だけ作成する。
podman machine init --rootful podman-machine-rehearsal
if ($LASTEXITCODE -ne 0) { throw '専用VMの作成に失敗しました。既存VMを削除せず停止してください' }
podman machine start podman-machine-rehearsal
if ($LASTEXITCODE -ne 0) { throw '専用VMの起動に失敗しました' }
```

準備後、同じPowerShellで接続とコンテナ起動を確認します。失敗した場合は停止し、[過去のVM起動障害の記録](mcguildlink-rehearsal-results.md)を参照してください。cgroup修復はVM再起動後も有効とは限りません。

```powershell
$buildConnection = 'podman-machine-rehearsal-root'
podman --connection $buildConnection info --format '{{.Host.Arch}}' | Out-Null
if ($LASTEXITCODE -ne 0) { throw '専用VMが未準備か接続不能です。準備手順を実施してください' }
podman --connection $buildConnection run --rm docker.io/library/busybox:1.37.0 true
if ($LASTEXITCODE -ne 0) { throw '専用VMでコンテナを起動できません。クラスタ作成へ進みません' }
$env:DOCKER_HOST = 'npipe:////./pipe/podman-machine-rehearsal'
```

## 再現する旧環境

2026-10-01にGitHubの公開タグを確認した基準は次のとおりです。GitHub Release一覧は未登録で、リリースタグを基準にしています。実行時もタグと対応する公開イメージを確認し、使用したdigestを記録します。開発中のHEADから旧版をビルドして置き換えません。

| 対象 | 基準 |
|---|---|
| Chart | `chart/v0.2.1` / OCI Chart `0.2.1` |
| Bot | `bot/v3.5.1` / `ghcr.io/anvilsaba/bot:v3.5.1` |
| Kotlin MCGuildLink | `mcguildlink/v1.0.2` / `ghcr.io/anvilsaba/mcguildlink:v1.0.2` |
| Kubernetes | 本番と同じk3sバージョンをk3dの `--image` に指定 |
| namespace / release | `anvilsaba` / `platform` |
| 旧MC公開経路 | LoadBalancer Service `mcguildlink-minecraft`、本番既定の25600/TCP |
| 旧HTTP公開経路 | 開発専用Tunnel・hostname → `http://mcguildlink-http:8080` |
| 保存先 | SQLite PVC `mcguildlink-data`、PostgreSQL PVC `postgres-data` |

旧Chartの既定PostgreSQLは `anvilsaba` DB / `anvilsaba` ユーザー / Secret `postgres` です。新構成の `platform` / `platform_admin` / `postgres-db-credentials` に値を変えるだけでは、既存PVC内のDB・ロール・passwordは初期化し直されません。今回は旧PostgreSQLをPVCから作り直します。旧SQLite PVCも保存済みDBファイルの検証後に削除し、切り戻しでは新しいPVCへファイルを復元します。最新の旧BotはPostgreSQLを使わず、移行元の業務データはSQLiteにあります。

旧リリースの公開イメージはarm64のみのため、このリハーサルでは別フォルダへリリースタグのソースを取得し、amd64向けにビルドします。使用するソースは本番リリースと同じですが、CPUアーキテクチャとイメージdigestは異なります。

## 手順一覧

1. 旧リリースを起動する
2. 停止・SQLiteファイルの保存
3. PostgreSQLを削除して作り直す（3.1 パスワードとSecret登録、3.2 Bot設定変換、3.3 Migrator実行と書き込み凍結）
4. 旧SQLiteを移行し、新版へ経路を切り替える

| 目的 | 実行する入口と順序 |
|---|---|
| 通常移行のみ（凍結を維持） | この文書の1〜4で終了 |
| 切り戻し | 1〜4 → [R0〜R3](mcguildlink-k3d-rollback.md)で終了 |
| 直接解禁 | 1〜4 → [U2〜U3](mcguildlink-k3d-unfreeze-test.md#u2-新アプリを停止して書き込みを解禁する)。U1は不要 |
| 切り戻し後解禁 | 1〜4 → R0〜R3 → [U1〜U3](mcguildlink-k3d-unfreeze-test.md) |

## 1. 旧リリースを起動する

Python 3.11以降、GitHub CLI、k3d、kubectl、Helm、Podmanを用意します。コマンドはリポジトリルートのPowerShellで実行します。作業ディレクトリはGitの外に作ります。エージェントが `gh` を実行する場合はsandbox外で実行します。

```powershell
$rehearsalDir = Join-Path $env:TEMP 'mcguildlink-rehearsal'
New-Item -ItemType Directory -Path $rehearsalDir -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $rehearsalDir 'sources') -Force | Out-Null
$botSource = Join-Path $rehearsalDir 'sources/bot-v3.5.1'
$mcSource = Join-Path $rehearsalDir 'sources/mcguildlink-v1.0.2'
$chartSource = Join-Path $rehearsalDir 'sources/chart-v0.2.1'
gh repo clone AnvilSaba/platform $botSource -- --branch bot/v3.5.1 --depth 1
gh repo clone AnvilSaba/platform $mcSource -- --branch mcguildlink/v1.0.2 --depth 1
gh repo clone AnvilSaba/platform $chartSource -- --branch chart/v0.2.1 --depth 1
podman --connection $buildConnection build --isolation=chroot --platform linux/amd64 --build-arg TARGETARCH=amd64 -f (Join-Path $botSource 'apps/bot/Dockerfile') -t localhost/anvilsaba/bot-old:v3.5.1-amd64 $botSource
if ($LASTEXITCODE -ne 0) { throw '旧Botのビルドに失敗しました' }
podman --connection $buildConnection build --isolation=chroot --platform linux/amd64 --build-arg TARGETARCH=amd64 -f (Join-Path $mcSource 'apps/mcguildlink/Dockerfile') -t localhost/anvilsaba/mcguildlink-old:v1.0.2-amd64 $mcSource
if ($LASTEXITCODE -ne 0) { throw '旧MCGuildLinkのビルドに失敗しました' }
$oldChart = Join-Path $chartSource 'deploy/helm/platform'
$rehearsalContext = 'k3d-mcguildlink-rehearsal'
$rehearsalNamespace = 'anvilsaba'
$env:DOCKER_HOST = 'npipe:////./pipe/podman-machine-rehearsal'
$env:KUBECONFIG = Join-Path $rehearsalDir 'kubeconfig.yaml'
# 本番で確認した v1.36.4+k3s1 に合わせる。
k3d cluster create mcguildlink-rehearsal --image rancher/k3s:v1.36.4-k3s1 --port '127.0.0.1:25600:25600@server:0' --timeout 120s
# buildとsaveは同じ専用VMの明示connectionを使う。
$oldImageArchive = Join-Path $rehearsalDir 'old-images.tar'
podman --connection $buildConnection save --format docker-archive --output $oldImageArchive localhost/anvilsaba/bot-old:v3.5.1-amd64 localhost/anvilsaba/mcguildlink-old:v1.0.2-amd64
if ($LASTEXITCODE -ne 0) { throw '旧イメージの保存に失敗しました' }
k3d image import $oldImageArchive --mode direct -c mcguildlink-rehearsal
kubectl --context $rehearsalContext create namespace $rehearsalNamespace
```

### 1.1. 旧values・設定ファイルとSecretを生成する

旧Chartのキーは `git show chart/v0.2.1:deploy/helm/platform/values.yaml`、Secretのキーは同タグの `templates/*` で確認済みです。既定valuesを `helm show values` で保存し、JSON差分を後から `-f` で重ねます。HelmはJSON形式のvaluesも受け付けます。旧Chartを使う後続の更新・切り戻しにも両方のファイルを渡します。

入力はテスト用に調整済みの元Bot設定・元MCGuildLink設定のパス、開発専用Tunnel tokenファイルのパス、開発専用hostnameのみです。元設定にテストBot token・guild・role・監査チャンネルなどを設定し、本番tokenを流用しません。元ファイルは変更しません。作業ディレクトリは秘密値を含むため、アクセスを作業者に限定し、共有・Git登録・内容の画面出力をしません。

| 生成物 / Secret | 取得元・キー |
|---|---|
| `old-values.yaml` / `old-overrides.json` | タグのChart既定値 / 下記のJSON差分 |
| `config/bot-old.toml` / `bot-config` | 元Bot設定から `[mcguildlink]` を除く / `config.toml` |
| `config/mcguildlink-old.toml` / `mcguildlink-config` | 元旧MC設定の待受ポートを25600に変更 / `app.toml` |
| `config/postgres.password` / `postgres` | Python stdlibで使い捨てpassword生成 / `password` |
| `config/cloudflare.token` / `cloudflare-tunnel` | 指定したテストtokenファイル（末尾改行を除去） / `token` |

```powershell
$originalBotConfig = Read-Host 'テスト用元Bot設定のパス'
$originalMcConfig = Read-Host 'テスト用元MCGuildLink app.tomlのパス'
$originalTunnelToken = Read-Host 'テスト用Tunnel tokenファイルのパス'
$testHostname = Read-Host '開発専用Tunnel hostname（schemeなし）'
if ([Uri]::CheckHostName($testHostname) -ne [UriHostNameType]::Dns) { throw 'hostnameを確認してください' }
$oldValues = Join-Path $rehearsalDir 'old-values.yaml'
$oldOverrides = Join-Path $rehearsalDir 'old-overrides.json'
if ((Test-Path $oldValues) -or (Test-Path $oldOverrides) -or (Test-Path (Join-Path $rehearsalDir 'config'))) {
    throw '設定保存先が既にあります。上書きせず別の作業ディレクトリを使用してください'
}
$chartValues = helm show values $oldChart
if ($LASTEXITCODE -ne 0) { throw '旧Chart valuesの取得に失敗しました' }
[IO.File]::WriteAllText($oldValues, ($chartValues -join "`n"), [Text.UTF8Encoding]::new($false))
@'
import copy, json, pathlib, re, secrets, sys, tomllib

root = pathlib.Path(sys.argv[1])
bot_text = pathlib.Path(sys.argv[2]).read_text(encoding='utf-8-sig')
mc_text = pathlib.Path(sys.argv[3]).read_text(encoding='utf-8-sig')
# 秘密値を含む行を例外メッセージへ出さない。
try:
    original_bot = tomllib.loads(bot_text)
    original_mc = tomllib.loads(mc_text)
    bot_text = re.sub(r'(?ms)^\[mcguildlink\][ \t]*(?:#[^\n]*)?\r?\n.*?(?=^\[|\Z)', '', bot_text)
    section = re.search(r'(?ms)^\[minecraft_server\][^\n]*\n(.*?)(?=^\[|\Z)', mc_text)
    if section is None:
        raise ValueError()
    body, count = re.subn(r'(?m)^([ \t]*port[ \t]*=[ \t]*)[0-9]+', r'\g<1>25600', section[1])
    mc_text = mc_text[:section.start(1)] + body + mc_text[section.end(1):]
    expected_mc = copy.deepcopy(original_mc)
    expected_mc['minecraft_server']['port'] = 25600
    if count != 1 or tomllib.loads(mc_text) != expected_mc:
        raise ValueError()
    if tomllib.loads(bot_text) != {k: v for k, v in original_bot.items() if k != 'mcguildlink'}:
        raise ValueError()
    for key in ('token', 'guild', 'moderator_role', 'log_channel', 'display_server_address'):
        if not original_mc['bot'][key]:
            raise ValueError()
    if not original_bot['bot']['token']:
        raise ValueError()
except (ValueError, KeyError, TypeError):
    sys.exit('設定形式・必須値を確認してください（内容は表示しません）')
token = pathlib.Path(sys.argv[4]).read_text(encoding='utf-8').rstrip('\r\n')
if not token or any(c.isspace() for c in token) or '\0' in token or token.startswith('\ufeff'):
    sys.exit('Tunnel tokenはBOMなしの空でない単一行にしてください')
config = root / 'config'
config.mkdir()
for name, text in {'bot-old.toml': bot_text, 'mcguildlink-old.toml': mc_text,
                   'postgres.password': secrets.token_hex(24), 'cloudflare.token': token}.items():
    (config / name).write_text(text, encoding='utf-8')
overrides = {
    'imagePullSecrets': [],
    'bot': {'image': {'repository': 'localhost/anvilsaba/bot-old', 'tag': 'v3.5.1-amd64'}, 'configSecretName': 'bot-config'},
    'mcguildlink': {'image': {'repository': 'localhost/anvilsaba/mcguildlink-old', 'tag': 'v1.0.2-amd64'},
                   'configSecretName': 'mcguildlink-config', 'minecraft': {'port': 25600},
                   'sqlite': {'storageSize': '1Gi', 'storageClassName': 'local-path'}},
    'postgres': {'database': 'anvilsaba', 'username': 'anvilsaba', 'secretName': 'postgres',
                 'storageSize': '10Gi', 'storageClassName': 'local-path'},
    'cloudflared': {'tokenSecretName': 'cloudflare-tunnel'},
}
(root / 'old-overrides.json').write_text(json.dumps(overrides, indent=2), encoding='utf-8')
'@ | python - $rehearsalDir $originalBotConfig $originalMcConfig $originalTunnelToken
if ($LASTEXITCODE -ne 0) { throw '旧設定の生成に失敗しました' }
$oldSecrets = @(
    @('postgres', 'password', 'postgres.password'),
    @('bot-config', 'config.toml', 'bot-old.toml'),
    @('mcguildlink-config', 'app.toml', 'mcguildlink-old.toml'),
    @('cloudflare-tunnel', 'token', 'cloudflare.token')
)
foreach ($entry in $oldSecrets) {
    $file = Join-Path $rehearsalDir ('config/' + $entry[2])
    kubectl --context $rehearsalContext -n $rehearsalNamespace create secret generic $entry[0] "--from-file=$($entry[1])=$file"
    if ($LASTEXITCODE -ne 0) { throw "旧Secret作成に失敗しました: $($entry[0])" }
}
```

テストTunnelの管理画面で `$testHostname` の宛先を `http://mcguildlink-http:8080` に設定します。Windowsサービスのインストールは不要です。

UDP接続が失敗する環境では、旧Chartの作業用コピーと、新Chartの作業用コピーの両方にHTTP/2指定を追加します。後続のHelm更新はこのコピーを使うため指定を維持できます。過去の適用記録は[実施記録](mcguildlink-rehearsal-results.md)を参照してください。

```powershell
$newChart = Join-Path $rehearsalDir 'new-chart'
if (Test-Path $newChart) { throw '新Chartのコピー先が既にあります' }
Copy-Item -LiteralPath 'deploy/helm/platform' -Destination $newChart -Recurse
foreach ($chart in @($oldChart, $newChart)) {
    $templateFile = Join-Path $chart 'templates/cloudflared-deployment.yaml'
    $template = [IO.File]::ReadAllText($templateFile)
    if ($template -notmatch 'TUNNEL_TRANSPORT_PROTOCOL') {
        $updated = $template.Replace('            - name: TUNNEL_TOKEN', "            - name: TUNNEL_TRANSPORT_PROTOCOL`n              value: http2`n            - name: TUNNEL_TOKEN")
        if ($updated -eq $template) { throw 'cloudflaredのenv位置を確認してください' }
        [IO.File]::WriteAllText($templateFile, $updated, [Text.UTF8Encoding]::new($false))
    }
}
```

HTTP/2が不要な環境でも上記の新Chartコピーを作成し、foreachによるenv追加だけを省略します。ソース側のChartは変更しません。

```powershell
helm upgrade --install platform $oldChart --kube-context $rehearsalContext -n $rehearsalNamespace -f $oldValues -f $oldOverrides --timeout 10m
kubectl --context $rehearsalContext -n $rehearsalNamespace rollout status deployment/mcguildlink --timeout=5m
kubectl --context $rehearsalContext -n $rehearsalNamespace rollout status deployment/bot --timeout=5m
kubectl --context $rehearsalContext -n $rehearsalNamespace rollout status statefulset/postgres --timeout=5m
```

旧版で実際にコード発行・紐付け・ブロックを操作し、多対多・未使用コード・ブロックグループを作ります。Javaクライアントは旧リリースが対応するバージョンを使用し、新Rust版の確認時はJava 26.3へ切り替えます。旧版のDBへ後からfixtureだけを直接INSERTする方法は移行元再現の代わりにしません。

`localhost:25600` のMC接続と開発専用hostnameの `/whitelist.json` を確認し、コード・UUID・ホワイトリスト・設定値・各replicasを記録します。HTTPのport-forwardは診断の補助で、Tunnelを含む公開経路の再現完了とは扱いません。

## 2. 停止・SQLiteファイルの保存

```powershell
helm get values platform --all --kube-context $rehearsalContext -n $rehearsalNamespace | Set-Content (Join-Path $rehearsalDir 'old-effective-values.yaml')
helm get manifest platform --kube-context $rehearsalContext -n $rehearsalNamespace | Set-Content (Join-Path $rehearsalDir 'old-manifest.yaml')
kubectl --context $rehearsalContext -n $rehearsalNamespace scale deployment/mcguildlink deployment/bot --replicas=0
kubectl --context $rehearsalContext -n $rehearsalNamespace wait --for=delete pod -l app.kubernetes.io/name=mcguildlink --timeout=5m
kubectl --context $rehearsalContext -n $rehearsalNamespace wait --for=delete pod -l app.kubernetes.io/name=bot --timeout=5m
```

停止後に、次の手順で `mcguildlink-data` を読み取り専用でマウントします。上で定義した変数をそのまま使います。`sqlite-source/raw` に元ファイル、`sqlite-source/source.sha256` に元ファイルのハッシュ、`$rehearsalDir/app.db` に移行用スナップショットを保存します。既存の保存先には上書きしません。

```powershell
$sourceDir = Join-Path $rehearsalDir 'sqlite-source'
$rawDir = Join-Path $sourceDir 'raw'
$exportPod = 'mcguildlink-sqlite-export'
if (Test-Path $sourceDir) { throw "保存先が既にあります: $sourceDir" }
if (Test-Path (Join-Path $rehearsalDir 'app.db')) { throw '移行用app.dbが既にあります' }

# replicas=0だけでなく、旧アプリのPodが終了済みであることも確認する。
$deployments = kubectl --context $rehearsalContext -n $rehearsalNamespace get deployment mcguildlink bot -o json | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { throw 'Deploymentの確認に失敗しました' }
if (@($deployments.items | Where-Object { $_.spec.replicas -ne 0 }).Count) {
    throw '先に旧MCGuildLinkとBotを停止してください'
}
$oldPods = kubectl --context $rehearsalContext -n $rehearsalNamespace get pods -l 'app.kubernetes.io/name in (mcguildlink,bot)' -o json | ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or @($oldPods.items).Count) { throw '旧アプリのPodがまだ残っています' }

@"
apiVersion: v1
kind: Pod
metadata:
  name: $exportPod
spec:
  restartPolicy: Never
  automountServiceAccountToken: false
  securityContext:
    runAsNonRoot: true
    runAsUser: 65532
    runAsGroup: 65532
    seccompProfile:
      type: RuntimeDefault
  containers:
    - name: export
      image: busybox:1.37.0
      command: ["sleep", "3600"]
      securityContext:
        allowPrivilegeEscalation: false
        readOnlyRootFilesystem: true
        capabilities:
          drop: ["ALL"]
      volumeMounts:
        - name: source
          mountPath: /source
          readOnly: true
  volumes:
    - name: source
      persistentVolumeClaim:
        claimName: mcguildlink-data
        readOnly: true
"@ | kubectl --context $rehearsalContext -n $rehearsalNamespace create -f -
if ($LASTEXITCODE -ne 0) { throw '作業Podの作成に失敗しました' }

try {
    kubectl --context $rehearsalContext -n $rehearsalNamespace wait --for=condition=Ready "pod/$exportPod" --timeout=3m
    if ($LASTEXITCODE -ne 0) { throw '作業Podが起動しませんでした' }
    $hashCommand = 'cd /source && sha256sum app.db && { test ! -f app.db-wal || sha256sum app.db-wal; } && { test ! -f app.db-shm || sha256sum app.db-shm; }'
    $before = @(kubectl --context $rehearsalContext -n $rehearsalNamespace exec $exportPod -- sh -c $hashCommand)
    if ($LASTEXITCODE -ne 0) { throw '元ファイルのハッシュ取得に失敗しました' }
    New-Item -ItemType Directory -Path $rawDir | Out-Null
    $before | Set-Content (Join-Path $sourceDir 'source.sha256') -Encoding ascii

    # kubectl cpがWindowsのドライブ文字をPod名と誤認しないよう、コピー先は相対パスにする。
    Push-Location $rehearsalDir
    try {
        foreach ($line in $before) {
            $name = ($line -split '\s+', 2)[1]
            if ($name -notin @('app.db', 'app.db-wal', 'app.db-shm')) { throw '想定外のファイル名です' }
            kubectl --context $rehearsalContext -n $rehearsalNamespace cp "${exportPod}:/source/$name" "./sqlite-source/raw/$name" -c export
            if ($LASTEXITCODE -ne 0) { throw "コピーに失敗しました: $name" }
        }
    } finally { Pop-Location }
    $after = @(kubectl --context $rehearsalContext -n $rehearsalNamespace exec $exportPod -- sh -c $hashCommand)
    if ($LASTEXITCODE -ne 0 -or ($before -join "`n") -cne ($after -join "`n")) {
        throw 'コピー中に元ファイルが変化しました。別の書き込み元を確認してください'
    }
} finally {
    kubectl --context $rehearsalContext -n $rehearsalNamespace delete pod $exportPod --wait=true --timeout=3m
}
```

次にコピーのハッシュを照合し、WALの内容を含む一体化したスナップショットを作ります。SQLiteがSHMを更新する可能性があるため、一時ディレクトリで処理し、取得した `raw` は保持します。[SQLite保護手順](mcguildlink-migration.md#旧版停止とデータ保護)の移行入力は、最後に生成する `app.db` です。

```powershell
@'
import hashlib, pathlib, shutil, sqlite3, sys, tempfile
from contextlib import closing

root = pathlib.Path(sys.argv[1])
source = root / 'sqlite-source'
raw = source / 'raw'
output = root / 'app.db'
if output.exists():
    raise FileExistsError(output)
for line in (source / 'source.sha256').read_text(encoding='ascii').splitlines():
    expected, name = line.split()
    if name not in ('app.db', 'app.db-wal', 'app.db-shm'):
        raise ValueError('Unexpected source file')
    actual = hashlib.sha256((raw / name).read_bytes()).hexdigest()
    if actual != expected:
        raise ValueError(f'Hash mismatch: {name}')
with tempfile.TemporaryDirectory() as work:
    work = pathlib.Path(work)
    for path in raw.iterdir():
        shutil.copy2(path, work / path.name)
    with closing(sqlite3.connect((work / 'app.db').as_uri() + '?mode=ro', uri=True)) as old:
        with closing(sqlite3.connect(work / 'snapshot.db')) as snapshot:
            old.backup(snapshot)
            if snapshot.execute('PRAGMA integrity_check').fetchall() != [('ok',)]:
                raise ValueError('SQLite integrity check failed')
    with (work / 'snapshot.db').open('rb') as src, output.open('xb') as dst:
        shutil.copyfileobj(src, dst)
print(f'Snapshot ready: {output}')
'@ | python - $rehearsalDir
if ($LASTEXITCODE -ne 0) { throw 'SQLiteスナップショットの作成に失敗しました' }
Get-FileHash (Join-Path $rehearsalDir 'app.db') -Algorithm SHA256 |
    Format-List | Out-File (Join-Path $sourceDir 'snapshot.sha256.txt')
```

失敗した場合はこの先へ進みません。保存済み `raw` と `source.sha256` は残して原因を確認します。ここまで成功したら、作業Podは削除済み、PVCの元ファイルと取得したコピーの一致を確認済み、移行用 `app.db` の整合性確認済みです。

旧SQLite PVCは保持しません。以前の手順で保持指定を追加していた場合も、作業用旧ChartのPVCテンプレートを元のリリース版に戻し、旧アプリを停止したままHelmの保存済みmanifestを更新します。DBファイル・ハッシュ・旧設定ファイルの存在を確認してから実行します。

```powershell
foreach ($path in @('app.db', 'sqlite-source/snapshot.sha256.txt', 'config/bot-old.toml', 'config/mcguildlink-old.toml', 'old-values.yaml', 'old-overrides.json')) {
    if (-not (Test-Path (Join-Path $rehearsalDir $path))) { throw "保存ファイルがありません: $path" }
}
$hashRecord = @(Select-String -Path (Join-Path $rehearsalDir 'sqlite-source/snapshot.sha256.txt') -Pattern '^\s*Hash\s*:\s*([A-Fa-f0-9]{64})\s*$')
if ($hashRecord.Count -ne 1) { throw '保存したSHA256が読み取れません' }
$snapshotHash = $hashRecord[0].Matches[0].Groups[1].Value
if ((Get-FileHash (Join-Path $rehearsalDir 'app.db') -Algorithm SHA256).Hash -ne $snapshotHash) { throw '保存したDBが変化しています' }
$sqlitePvcUid = kubectl --context $rehearsalContext -n $rehearsalNamespace get pvc mcguildlink-data -o jsonpath='{.metadata.uid}'
if ($LASTEXITCODE -ne 0) { throw '旧SQLite PVCの確認に失敗しました' }
$sqlitePvcUid | Set-Content (Join-Path $rehearsalDir 'sqlite-source/pvc-uid.txt') -Encoding ascii
$pvcTemplate = git -C $oldChart show HEAD:deploy/helm/platform/templates/mcguildlink-pvc.yaml
if ($LASTEXITCODE -ne 0) { throw '旧PVCテンプレートの取得に失敗しました' }
$pvcTemplate | Set-Content (Join-Path $oldChart 'templates/mcguildlink-pvc.yaml') -Encoding utf8
helm upgrade platform $oldChart --kube-context $rehearsalContext -n $rehearsalNamespace -f $oldValues -f $oldOverrides --set mcguildlink.replicas=0 --set bot.replicas=0 --timeout 10m
if ($LASTEXITCODE -ne 0) { throw '旧releaseの更新に失敗しました' }
```

保存するのはDBファイル・ハッシュ・旧設定ファイル・旧イメージ・旧valuesです。開発Tunnelの旧宛先は `http://mcguildlink-http:8080` です。旧SQLite PVCは次の新Chartへの更新で削除されます。

## 3. PostgreSQLを削除して作り直す

以下は専用リハーサルクラスタだけで実行します。旧PostgreSQLのロールpasswordをSecretと異なる値に変えておくと、「中途半端な設定を引き継がない」ことも再現できます。削除前のDB名・ロールを記録します。

```powershell
# 旧PGを停止して削除。SQLite PVCの削除は新Chartへの更新時に行う。
kubectl --context $rehearsalContext -n $rehearsalNamespace scale statefulset/postgres --replicas=0
kubectl --context $rehearsalContext -n $rehearsalNamespace wait --for=delete pod/postgres-0 --timeout=5m
kubectl --context $rehearsalContext -n $rehearsalNamespace delete pvc postgres-data
kubectl --context $rehearsalContext -n $rehearsalNamespace wait --for=delete pvc/postgres-data --timeout=5m
kubectl --context $rehearsalContext -n $rehearsalNamespace delete secret postgres
kubectl --context $rehearsalContext -n $rehearsalNamespace create secret generic postgres-db-credentials --from-literal=password=admin-rehearsal
```

新Bot・MC・API・Migratorは、すべて同じソースと同じ `migrations` からビルドします。以前の `:test` イメージは流用しません。SQLの内容が異なるイメージを混ぜると、起動時にチェックサム不一致になります。新Chartのvaluesは旧値を継承せず、同じreleaseを更新します。

```powershell
$newImageTag = 'rehearsal-' + (git rev-parse --short HEAD).Trim()
foreach ($service in @('bot', 'mc-link-server', 'public-api', 'db-migrator')) {
    podman --connection $buildConnection build --isolation=chroot --platform linux/amd64 --build-arg TARGETARCH=amd64 "--build-arg=PACKAGE=$service" "--build-arg=BINARY=$service" "--build-arg=SOURCE_DIR=apps/$service" -f deploy/rust/Dockerfile -t "localhost/anvilsaba/${service}:$newImageTag" .
    if ($LASTEXITCODE -ne 0) { throw "新イメージのビルドに失敗しました: $service" }
}
$newImageArchive = Join-Path $rehearsalDir 'new-images.tar'
podman --connection $buildConnection save --format docker-archive --output $newImageArchive "localhost/anvilsaba/bot:$newImageTag" "localhost/anvilsaba/mc-link-server:$newImageTag" "localhost/anvilsaba/public-api:$newImageTag" "localhost/anvilsaba/db-migrator:$newImageTag"
if ($LASTEXITCODE -ne 0) { throw '新イメージの書き出しに失敗しました' }
k3d image import $newImageArchive --mode direct -c mcguildlink-rehearsal
if ($LASTEXITCODE -ne 0) { throw '新イメージの取り込みに失敗しました' }
helm upgrade platform $newChart --kube-context $rehearsalContext -n $rehearsalNamespace --reset-values -f deploy/helm/platform/values.integration.yaml --set bot.image.repository=localhost/anvilsaba/bot "--set=bot.image.tag=$newImageTag" "--set=mcLinkServer.image.tag=$newImageTag" "--set=publicApi.image.tag=$newImageTag" "--set=dbMigrator.image.tag=$newImageTag" --set bot.replicas=0 --set mcLinkServer.replicas=0 --set mcLinkServer.port=25600 --set publicApi.replicas=0 --set dbMigrator.enabled=false
kubectl --context $rehearsalContext -n $rehearsalNamespace rollout status statefulset/postgres --timeout=5m
kubectl --context $rehearsalContext -n $rehearsalNamespace get pvc postgres-data
$remainingSqlitePvc = kubectl --context $rehearsalContext -n $rehearsalNamespace get pvc mcguildlink-data --ignore-not-found -o name
if ($LASTEXITCODE -ne 0 -or $remainingSqlitePvc) { throw '旧SQLite PVCが削除されていません' }
Get-Content deploy/postgres/bootstrap.sql -Raw | kubectl --context $rehearsalContext -n $rehearsalNamespace exec -i postgres-0 -- psql -U platform_admin -d platform -v ON_ERROR_STOP=1
if ($LASTEXITCODE -ne 0) { throw 'DBロールの初期化に失敗しました' }
```

`postgres-data` のUIDが変わり、旧DB/旧ロールが残っていないことを確認します。旧SQLite PVCが削除され、保存済み `app.db` とハッシュが手元に残っていることを確認します。

### 3.1. DBログインのパスワードとSecretを設定する

直前の `bootstrap.sql` 適用が成功し、新Bot・MC・APIが0、Migratorが無効の状態で実行します。4つのパスワードをGitの外の `config/db-credentials` に生成し、同じファイルの値をDBロールとSecretへ登録します。既存ファイルはUTF-8（BOMなし）で、記号や日本語も使用できます。テキストエディターなどが付けた末尾の改行だけは取り除き、改行なしで保存し直します。パスワード内部の改行・NUL・空文字は受け付けません。未作成のファイルだけランダムな48文字の16進数で生成します。パスワードとSecretのYAMLは画面に出力しません。

| DBログイン | Secretの名前 / キー |
|---|---|
| `platform_bot` | `bot-db-credentials` / `password` |
| `platform_mc_link_server` | `mc-link-server-db-credentials` / `password` |
| `platform_public_api` | `public-api-db-credentials` / `password` |
| `platform_db_migrator` | `db-migrator-db-credentials` / `password` |

```powershell
$dbCredentialDir = Join-Path $rehearsalDir 'config/db-credentials'
New-Item -ItemType Directory -Path $dbCredentialDir -Force | Out-Null
$dbLogins = [ordered]@{
    platform_bot = 'bot-db-credentials'
    platform_mc_link_server = 'mc-link-server-db-credentials'
    platform_public_api = 'public-api-db-credentials'
    platform_db_migrator = 'db-migrator-db-credentials'
}
$roleSql = @('BEGIN;', "SET LOCAL password_encryption = 'scram-sha-256';", 'SET LOCAL standard_conforming_strings = on;')
foreach ($role in $dbLogins.Keys) {
    $passwordFile = Join-Path $dbCredentialDir "$role.password"
    if (-not (Test-Path $passwordFile)) {
        $password = [Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(24))
        [IO.File]::WriteAllText($passwordFile, $password)
    }
    $password = [Text.UTF8Encoding]::new($false, $true).GetString([IO.File]::ReadAllBytes($passwordFile))
    $password = $password.TrimEnd([char[]]"`r`n")
    if ([string]::IsNullOrEmpty($password) -or $password[0] -eq [char]0xFEFF -or $password.IndexOfAny([char[]]"`0`r`n") -ge 0) {
        throw "パスワードはUTF-8（BOMなし）・改行なしの空でない値にしてください: $role"
    }
    $sqlPassword = $password.Replace("'", "''")
    [IO.File]::WriteAllText($passwordFile, $password, [Text.UTF8Encoding]::new($false))
    $roleSql += "ALTER ROLE `"$role`" PASSWORD '$sqlPassword';"
}
$roleSql += 'COMMIT;'
$previousOutputEncoding = $OutputEncoding
try {
    $OutputEncoding = [Text.UTF8Encoding]::new($false)
    $roleSql -join "`n" | kubectl --context $rehearsalContext -n $rehearsalNamespace exec -i postgres-0 -- psql -X -U platform_admin -d platform -v ON_ERROR_STOP=1
} finally { $OutputEncoding = $previousOutputEncoding }
if ($LASTEXITCODE -ne 0) { throw 'DBログインのパスワード設定に失敗しました' }
foreach ($role in $dbLogins.Keys) {
    $passwordFile = Join-Path $dbCredentialDir "$role.password"
    $secretYaml = kubectl --context $rehearsalContext -n $rehearsalNamespace create secret generic $dbLogins[$role] "--from-file=password=$passwordFile" --dry-run=client -o yaml
    if ($LASTEXITCODE -ne 0) { throw "Secretの生成に失敗しました: $role" }
    $secretYaml -join "`n" | kubectl --context $rehearsalContext -n $rehearsalNamespace apply -f -
    if ($LASTEXITCODE -ne 0) { throw "Secretの登録に失敗しました: $role" }
}
Remove-Variable password, sqlPassword, roleSql, secretYaml
```

管理者用Secret `postgres-db-credentials` はそのまま使用します。上の4つのSecretとロール名はChart既定値に合わせています。一般的な接続権限の説明は[独立デプロイ手順の「DB と Secret の準備」](independent-deployment.md#db-secrets)を参照してください。

### 3.2. 旧Bot設定を変換してbot-configへ登録する

保存済みの `config/bot-old.toml` と `config/mcguildlink-old.toml` を使います。旧専用Botのguild・moderator_role・log_channel・display_server_addressを新Botの `[mcguildlink]` へ移し、IDは文字列にします。統合Botのtoken・owners・他機能の設定と、旧設定のコピーは維持します。

```powershell
@'
import json, pathlib, re, sys, tomllib

config_dir = pathlib.Path(sys.argv[1]) / 'config'
old_text = (config_dir / 'bot-old.toml').read_text(encoding='utf-8-sig')
old_bot = tomllib.loads(old_text)
old_mc = tomllib.loads((config_dir / 'mcguildlink-old.toml').read_text(encoding='utf-8-sig'))['bot']
values = {
    'guild_id': str(old_mc['guild']),
    'moderator_role_id': str(old_mc['moderator_role']),
    'audit_channel_id': str(old_mc['log_channel']),
    'display_server_address': old_mc['display_server_address'],
}
text = re.sub(r'(?ms)^\[mcguildlink\][ \t]*\r?\n.*?(?=^\[|\Z)', '', old_text)
text = text.rstrip() + '\n\n[mcguildlink]\n'
text += '\n'.join(f'{key} = {json.dumps(value, ensure_ascii=False)}' for key, value in values.items()) + '\n'
new_bot = tomllib.loads(text)
if {k: v for k, v in old_bot.items() if k != 'mcguildlink'} != {k: v for k, v in new_bot.items() if k != 'mcguildlink'}:
    raise ValueError('Other Bot settings changed')
if new_bot['mcguildlink'] != values:
    raise ValueError('MCGuildLink settings mismatch')
(config_dir / 'bot-new.toml').write_text(text, encoding='utf-8')
print('Bot settings converted; original settings preserved')
'@ | python - $rehearsalDir
if ($LASTEXITCODE -ne 0) { throw 'Bot設定の変換に失敗しました' }
$botConfigFile = Join-Path $rehearsalDir 'config/bot-new.toml'
$botSecretYaml = kubectl --context $rehearsalContext -n $rehearsalNamespace create secret generic bot-config "--from-file=config.toml=$botConfigFile" --dry-run=client -o yaml
if ($LASTEXITCODE -ne 0) { throw 'Bot設定Secretの生成に失敗しました' }
$botSecretYaml -join "`n" | kubectl --context $rehearsalContext -n $rehearsalNamespace apply -f -
if ($LASTEXITCODE -ne 0) { throw 'Bot設定Secretの登録に失敗しました' }
Remove-Variable botSecretYaml
```

### 3.3. Migratorを実行し、書き込みを凍結する

ここまで成功したら、4つのDB Secretと新Bot設定は登録済みです。次のJobでスキーマを作り、Job完了後に書き込みを凍結します。

```powershell
foreach ($name in @('bot-db-credentials', 'mc-link-server-db-credentials', 'public-api-db-credentials', 'db-migrator-db-credentials')) {
    $secret = kubectl --context $rehearsalContext -n $rehearsalNamespace get secret $name -o json | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrEmpty($secret.data.password)) { throw "DB Secretが未登録です: $name" }
}
Remove-Variable secret
helm upgrade platform $newChart --kube-context $rehearsalContext -n $rehearsalNamespace --reuse-values --set dbMigrator.enabled=true --timeout 10m
if ($LASTEXITCODE -ne 0) { throw 'Migrator起動に失敗しました。Pod・Job・イベントを確認し、この先へ進まないでください' }
kubectl --context $rehearsalContext -n $rehearsalNamespace wait --for=condition=Complete job/db-migrator --timeout=5m
if ($LASTEXITCODE -ne 0) { throw 'Migratorが完了していません。凍結SQLは実行しません' }
kubectl --context $rehearsalContext -n $rehearsalNamespace logs job/db-migrator
if ($LASTEXITCODE -ne 0) { throw 'Migratorのログ取得に失敗しました' }
helm upgrade platform $newChart --kube-context $rehearsalContext -n $rehearsalNamespace --reuse-values --set dbMigrator.enabled=false
if ($LASTEXITCODE -ne 0) { throw 'Migrator無効化に失敗しました' }
Get-Content scripts/mcguildlink-cutover-freeze.sql -Raw | kubectl --context $rehearsalContext -n $rehearsalNamespace exec -i postgres-0 -- psql -X -U platform_admin -d platform -v ON_ERROR_STOP=1
if ($LASTEXITCODE -ne 0) { throw '書き込み凍結に失敗しました' }
```

## 4. 旧SQLiteを移行し、新版へ経路を切り替える

```powershell
python scripts/migrate-mcguildlink.py (Join-Path $rehearsalDir 'app.db') (Join-Path $rehearsalDir 'import.sql') --source-utc-offset=+00:00
if ($LASTEXITCODE -ne 0) { throw '移行SQLの生成に失敗しました' }
Get-Content (Join-Path $rehearsalDir 'import.sql') -Raw | kubectl --context $rehearsalContext -n $rehearsalNamespace exec -i postgres-0 -- psql -U platform_admin -d platform -v ON_ERROR_STOP=1
if ($LASTEXITCODE -ne 0) { throw 'SQLiteデータの移行に失敗しました' }
python scripts/migrate-mcguildlink.py (Join-Path $rehearsalDir 'app.db') (Join-Path $rehearsalDir 'verify.sql') --source-utc-offset=+00:00 --verify-only
if ($LASTEXITCODE -ne 0) { throw '照合SQLの生成に失敗しました' }
Get-Content (Join-Path $rehearsalDir 'verify.sql') -Raw | kubectl --context $rehearsalContext -n $rehearsalNamespace exec -i postgres-0 -- psql -U platform_admin -d platform -v ON_ERROR_STOP=1
if ($LASTEXITCODE -ne 0) { throw '移行データの照合に失敗しました' }
helm upgrade platform $newChart --kube-context $rehearsalContext -n $rehearsalNamespace --reuse-values --set bot.replicas=1 --set mcLinkServer.replicas=1 --set publicApi.replicas=1 --set cloudflared.replicas=1
if ($LASTEXITCODE -ne 0) { throw '新版の起動に失敗しました' }
foreach ($service in @('bot', 'mc-link-server', 'public-api')) {
    kubectl --context $rehearsalContext -n $rehearsalNamespace rollout status "deployment/$service" --timeout=5m
    if ($LASTEXITCODE -ne 0) { throw "新版が起動できません。ログを確認し、公開経路を切り替えないでください: $service" }
}
```

UTCオフセットは旧JVMの設定に合わせます。新Chartでprivateイメージを使う場合はpull Secretの指定も設定します。各Podの起動、スキーマ互換性、凍結中の書き込み拒否を確認します。凍結中の業務書き込みエラーは想定内です。

同じ開発専用hostnameのTunnel宛先を `http://public-api:8080` へ変更し、旧版とホワイトリストのUUID・名前・ブロック除外を比較します。MCは同じ `localhost:25600` から新版へ接続します。port-forwardで新Serviceへ直接つなぎ直すだけで切替成功と扱いません。再照合し、未使用コードが消費されず監査ログ・outboxが空であることを確認します。

手順4の起動・公開経路・データ照合を確認したら、通常移行のみの場合は凍結を維持して終了します。ほかの目的は冒頭の入口表に従ってください。

## 別の回で行う解禁確認と補助テスト

切り戻し・解禁の順序は冒頭の入口表を参照してください。

`test-mcguildlink-migration.py --psql ...` は、スクリプトの異常系を確認する補助テストです。代表fixtureを空DBへ移すだけのこのテストを、旧環境からのリハーサル完了とは扱いません。実行する場合はさらに別の空テストDBを使います。

## 結果の記録と後片付け

過去の実施時点ごとの履歴・結果は[実施記録](mcguildlink-rehearsal-results.md)を参照してください。新しい回は日時・結果・未検証項目を追記し、前の回の成功を引き継ぎません。

再開後は旧新のイメージdigest、values差分、PVC UID、DB名/ロール、SQL照合結果、各公開経路の確認結果を記録します。終了後は `k3d cluster delete mcguildlink-rehearsal` で専用クラスタを削除し、作業ディレクトリのDBコピー・SQL・設定を片付けます。本番環境ではこの文書の削除コマンドを実行しません。
