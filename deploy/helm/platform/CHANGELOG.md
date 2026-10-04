# 変更履歴
## [1.0.0](https://github.com/anvilsaba/platform/compare/chart/v0.2.1..chart/v1.0.0) - 2026-10-04

### 機能追加

- (bot): 対話型コマンドコンソールを追加 - ([b36db4d](https://github.com/anvilsaba/platform/commit/b36db4dd31c36c6eb29616027b9e9b76fb92a6cf))
- (bot): PostgreSQL で紐付けコードを発行する ([#56](https://github.com/anvilsaba/platform/pull/56)) ([#67](https://github.com/anvilsaba/platform/pull/67)) - ([a0e9c05](https://github.com/anvilsaba/platform/commit/a0e9c058666d46f0cbacd00a700e0706db5436d8))
- (deploy): [**breaking**] 分離サービスの独立リリースと新Chartを整備する ([#77](https://github.com/anvilsaba/platform/pull/77)) - ([fdce955](https://github.com/anvilsaba/platform/commit/fdce95598c56c734b115d49c88983849c8a8b06d))
- (deploy): DBマイグレーションとMCGuildLink移行手順を整理する ([#82](https://github.com/anvilsaba/platform/pull/82)) - ([3fd1914](https://github.com/anvilsaba/platform/commit/3fd19145d51c1f15486628d0cb592cf670bf75a7))

---
## [0.2.1](https://github.com/anvilsaba/platform/compare/chart/v0.2.0..chart/v0.2.1) - 2026-08-30

### バグ修正

- (deploy): 本番デプロイでvalues.prod.yamlを適用 - ([c053fa8](https://github.com/anvilsaba/platform/commit/c053fa8e5e5e8f1eb0dca07abf75215d46c96871))

---
## [0.2.0](https://github.com/anvilsaba/platform/compare/chart/v0.1.0..chart/v0.2.0) - 2026-08-30

### その他

- (deploy): 本番環境のMinecraftポートを25600に変更 - ([955b064](https://github.com/anvilsaba/platform/commit/955b064162d7274f98b98754a0f73c5f6cf07843))

---
## [0.1.0](https://github.com/anvilsaba/platform/commits/chart/v0.1.0) - 2026-08-30

### 機能追加

- (helm): GHCRイメージ取得用Secretの設定を追加 - ([80db163](https://github.com/anvilsaba/platform/commit/80db1636a537752f737ee954b697223d290c918d))

### バグ修正

- (mcguildlink): 起動できない問題を修正 - ([6ff600c](https://github.com/anvilsaba/platform/commit/6ff600cba54adfeb4ef04f26a3dfe7367d28b362))

### その他

- (deploy): Harden workload security contexts ([#1](https://github.com/anvilsaba/platform/pull/1)) - ([48aec33](https://github.com/anvilsaba/platform/commit/48aec339d6eb0f7a635281ea403309cb284fb12d))
- Migrate applications into platform monorepo - ([ead3cf1](https://github.com/anvilsaba/platform/commit/ead3cf167e3017a6ea260c1bee5ce859e32c9526))
- Target arm64 and harden container runtimes - ([0c3f2ad](https://github.com/anvilsaba/platform/commit/0c3f2ad564be5c01c2173061d7b4ba4f9c787543))

---
