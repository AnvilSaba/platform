# 旧MCGuildLinkから分離構成への移行

本番の操作は[本番移行手順](mcguildlink-production-cutover.md)を順に実行します。

## 1. デプロイ前準備

1. Chart・Bot・MC・APIを自動デプロイなしで公開する。
2. 旧版を停止し、SQLite・旧設定・旧Chartを外部へバックアップする。
3. 準備専用ChartでPostgreSQLを初期化し、DB認証情報と新Bot設定を登録する。
4. Migratorを完了させ、DBを凍結する。
5. SQLiteを移行して全件照合する。

### 旧版停止とデータ保護

旧版の停止後、SQLite本体とWAL・SHMを取り出し、WALを取り込んだ `app.db` を作ります。整合性とハッシュを確認してから、旧PVCを削除する手順へ進みます。

## 2. GitHub Actionsでデプロイ

Production deploymentを `target=chart`、`release_ref=chart/v<公開バージョン>` で実行し、成功を確認します。

## 3. デプロイの確認

1. 新アプリの起動とBotコマンド登録を確認する。
2. クラスタ内のホワイトリストを照合し、Cloudflareの宛先を切り替えて外部応答を確認する。
3. Minecraftの接続とDBの全件照合を確認する。
4. 問題がなければ書き込みを解禁し、パネル・紐付け・ブロック・監査通知を確認して終了する。

解禁前の失敗は[本番切り戻し手順](mcguildlink-production-rollback.md)へ進みます。解禁開始後は旧SQLiteへ切り戻せません。

事前確認は[k3dリハーサル手順](mcguildlink-k3d-rehearsal.md)、検証済みの範囲は[実行記録](mcguildlink-rehearsal-results.md)を参照してください。
