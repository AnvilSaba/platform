# MCGuildLinkリハーサルの実施記録

2026-10-01〜02の特定回の記録です。[共通手順](mcguildlink-k3d-rehearsal.md)の完了保証ではありません。以下の初回ログはそのまま保持し、異なる時点の記録を区別します。正確な時刻は元記録にありません。

| 記録時点 | 実施状況・未確認項目 |
|---|---|
| 2026-10-01 | 旧タグ取得・amd64ビルド、専用VMの起動障害とcgroup修復 |
| 2026-10-02、旧環境起動時（revision 2） | 旧環境・公開経路・SQLite作成を確認。この時点では元データ作成・移行・切り戻しは未実施 |
| 同日、移行後の集計・Bot修正（revision 12） | 新アプリ起動・全件照合・凍結を確認。この時点では新PVCへの実データ復元・公開経路比較は未実施 |
| 同日、その後のユーザー手動確認 | 同じreleaseの旧版復帰、公開経路と旧データ維持、解禁後のコード消費・監査は成功の報告あり。未使用コードは0件のため移行前コードの保持・消費は未検証 |

原記録の「未実施」「まだ実施していません」はそれぞれの記録時点の状態です。後日の手動成功報告は個別のPVC UID・復元コマンドの証跡や新版公開経路比較の全項目確認を補完しません。多対多、移行前未使用コードを含む最終テストの全項目完了とは扱いません。

## 別の一時namespaceで実施した補助検証

2026-10-02に、別文書の切り戻し手順R2の復元Pod・コピー・検証・削除のコマンドを、別の一時namespaceと空PVCで実行しました。検証用SQLiteのコピー、SHA256一致、所有者65532:65532、権限600、`app.db` への配置、作業Pod削除を確認しています。検証namespaceは削除済みです。実データのPVCは操作しておらず、同じreleaseでの移行・切り戻しの完了を意味しません。

同日に手順3のパスワード設定・Secret登録・Bot設定変換も別の一時PostgreSQLで実行し、4ロールすべてのTCPパスワード認証、各Secretとパスワードファイルの一致、Bot設定Secretの一致と他機能の設定保持を確認しました。こちらの検証namespaceも削除済みです。

手動のパスワードファイルについても、記号・単一引用符・バックスラッシュ・日本語を含む値で同じコマンドを実行し、4ロールのTCP認証とSecretのバイト一致を確認しました。

## 初回の作業ログと結果（原記録）

2026-10-02、古い `bot:test` は起動時に `Migration 20260927120000 checksum does not match this application` で停止しました。DBの全マイグレーションのSHA384は現在のソースと一致しており、Botのみビルドが古い状態でした。現在のソースから再ビルドした `localhost/anvilsaba/bot:rehearsal-5310812`（イメージID `90eb6a035149ddc5810f31f96ca68209927ab39cca2276d487d30e0b109f920b`）へ差し替え、Helm revision 12でBotのReady・再起動0回・Discord接続を確認しました。DB履歴は変更せず、書き込み凍結を維持しています。

| 確認項目 | 結果 |
|---|---|
| リリースタグの別フォルダ取得・旧Bot/MCGuildLinkのamd64ビルド | 2026-10-01成功 |
| 最新旧リリース・同じk3s・旧公開経路の起動 | 2026-10-02成功。4 Pod Ready、MC状態応答、公開HTTP 200 |
| 旧版から作ったSQLite・未使用コード・ブロック | 保存済みSQLiteとの全件照合成功。紐付け1件、ブロックグループ1件、未使用コード0件 |
| PostgreSQL削除後のDB/ロール/password再初期化 | 2026-10-02成功。4つのDB Secret登録、Migrator完了、スキーマ作成、書き込み凍結を確認 |
| 同じrelease更新後の旧SQLite PVC削除・新PVCへのDBファイル復元 | 未実施 |
| 新版起動・経路切替・凍結中の全件一致 | 新Bot・MC・API起動、凍結中の全件照合成功。公開経路の比較は未実施 |
| 同じreleaseでの旧版復帰・公開経路と旧データ維持 | 2026-10-02、ユーザーによる手動確認成功 |
| 解禁後のコード消費・監査 | 2026-10-02、ユーザーによる手動確認成功。移行前の未使用コード保持は対象データ0件のため未検証 |

2026-10-01のk3d作成は、Podmanコンテナ単体でも再現する `controller pids is not available` エラーで失敗しました。WSL 3.0.1.0 / Podman 6.1.0で、別名のrootful VMを作成しても再現しました。新VM `podman-machine-rehearsal` だけに識別用cgroupを置き、管理者のWSLデバッグシェルからそのVMの親階層のプロセスを子階層へ移して制御を委譲すると、コンテナ起動とk3d作成に成功しました。既存VM・既存 `anvilsaba` クラスタは保持しています。cgroup修復は揮発設定のため、VM再起動後は起動確認が必要です。

2026-10-02に旧Chartをrelease `platform` / namespace `anvilsaba` へインストールし、HTTP/2指定を含むHelm revision 2まで更新しました。4 PodはReady、Botと旧MCGuildLinkのDiscord接続、PostgreSQL 18.6のDB/ユーザー `anvilsaba`、旧アプリ自身によるSQLite作成を確認しています。Minecraftは `localhost:25600` で26.2 / protocol 776の状態応答を返し、テストTunnelの `https://mcguildlink.lapis256.dev/whitelist.json` はHTTP 200 / `[]` を返しました。HTTP確認は `User-Agent: Mozilla/5.0` を指定しています。元データ作成・移行・切り戻しはまだ実施していません。

取得ソースのSHAはBot `60978a586a08f166380c3a076eba83d336c0627f`、MCGuildLink `7bae0288ae856531bc7a156c6ca1a389b56fff1c`、Chart `112188c169a6f306e9a67ccdb255ba875064572b`。ビルド済みイメージIDはBot `b2b0e9111a53a41e7b22088c4025d9d42d8dcb7d35dad8b07ba2504613de0efe`、MCGuildLink `b4879ef9ba1bb972445d4864a0cc6da984885ce391f13e1f2bdf8ca6a6588133` です。
