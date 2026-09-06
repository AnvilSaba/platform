# Domain Docs

This repository uses a single-context domain documentation layout.

## Before exploring

- Read `CONTEXT.md` at the repository root.
- Read ADRs in `docs/adr/` that touch the area being changed.

## Vocabulary

When naming domain concepts in issue titles, refactor proposals, hypotheses, or tests, use the vocabulary defined in `CONTEXT.md`. If an existing ADR conflicts with proposed work, surface the conflict explicitly rather than silently overriding it.

## 文書の配置

- ドメイン用語はルートの `CONTEXT.md`、長期的な設計判断と理由は `docs/adr/` に記録する。
- API・ライブラリ・既存コードの根拠を確認する際は `docs/research/` を参照する。
- 機能仕様と受け入れ条件は GitHub Issue を正本とし、Grill の草案や質問履歴を恒久 docs に重複保存しない。Discord 構成管理の仕様は [Feature Spec #2](https://github.com/AnvilSaba/platform/issues/2)。
