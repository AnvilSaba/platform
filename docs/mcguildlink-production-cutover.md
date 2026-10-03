# 本番の旧MCGuildLinkから分離構成へ移行する手順

デプロイ前準備 → GitHub Actionsでデプロイ → デプロイの確認、の順に進めます。エラーが出たら次へ進みません。

## 1. デプロイ前準備

### 1.1. 公開物と作業環境を準備する

1. 停止時間と対応クライアント（Minecraft Java 26.3）を案内する。
2. 外部バックアップ先を本番サーバーへマウントする。
3. ReleaseでChartを `bump=major / dry_run=false / deploy=false` で公開する。
4. Bot・MC・APIを、同じマイグレーションSQLを含むソースから `dry_run=false / deploy=false` で公開し、タグ・digest・ソースSHAを記録する。
5. `production/chart` のSSH接続先・ユーザーと本番kubeconfigを確認する。

本番デプロイ用ユーザーのzshで、以下のブロックを順に実行します。Python 3.11以降を使い、移行中は他のデプロイを実行しません。

途中から再開するときは、zshで `setopt ERR_EXIT PIPE_FAIL; umask 077` を設定し、作成済みの `$cutoverDir/context.zsh` を読み込みます。Actions実行中は読み込みません。

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
prepareChart="$cutoverDir/charts/prepare/platform"
sourceDir="$cutoverDir/source"
export productionContext productionNamespace releaseName chartOci newChartVersion oldChart newChart prepareChart sourceDir productionHostname sourceUtcOffset botTag mcTag apiTag migratorTag cutoverDir oldValues newValuesFile stateDir
k() { kubectl --context "$productionContext" -n "$productionNamespace" "$@"; }
h() { helm "$@" --kube-context "$productionContext" -n "$productionNamespace"; }
db() { k exec -i postgres-0 -- psql -X -U platform_admin -d platform -v ON_ERROR_STOP=1; }
typeset -p KUBECONFIG productionContext productionNamespace releaseName chartOci newChartVersion oldChart newChart prepareChart sourceDir productionHostname sourceUtcOffset botTag mcTag apiTag migratorTag cutoverDir oldValues newValuesFile stateDir > "$cutoverDir/context.zsh"
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
# 準備中だけ使うChartを複製する。公開Chart・リポジトリは変更しない。
mkdir -p "$cutoverDir/charts/prepare"
cp -a "$newChart" "$prepareChart"
python3 - "$prepareChart" <<'PY'
import pathlib, sys
chart = pathlib.Path(sys.argv[1])
for name, key in (('bot', 'bot'), ('mc-link-server', 'mcLinkServer'), ('public-api', 'publicApi')):
    path = chart / 'templates' / f'{name}-deployment.yaml'
    text = path.read_text(encoding='utf-8')
    source = '{{ .Values.' + key + '.replicas }}'
    if text.count(source) != 1:
        raise ValueError(f'準備用Chartのreplicasを設定できません: {name}')
    path.write_text(text.replace(source, '0'), encoding='utf-8')
PY
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
    'bot': {'replicas': 1, 'image': {'repository': 'ghcr.io/anvilsaba/bot', 'tag': os.environ['botTag']}},
    'mcLinkServer': {'replicas': 1, 'image': {'repository': 'ghcr.io/anvilsaba/mc-link-server', 'tag': os.environ['mcTag']}},
    'publicApi': {'replicas': 1, 'image': {'repository': 'ghcr.io/anvilsaba/public-api', 'tag': os.environ['apiTag']}},
    'dbMigrator': {'enabled': False, 'image': {'repository': 'ghcr.io/anvilsaba/db-migrator', 'tag': os.environ['migratorTag']}},
    'postgres': {'username': 'platform_admin', 'database': 'platform', 'secretName': 'postgres-db-credentials', 'storageSize': pvc['resources']['requests']['storage'], 'storageClassName': pvc.get('storageClassName', '')},
    'cloudflared': {'replicas': 1, 'tokenSecretName': 'cloudflare-tunnel'},
}
(root / 'new-values.json').write_text(json.dumps(values, ensure_ascii=False), encoding='utf-8')
PY
curl --fail --silent --show-error -A 'Mozilla/5.0' "https://$productionHostname/whitelist.json" > "$cutoverDir/old-whitelist.json"
k get secret ghcr-pull cloudflare-tunnel -o name
h template "$releaseName" "$prepareChart" -f "$newChart/values.prod.yaml" -f "$newValuesFile" > "$cutoverDir/new-manifest.yaml"
```

<a id="2-旧アプリを停止し復元に必要なものを保存する"></a>

### 1.2. 旧版を停止してバックアップする

旧設定を保存し、旧Bot・MCGuildLinkを停止します。ほかの書き込み元も停止してください。

```zsh
k get secret bot-config -o json | python3 -c 'import base64,json,pathlib,sys; pathlib.Path(sys.argv[1]).write_bytes(base64.b64decode(json.load(sys.stdin)["data"]["config.toml"]))' "$cutoverDir/config/bot-old.toml"
k get secret mcguildlink-config -o json | python3 -c 'import base64,json,pathlib,sys; pathlib.Path(sys.argv[1]).write_bytes(base64.b64decode(json.load(sys.stdin)["data"]["app.toml"]))' "$cutoverDir/config/mcguildlink-old.toml"
k scale deployment/mcguildlink deployment/bot --replicas=0
oldPods=$(k get pods -l 'app.kubernetes.io/name in (mcguildlink,bot)' -o name)
if [[ -n "$oldPods" ]]; then
  k wait --for=delete pod -l 'app.kubernetes.io/name in (mcguildlink,bot)' --timeout=5m
fi
```

旧PVCからSQLite本体・WAL・SHMを取り出し、コピー前後のハッシュを照合します。

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

WALを取り込んだ `app.db` を作り、整合性とハッシュを確認します。

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

同じ `app.db` から照合用ホワイトリストを生成します。

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

マウント済みの外部バックアップ先へコピーし、`app.db: OK` を確認します。

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

### 1.3. PostgreSQLを初期化する

旧PostgreSQLに保存すべきデータがないことを確認してから実行します。バックアップ済みの旧SQLite PVCも、この更新で削除されます。

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
h upgrade "$releaseName" "$prepareChart" --reset-values -f "$newChart/values.prod.yaml" -f "$newValuesFile" --timeout 10m
k rollout status statefulset/postgres --timeout=5m
remainingPvc=$(k get pvc mcguildlink-data --ignore-not-found -o name)
[[ -z "$remainingPvc" ]]
# 管理ユーザーplatform_adminでロール作成SQLを実行する。
db < "$sourceDir/deploy/postgres/bootstrap.sql"
```

### 1.4. DB認証情報とBot設定を登録する

DBロールのパスワードを生成し、DBとSecretへ登録します。

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

旧設定から新Bot設定を生成し、本番ファイルとSecretへ登録します。

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

<a id="5-migratorを完了させ凍結してデータを移行する"></a>

### 1.5. スキーマとデータを移行する

Migratorを実行し、完了後に無効化します。

```zsh
for secretName in bot-db-credentials mc-link-server-db-credentials public-api-db-credentials db-migrator-db-credentials; do
  k get secret "$secretName" -o json | python3 -c 'import json,sys; assert json.load(sys.stdin)["data"].get("password"), "DB Secretのpasswordがありません"'
done
h upgrade "$releaseName" "$prepareChart" --reuse-values --set dbMigrator.enabled=true --timeout 10m
k wait --for=condition=Complete job/db-migrator --timeout=5m
k logs job/db-migrator
h upgrade "$releaseName" "$prepareChart" --reuse-values --set dbMigrator.enabled=false
```

アプリの停止を確認してDBを凍結します。

```zsh
appPods=$(k get pods -l 'app.kubernetes.io/name in (bot,mc-link-server,public-api)' -o name)
if [[ -n "$appPods" ]]; then
  k wait --for=delete pod -l 'app.kubernetes.io/name in (bot,mc-link-server,public-api)' --timeout=5m
fi
kubectl --context default -n anvilsaba exec -i postgres-0 -- \
  psql -X -U platform_admin -d platform -v ON_ERROR_STOP=1 \
  < "$sourceDir/scripts/mcguildlink-cutover-freeze.sql"
```

凍結後はbootstrapを再実行しません。SQLiteを移行し、全件照合します。

```zsh
python3 "$sourceDir/scripts/migrate-mcguildlink.py" "$cutoverDir/app.db" "$cutoverDir/import.sql" "--source-utc-offset=$sourceUtcOffset"
db < "$cutoverDir/import.sql"
python3 "$sourceDir/scripts/migrate-mcguildlink.py" "$cutoverDir/app.db" "$cutoverDir/verify.sql" "--source-utc-offset=$sourceUtcOffset" --verify-only
db < "$cutoverDir/verify.sql"
```

全件照合と最後の `COMMIT` が成功したら手順2へ進みます。

#### 移行・照合に失敗した場合

凍結を維持して次を実行します。DBの再初期化や生成済みファイルの削除は行いません。不一致が解消しなければ[切り戻し手順](mcguildlink-production-rollback.md)へ進みます。

```zsh
(cd "$cutoverDir" && sha256sum -c sqlite-source/snapshot.sha256)
[[ ! -e "$cutoverDir/unfreeze-started" ]]
db < "$sourceDir/scripts/mcguildlink-cutover-freeze.sql"
recoveryDir=$(mktemp -d "$cutoverDir/sql-retry.XXXXXX")
python3 "$sourceDir/scripts/migrate-mcguildlink.py" "$cutoverDir/app.db" "$recoveryDir/verify.sql" "--source-utc-offset=$sourceUtcOffset" --verify-only
if db < "$recoveryDir/verify.sql"; then
  printf '%s\n' '移行済みデータが一致しました。再インポートせず手順2へ進みます。'
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

成功したら照合SQLを更新し、手順2へ進みます。

```zsh
cp "$recoveryDir/verify.sql" "$cutoverDir/verify.sql"
```

## 2. GitHub Actionsでデプロイ

アプリの停止と全件照合を確認し、作業ロックを解放します。

```zsh
k get deployment bot mc-link-server public-api -o json | python3 -c 'import json,sys; assert all(d["spec"]["replicas"] == 0 for d in json.load(sys.stdin)["items"]), "準備中のアプリが起動しています"'
db < "$cutoverDir/verify.sql"
flock -u 9
exec 9>&-
```

1. **Actions → Production deployment → Run workflow** を開く。
2. ブランチを `main`、`target` を `chart`、`release_ref` を `chart/v<手順1.1の公開バージョン>` にして実行する。
3. 成功を確認し、実行URLを記録する。

## 3. デプロイの確認

Actions終了後、同じzshでロックを再取得します。別シェルで `context.zsh` を読み込んだ場合、このブロックは不要です。

```zsh
exec 9>"$stateDir/deploy.lock"
flock -n 9
```

### 3.1. 起動と公開経路を確認する

デプロイされたイメージとPodの起動を確認し、Botコンソールへ接続します。

```zsh
k get deployment bot mc-link-server public-api -o json > "$cutoverDir/new-deployments.json"
python3 - "$cutoverDir" <<'PY'
import json, os, pathlib, sys
expected = {
    'bot': 'ghcr.io/anvilsaba/bot:' + os.environ['botTag'],
    'mc-link-server': 'ghcr.io/anvilsaba/mc-link-server:' + os.environ['mcTag'],
    'public-api': 'ghcr.io/anvilsaba/public-api:' + os.environ['apiTag'],
}
items = json.loads((pathlib.Path(sys.argv[1]) / 'new-deployments.json').read_text())['items']
if len(items) != 3 or any(
    item['spec']['replicas'] != 1
    or item['spec']['template']['spec']['containers'][0]['image'] != expected[item['metadata']['name']]
    for item in items
):
    raise ValueError('Actions のデプロイ結果が選択したイメージ・replicasと一致しません')
PY
k get pods -o json > "$cutoverDir/new-pods.json"
for service in bot mc-link-server public-api; do
  k rollout status "deployment/$service" --timeout=5m
done
k attach -it --detach-keys=ctrl-c deployment/bot
```

Botコンソールで次を入力します。

```text
register
list
```

`/create_panel` の登録を確認してCtrl+Cで抜け、クラスタ内のホワイトリストを照合します。

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

Cloudflareで既存ルートのService URLを `http://public-api:8080` に変更し、外部応答とDBを照合します。

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

Minecraft Java 26.3で本番ポート25600へ接続し、本人認証とダイアログを確認します。コードの発行・消費はまだ行いません。

### 3.2. 書き込みを解禁する

問題があれば解禁せず[切り戻し手順](mcguildlink-production-rollback.md)へ進みます。

継続する場合はアプリを停止し、DBを解禁して同じイメージで再開します。**解禁開始後は旧SQLiteへ切り戻せません。**

```zsh
k scale deployment/bot deployment/mc-link-server deployment/public-api --replicas=0
newPods=$(k get pods -l 'app.kubernetes.io/name in (bot,mc-link-server,public-api)' -o name)
if [[ -n "$newPods" ]]; then
  k wait --for=delete pod -l 'app.kubernetes.io/name in (bot,mc-link-server,public-api)' --timeout=5m
fi
# 解禁の実行結果が不明な場合も、旧版へ戻す手順へ進まないための記録。
printf '%s\n' '書き込み解禁を開始' > "$cutoverDir/unfreeze-started"
db < "$sourceDir/scripts/mcguildlink-cutover-unfreeze.sql"
k scale deployment/bot deployment/mc-link-server deployment/public-api --replicas=1
for service in bot mc-link-server public-api; do
  k rollout status "deployment/$service" --timeout=5m
done
```

### 3.3. 運用を確認して終了する

1. moderatorロールのアカウントで、旧パネルと同じチャンネルに `/create_panel` で新パネルを設置する。
2. 協力者のテストアカウントで、コード発行・未使用コードの再表示・Minecraftでの紐付けを確認する。
3. 使用済みコードの再利用とブロック対象の操作が拒否されることを確認する。
4. ホワイトリストへの反映と監査チャンネルへの通知を確認する。
5. 旧パネルを削除し、Chart・イメージdigest・照合結果・解禁時刻を記録する。
6. 最終設定をバックアップへ追加する。

```zsh
k get pods
h get values "$releaseName" -o yaml > "$cutoverDir/new-values-after-cutover.yaml"
date --iso-8601=seconds > "$cutoverDir/completed-at.txt"
# 本番作業中に生成したDBパスワード・新Bot設定・最終valuesも別保管先へ追加する。
cp -a "$cutoverDir/." "$backupCopy/"
```

ここで移行完了です。バックアップは保持します。
