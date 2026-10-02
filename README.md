# Platform

あんびる鯖 の Discord Bot、MC Link Server、公開 API、DB Migrator、旧 Kotlin 版 MCGuildLink、および k3s / Helm デプロイ設定を管理するモノレポです。

## ドキュメント

`mise run test` で DB 起動とテストをまとめて実行します。
`mise run up` で DB を準備して全アプリを並列起動します。Ctrl+C でアプリを終了し、`mise run down` で DB を停止します。
前提ツールと設定方法は [開発手順](docs/development-and-integration-testing.md#ローカル開発) を参照してください。

- [開発・個別テスト・統合テスト手順](docs/development-and-integration-testing.md)
- [本番デプロイ手順](docs/deployment.md)
- [変更履歴とリリース手順](docs/releases.md)

## ライセンス

このリポジトリの本体コードは、ルートの [MIT License](LICENSE) で公開しています。

各アプリは別々のコンテナイメージとして配布するため、第三者ライセンスもイメージごとに同梱します。

- Rust アプリ：`/app/THIRD_PARTY_LICENSES`
- 旧 Kotlin 版：`/app/THIRD_PARTY_LICENSES/index.html` と同ディレクトリ内のライセンスファイル

第三者ライセンス一覧は、コンテナイメージのビルド時に自動生成されます。DockerfileはPodmanでビルドできます。
