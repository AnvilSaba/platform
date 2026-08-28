# Platform

あんびる鯖 の Discord Bot、MCGuildLink、および k3s / Helm デプロイ設定を管理するモノレポです。

## ドキュメント

- [開発・個別テスト・統合テスト手順](docs/development-and-integration-testing.md)
- [本番デプロイ手順](docs/deployment.md)
- [変更履歴とリリース手順](docs/releases.md)

## ライセンス

このリポジトリの本体コードは、ルートの [MIT License](LICENSE) で公開しています。

BotとMCGuildLinkは別々のコンテナイメージとして配布するため、第三者ライセンスもイメージごとに同梱します。

- Bot：`/app/THIRD_PARTY_LICENSES`
- MCGuildLink：`/app/THIRD_PARTY_LICENSES/index.html` と同ディレクトリ内のライセンスファイル

第三者ライセンス一覧は、コンテナイメージのビルド時に自動生成されます。DockerfileはPodmanでビルドできます。
