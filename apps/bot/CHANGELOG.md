# 変更履歴
## [4.0.0](https://github.com/anvilsaba/platform/compare/bot/v3.5.1..bot/v4.0.0) - 2026-10-04

### 機能追加

- (bot): 対話型コマンドコンソールを追加 - ([b36db4d](https://github.com/anvilsaba/platform/commit/b36db4dd31c36c6eb29616027b9e9b76fb92a6cf))
- (bot): PostgreSQL で紐付けコードを発行する ([#56](https://github.com/anvilsaba/platform/pull/56)) ([#67](https://github.com/anvilsaba/platform/pull/67)) - ([a0e9c05](https://github.com/anvilsaba/platform/commit/a0e9c058666d46f0cbacd00a700e0706db5436d8))
- (deploy): [**breaking**] 分離サービスの独立リリースと新Chartを整備する ([#77](https://github.com/anvilsaba/platform/pull/77)) - ([fdce955](https://github.com/anvilsaba/platform/commit/fdce95598c56c734b115d49c88983849c8a8b06d))
- (deploy): DBマイグレーションとMCGuildLink移行手順を整理する ([#82](https://github.com/anvilsaba/platform/pull/82)) - ([3fd1914](https://github.com/anvilsaba/platform/commit/3fd19145d51c1f15486628d0cb592cf670bf75a7))
- (mcguildlink): PostgreSQL でコード消費と紐付けを完了 ([#58](https://github.com/anvilsaba/platform/pull/58)) ([#70](https://github.com/anvilsaba/platform/pull/70)) - ([795c834](https://github.com/anvilsaba/platform/commit/795c834ae76b222117a8febd82bd3a1e57e11c83))
- (mcguildlink): 紐付け一覧・解除・退出処理をBotへ移行する ([#60](https://github.com/anvilsaba/platform/pull/60)) ([#73](https://github.com/anvilsaba/platform/pull/73)) - ([341bdea](https://github.com/anvilsaba/platform/commit/341bdeaf516319877074654335f0cb3be378795d))
- (mcguildlink): 関連アカウントのブロックをBotへ移行する ([#61](https://github.com/anvilsaba/platform/pull/61)) ([#74](https://github.com/anvilsaba/platform/pull/74)) - ([0dfdd65](https://github.com/anvilsaba/platform/commit/0dfdd65dd7b3c40c75e6c08441fa1619790c4bde))
- (mcguildlink): Botから監査ログを配送する ([#62](https://github.com/anvilsaba/platform/pull/62)) ([#75](https://github.com/anvilsaba/platform/pull/75)) - ([3e2b931](https://github.com/anvilsaba/platform/commit/3e2b9315bbd810d8519ab0f5cf1ca0453976d6b1))
- (mcguildlink): 監査配送の再試行と管理者再開を追加する ([#64](https://github.com/anvilsaba/platform/pull/64)) ([#76](https://github.com/anvilsaba/platform/pull/76)) - ([03a9287](https://github.com/anvilsaba/platform/commit/03a92872d9668d8038eee080de30fb23cc8dc4fa))
- (public-api): 更新番号付きホワイトリスト API を実装 ([#59](https://github.com/anvilsaba/platform/pull/59)) ([#72](https://github.com/anvilsaba/platform/pull/72)) - ([41eecae](https://github.com/anvilsaba/platform/commit/41eecae0bc26b3b6e36d3d582b2a7660437b0a39))

### リファクタリング

- (deps): クレートの依存バージョンをworkspaceに共通化する ([#79](https://github.com/anvilsaba/platform/pull/79)) - ([d0008e1](https://github.com/anvilsaba/platform/commit/d0008e15815ec01a64ceb154186cdce0fb0ed3ca))
- 終了シグナル処理を共通クレートに切り出す - ([0d7e54b](https://github.com/anvilsaba/platform/commit/0d7e54b0e99bbb213cd0e4fe466a897cc75ea577))

### その他

- (deps): SQLx 0.9へ更新しマイグレーション追跡とCIのCLI管理を整える - ([c650eb3](https://github.com/anvilsaba/platform/commit/c650eb36819fceb56b4b66e6a19b3a889f96d89e))
- (rust): Nightly と依存公開待機期間を設定する - ([ba05b64](https://github.com/anvilsaba/platform/commit/ba05b640fa1ebc70083f20554fb3f3bde8c481d5))
- (rustfmt): Nightlyのimportグループ整列を有効にする - ([93f5f52](https://github.com/anvilsaba/platform/commit/93f5f52fad5a52e55bd2383668a7c9d2cd630293))
- Clippyの指摘と細かな整形を修正する - ([1d42bc5](https://github.com/anvilsaba/platform/commit/1d42bc5cc21a9a6e21f706f1714460f16663618c))

---
## [3.5.1](https://github.com/anvilsaba/platform/compare/bot/v3.5.0..bot/v3.5.1) - 2026-08-30

### バグ修正

- 自動招待が失敗する問題を修正 - ([d616fa0](https://github.com/anvilsaba/platform/commit/d616fa01eb6e549277634ee25a75d6a99dd490c3))

### CI

- (release): Automate app-specific releases and changelogs - ([fa4b1c0](https://github.com/anvilsaba/platform/commit/fa4b1c067ff23ec78611b2207a949b20254ae6db))

### ビルド

- Dockerビルドキャッシュとランタイムを最適化 - ([2894c8e](https://github.com/anvilsaba/platform/commit/2894c8e8754c87c684ed80c8693c50035bdb76d2))

### その他

- Serenity を更新 - ([39cc47d](https://github.com/anvilsaba/platform/commit/39cc47d487d9ff13eb81a17e3e2c3ccef3e52a9e))
- Add 'apps/bot/' from commit '39cc47d487d9ff13eb81a17e3e2c3ccef3e52a9e' - ([7848df1](https://github.com/anvilsaba/platform/commit/7848df1181acca1d3d4d19d658337f57b1392795))
- Migrate applications into platform monorepo - ([ead3cf1](https://github.com/anvilsaba/platform/commit/ead3cf167e3017a6ea260c1bee5ce859e32c9526))
- Centralize licensing and bundle third-party notices - ([ee1f9ee](https://github.com/anvilsaba/platform/commit/ee1f9eea437f8db6b28fb48843914344b7d2acb3))
- Target arm64 and harden container runtimes - ([0c3f2ad](https://github.com/anvilsaba/platform/commit/0c3f2ad564be5c01c2173061d7b4ba4f9c787543))

---

## 3.5.0 以前

以前の変更は[旧Botコミット履歴](https://github.com/anvilsaba/platform/commits/bot/v3.5.0)を参照してください。
