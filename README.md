# Platform

あんびる鯖 の Discord Bot、MC Link Server、公開 API、DB Migrator、および k3s / Helm デプロイ設定を管理するモノレポです。

## ドキュメント

`mise :test` で DB 起動とテストをまとめて実行します。
`mise :up` で DB を準備して全アプリを並列起動します。Ctrl+C でアプリを終了し、`mise :down` で DB を停止します。
前提ツールと設定方法は [開発手順](docs/development-and-integration-testing.md#ローカル開発) を参照してください。
個別タスクは `mise //apps/bot:check` のように指定します。全体の一覧は `mise tasks ls --all` で確認できます。

- [開発・個別テスト・統合テスト手順](docs/development-and-integration-testing.md)
- [本番デプロイ手順](docs/deployment.md)
- [変更履歴とリリース手順](docs/releases.md)

## ライセンス

このリポジトリの本体コードは、ルートの [MIT License](LICENSE) で公開しています。

各アプリは別々のコンテナイメージとして配布するため、第三者ライセンスもイメージごとに同梱します。

Rust アプリの第三者ライセンスは `/app/THIRD_PARTY_LICENSES` に同梱します。

第三者ライセンス一覧は、コンテナイメージのビルド時に自動生成されます。DockerfileはPodmanでビルドできます。
