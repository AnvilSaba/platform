# 本番の旧MCGuildLinkから分離構成へ移行する手順

本番では次の1〜8を順に実行します。この文書は作業手順で、本番環境を操作した記録ではありません。リハーサルの実施済み・未検証の範囲は[実行記録](mcguildlink-rehearsal-results.md)を参照してください。

切り戻しは通常手順の続きとして実行しません。必要になった場合だけ、別文書の[本番切り戻し手順](mcguildlink-production-rollback.md)を選びます。

コマンド以外に必要な操作は次のとおりです。設定ファイル・DBパスワード・SQLは掲載コマンドで生成します。

| タイミング | 手動操作 |
|---|---|
| 手順1の前 | イメージ・Chartの公開、停止時間の案内、バックアップ先のマウント |
| 手順1・2 | 公開バージョン・タグ、バックアップ先の入力、旧PostgreSQLに必要データがないことの確認 |
| 手順6 | Botコンソールへの入力、Cloudflareの既存ルート変更、Minecraft接続確認 |
| 手順7 | 新版で継続するか切り戻すかの判断 |
| 手順8 | Discordパネル設置、協力者のテストアカウントでの操作確認 |

## 1. 公開物と本番設定を準備する

入力が必要なのは、新Chartの公開バージョンと4イメージの公開タグです。本番hostnameは確認済みの `api.anvilsaba.org` に設定しています。バックアップ先は手順2で入力します。contextは確認済みの `default`、namespace・release・OCI URLは既存デプロイと同じ固定値です。旧Chartバージョンは既存releaseから取得します。MCポートは本番用 `values.prod.yaml` の25600、Public APIポートは8080を使います。PostgreSQLの容量とStorageClassは既存PVCから自動で引き継ぎ、入力や確認は求めません。旧JVMのUTCオフセットは、標準旧Chart・イメージとリハーサルの結果に基づき `+00:00` を使います。本番Deploymentにも明示的なタイムゾーン指定・時刻設定ファイルのマウントはありません。追加の実機確認は行いません。Cloudflareの宛先は移行時に `http://public-api:8080`、切り戻し時に `http://mcguildlink-http:8080` を設定します。

- Bot・MC Link Server・Public APIを、同じマイグレーションSQLを含むソースからリリースします。各アプリと同じ revision の DB Migrator も自動公開されます。初回は自動デプロイを行わず、各イメージのタグ・digest・ソースSHAを記録します。Migrator は Bot と同じ revision の `sha-<完全なGit SHA>` タグを使用します。古いBotイメージを流用しません。
- 分離構成のChartを公開します。初回はRelease Actionで `chart / bump=major / deploy=false` を指定します。
- 本番のkube-context・namespace・Helm release・旧Chart・旧イメージ・本番values・Tunnel hostnameを記録します。手順6でCloudflareの宛先を `http://public-api:8080` へ変更します。切り戻し時は宛先を `http://mcguildlink-http:8080` に戻します。k3d用のcontext、`localhost/...` イメージ、開発用token・パスワードは使用しません。
- 新valuesは新Chartに同梱された `values.prod.yaml` を基準に作り、各公開イメージタグと本番設定を明示します。初期状態はBot・MC・APIのreplicasを0、Migratorを無効にします。
- 対象プレイヤーに停止時間と新版の対応クライアント（Minecraft Java 26.3）を案内します。

コマンドは本番kubeconfigを使えるLinuxのzshとPython 3.11以降（`python3`）で実行します。Chartは既存の運用と同じOCIから取得し、移行用ファイルは公開Chartタグから自動取得するため、本番にリポジトリ全体を配置する必要はありません。認証情報は本番サーバーのHelmログイン、本番contextから取得するBot設定、本番の `ghcr-pull`・`cloudflare-tunnel` だけを継続します。リハーサルのtoken・パスワード・Secret・設定ファイル・作業ディレクトリは一切参照しません。DB用パスワードは本番作業ディレクトリ内で新規生成します。秘密値を保存する `$cutoverDir` はGitの外です。`kubectl`・`helm`・`curl`・`sha256sum`・GNU tar・`flock` も必要です。本番デプロイ用ユーザーのHelm registry loginと本番サーバーの `~/.kube/config` を使います。最初に `zsh` で作業用シェルを開き、各番号の確認が完了してから次の番号へ進みます。エラーで作業用シェルが終了した場合は、再び `zsh` を開き、`setopt ERR_EXIT PIPE_FAIL; umask 077` と `source "$HOME/mcguildlink-cutover-日時/context.zsh"` で変数・関数を復元します。手順1からやり直して別ディレクトリを作りません。全ブロックを一括で貼り付けません。公開イメージは本番CPUアーキテクチャに対応するものを使います。

```zsh
setopt ERR_EXIT PIPE_FAIL
umask 077
export KUBECONFIG="$HOME/.kube/config"
[[ -r "$KUBECONFIG" ]]
# 既存の本番デプロイスクリプトと同じ固定値。
productionContext=default
productionNamespace=anvilsaba
releaseName=platform
chartOci=oci://ghcr.io/anvilsaba/charts/platform
productionHostname=api.anvilsaba.org
sourceUtcOffset=+00:00
read -r 'newChartVersion?新Chartの公開バージョン（例: 1.0.0）: '
read -r 'botTag?新Botの公開タグ（vX.Y.Z または bot/vX.Y.Z）: '
read -r 'mcTag?新MC Link Serverの公開タグ: '
read -r 'apiTag?新Public APIの公開タグ: '
read -r 'migratorTag?Botのリリースコミットに対応するDB Migratorタグ（sha-完全な40桁のGit SHA）: '
newChartVersion=${newChartVersion#chart/v}
botTag=${botTag#bot/}
mcTag=${mcTag#mc-link-server/}
apiTag=${apiTag#public-api/}
for value in "$newChartVersion" "$botTag" "$mcTag" "$apiTag" "$migratorTag" "$productionHostname" "$sourceUtcOffset"; do
  [[ -n "$value" ]]
done
stateDir="${XDG_STATE_HOME:-$HOME/.local/state}/anvilsaba"
install -d -m 700 "$stateDir"
# GitHub Actionsの通常デプロイと同じロック。作業用zsh終了時に解放される。
exec 9>"$stateDir/deploy.lock"
flock -n 9
cutoverDir="$HOME/mcguildlink-cutover-$(date +%Y%m%d-%H%M%S)"
[[ ! -e "$cutoverDir" ]]
mkdir -p "$cutoverDir/config" "$cutoverDir/charts/old" "$cutoverDir/charts/new" "$cutoverDir/source"
oldValues="$cutoverDir/old-values.yaml"
newValuesFile="$cutoverDir/new-values.json"
oldChart="$cutoverDir/charts/old/platform"
newChart="$cutoverDir/charts/new/platform"
sourceDir="$cutoverDir/source"
export productionContext productionNamespace releaseName chartOci newChartVersion oldChart newChart sourceDir productionHostname sourceUtcOffset botTag mcTag apiTag migratorTag cutoverDir oldValues newValuesFile stateDir
k() { kubectl --context "$productionContext" -n "$productionNamespace" "$@"; }
h() { helm "$@" --kube-context "$productionContext" -n "$productionNamespace"; }
db() { k exec -i postgres-0 -- psql -X -U platform_admin -d platform -v ON_ERROR_STOP=1; }
typeset -p KUBECONFIG productionContext productionNamespace releaseName chartOci newChartVersion oldChart newChart sourceDir productionHostname sourceUtcOffset botTag mcTag apiTag migratorTag cutoverDir oldValues newValuesFile stateDir > "$cutoverDir/context.zsh"
functions k h db >> "$cutoverDir/context.zsh"
# 失敗後に別の作業用zshからsourceする場合も、通常デプロイと排他する。
printf '%s\n' 'exec 9>"$stateDir/deploy.lock"' 'flock -n 9' >> "$cutoverDir/context.zsh"
k get deployment/bot deployment/mcguildlink statefulset/postgres
h status "$releaseName"
h list -o json > "$cutoverDir/old-release.json"
oldChartVersion=$(python3 - "$cutoverDir/old-release.json" <<'PY'
import json, pathlib, sys
release = next(x for x in json.loads(pathlib.Path(sys.argv[1]).read_text()) if x['name'] == 'platform')
chart = release['chart']
if not chart.startswith('platform-'):
    raise ValueError('想定外のChart名')
print(chart.removeprefix('platform-'))
PY
)
h pull "$chartOci" --version "$oldChartVersion" --untar --untardir "$cutoverDir/charts/old"
h pull "$chartOci" --version "$newChartVersion" --untar --untardir "$cutoverDir/charts/new"
[[ -f "$newChart/values.prod.yaml" ]]
# 本番へリポジトリ全体を配置せず、新Chartと同じ公開タグから4ファイルだけ取り出す。
curl --fail --location --silent --show-error "https://api.github.com/repos/AnvilSaba/platform/tarball/chart%2Fv${newChartVersion}" -o "$cutoverDir/source.tar.gz"
tar -xzf "$cutoverDir/source.tar.gz" -C "$sourceDir" --strip-components=1 --wildcards \
  '*/scripts/migrate-mcguildlink.py' \
  '*/scripts/mcguildlink-cutover-freeze.sql' \
  '*/scripts/mcguildlink-cutover-unfreeze.sql' \
  '*/deploy/postgres/bootstrap.sql'
h get values "$releaseName" --all -o yaml > "$oldValues"
h get manifest "$releaseName" > "$cutoverDir/old-manifest.yaml"
k get pods -o json > "$cutoverDir/old-pods.json"
k get pvc postgres-data -o json > "$cutoverDir/old-postgres-pvc.json"
k get deployment mcguildlink -o json > "$cutoverDir/old-mcguildlink-deployment.json"
# 既存PVCの容量とStorageClassを引き継ぐ。ポートはChartの本番設定を使う。
python3 - <<'PY'
import json, os, pathlib
root = pathlib.Path(os.environ['cutoverDir'])
pvc = json.loads((root / 'old-postgres-pvc.json').read_text())['spec']
values = {
    'bot': {'replicas': 0, 'image': {'repository': 'ghcr.io/anvilsaba/bot', 'tag': os.environ['botTag']}},
    'mcLinkServer': {'replicas': 0, 'image': {'repository': 'ghcr.io/anvilsaba/mc-link-server', 'tag': os.environ['mcTag']}},
    'publicApi': {'replicas': 0, 'image': {'repository': 'ghcr.io/anvilsaba/public-api', 'tag': os.environ['apiTag']}},
    'dbMigrator': {'enabled': False, 'image': {'repository': 'ghcr.io/anvilsaba/db-migrator', 'tag': os.environ['migratorTag']}},
    'postgres': {'username': 'platform_admin', 'database': 'platform', 'secretName': 'postgres-db-credentials', 'storageSize': pvc['resources']['requests']['storage'], 'storageClassName': pvc.get('storageClassName', '')},
    'cloudflared': {'replicas': 1, 'tokenSecretName': 'cloudflare-tunnel'},
}
(root / 'new-values.json').write_text(json.dumps(values, ensure_ascii=False), encoding='utf-8')
PY
curl --fail --silent --show-error -A 'Mozilla/5.0' "https://$productionHostname/whitelist.json" > "$cutoverDir/old-whitelist.json"
k get secret ghcr-pull cloudflare-tunnel -o name
h template "$releaseName" "$newChart" -f "$newChart/values.prod.yaml" -f "$newValuesFile" > "$cutoverDir/new-manifest.yaml"
```

手順1のコマンドが作るファイルは次のとおりです。保存先を手で作ったり、Chartの絶対パスを入力したりする必要はありません。

| 変数・ファイル | 保存先と内容 |
|---|---|
| `$cutoverDir` | 本番ユーザーの `$HOME/mcguildlink-cutover-YYYYMMDD-HHMMSS`。今回の作業で自動作成するディレクトリ |
| `$cutoverDir/context.zsh` | この作業の変数・`k`・`h`・`db` 関数。失敗後の再開時に読み込む |
| `$oldValues` | `$cutoverDir/old-values.yaml`。本番releaseから取得した旧values |
| `$newValuesFile` | `$cutoverDir/new-values.json`。新イメージタグと本番設定をコマンドが生成する。手編集不要 |
| `$oldChart`・`$newChart` | `$cutoverDir/charts/old/platform`・`$cutoverDir/charts/new/platform`。OCIから展開したChart |
| `$sourceDir/scripts/` | 移行Python・凍結SQL・解禁SQL。新Chartの公開タグから取得する |
| `$sourceDir/deploy/postgres/bootstrap.sql` | DBロールを準備するSQL。同じ公開タグから取得する |
| `$cutoverDir/old-whitelist.json` | 停止前の公開応答の参考記録。移行DBとの比較基準は手順2で別に生成する |

`k` は `kubectl --context default -n anvilsaba`、`h` は本番context・namespace指定付きのHelm、`db` は `postgres-0` 内で `platform_admin` として `platform` DBへ接続する `psql` です。SQLの `< ファイル名` は、本番サーバー上のファイルを標準入力でDBへ送ります。

## 2. 旧アプリを停止し、復元に必要なものを保存する

旧Bot・旧MCGuildLinkと別の書き込み元を停止し、Pod終了を確認します。旧Chart・values・manifest・設定Secretの復元用ファイル・イメージdigest・公開経路を保存します。旧Bot設定は新設定で上書きせず別ファイルに残します。

停止後、作業Podから `mcguildlink-data` を読み取り専用でマウントし、`app.db` と存在するWAL/SHMを取り出します。PythonのSQLite backupでWALを取り込んだ単一の `app.db` を作り、整合性とSHA256を記録します。作業Podを削除し、保存したDBと設定を作業マシン以外にも保管します。

次のブロックで、本番Secretの `config.toml` を `$cutoverDir/config/bot-old.toml`、`app.toml` を `$cutoverDir/config/mcguildlink-old.toml` に保存します。その後、旧Bot・MCGuildLinkのreplicasを0にし、Podが消えるまで待ちます。設定ファイルを手入力する必要はありません。


```zsh
k get secret bot-config -o json | python3 -c 'import base64,json,pathlib,sys; pathlib.Path(sys.argv[1]).write_bytes(base64.b64decode(json.load(sys.stdin)["data"]["config.toml"]))' "$cutoverDir/config/bot-old.toml"
k get secret mcguildlink-config -o json | python3 -c 'import base64,json,pathlib,sys; pathlib.Path(sys.argv[1]).write_bytes(base64.b64decode(json.load(sys.stdin)["data"]["app.toml"]))' "$cutoverDir/config/mcguildlink-old.toml"
k scale deployment/mcguildlink deployment/bot --replicas=0
oldPods=$(k get pods -l 'app.kubernetes.io/name in (mcguildlink,bot)' -o name)
if [[ -n "$oldPods" ]]; then
  k wait --for=delete pod -l 'app.kubernetes.io/name in (mcguildlink,bot)' --timeout=5m
fi
```

次のブロックで一時Pod `mcguildlink-sqlite-export` を作り、旧PVCのファイルを `$cutoverDir/sqlite-source/raw/` へコピーします。`source.sha256` と `source-after.sha256` はコピー前後のハッシュです。一致しなければ処理を止めます。ブロック終了時に一時Podを削除します。

```zsh
[[ ! -e "$cutoverDir/sqlite-source" && ! -e "$cutoverDir/app.db" ]]
k get deployment mcguildlink bot -o json | python3 -c 'import json,sys; assert all(d["spec"]["replicas"] == 0 for d in json.load(sys.stdin)["items"]), "旧アプリが停止していません"'
k get pods -l 'app.kubernetes.io/name in (mcguildlink,bot)' -o json | python3 -c 'import json,sys; assert not json.load(sys.stdin)["items"], "旧Podが残っています"'
exportPod=mcguildlink-sqlite-export
k create -f - <<YAML
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
    seccompProfile: {type: RuntimeDefault}
  containers:
    - name: export
      image: busybox:1.37.0
      command: [sleep, "3600"]
      securityContext:
        allowPrivilegeEscalation: false
        readOnlyRootFilesystem: true
        capabilities: {drop: [ALL]}
      volumeMounts:
        - {name: source, mountPath: /source, readOnly: true}
  volumes:
    - name: source
      persistentVolumeClaim: {claimName: mcguildlink-data, readOnly: true}
YAML
# 失敗時も作業Podを削除する。rawとハッシュは残す。
(
  trap 'k delete pod "$exportPod" --ignore-not-found --wait=true --timeout=3m' EXIT
  k wait --for=condition=Ready "pod/$exportPod" --timeout=3m
  hashCommand='cd /source && sha256sum app.db && { test ! -f app.db-wal || sha256sum app.db-wal; } && { test ! -f app.db-shm || sha256sum app.db-shm; }'
  mkdir -p "$cutoverDir/sqlite-source/raw"
  k exec "$exportPod" -- sh -c "$hashCommand" > "$cutoverDir/sqlite-source/source.sha256"
  while read -r hash name; do
    [[ "$name" == app.db || "$name" == app.db-wal || "$name" == app.db-shm ]]
    k cp "${exportPod}:/source/$name" "$cutoverDir/sqlite-source/raw/$name" -c export
  done < "$cutoverDir/sqlite-source/source.sha256"
  k exec "$exportPod" -- sh -c "$hashCommand" > "$cutoverDir/sqlite-source/source-after.sha256"
  cmp "$cutoverDir/sqlite-source/source.sha256" "$cutoverDir/sqlite-source/source-after.sha256"
)
k get pvc mcguildlink-data -o jsonpath='{.metadata.uid}' > "$cutoverDir/sqlite-source/pvc-uid.txt"
```

次のブロックで、取得したrawファイルを一時ディレクトリへ複製し、SQLiteのbackup APIでWALを取り込んだ `$cutoverDir/app.db` を作ります。復元・移行に使うのはこの単一ファイルです。`Snapshot ready:` が表示され、整合性チェックが成功すると、`sqlite-source/snapshot.sha256` にそのハッシュを保存します。

```zsh
python3 - "$cutoverDir" <<'PY'
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
PY
(cd "$cutoverDir" && sha256sum app.db > sqlite-source/snapshot.sha256)
```

比較用の `expected-whitelist.json` を、移行に使う同じスナップショットから生成します。紐付けのあるMinecraftアカウントを重複なく抽出し、Discord・Minecraft双方のブロックを除外します。停止前の公開応答は、その後の操作や非同期更新によって保存済みDBと時点が異なるため、比較基準には使いません。

```zsh
python3 - "$cutoverDir" <<'PY'
import json, pathlib, sqlite3, sys, uuid
from contextlib import closing

root = pathlib.Path(sys.argv[1])
with closing(sqlite3.connect((root / 'app.db').as_uri() + '?mode=ro', uri=True)) as db:
    rows = db.execute('''
        SELECT DISTINCT m.uuid, m.last_known_name
        FROM minecraft_accounts m JOIN account_links l ON l.minecraft_account_id = m.id
        WHERE NOT EXISTS (SELECT 1 FROM blocked_discord_accounts b WHERE b.discord_account_id = l.discord_account_id)
          AND NOT EXISTS (SELECT 1 FROM blocked_minecraft_accounts b WHERE b.minecraft_account_id = m.id)
    ''').fetchall()
entries = [{'uuid': str(uuid.UUID(bytes=value) if isinstance(value, bytes) else uuid.UUID(value)), 'name': name}
           for value, name in rows]
entries.sort(key=lambda entry: entry['uuid'])
with (root / 'expected-whitelist.json').open('x', encoding='utf-8') as output:
    json.dump(entries, output, ensure_ascii=False)
print(f'比較用ホワイトリスト: {len(entries)}件')
PY
```

次の入力には、別マシン・外部ストレージなどを本番サーバーへマウント済みのディレクトリを指定します。コマンドはマウント操作を行いません。`$cutoverDir` 全体をその配下へコピーし、`app.db: OK` と表示されればDBコピーの照合成功です。以後、このコピー先を `$backupCopy` として使います。

```zsh
read -r 'backupDir?作業マシン以外に保管するバックアップ先（マウント済み絶対パス）: '
[[ -d "$backupDir" ]]
backupCopy="$backupDir/$(basename "$cutoverDir")"
[[ ! -e "$backupCopy" ]]
cp -a "$cutoverDir" "$backupCopy"
(cd "$backupCopy" && sha256sum -c sqlite-source/snapshot.sha256)
export backupCopy
typeset -p backupCopy >> "$cutoverDir/context.zsh"
read -r 'oldPgUses?旧PostgreSQLに保存すべき他アプリのデータがないことを確認済みなら「確認済み」と入力: '
[[ "$oldPgUses" == 確認済み ]]
```

## 3. PostgreSQLを作り直し、新Chartを配置する

現行の旧BotがPostgreSQLを使用せず、他のアプリの必要データも入っていないことを確認してから、旧PostgreSQLを停止し、そのPVCを削除します。保存すべきデータがある場合は、この削除手順へ進みません。

次のコマンドが `$cutoverDir/config/platform_admin.password` を自動作成します。中身は本番用に新しく生成する英字・数字・記号から生成する64文字のランダムな文字列（記号を必ず含む）だけで、UTF-8・BOMなし・改行なしです。`password=`、引用符、ユーザー名は書きません。手作業でのファイル作成は不要です。同じファイルからnamespace `anvilsaba` のSecret `postgres-db-credentials` の `password` キーを作り、新PostgreSQLの管理ユーザー `platform_admin` に使います。

同じHelm releaseを新Chartへ更新し、アプリreplicas=0・Migrator無効のまま、空のPostgreSQLを起動します。旧valuesを `--reuse-values` で引き継がず、新valuesを使います。

旧SQLite PVCは新Chartへの更新で削除します。`keep` は指定しません。手順2のDB保存が成功していること、新PostgreSQLのReady、旧SQLite PVCの削除を確認します。


```zsh
for file in app.db sqlite-source/snapshot.sha256 config/bot-old.toml config/mcguildlink-old.toml old-values.yaml; do
  [[ -f "$cutoverDir/$file" ]]
done
(cd "$cutoverDir" && sha256sum -c sqlite-source/snapshot.sha256)
(cd "$backupCopy" && sha256sum -c sqlite-source/snapshot.sha256)
python3 - <<'PY'
import os, pathlib, secrets, string
path = pathlib.Path(os.environ['cutoverDir']) / 'config/platform_admin.password'
if not path.exists():
    path.write_text(''.join(secrets.choice(string.ascii_letters + string.digits + string.punctuation) for _ in range(63)) + secrets.choice(string.punctuation), encoding='utf-8')
password = path.read_text(encoding='utf-8').rstrip('\r\n')
if not password or password.startswith('\ufeff') or any(c in password for c in '\0\r\n'):
    raise ValueError('管理者パスワードの形式不正')
path.write_text(password, encoding='utf-8')
PY
k create secret generic postgres-db-credentials --from-file="password=$cutoverDir/config/platform_admin.password" --dry-run=client -o yaml | k apply -f -
k scale statefulset/postgres --replicas=0
k wait --for=delete pod/postgres-0 --timeout=5m
# このファイルがあれば、切り戻しはPostgreSQL/PVCの現状を確認するR1から進む。
printf '%s\n' 'PostgreSQL再初期化を開始' > "$cutoverDir/postgres-rebuild-started"
k delete pvc postgres-data
k wait --for=delete pvc/postgres-data --timeout=5m
h upgrade "$releaseName" "$newChart" --reset-values -f "$newChart/values.prod.yaml" -f "$newValuesFile" --timeout 10m
k rollout status statefulset/postgres --timeout=5m
remainingPvc=$(k get pvc mcguildlink-data --ignore-not-found -o name)
[[ -z "$remainingPvc" ]]
# 管理ユーザーplatform_adminでロール作成SQLを実行する。
db < "$sourceDir/deploy/postgres/bootstrap.sql"
```

PostgreSQLのrolloutが成功し、bootstrapの `CREATE ROLE`・`GRANT` などの実行結果が表示されてエラーなく正常終了すれば手順3完了です。bootstrapはこのブロックで実行済みなので、手順4で再実行しません。

## 4. DBロール・Secret・Bot設定を登録する

手順3のbootstrapで4つのLOGIN（PostgreSQLのログイン用ロール）は作成済みです。次の最初のコードブロックが、以下の4ファイルを**本番サーバー上に自動作成**し、それぞれの値をDBとKubernetes Secretの両方に登録します。自分でファイルを作る必要はありません。

| パスワードファイル | DBのLOGIN | Secret（namespace `anvilsaba`）・キー |
|---|---|---|
| `$cutoverDir/config/db-credentials/platform_bot.password` | `platform_bot` | `bot-db-credentials` の `password` |
| `$cutoverDir/config/db-credentials/platform_mc_link_server.password` | `platform_mc_link_server` | `mc-link-server-db-credentials` の `password` |
| `$cutoverDir/config/db-credentials/platform_public_api.password` | `platform_public_api` | `public-api-db-credentials` の `password` |
| `$cutoverDir/config/db-credentials/platform_db_migrator.password` | `platform_db_migrator` | `db-migrator-db-credentials` の `password` |

各ファイルの書式は **パスワードの文字列だけ**です。UTF-8・BOMなし・1行で、`password=` や引用符、LOGIN名を付けません。コマンドはファイルごとに別々の64文字のランダムな文字列を英字・数字・記号からを生成し、改行なしで保存します。記号を必ず1文字以上含めます。64文字は自動生成時の長さで、入力の制限ではありません。自分で設定する場合は記号も使えますが、空文字・BOM・NUL・途中の改行は不可です。末尾のCR/LFはコマンドが除去します。

初回は必ずこの本番作業ディレクトリで新規生成します。リハーサルのファイルはコピーしません。エラー後の再実行では既に生成した本番ファイルを使い、値を変えません。手順1の `umask 077` により、ディレクトリは700、パスワードファイルは600で作成されます。

最初のPythonは4ファイルを生成・検査してから `ALTER ROLE ... PASSWORD` のSQLを `db` に渡します。続く `for` が各ファイルを `--from-file=password=...` でSecretへ登録します。**コードブロック全体を実行してください。ファイル生成だけで止めると登録は完了しません。** 最後の `COMMIT` と4つの `secret/... created` または `configured` が成功の目印です。秘密値は表示しません。

```zsh
# ファイルの値をSQLとSecretの両方へ登録する。秘密値は表示しない。
python3 - <<'PY' | db
import os, pathlib, secrets, string
root = pathlib.Path(os.environ['cutoverDir']) / 'config/db-credentials'
root.mkdir(exist_ok=True)
roles = ('platform_bot', 'platform_mc_link_server', 'platform_public_api', 'platform_db_migrator')
sql = ["BEGIN;", "SET LOCAL password_encryption = 'scram-sha-256';", "SET LOCAL standard_conforming_strings = on;"]
for role in roles:
    path = root / f'{role}.password'
    if not path.exists():
        path.write_text(''.join(secrets.choice(string.ascii_letters + string.digits + string.punctuation) for _ in range(63)) + secrets.choice(string.punctuation), encoding='utf-8')
    password = path.read_text(encoding='utf-8').rstrip('\r\n')
    if not password or password.startswith('\ufeff') or any(c in password for c in '\0\r\n'):
        raise ValueError(f'パスワードの形式不正: {role}')
    path.write_text(password, encoding='utf-8')
    escaped = password.replace("'", "''")
    sql.append(f'ALTER ROLE "{role}" PASSWORD \'{escaped}\';')
sql.append('COMMIT;')
print('\n'.join(sql))
PY
for pair in platform_bot:bot-db-credentials platform_mc_link_server:mc-link-server-db-credentials platform_public_api:public-api-db-credentials platform_db_migrator:db-migrator-db-credentials; do
  role=${pair%%:*}
  secretName=${pair#*:}
  k create secret generic "$secretName" --from-file="password=$cutoverDir/config/db-credentials/$role.password" --dry-run=client -o yaml | k apply -f -
done
```

DBとSecretの登録が成功したら、次のブロックでBot設定を変換します。入力は手順2で保存した `$cutoverDir/config/bot-old.toml` と `$cutoverDir/config/mcguildlink-old.toml`、出力は `$cutoverDir/config/bot-new.toml` です。旧Botのtoken・owners・他機能の設定はそのまま引き継ぎ、旧MC設定の `[bot]` から次の項目を追加します。手編集は不要です。

| 旧MC設定 | 新Botの `[mcguildlink]` |
|---|---|
| `guild` | `guild_id` |
| `moderator_role` | `moderator_role_id` |
| `log_channel` | `audit_channel_id` |
| `display_server_address` | `display_server_address` |

変換後のファイルを、通常デプロイで使う `/etc/anvilsaba/secrets/bot/config.toml` に所有者root・権限600で配置し、Secret `bot-config` の `config.toml` キーにも登録します。`sudo` は本番サーバー上の設定ファイルを配置するために使います。旧設定のコピーは `$cutoverDir/config/bot-old.toml` に残ります。

```zsh
python3 - "$cutoverDir" <<'PY'
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
PY
sudo install -d -o root -g root -m 700 /etc/anvilsaba/secrets/bot
sudo install -o root -g root -m 600 "$cutoverDir/config/bot-new.toml" /etc/anvilsaba/secrets/bot/config.toml
k create secret generic bot-config --from-file="config.toml=$cutoverDir/config/bot-new.toml" --dry-run=client -o yaml | k apply -f -
```

`Bot settings converted; original settings preserved` と `secret/bot-config created` または `configured` が表示され、全コマンドが正常終了したら手順4完了です。

## 5. Migratorを完了させ、凍結してデータを移行する

次のブロックを実行します。最初の `for` は手順4の4つのSecretに `password` キーがあることを検査します。Helmで `dbMigrator.enabled=true` にするとJob `db-migrator` が起動します。JobがCompleteになり、ログ取得まで成功したら `enabled=false` に戻します。失敗した場合は後続ブロックへ進みません。

失敗時の調査には、同じ作業用zshで `k get pods -l job-name=db-migrator -o wide`、`k describe job db-migrator`、`k get events --sort-by=.metadata.creationTimestamp`、`k logs job/db-migrator --all-containers=true` を実行します。SQLの凍結はMigrator成功後に行います。

保存したSQLiteから `scripts/migrate-mcguildlink.py` で移行SQLを生成し、新PostgreSQLへ適用します。手順1の `sourceUtcOffset=+00:00` を `--source-utc-offset` に指定します。続いて同じSQLiteから `--verify-only` のSQLを生成・実行し、紐付け・ブロック・未使用コードなどの全件一致と監査ログ・outboxが空であることを確認します。

SQL適用は本番の新DBへ接続する管理者で行い、`psql -X -v ON_ERROR_STOP=1` を使います。各コマンドの失敗時は先へ進みません。移行先に既存の業務データがある場合、上書きやマージはしません。


```zsh
for secretName in bot-db-credentials mc-link-server-db-credentials public-api-db-credentials db-migrator-db-credentials; do
  k get secret "$secretName" -o json | python3 -c 'import json,sys; assert json.load(sys.stdin)["data"].get("password"), "DB Secretのpasswordがありません"'
done
h upgrade "$releaseName" "$newChart" --reuse-values --set dbMigrator.enabled=true --timeout 10m
k wait --for=condition=Complete job/db-migrator --timeout=5m
k logs job/db-migrator
h upgrade "$releaseName" "$newChart" --reuse-values --set dbMigrator.enabled=false
```

Migratorの完了・無効化が成功したら、次を実行します。Bot・MC・APIはここまでの手順でreplicas=0です。残ったPodの終了を待ってから、`postgres-0` 内の `psql` を管理ユーザー `platform_admin` で実行し、手順1で取得した凍結SQLを標準入力へ渡します。

```zsh
appPods=$(k get pods -l 'app.kubernetes.io/name in (bot,mc-link-server,public-api)' -o name)
if [[ -n "$appPods" ]]; then
  k wait --for=delete pod -l 'app.kubernetes.io/name in (bot,mc-link-server,public-api)' --timeout=5m
fi
kubectl --context default -n anvilsaba exec -i postgres-0 -- \
  psql -X -U platform_admin -d platform -v ON_ERROR_STOP=1 \
  < "$sourceDir/scripts/mcguildlink-cutover-freeze.sql"
```

最後に `COMMIT` が表示され、コマンドが正常終了したら凍結完了です。エラーの場合は次の移行SQLを実行しません。凍結後に `bootstrap.sql` を再実行すると書き込み権限が復活するため、再実行しません。

次のブロックは `$cutoverDir/app.db` から `$cutoverDir/import.sql` を生成して新DBへ適用し、さらに `$cutoverDir/verify.sql` を生成して全件照合します。両SQLの生成先は新規ファイルである必要があり、既存ファイルは上書きしません。途中で失敗した場合は、DBへの適用状況を確認せずにブロックを最初から再実行しないでください。

```zsh
python3 "$sourceDir/scripts/migrate-mcguildlink.py" "$cutoverDir/app.db" "$cutoverDir/import.sql" "--source-utc-offset=$sourceUtcOffset"
db < "$cutoverDir/import.sql"
python3 "$sourceDir/scripts/migrate-mcguildlink.py" "$cutoverDir/app.db" "$cutoverDir/verify.sql" "--source-utc-offset=$sourceUtcOffset" --verify-only
db < "$cutoverDir/verify.sql"
```

各テーブルの `table_name`・`verified_rows` と最後の `COMMIT` が表示され、エラーなしで終了したら手順5完了です。照合SQLは件数だけでなく全行の値も比較し、不一致や監査データの混入があれば失敗します。

### 手順5のSQL生成・適用・照合に失敗した場合

同じ作業ディレクトリの `context.zsh` を読み込み、凍結を維持したまま次を実行します。生成済みファイルの削除や、手順3のDB再初期化は行いません。接続切断ではCOMMIT済みか分からないため、まず新しい照合SQLで状態を判定します。

| 状態 | 再開方法 |
|---|---|
| 移行SQLの生成前・適用前に失敗 | 下の照合・空状態確認を経て、新しいパスに移行SQLを生成して適用する |
| 適用中に失敗、またはCOMMIT結果が不明 | 下の全件照合が成功すれば再インポートせず手順6へ進む |
| 適用成功後の照合で失敗 | 下の照合を再試行。空でなく一致もしない場合は停止し、切り戻しを選ぶ |

```zsh
(cd "$cutoverDir" && sha256sum -c sqlite-source/snapshot.sha256)
[[ ! -e "$cutoverDir/unfreeze-started" ]]
db < "$sourceDir/scripts/mcguildlink-cutover-freeze.sql"
recoveryDir=$(mktemp -d "$cutoverDir/sql-retry.XXXXXX")
python3 "$sourceDir/scripts/migrate-mcguildlink.py" "$cutoverDir/app.db" "$recoveryDir/verify.sql" "--source-utc-offset=$sourceUtcOffset" --verify-only
if db < "$recoveryDir/verify.sql"; then
  printf '%s\n' '移行済みデータが一致しました。再インポートせず手順6へ進みます。'
else
  # 接続・スキーマの問題もここで停止する。空でないDBへ再投入しない。
  db <<'SQL'
DO $$ BEGIN
    IF EXISTS (SELECT FROM mcguildlink.discord_accounts) OR EXISTS (SELECT FROM mcguildlink.minecraft_accounts)
       OR EXISTS (SELECT FROM mcguildlink.block_groups) OR EXISTS (SELECT FROM mcguildlink.blocked_discord_accounts)
       OR EXISTS (SELECT FROM mcguildlink.blocked_minecraft_accounts) OR EXISTS (SELECT FROM mcguildlink.account_links)
       OR EXISTS (SELECT FROM mcguildlink.link_requests) OR EXISTS (SELECT FROM mcguildlink.audit_logs)
       OR EXISTS (SELECT FROM mcguildlink.audit_outbox) THEN
        RAISE EXCEPTION '移行先が空でなく、全件照合も失敗しました。再投入せず切り戻しを選んでください';
    END IF;
END $$;
SQL
  python3 "$sourceDir/scripts/migrate-mcguildlink.py" "$cutoverDir/app.db" "$recoveryDir/import.sql" "--source-utc-offset=$sourceUtcOffset"
  db < "$recoveryDir/import.sql"
  db < "$recoveryDir/verify.sql"
fi
```

ブロック全体が成功したら手順5完了です。以後の再照合に使うファイルを更新し、手順6へ進みます。失敗した場合はここで停止し、データやDB履歴を削除して回避しません。

```zsh
cp "$recoveryDir/verify.sql" "$cutoverDir/verify.sql"
```

## 6. 凍結を維持して起動・公開経路を確認する

新Bot・MC・APIを起動し、各PodのReady・ログ・マイグレーション互換性を確認します。チェックサム不一致の場合はイメージのソースを修正し、DBの履歴を書き換えて回避しません。

BotへTTY付きでattachし、コンソールの `register` と `list` でグローバルコマンドを登録・確認します。既存Botの他機能も確認します。

クラスタ内のPublic APIと保存済みSQLiteから作ったホワイトリストを比較してから、既存の本番Tunnelにある `api.anvilsaba.org` のService URLを `http://public-api:8080` に変更します。本番Tunnelのtokenと公開hostnameは維持します。旧HTTP Service名を残す互換Serviceは作成しません。外部からUUID・名前・ブロック除外の一致を確認します。MCは従来の公開アドレス・ポートから、Java 26.3で状態応答・本人認証・ダイアログまで確認します。

この段階ではコード消費・発行などの業務書き込みは拒否される想定です。公開経路の確認後に再度全件照合し、データが変わっていないことを確認します。


```zsh
h upgrade "$releaseName" "$newChart" --reuse-values --set bot.replicas=1 --set mcLinkServer.replicas=1 --set publicApi.replicas=1
for service in bot mc-link-server public-api; do
  k rollout status "deployment/$service" --timeout=5m
done
k attach -it --detach-keys=ctrl-c deployment/bot
```

attachした画面で、次を1行ずつ入力してEnterを押します。この2行はzshではなくBotコンソールへ入力します。

```text
register
list
```

登録結果と一覧に `/create_panel` が含まれることを確認し、Ctrl+Cで接続を終了します。`--detach-keys=ctrl-c` を指定しているのでBotは停止しません。

次のブロックは一時Podから `http://public-api:8080/whitelist.json` を取得し、`$cutoverDir/internal-whitelist.json` に保存します。保存済みSQLiteから作った `expected-whitelist.json` とUUID・名前の全件一致を検査します。Pythonが何も表示せず正常終了すれば一致です。その後、一時Podを削除します。失敗時にPodが残っている場合は `k delete pod cutover-http-check --ignore-not-found` を実行してから、このブロックを再試行します。

```zsh
k run cutover-http-check --restart=Never --image=curlimages/curl:8.15.0 --command -- curl --fail --silent --show-error "http://public-api:8080/whitelist.json"
k wait --for=jsonpath='{.status.phase}'=Succeeded pod/cutover-http-check --timeout=60s
k logs pod/cutover-http-check > "$cutoverDir/internal-whitelist.json"
python3 - "$cutoverDir/expected-whitelist.json" "$cutoverDir/internal-whitelist.json" <<'PY'
import json, pathlib, sys
def rows(path):
    return sorted((x['uuid'], x['name']) for x in json.loads(pathlib.Path(path).read_text()))
if rows(sys.argv[1]) != rows(sys.argv[2]):
    raise ValueError('クラスタ内ホワイトリストが旧版と一致しません')
PY
k delete pod/cutover-http-check
```

クラスタ内の照合成功後、Cloudflareで本番Tunnelの既存ルートを編集します。hostnameは `api.anvilsaba.org` のまま、ServiceのTypeを `HTTP`、URLを `public-api:8080` にして保存します。これは接続先URL全体では `http://public-api:8080` です。Service URLに `/whitelist.json` は付けません。新しいTunnelやtokenは作成せず、既存本番のルートだけ変更します。HelmではCloudflare側の設定は更新されません。保存後、次のコマンドで外部から確認します。

```zsh
curl --fail --silent --show-error -A 'Mozilla/5.0' "https://$productionHostname/whitelist.json" > "$cutoverDir/new-whitelist.json"
python3 - "$cutoverDir/expected-whitelist.json" "$cutoverDir/new-whitelist.json" <<'PY'
import json, pathlib, sys
def rows(path):
    return sorted((x['uuid'], x['name']) for x in json.loads(pathlib.Path(path).read_text()))
if rows(sys.argv[1]) != rows(sys.argv[2]):
    raise ValueError('本番公開ホワイトリストが旧版と一致しません')
PY
db < "$cutoverDir/verify.sql"
```

MCの本番アドレスのポート25600にJava 26.3で接続し、本人認証・ダイアログを確認します。コード消費はまだ行いません。

## 7. 結果を確認して解禁する

手順6まで成功した時点で、本番を新版で継続するか判断します。不一致や起動・公開経路の問題が残る場合は解禁せず、別文書の切り戻し手順を選びます。

継続する場合はBot・MC・APIを停止してPod終了を確認し、管理者で `scripts/mcguildlink-cutover-unfreeze.sql` を適用します。その後アプリを再開し、起動を確認します。ここから先の変更は旧SQLiteへ戻せません。


手順6の確認が全て成功し、新版で運用を継続すると決めた場合だけ実行します。

```zsh
k scale deployment/bot deployment/mc-link-server deployment/public-api --replicas=0
newPods=$(k get pods -l 'app.kubernetes.io/name in (bot,mc-link-server,public-api)' -o name)
if [[ -n "$newPods" ]]; then
  k wait --for=delete pod -l 'app.kubernetes.io/name in (bot,mc-link-server,public-api)' --timeout=5m
fi
# 解禁の実行結果が不明な場合も、旧版へ戻す手順へ進まないための記録。
printf '%s\n' '書き込み解禁を開始' > "$cutoverDir/unfreeze-started"
db < "$sourceDir/scripts/mcguildlink-cutover-unfreeze.sql"
h upgrade "$releaseName" "$newChart" --reuse-values --set bot.replicas=1 --set mcLinkServer.replicas=1 --set publicApi.replicas=1
for service in bot mc-link-server public-api; do
  k rollout status "deployment/$service" --timeout=5m
done
```

## 8. 運用再開を確認する

Discordで、旧パネルを置いていたチャンネルを開き、設定済みのmoderatorロールを持つアカウントから統合Botの `/create_panel` を実行します。「パネルを作成しました！」と表示され、そのチャンネルに新しいパネルが投稿されれば設置成功です。新パネルの動作確認後、旧専用Botのパネルメッセージを削除します。

本人の協力を得たテスト用アカウントで、次を順に確認します。通常利用者のデータをテスト目的で変更しません。

1. 新パネルの「MCアカウントと紐付ける」を押し、接続先とコードが表示されることを確認します。もう一度押し、同じ未使用コードが表示されることを確認します。
2. 表示されたサーバーへMinecraft Java 26.3で接続し、入力欄にコードを入れます。紐付け成功後、新パネルの「紐付けられたアカウントを確認する」で対象アカウントを確認します。
3. 同じコードをもう一度使い、再利用が拒否されることを確認します。
4. 同意済みのブロック対象テストアカウントでは、コード取得または紐付けが拒否されることを確認します。
5. `https://api.anvilsaba.org/whitelist.json` で新しい紐付けが反映され、ブロック対象は除外されることを確認します。
6. 旧 `log_channel` から引き継いだ監査チャンネルに、紐付けなどの新しい操作の監査が届くことを確認します。

リハーサルの移行元には未使用コードがなかったため、「旧コードの保持・消費」は実データでのリハーサル未検証です。本番の保存データに存在するコードは手順5・6のSQLで全件照合します。消費テストは解禁後に本人の協力を得たテスト用コードで行います。

使用したChart・イメージdigest・設定・バックアップの場所・照合結果・解禁時刻を記録します。バックアップは直ちに削除しません。通常デプロイはMigrator無効の値を維持し、移行用コードと旧MCGuildLinkの削除は別作業にします。


```zsh
k get pods
h get values "$releaseName" -o yaml > "$cutoverDir/new-values-after-cutover.yaml"
date --iso-8601=seconds > "$cutoverDir/completed-at.txt"
# 本番作業中に生成したDBパスワード・新Bot設定・最終valuesも別保管先へ追加する。
cp -a "$cutoverDir/." "$backupCopy/"
```
